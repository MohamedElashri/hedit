use super::*;
use crate::{
    plan::Change,
    workspace::{Policy, Protected, Right},
};
use hedit_plugin::sdk::Edit;
use std::{sync::atomic::AtomicBool, time::Duration};
fn session(path: &std::path::Path) -> (Resources, crate::recovery_store::RecoveryStore) {
    let root = Dir::open_ambient_dir(path, cap_std::ambient_authority()).unwrap();
    let config = native::private_directory(&root, ".test-config").unwrap();
    let protected = Protected::persistent(&config, []).unwrap();
    let store_dir = native::private_directory(&config, "recovery").unwrap();
    let store = crate::recovery_store::RecoveryStore::open(store_dir, &protected).unwrap();
    let resources = Resources::new(
        workspace::Root::open_selected(
            path,
            Policy::new([Right::Read, Right::Edit, Right::Discover]).unwrap(),
            protected,
        )
        .unwrap(),
    )
    .unwrap();
    (resources, store)
}
fn budget(cancel: &AtomicBool) -> Budget<'_> {
    Budget::new(cancel, Duration::from_secs(5), 32 * 1024 * 1024)
}
fn prepare(resources: &mut Resources, cancel: &AtomicBool) -> Plan {
    let scope = resources.scope();
    let mut changes = Vec::new();
    for name in ["first.txt", "second.txt"] {
        let version = resources.load(scope, name, &mut budget(cancel)).unwrap();
        changes.push(Change {
            resource: version.resource,
            version: version.version,
            edits: vec![Edit {
                start: 0,
                finish: 3,
                text: "new".into(),
            }],
        });
    }
    Plan::prepare(resources, scope, changes, &mut budget(cancel)).unwrap()
}
fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["first.txt", "second.txt"] {
        std::fs::write(dir.path().join(name), "old café / 東京\r\n").unwrap();
    }
    dir
}
#[test]
fn reviewed_plan_commits_and_explicitly_restores_without_losing_unicode_or_permissions() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    assert_eq!(
        root.discover("", &mut budget(&cancel))
            .unwrap()
            .entries
            .len(),
        2
    );
    assert_eq!(
        journal
            .apply(root, &mut budget(&cancel), || Ok(()))
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "new café / 東京\r\n"
    );
    let reopened = Journal::open(root, journal.name(), &store).unwrap();
    assert_eq!(
        reopened
            .apply(root, &mut budget(&cancel), || Ok(()))
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        reopened
            .restore(root, &mut budget(&cancel), || Ok(()))
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "old café / 東京\r\n"
    );
    assert_eq!(
        reopened
            .restore(root, &mut budget(&cancel), || Ok(()))
            .unwrap()
            .len(),
        2
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(directory.path().join("first.txt"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
    }
}
#[test]
fn revoked_partial_apply_recovery_and_competing_writes_preserve_both_versions() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    let mut calls = 0;
    assert!(journal
        .apply(root, &mut budget(&cancel), || {
            calls += 1;
            if calls == 2 {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "revoked"))
            } else {
                Ok(())
            }
        })
        .is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "new café / 東京\r\n"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("second.txt")).unwrap(),
        "old café / 東京\r\n"
    );
    std::fs::write(directory.path().join("first.txt"), "competing writer").unwrap();
    assert!(journal
        .restore(root, &mut budget(&cancel), || Ok(()))
        .is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "competing writer"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join(journal.name()).join("0.old")).unwrap(),
        "old café / 東京\r\n"
    );
}
#[test]
fn interrupted_capture_and_unrecorded_publication_are_reconciled_by_identity_and_hash() {
    for installed in [false, true] {
        let directory = fixture();
        let (mut resources, store) = session(directory.path());
        let cancel = AtomicBool::new(false);
        let plan = prepare(&mut resources, &cancel);
        let root = resources.root(plan.scope()).unwrap();
        let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
        let (parent, name) = root.parent("first.txt").unwrap();
        let old = native::writable(&parent, &name).unwrap();
        native::rename(&parent, &name, Some(&old), &journal.directory, "0.old").unwrap();
        drop(old);
        if installed {
            let new = native::writable(&journal.directory, "0.new").unwrap();
            native::rename(&journal.directory, "0.new", Some(&new), &parent, &name).unwrap();
        }
        let reopened = Journal::open(root, journal.name(), &store).unwrap();
        reopened
            .apply(root, &mut budget(&cancel), || Ok(()))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
            "new café / 東京\r\n"
        );
        reopened
            .restore(root, &mut budget(&cancel), || Ok(()))
            .unwrap();
    }
}
#[test]
fn concurrent_creators_and_replaced_roots_never_get_overwritten() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    let (parent, name) = root.parent("first.txt").unwrap();
    let old = native::writable(&parent, &name).unwrap();
    native::rename(&parent, &name, Some(&old), &journal.directory, "0.old").unwrap();
    drop(old);
    std::fs::write(directory.path().join("first.txt"), "new creator").unwrap();
    assert!(journal
        .apply(root, &mut budget(&cancel), || Ok(()))
        .is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "new creator"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join(journal.name()).join("0.old")).unwrap(),
        "old café / 東京\r\n"
    );
    let foreign = fixture();
    let (other, _other_store) = session(foreign.path());
    assert!(journal
        .apply(
            other.root(other.scope()).unwrap(),
            &mut budget(&cancel),
            || Ok(())
        )
        .is_err());
}
#[cfg(unix)]
#[test]
fn protected_staging_identity_survives_renaming_and_parent_grants() {
    let directory = fixture();
    let (mut session, store) = session(directory.path());
    let protected = session.root(session.scope()).unwrap().protected().clone();
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut session, &cancel);
    let journal = Journal::prepare(&session, &plan, &mut budget(&cancel), &store).unwrap();
    std::fs::rename(
        directory.path().join(journal.name()),
        directory.path().join("visible-backups"),
    )
    .unwrap();
    let root = session.root(session.scope()).unwrap();
    assert!(root
        .read("visible-backups/0.new", &mut budget(&cancel))
        .is_err());
    assert!(root
        .discover("", &mut budget(&cancel))
        .unwrap()
        .entries
        .iter()
        .all(|entry| !entry.relative.starts_with("visible-backups")));
    assert!(workspace::Root::open_selected(
        &directory.path().join("visible-backups"),
        Policy::new([Right::Read]).unwrap(),
        protected
    )
    .is_err());
}
#[test]
fn recovery_reservations_survive_failures_and_explicit_discard_frees_storage() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    let name = journal.name().to_owned();
    assert_eq!(store.records().unwrap().len(), 1);
    journal
        .apply(root, &mut budget(&cancel), || Ok(()))
        .unwrap();
    drop(journal);
    let reopened = Journal::open(root, &name, &store).unwrap();
    reopened
        .discard(root, &store, &mut budget(&cancel))
        .unwrap();
    assert!(store.records().unwrap().is_empty());
    assert!(!directory.path().join(name).exists());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "new café / 東京\r\n"
    );
}
#[test]
fn discard_refuses_unknown_private_contents_and_preserves_the_reservation() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    let name = journal.name().to_owned();
    std::fs::write(
        directory.path().join(&name).join("unexpected.txt"),
        "preserve this private fixture",
    )
    .unwrap();
    assert!(journal.discard(root, &store, &mut budget(&cancel)).is_err());
    assert_eq!(store.records().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(directory.path().join(&name).join("unexpected.txt")).unwrap(),
        "preserve this private fixture"
    );
    assert!(directory.path().join(name).join("0.new").exists());
}

