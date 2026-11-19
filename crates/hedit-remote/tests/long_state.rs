#![cfg(unix)]
use hedit_remote::{
    client::Client,
    protocol::{Request, Response},
};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Worker(Child);
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn bridges_connect_with_state_paths_longer_than_a_unix_socket_address() {
    let directory = tempfile::Builder::new()
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .tempdir()
        .unwrap();
    let state = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("long-directory-".repeat(12));
    assert!(state.as_os_str().len() > 108);
    let executable = env!("CARGO_BIN_EXE_hedit-remote");
    let mut worker = Worker(
        Command::new(executable)
            .arg("worker")
            .arg("--state")
            .arg(&state)
            .args(["--idle-seconds", "5"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let started = Instant::now();
    while !state.join("session.sock").exists() {
        assert!(
            worker.0.try_wait().unwrap().is_none(),
            "worker exited before binding its socket"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut bridge = Command::new(executable);
    bridge.arg("bridge").arg("--state").arg(&state);
    let mut client = Client::spawn(bridge).unwrap();
    assert!(
        matches!(client.call(Request::List{}).unwrap(),Response::Documents{items} if items.is_empty())
    );
}
