#!/usr/bin/env python3
"""Exercise cleanup boundaries, worktrees, previews and concurrent-build locks."""
from contextlib import redirect_stdout
import importlib.util
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('build_storage', Path(__file__).with_name('build-storage.py'))
storage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(storage)


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.target = self.root / 'target'
        self.target.mkdir()

    def profile(self, relative='debug'):
        profile = self.target / relative
        (profile / 'incremental').mkdir(parents=True)
        (profile / '.cargo-lock').touch()
        (profile / 'incremental/cache').write_bytes(b'cache')
        (profile / 'artifact').write_bytes(b'compiled')
        return profile

    def test_incremental_cleanup_keeps_binaries_source_and_reports(self):
        profile = self.profile()
        source = self.root / 'source.rs'; source.write_text('source')
        report = self.target / 'plugin-validation/result.json'
        report.parent.mkdir(); report.write_text('evidence')
        storage.apply_cleanup(storage.plan_cleanup([self.target], 'incremental'))
        self.assertFalse((profile / 'incremental').exists())
        self.assertEqual((profile / 'artifact').read_bytes(), b'compiled')
        self.assertEqual(source.read_text(), 'source')
        self.assertEqual(report.read_text(), 'evidence')

    def test_build_cleanup_keeps_tools_evidence_and_lock_inodes(self):
        debug = self.profile()
        release = self.profile('aarch64-apple-darwin/release')
        auxiliary = self.profile('remote-fault-tests/debug')
        (auxiliary.parent / '.rustc_info.json').write_text('{}')
        names = ('ghostty-toolchain/compiler', 'docs-tools/nida', 'release-validation/checks.json')
        for relative in names:
            path = self.target / relative; path.parent.mkdir(parents=True); path.write_text('retain')
        inode = (debug / '.cargo-lock').stat().st_ino
        storage.apply_cleanup(storage.plan_cleanup([self.target], 'builds'))
        for profile in (debug, release, auxiliary):
            self.assertEqual(set(p.name for p in profile.iterdir()), set(storage.LOCKS))
        self.assertEqual((debug / '.cargo-lock').stat().st_ino, inode)
        for relative in names:
            self.assertEqual((self.target / relative).read_text(), 'retain')

    def test_dev_cleanup_keeps_release_slices(self):
        debug = self.profile()
        full = self.profile('debugging')
        release = self.profile('x86_64-apple-darwin/release')
        storage.apply_cleanup(storage.plan_cleanup([self.target], 'dev'))
        self.assertFalse((debug / 'artifact').exists())
        self.assertFalse((full / 'artifact').exists())
        self.assertTrue((release / 'artifact').is_file())

    def test_unrecognized_directories_are_retained(self):
        (self.target / 'debug').mkdir()
        (self.target / 'debug/note.txt').write_text('retain')
        self.assertEqual(storage.plan_cleanup([self.target], 'builds'), [])

    def test_copied_build_snapshots_remain_retained_as_evidence(self):
        profile = self.profile('validation-snapshot/debug')
        (profile.parent / '.rustc_info.json').write_text('{}')
        self.assertEqual(storage.plan_cleanup([self.target], 'builds'), [])
        self.assertTrue((profile / 'artifact').is_file())

    def test_preview_does_not_create_locks_or_remove_output(self):
        self.profile()
        before = sorted(p.relative_to(self.root) for p in self.root.rglob('*'))
        with patch.object(storage, 'ROOT', self.root), redirect_stdout(io.StringIO()):
            storage.main(['clean', '--target-dir', str(self.target)])
        self.assertEqual(before, sorted(p.relative_to(self.root) for p in self.root.rglob('*')))

    def test_symlink_target_and_incremental_directory_cannot_escape(self):
        outside = self.root / 'outside'; outside.mkdir()
        protected = outside / 'source'; protected.write_text('retain')
        link = self.root / 'linked-target'
        try:
            link.symlink_to(outside, target_is_directory=True)
        except OSError as error:
            self.skipTest(f'Symlinks unavailable: {error}')
        with self.assertRaises(ValueError):
            storage.targets([], link)
        profile = self.profile()
        (profile / 'incremental/cache').unlink(); (profile / 'incremental').rmdir()
        (profile / 'incremental').symlink_to(outside, target_is_directory=True)
        with self.assertRaises(ValueError):
            storage.plan_cleanup([self.target], 'incremental')
        self.assertEqual(protected.read_text(), 'retain')

    def test_busy_cargo_lock_aborts_before_any_profile_is_cleaned(self):
        first = self.profile()
        second = self.profile('release')
        program = '''import os,sys
f=open(sys.argv[1],'a+b')
if os.name=='nt':
 import msvcrt
 f.seek(0); msvcrt.locking(f.fileno(),msvcrt.LK_NBLCK,1)
else:
 import fcntl
 fcntl.flock(f.fileno(),fcntl.LOCK_EX)
print('locked',flush=True)
sys.stdin.read()
'''
        process = subprocess.Popen([sys.executable, '-c', program, str(second / '.cargo-build-lock')],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            self.assertEqual(process.stdout.readline().strip(), 'locked')
            with self.assertRaisesRegex(ValueError, 'Cargo is using'):
                storage.apply_cleanup(storage.plan_cleanup([self.target], 'builds'))
            self.assertTrue((first / 'artifact').is_file())
            self.assertTrue((second / 'artifact').is_file())
        finally:
            process.communicate(timeout=10)

    @unittest.skipUnless(shutil.which('cargo'), 'Cargo is not installed')
    def test_real_cargo_builds_hold_cleanup_locks(self):
        (self.root / 'src').mkdir()
        (self.root / 'Cargo.toml').write_text(
            '[package]\nname="cleanup-lock-probe"\nversion="0.1.0"\nedition="2021"\n[workspace]\n')
        (self.root / 'src/main.rs').write_text('fn main() {}')
        (self.root / 'build.rs').write_text('''fn main() {
    let signal = std::env::var("HEDIT_LOCK_PROBE").unwrap();
    std::fs::write(&signal, "ready").unwrap();
    while std::path::Path::new(&signal).exists() {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}''')
        for shared in (False, True):
            with self.subTest(shared=shared):
                signal = self.root / 'waiting'
                output = self.root / ('shared-final' if shared else 'target')
                intermediate = self.root / 'intermediates'
                environment = dict(os.environ, CARGO_TARGET_DIR=str(output), HEDIT_LOCK_PROBE=str(signal))
                environment.pop('CARGO_BUILD_BUILD_DIR', None)
                if shared:
                    environment['CARGO_BUILD_BUILD_DIR'] = str(intermediate)
                directories = [output, intermediate] if shared else [output]
                with tempfile.TemporaryFile(mode='w+') as log:
                    process = subprocess.Popen(['cargo', 'build', '--offline'], cwd=self.root, env=environment,
                                               stdout=log, stderr=log)
                    try:
                        deadline = time.monotonic() + 60
                        while not signal.exists():
                            if process.poll() is not None or time.monotonic() > deadline:
                                log.seek(0)
                                self.fail(f'Cargo did not reach the lock probe: {log.read()}')
                            time.sleep(.02)
                        for directory in directories:
                            planned = storage.plan_cleanup([directory], 'builds')
                            self.assertTrue(planned, f'No Cargo profile recognized: {directory}')
                            with self.assertRaisesRegex(ValueError, 'Cargo is using'):
                                storage.apply_cleanup(planned)
                    finally:
                        signal.unlink(missing_ok=True)
                        try:
                            process.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
                    log.seek(0)
                    self.assertEqual(process.returncode, 0, log.read())
                for directory in directories:
                    storage.apply_cleanup(storage.plan_cleanup([directory], 'builds'))
                self.assertFalse(any(output.glob('debug/cleanup-lock-probe*')))

    def test_size_report_does_not_follow_directory_symlinks(self):
        self.profile()
        outside = self.root / 'outside'; outside.mkdir()
        (outside / 'large').write_bytes(b'x' * 1024 * 1024)
        try:
            (self.target / 'link').symlink_to(outside, target_is_directory=True)
        except OSError as error:
            self.skipTest(f'Symlinks unavailable: {error}')
        self.assertLess(storage.allocated_size(self.target), 1024 * 1024)

    def test_worktree_paths_with_spaces_are_parsed_without_shell_splitting(self):
        output = b'worktree /repo/a tree\0HEAD abc\0branch refs/heads/main\0\0worktree /repo/other\0HEAD def\0\0'
        with patch.object(storage, 'git', return_value=output):
            self.assertEqual(storage.worktrees(), [Path('/repo/a tree'), Path('/repo/other')])


if __name__ == '__main__':
    unittest.main()