#[test]
fn work_limit_after_capture_restores_original_without_replacing_a_competitor() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    // The first read succeeds; hashing the captured entry exhausts the budget.
    let mut limited = Budget::new(&cancel, Duration::from_secs(5), "old café / 東京\r\n".len());
    assert!(journal.apply(root, &mut limited, || Ok(())).is_err());
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "old café / 東京\r\n"
    );
    assert!(!directory.path().join(journal.name()).join("0.old").exists());
    assert!(directory.path().join(journal.name()).join("0.new").exists());
}

#[test]
fn history_discard_refuses_missing_destinations_and_competing_contents() {
    for competitor in [false, true] {
        let directory = fixture();
        let (mut resources, store) = session(directory.path());
        let cancel = AtomicBool::new(false);
        let plan = prepare(&mut resources, &cancel);
        let root = resources.root(plan.scope()).unwrap();
        let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
        let name = journal.name().to_string();
        let (parent, file_name) = root.parent("first.txt").unwrap();
        let file = native::writable(&parent, &file_name).unwrap();
        native::rename(
            &parent,
            &file_name,
            Some(&file),
            &journal.directory,
            "0.old",
        )
        .unwrap();
        drop(file);
        if competitor {
            std::fs::write(directory.path().join("first.txt"), "competing writer").unwrap();
        }
        assert!(journal.discard(root, &store, &mut budget(&cancel)).is_err());
        assert_eq!(
            std::fs::read_to_string(directory.path().join(name).join("0.old")).unwrap(),
            "old café / 東京\r\n"
        );
        assert_eq!(store.records().unwrap().len(), 1);
    }
}

