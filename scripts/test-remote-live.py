#!/usr/bin/env python3
"""Real loopback OpenSSH and process-crash tests. Never uses the user's SSH home.

Run with a built helper and a separate --features test-faults binary. Requires
OpenSSH client/server/keygen and a supported host-local recovery filesystem.
Creates a server bound ONLY to 127.0.0.1 with temporary keys, config, and homes.
"""
import argparse
import getpass
import json
import os
from pathlib import Path
import select
import shlex
import shutil
import signal
import socket
import struct
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
MAX_FRAME = 65536
ARGS = None
LAB = None


def run(*args, **kwargs):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, timeout=15, **kwargs)


def wait_for(predicate, seconds=10):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if predicate():
            return
        time.sleep(0.01)
    raise AssertionError("timed out waiting for test boundary")


def available_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Lab:
    def __init__(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hedit-ssh-", dir="/tmp")
        self.base = Path(self.temp.name).resolve()
        self.host_key = self.base / "host-key"
        self.client_key = self.base / "client-key"
        self.wrong_key = self.base / "wrong-host-key"
        self.port = available_port()
        self.forward_port = available_port()
        self.sshd = None
        self.log = None
        self.count = 0

    def start(self):
        for key in [self.host_key, self.client_key, self.wrong_key]:
            run("ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key))
        authorized = self.base / "authorized_keys"
        authorized.write_bytes(self.client_key.with_suffix(".pub").read_bytes())
        authorized.chmod(0o600)
        self.known = self.base / "known_hosts"
        public = self.host_key.with_suffix(".pub").read_text().split()
        self.known.write_text(f"[127.0.0.1]:{self.port} {public[0]} {public[1]}\n")
        self.sentinel = self.base / "MUST_NOT_EXECUTE"
        self.client_config = self.base / "client.conf"
        self.client_config.write_text(f"""Host lab
    HostName 127.0.0.1
    User {getpass.getuser()}
    Port {self.port}
    IdentityFile {self.client_key}
    IdentitiesOnly yes
    IdentityAgent none
    UserKnownHostsFile {self.known}
    GlobalKnownHostsFile /dev/null
    StrictHostKeyChecking yes
    BatchMode yes
    PermitLocalCommand yes
    LocalCommand touch {self.sentinel}
    LocalForward 127.0.0.1:{self.forward_port} 127.0.0.1:1
""")
        config = self.base / "sshd.conf"
        remote_home = self.base / "remote-home"
        remote_home.mkdir(mode=0o700)
        config.write_text(f"""Port {self.port}
ListenAddress 127.0.0.1
HostKey {self.host_key}
AuthorizedKeysFile {authorized}
PidFile {self.base / 'sshd.pid'}
PasswordAuthentication no
KbdInteractiveAuthentication no
PubkeyAuthentication yes
UsePAM no
PermitUserRC no
PermitUserEnvironment no
SetEnv HOME={remote_home} ZDOTDIR={remote_home} ENV=/dev/null BASH_ENV=/dev/null
# Test-only: authorized_keys lives under sticky /tmp, outside a login home.
# Its containing directory is private and all keys are generated for this run.
StrictModes no
PermitRootLogin prohibit-password
AllowUsers {getpass.getuser()}
AllowAgentForwarding no
AllowTcpForwarding no
X11Forwarding no
PermitTunnel no
UseDNS no
LogLevel {"DEBUG3" if ARGS.debug_server else "ERROR"}
""")
        executable = shutil.which("sshd") or "/usr/sbin/sshd"
        version = subprocess.run([executable, "-V"], capture_output=True, text=True, timeout=5)
        import re
        match = re.search(r"OpenSSH_(\d+)\.(\d+)", version.stdout + version.stderr)
        if match and tuple(map(int, match.groups())) >= (9, 8):
            with config.open("a") as stream:
                stream.write("PerSourcePenalties no\n")
        run(executable, "-t", "-f", str(config))
        self.log = (self.base / "sshd.log").open("wb")
        self.sshd = subprocess.Popen([executable, "-D", "-e", "-f", str(config)],
                                     stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                     stderr=self.log, start_new_session=True)
        def ready():
            if self.sshd.poll() is not None:
                raise RuntimeError("isolated sshd exited: " + (self.base / "sshd.log").read_text())
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.1):
                    return True
            except OSError:
                return False
        wait_for(ready)

    def close(self):
        if self.sshd is not None:
            self.sshd.terminate()
            try:
                self.sshd.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.sshd.kill()
                self.sshd.wait()
        if self.log:
            self.log.close()
        self.temp.cleanup()


class Wire:
    def __init__(self, state, helper, home):
        command = f"exec {shlex.quote(str(helper))} bridge --state {shlex.quote(str(state))}"
        env = dict(os.environ, HOME=str(home))
        self.proc = subprocess.Popen([
            "/usr/bin/ssh", "-F", str(LAB.client_config), "-T",
            "-o", "PermitLocalCommand=no", "-o", "ClearAllForwardings=yes",
            "-o", "ControlPath=none", "-o", "ProxyCommand=none",
            "-o", "ProxyJump=none", "--", "lab", command],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=env, bufsize=0)
        self.next_id = 0
        try:
            self.hello = self.call(op="hello")
        except BaseException as error:
            self.close()
            raise AssertionError(f"{error}; SSH diagnostic: {self.diagnostic!r}") from error
        self.session = self.hello["session"]

    def send(self, **request):
        self.next_id += 1
        data = json.dumps(dict(version=1, id=self.next_id, request=request),
                          ensure_ascii=False, separators=(",", ":")).encode()
        assert 0 < len(data) <= MAX_FRAME
        self.proc.stdin.write(struct.pack(">I", len(data)) + data)
        self.proc.stdin.flush()
        return self.next_id

    def exact(self, count):
        data = bytearray()
        end = time.monotonic() + 10
        while len(data) < count:
            remaining = end - time.monotonic()
            if remaining <= 0 or not select.select([self.proc.stdout], [], [], remaining)[0]:
                raise AssertionError("SSH reply timed out")
            part = os.read(self.proc.stdout.fileno(), count - len(data))
            if not part:
                raise AssertionError("SSH closed before reply")
            data.extend(part)
        return data

    def receive(self):
        length = struct.unpack(">I", self.exact(4))[0]
        assert 0 < length <= MAX_FRAME
        reply = json.loads(self.exact(length))
        assert reply["version"] == 1 and reply["id"] == self.next_id
        return reply["result"]

    def call(self, **request):
        self.send(**request)
        result = self.receive()
        if "Err" in result:
            raise AssertionError(f"unexpected helper error: {result['Err']}")
        return result["Ok"]

    def close(self):
        if self.proc.poll() is None:
            self.proc.kill()
        self.proc.wait(timeout=5)
        if not hasattr(self, "diagnostic"):
            self.diagnostic = self.proc.stderr.read(4096).decode(errors="replace")
        for stream in [self.proc.stdin, self.proc.stdout]:
            stream.close()
        self.proc.stderr.close()


class LiveTests(unittest.TestCase):
    def setUp(self):
        LAB.count += 1
        self.base = LAB.base / f"c{LAB.count}"
        self.base.mkdir(mode=0o700)
        self.root = self.base / "project"
        self.root.mkdir()
        self.home = self.base / "client-home"
        self.home.mkdir(mode=0o700)
        self.state = self.base / "state"
        self.workers = []
        self.wires = []
        self.clients = []
        self.auto_pid = None

    def tearDown(self):
        for client in self.clients:
            if client.poll() is None:
                client.kill()
            client.wait(timeout=5)
            client.stdin.close()
            client.stdout.close()
        for wire in self.wires:
            wire.close()
        for worker in self.workers:
            if worker.poll() is None:
                worker.kill()
            worker.wait(timeout=5)
        if self.auto_pid is not None:
            # PID comes only from our fixed helper on this isolated loopback server.
            command = run("ps", "-p", str(self.auto_pid), "-o", "command=").stdout.decode()
            self.assertIn(str(self.state), command)
            os.kill(self.auto_pid, signal.SIGTERM)
        elif not self.workers and (self.state / "worker.pid").exists():
            # Also clean up when a bootstrap/handshake fails before Hello.
            pid = int((self.state / "worker.pid").read_text())
            process = subprocess.run(["ps", "-p", str(pid), "-o", "command="],
                                     capture_output=True, text=True, timeout=5)
            if process.returncode == 0 and str(self.state) in process.stdout:
                os.kill(pid, signal.SIGTERM)

    def worker(self, stage=None):
        marker = self.base / "gate"
        env = dict(PATH="/usr/bin:/bin", LC_ALL="C")
        executable = ARGS.fault_binary if stage else ARGS.binary
        if stage:
            self.assertIsNotNone(executable, "--fault-binary required for deterministic crash tests")
            env.update(HEDIT_REMOTE_TEST_STAGE=stage, HEDIT_REMOTE_TEST_GATE=str(marker))
        proc = subprocess.Popen([str(executable), "worker", "--state", str(self.state)],
                                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                stderr=subprocess.DEVNULL, env=env, start_new_session=True)
        self.workers.append(proc)
        def ready():
            if proc.poll() is not None:
                raise AssertionError("worker exited before readiness; check recovery filesystem support")
            return (self.state / "session.sock").exists()
        # A stale socket after SIGKILL is removed/rebound by the new worker.
        wait_for(ready)
        def listening():
            try:
                with socket.socket(socket.AF_UNIX) as probe:
                    probe.connect(str(self.state / "session.sock"))
                return True
            except OSError:
                return False
        wait_for(listening)
        return proc, marker

    def wire(self):
        wire = Wire(self.state, ARGS.binary, self.home)
        self.wires.append(wire)
        return wire

    def document(self, wire, text="base\r\nب🦀"):
        self.root.joinpath("file").write_bytes(text.encode())
        wire.call(op="workspace", path=list(os.fsencode(self.root)))
        epoch = wire.call(op="acquire", takeover=False)["epoch"]
        doc = wire.call(op="open", epoch=epoch, path=list(b"file"))["info"]["document"]
        return epoch, doc

    def edit(self, wire, epoch, doc, sequence=1, text="pending", session=None):
        return dict(op="apply", session=session or wire.session, epoch=epoch, document=doc,
                    sequence=sequence, base_revision=sequence-1,
                    action=dict(kind="edit", start=0, end=0, text=text))

    def text(self, wire, doc):
        result = ""
        while True:
            page = wire.call(op="read", document=doc, offset=len(result.encode()))
            result += page["text"]
            if page["eof"]:
                return result

    def test_auto_started_worker_survives_client_sigkill_and_source_deletion(self):
        wire = self.wire()
        self.auto_pid = wire.hello["worker_pid"]
        epoch, doc = self.document(wire)
        wire.call(**self.edit(wire, epoch, doc, text="RECOVERY_CANARY"))
        wire.close()
        self.root.joinpath("file").unlink()
        wire = self.wire()
        self.assertEqual(wire.call(op="list")["items"][0]["revision"], 1)
        self.assertEqual(self.text(wire, doc), "RECOVERY_CANARYbase\r\nب🦀")
        for path in self.home.rglob("*"):
            if path.is_file():
                self.assertNotIn(b"RECOVERY_CANARY", path.read_bytes())

    def test_takeover_fences_old_writer_and_session_identity_rejects_reused_sequences(self):
        self.worker()
        first = self.wire()
        epoch, doc = self.document(first)
        second = self.wire()
        second.send(op="acquire", takeover=False)
        self.assertEqual(second.receive(), {"Err": "busy"})
        new_epoch = second.call(op="acquire", takeover=True)["epoch"]
        self.assertGreater(new_epoch, epoch)
        first.send(**self.edit(first, epoch, doc))
        self.assertEqual(first.receive(), {"Err": "fenced"})
        second.send(**self.edit(second, new_epoch, doc, session="0"*64))
        self.assertEqual(second.receive(), {"Err": "conflict"})
        second.call(**self.edit(second, new_epoch, doc, text="once"))
        second.call(**self.edit(second, new_epoch, doc, text="once"))
        self.assertEqual(self.text(second, doc), "oncebase\r\nب🦀")

    def test_worker_kill_before_commit_restores_prior_revision(self):
        worker, marker = self.worker("before_commit")
        wire = self.wire()
        epoch, doc = self.document(wire)
        wire.send(**self.edit(wire, epoch, doc))
        wait_for(marker.exists)
        wire.close()
        worker.kill()
        worker.wait(timeout=5)
        self.worker()
        recovered = self.wire()
        self.assertEqual(self.text(recovered, doc), "base\r\nب🦀")
        self.assertEqual(recovered.call(op="list")["items"][0]["revision"], 0)

    def test_worker_kill_after_commit_before_ack_restores_once_with_undo(self):
        worker, marker = self.worker("after_commit")
        wire = self.wire()
        epoch, doc = self.document(wire)
        original_session = wire.session
        wire.send(**self.edit(wire, epoch, doc, text="once"))
        wait_for(marker.exists)
        wire.close()
        worker.kill()
        worker.wait(timeout=5)
        self.root.joinpath("file").unlink()
        self.worker()
        recovered = self.wire()
        self.assertEqual(recovered.session, original_session)
        epoch = recovered.call(op="acquire", takeover=True)["epoch"]
        recovered.call(**self.edit(recovered, epoch, doc, text="once"))
        self.assertEqual(self.text(recovered, doc), "oncebase\r\nب🦀")
        recovered.call(op="apply", session=recovered.session, epoch=epoch, document=doc,
                       sequence=2, base_revision=1, action=dict(kind="undo"))
        self.assertEqual(self.text(recovered, doc), "base\r\nب🦀")

    def test_client_kill_before_commit_does_not_cancel_accepted_edit(self):
        _, marker = self.worker("before_commit")
        wire = self.wire()
        epoch, doc = self.document(wire)
        wire.send(**self.edit(wire, epoch, doc, text="survived"))
        wait_for(marker.exists)
        wire.close()
        marker.with_suffix(".continue").touch()
        recovered = self.wire()
        self.assertEqual(self.text(recovered, doc), "survivedbase\r\nب🦀")

    def test_production_client_blocks_local_command_forwarding_and_unknown_host(self):
        self.worker()
        command = [str(ARGS.binary), "connect", "--host", "lab", "--port", str(LAB.port),
                   "--helper", str(ARGS.binary), "--state", str(self.state),
                   "--ssh-config", str(LAB.client_config)]
        env = dict(os.environ, HOME=str(self.home))
        result = run(*command, input=b'{"op":"ping"}\n', env=env)
        self.assertEqual(json.loads(result.stdout), {"Ok": {"kind": "pong"}})
        self.assertFalse(LAB.sentinel.exists())
        with socket.socket() as s:
            self.assertNotEqual(s.connect_ex(("127.0.0.1", LAB.forward_port)), 0)
        original = LAB.known.read_bytes()
        wrong = LAB.wrong_key.with_suffix(".pub").read_text().split()
        changed = f"[127.0.0.1]:{LAB.port} {wrong[0]} {wrong[1]}\n".encode()
        try:
            for contents in [b"", changed]:
                LAB.known.write_bytes(contents)
                result = subprocess.run(command, input=b'{"op":"ping"}\n', stdout=subprocess.PIPE,
                                        stderr=subprocess.PIPE, timeout=15, env=env)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, b"")
        finally:
            LAB.known.write_bytes(original)

    def test_actual_client_process_kill_preserves_accepted_edits(self):
        _, marker = self.worker("before_commit")
        client = subprocess.Popen([
            str(ARGS.binary), "connect", "--host", "lab", "--port", str(LAB.port),
            "--helper", str(ARGS.binary), "--state", str(self.state),
            "--ssh-config", str(LAB.client_config)], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
            env=dict(os.environ, HOME=str(self.home)))
        self.clients.append(client)
        def call(**request):
            client.stdin.write(json.dumps(request).encode() + b"\n")
            client.stdin.flush()
            self.assertTrue(select.select([client.stdout], [], [], 10)[0])
            result = json.loads(client.stdout.readline())
            self.assertIn("Ok", result)
            return result["Ok"]
        session = call(op="hello")["session"]
        self.root.joinpath("file").write_text("base")
        call(op="workspace", path=list(os.fsencode(self.root)))
        epoch = call(op="acquire", takeover=False)["epoch"]
        doc = call(op="open", epoch=epoch, path=list(b"file"))["info"]["document"]
        client.stdin.write(json.dumps(dict(op="apply", session=session, epoch=epoch,
            document=doc, sequence=1, base_revision=0,
            action=dict(kind="edit", start=0, end=0, text="after-client-crash"))).encode()+b"\n")
        client.stdin.flush()
        wait_for(marker.exists)
        client.kill()
        client.wait(timeout=5)
        marker.with_suffix(".continue").touch()
        recovered = self.wire()
        self.assertEqual(self.text(recovered, doc), "after-client-crashbase")

    def test_production_client_bootstraps_quoted_install_and_state_paths(self):
        helper = self.base / "helper with ' quote"
        shutil.copy2(ARGS.binary, helper)
        helper.chmod(0o700)
        self.state = self.base / "state with ' quote"
        result = run(str(ARGS.binary), "connect", "--host", "lab", "--port", str(LAB.port),
                     "--helper", str(helper), "--state", str(self.state),
                     "--ssh-config", str(LAB.client_config), input=b'{"op":"ping"}\n',
                     env=dict(os.environ, HOME=str(self.home)))
        self.assertEqual(json.loads(result.stdout), {"Ok": {"kind": "pong"}})
        self.assertTrue((self.state / "worker.pid").is_file())

    def test_peer_limit_rejects_excess_without_stalling_existing_clients(self):
        self.worker()
        peers = [self.wire() for _ in range(16)]
        with self.assertRaises(AssertionError):
            self.wire()
        self.assertEqual(peers[0].call(op="ping"), {"kind": "pong"})

    def test_worker_crash_after_source_publication_reconciles_save(self):
        worker, marker = self.worker("save_after_publish")
        wire = self.wire()
        epoch, doc = self.document(wire, "base")
        wire.call(**self.edit(wire, epoch, doc, text="saved "))
        operation = "a" * 64
        wire.send(op="save", session=wire.session, epoch=epoch, document=doc,
                  sequence=1, operation=operation, target=None)
        wait_for(marker.exists)
        self.assertEqual(self.root.joinpath("file").read_text(), "saved base")
        wire.close(); worker.kill(); worker.wait(timeout=5)
        self.worker()
        recovered = self.wire()
        recovered.call(op="workspace", path=list(os.fsencode(self.root)))
        epoch = recovered.call(op="acquire", takeover=False)["epoch"]
        info = recovered.call(op="list")["items"][0]
        self.assertFalse(info["dirty"])
        recovered.call(op="save", session=recovered.session, epoch=epoch, document=doc,
                       sequence=1, operation=operation, target=None)
        self.assertEqual(self.text(recovered, doc), "saved base")

    def test_worker_crash_during_compaction_preserves_last_commit(self):
        for stage in ["compact_before_commit", "compact_after_commit"]:
            with self.subTest(stage=stage):
                # Separate state/workspace for each deterministic crash boundary.
                self.state = self.base / stage
                self.root = self.base / (stage + "-root"); self.root.mkdir()
                worker, marker = self.worker(stage)
                if marker.exists(): marker.unlink()
                wire = self.wire(); epoch, doc = self.document(wire, "base")
                for sequence in range(1, 512):
                    wire.call(**self.edit(wire, epoch, doc, sequence=sequence, text="x"))
                wire.send(**self.edit(wire, epoch, doc, sequence=512, text="x"))
                wait_for(marker.exists); wire.close(); worker.kill(); worker.wait(timeout=5)
                self.worker(); recovered = self.wire()
                self.assertEqual(self.text(recovered, doc), "x" * 512 + "base")
                epoch = recovered.call(op="acquire", takeover=False)["epoch"]
                recovered.call(**self.edit(recovered, epoch, doc, sequence=512, text="x"))
                recovered.call(op="apply", session=recovered.session, epoch=epoch,
                               document=doc, sequence=513, base_revision=512, action=dict(kind="undo"))
                self.assertEqual(self.text(recovered, doc), "x" * 511 + "base")
                recovered.close()
                for process in self.workers:
                    if process.poll() is None: process.terminate(); process.wait(timeout=5)

    def test_service_authorization_and_disconnected_job_cleanup(self):
        wire = self.wire(); epoch, _ = self.document(wire)
        token = wire.call(op="acquire", takeover=False)["token"]
        wire.send(op="process_start", session=wire.session, epoch=epoch,
                  directory=[], command="printf forbidden", file=None, cols=80, rows=24)
        self.assertEqual(wire.receive(), {"Err": "permission"})
        service = self.wire()
        service.send(op="bind", session=wire.session, epoch=epoch, token="0" * 64)
        self.assertEqual(service.receive(), {"Err": "fenced"})
        service.call(op="bind", session=wire.session, epoch=epoch, token=token)
        service.send(op="trust", epoch=epoch, allow=True)
        self.assertEqual(service.receive(), {"Err": "fenced"})
        wire.call(op="trust", epoch=epoch, allow=True)
        process = service.call(op="process_start", session=wire.session, epoch=epoch,
                               directory=[], command="printf started; sleep 60", file=None,
                               cols=80, rows=24)["id"]
        service.close()
        def removed():
            wire.send(op="process_poll", id=process)
            return wire.receive() == {"Err": "missing"}
        wait_for(removed)
        # Abandon more tasks than the session cap; finished abandoned jobs must
        # be reclaimed, including when their result was never polled.
        for _ in range(12):
            peer = self.wire()
            peer.call(op="bind", session=wire.session, epoch=epoch, token=token)
            peer.call(op="task_start", session=wire.session, epoch=epoch,
                      call={"File": {"path": "", "query": "List"}})
            peer.close()
            time.sleep(0.02)
        wire.call(op="ping")

    def test_editor_reconnect_replays_lost_receipt_once(self):
        _, marker = self.worker("after_commit")
        self.root.joinpath("file").write_text("base")
        probe = ROOT / "target/debug/examples/remote_live_probe"
        process = subprocess.Popen([str(probe), str(LAB.client_config), str(ARGS.binary),
                                    str(self.state), str(self.root), str(LAB.port), "recover_pending"],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.clients.append(process)
        wait_for(marker.exists)
        listing = run("ps", "-axo", "pid=,ppid=,command=").stdout.decode()
        children = []
        for row in listing.splitlines():
            fields = row.split(None, 2)
            if len(fields) == 3 and int(fields[1]) == process.pid and fields[2].startswith("/usr/bin/ssh "):
                children.append(int(fields[0]))
        self.assertEqual(len(children), 1)
        os.kill(children[0], signal.SIGKILL)
        marker.with_suffix(".continue").touch()
        stdout, stderr = process.communicate(timeout=30)
        self.assertEqual(process.returncode, 0, stderr.decode())
        self.assertIn(b"PENDING_RECONCILIATION_OK", stdout)
        self.assertEqual(self.root.joinpath("file").read_text(), "PENDING_ONCEbase")

    def test_editor_backend_full_workspace(self):
        self.root.joinpath("file").write_text("BASE\n")
        self.root.joinpath("large.txt").write_text("x" * (2 * 1024 * 1024))
        run("git", "init", "-q", str(self.root))
        run("git", "-C", str(self.root), "config", "user.email", "lab@example.invalid")
        run("git", "-C", str(self.root), "config", "user.name", "SSH Test")
        probe = ROOT / "target/debug/examples/remote_live_probe"
        result = subprocess.run([str(probe), str(LAB.client_config), str(ARGS.binary), str(self.state), str(self.root), str(LAB.port)], capture_output=True, text=True, timeout=90)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("EDITOR_REMOTE_OK", result.stdout)
        self.assertIn("ب🦀", self.root.joinpath("file").read_text())
        self.assertEqual(self.root.joinpath("scratch-saved.txt").read_text(), "SCRATCH_HOST_ONLY")
        self.assertFalse(LAB.sentinel.exists())

    def test_live_commit_latency_distribution(self):
        self.worker()
        wire = self.wire()
        epoch, doc = self.document(wire, "")
        timings = []
        for sequence in range(1, 101):
            start = time.perf_counter()
            wire.call(**self.edit(wire, epoch, doc, sequence, "ب🦀"))
            timings.append((time.perf_counter()-start)*1000)
        timings.sort()
        print(f"\nLoopback durable RPC ms: p50={timings[49]:.2f} "
              f"p95={timings[94]:.2f} p99={timings[98]:.2f}", flush=True)
        self.assertEqual(self.text(wire, doc), "ب🦀"*100)
        self.assertLess(timings[98], 2000, "local commit stalled; inspect storage/transport")


def main():
    global ARGS, LAB
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/debug/hedit-remote")
    parser.add_argument("--fault-binary", type=Path, required=True)
    parser.add_argument("--test", help="Run one named LiveTests method")
    parser.add_argument("--debug-server", action="store_true", help="Bounded diagnostics from the disposable server")
    ARGS = parser.parse_args()
    ARGS.binary = ARGS.binary.resolve(strict=True)
    ARGS.fault_binary = ARGS.fault_binary.resolve(strict=True)
    LAB = Lab()
    try:
        LAB.start()
        suite = (unittest.TestSuite([LiveTests(ARGS.test)]) if ARGS.test else
                 unittest.defaultTestLoader.loadTestsFromTestCase(LiveTests))
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        if not result.wasSuccessful():
            print("Isolated sshd diagnostics:", (LAB.base / "sshd.log").read_text()[-4096:])
        return 0 if result.wasSuccessful() else 1
    finally:
        LAB.close()


if __name__ == "__main__":
    raise SystemExit(main())
