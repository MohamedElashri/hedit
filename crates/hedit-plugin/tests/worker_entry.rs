use std::process::{Command, Stdio};

#[test]
fn worker_version_and_unconfined_entry_are_bounded() {
    let binary = env!("CARGO_BIN_EXE_hedit-plugin-worker");
    let version = Command::new(binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap().trim(),
        concat!("hedit-plugin-worker ", env!("CARGO_PKG_VERSION"))
    );
    let result = Command::new(binary)
        .arg("--hedit-plugin-worker")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(result.stderr.is_empty());
    if hedit_plugin::SUPPORTED {
        assert!(result.status.success());
        assert_eq!(result.stdout, [1]);
    } else {
        assert_eq!(result.status.code(), Some(1));
        assert!(result.stdout.is_empty());
    }
}