#[test]
fn interrupted_preparation_cleanup_is_explicit_and_never_deletes_captured_originals() {
    for bound in [false, true] {
        let directory = fixture();
        let (resources, store) = session(directory.path());
        let root = resources.root(resources.scope()).unwrap();
        let cancel = AtomicBool::new(false);
        let mut lease = store.reserve(root.identity(), 1024).unwrap();
        let id = lease.record().id;
        if bound {
            let stage =
                native::private_directory(root.directory(), &lease.record().stage_name).unwrap();
            let identity = root.protected().insert(stage.try_clone().unwrap()).unwrap();
            store.bind_stage(&mut lease, identity).unwrap();
            let mut file = native::new_file(&stage, "0.new").unwrap();
            file.write_all(b"unfinished replacement").unwrap();
            drop(file);
            // An original or a receipt means this is not merely preparation.
            let file = native::new_file(&stage, "0.old").unwrap();
            drop(file);
            assert!(Journal::discard_unprepared(root, id, &store, &mut budget(&cancel)).is_err());
            assert!(stage.open("0.new").is_ok());
            stage.remove_file("0.old").unwrap();
        }
        Journal::discard_unprepared(root, id, &store, &mut budget(&cancel)).unwrap();
        assert!(store.records().unwrap().is_empty());
        assert!(!directory.path().join(&lease.record().stage_name).exists());
        assert_eq!(
            std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
            "old café / 東京\r\n"
        );
    }
}

#[test]
fn a_replaced_or_moved_preparation_stage_cannot_release_its_retention_reservation() {
    let directory = fixture();
    let (resources, store) = session(directory.path());
    let root = resources.root(resources.scope()).unwrap();
    let cancel = AtomicBool::new(false);
    let mut lease = store.reserve(root.identity(), 1024).unwrap();
    let id = lease.record().id;
    let name = lease.record().stage_name.clone();
    let stage = native::private_directory(root.directory(), &name).unwrap();
    let identity = root.protected().insert(stage.try_clone().unwrap()).unwrap();
    store.bind_stage(&mut lease, identity).unwrap();
    drop(stage);
    root.directory()
        .rename(&name, root.directory(), "moved-stage")
        .unwrap();
    assert!(Journal::discard_unprepared(root, id, &store, &mut budget(&cancel)).is_err());
    let replacement = native::private_directory(root.directory(), &name).unwrap();
    assert!(Journal::discard_unprepared(root, id, &store, &mut budget(&cancel)).is_err());
    assert!(replacement.entries().unwrap().next().is_none());
    assert_eq!(store.records().unwrap().len(), 1);
}

