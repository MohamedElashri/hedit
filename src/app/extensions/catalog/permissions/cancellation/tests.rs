use super::*;
#[test]
fn cancellation_is_visible_before_the_disclosure_lock_drains() {
    let config = tempfile::tempdir().unwrap();
    drop(change_lock(config.path()).unwrap());
    let disclosure = disclosure_lock(config.path()).unwrap();
    let before = broker_epoch(config.path(), &disclosure).unwrap();
    let path = config.path().to_owned();
    let writer = std::thread::spawn(move || mutation_lock(&path).unwrap());
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !marker(config.path()).unwrap().is_some_and(|m| m.pending) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(broker_epoch(config.path(), &disclosure).is_err());
    repair_orphan(config.path()).unwrap();
    assert!(marker(config.path()).unwrap().unwrap().pending);
    drop(disclosure);
    let guard = writer.join().unwrap();
    assert!(marker(config.path()).unwrap().unwrap().pending);
    drop(guard);
    let disclosure = disclosure_lock(config.path()).unwrap();
    assert_ne!(broker_epoch(config.path(), &disclosure).unwrap(), before);
}
#[test]
fn competing_writers_and_orphan_repair_cannot_clear_a_live_writer() {
    let config = tempfile::tempdir().unwrap();
    let held = mutation_lock(config.path()).unwrap();
    let generation = marker(config.path()).unwrap().unwrap().generation;
    assert!(mutation_lock(config.path()).is_err());
    repair_orphan(config.path()).unwrap();
    let current = marker(config.path()).unwrap().unwrap();
    assert!(current.pending);
    assert_eq!(current.generation, generation);
    drop(held);
    let before = broker_epoch(config.path(), &change_lock(config.path()).unwrap()).unwrap();
    // Model a crash after publishing intent and before obtaining policy authority.
    write_marker(
        config.path(),
        &Marker {
            generation: fresh().unwrap(),
            pending: true,
        },
    )
    .unwrap();
    repair_orphan(config.path()).unwrap();
    assert_ne!(
        broker_epoch(config.path(), &change_lock(config.path()).unwrap()).unwrap(),
        before
    );
    assert!(!marker(config.path()).unwrap().unwrap().pending);
}
#[test]
fn malformed_epochs_and_markers_deny_sessions() {
    let config = tempfile::tempdir().unwrap();
    let lock = change_lock(config.path()).unwrap();
    (&lock).write_all(&[1]).unwrap();
    assert!(broker_epoch(config.path(), &lock).is_err());
    advance_one_time_epoch(&lock).unwrap();
    for content in [
        r#"{"generation":[],"pending":false}"#,
        r#"{"generation":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"pending":false,"extra":true}"#,
        r#"{"pending":false,"pending":true}"#,
    ] {
        fs::write(config.path().join(STOP_FILE), content).unwrap();
        assert!(broker_epoch(config.path(), &lock).is_err());
        assert!(repair_orphan(config.path()).is_err());
    }
}
#[cfg(unix)]
#[test]
fn cancellation_controls_reject_links_and_shared_write_access() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let config = tempfile::tempdir().unwrap();
    let lock = change_lock(config.path()).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    symlink(outside.path(), config.path().join(STOP_FILE)).unwrap();
    assert!(broker_epoch(config.path(), &lock).is_err());
    fs::remove_file(config.path().join(STOP_FILE)).unwrap();
    write_marker(
        config.path(),
        &Marker {
            generation: fresh().unwrap(),
            pending: false,
        },
    )
    .unwrap();
    fs::set_permissions(
        config.path().join(STOP_FILE),
        fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    assert!(broker_epoch(config.path(), &lock).is_err());
    fs::remove_file(config.path().join(STOP_FILE)).unwrap();
    fs::hard_link(outside.path(), config.path().join(STOP_LOCK)).unwrap();
    assert!(mutation_lock(config.path()).is_err());
}