#[test]
fn reopening_rejects_duplicate_identities_paths_unbounded_sizes_and_unknown_metadata() {
    for mutation in ["path", "identity", "volume", "size", "unknown", "quota"] {
        let directory = fixture();
        let (mut resources, store) = session(directory.path());
        let cancel = AtomicBool::new(false);
        let plan = prepare(&mut resources, &cancel);
        let root = resources.root(plan.scope()).unwrap();
        let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
        let mut value = serde_json::to_value(&journal.ledger).unwrap();
        match mutation {
            "path" => value["files"][1]["relative"] = value["files"][0]["relative"].clone(),
            "identity" => {
                value["files"][0]["new"]["identity"] = value["files"][0]["old"]["identity"].clone()
            }
            "volume" => {
                value["files"][0]["old"]["identity"]["volume"] =
                    (root.identity().volume.wrapping_add(1)).into()
            }
            "size" => {
                value["files"][0]["old"]["bytes"] = ((workspace::MAX_FILE_BYTES + 1) as u64).into()
            }
            "unknown" => value["files"][0]["old"]["owner_assertion"] = true.into(),
            "quota" => {
                let record = store.records().unwrap().pop().unwrap();
                let path = directory
                    .path()
                    .join(".test-config/recovery")
                    .join(format!("{}.lease", record.id));
                let mut record = serde_json::to_value(&record).unwrap();
                record["reserved"] = (1024 * 1024).into();
                std::fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        std::fs::write(
            directory.path().join(journal.name()).join("prepared.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(
            Journal::open(root, journal.name(), &store).is_err(),
            "{mutation}"
        );
        assert_eq!(
            std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
            "old café / 東京\r\n"
        );
    }
}

#[test]
fn corrupt_receipts_fail_closed_before_capture_or_preserve_recovery_after_publication() {
    for receipt in ["started", "applied"] {
        let directory = fixture();
        let (mut resources, store) = session(directory.path());
        let cancel = AtomicBool::new(false);
        let plan = prepare(&mut resources, &cancel);
        let root = resources.root(plan.scope()).unwrap();
        let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
        let mut file = native::new_file(&journal.directory, &format!("0.{receipt}")).unwrap();
        file.write_all(b"corrupt receipt").unwrap();
        drop(file);
        assert!(journal
            .apply(root, &mut budget(&cancel), || Ok(()))
            .is_err());
        let expected = if receipt == "started" {
            "old café / 東京\r\n"
        } else {
            "new café / 東京\r\n"
        };
        assert_eq!(
            std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
            expected
        );
        journal
            .restore(root, &mut budget(&cancel), || Ok(()))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
            "old café / 東京\r\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn capture_validation_uses_the_renamed_entry_and_preserves_a_racing_replacement() {
    let directory = fixture();
    let (mut resources, store) = session(directory.path());
    let cancel = AtomicBool::new(false);
    let plan = prepare(&mut resources, &cancel);
    let root = resources.root(plan.scope()).unwrap();
    let journal = Journal::prepare(&resources, &plan, &mut budget(&cancel), &store).unwrap();
    let (parent, name) = root.parent("first.txt").unwrap();
    let original = native::writable(&parent, &name).unwrap();
    std::fs::rename(
        directory.path().join("first.txt"),
        directory.path().join("original.txt"),
    )
    .unwrap();
    std::fs::write(directory.path().join("first.txt"), "racing writer").unwrap();
    native::rename(&parent, &name, Some(&original), &journal.directory, "0.old").unwrap();
    let capture = Capture {
        stage: &journal.directory,
        backup: "0.old",
        parent: &parent,
        name: &name,
        file: original,
        published: false,
    };
    let (_, hash) = capture.digest(&mut budget(&cancel)).unwrap();
    assert_eq!(hash, <[u8; 32]>::from(Sha256::digest(b"racing writer")));
    drop(capture);
    assert_eq!(
        std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
        "racing writer"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path().join("original.txt")).unwrap(),
        "old café / 東京\r\n"
    );
}
