#![cfg(unix)]
use hedit_remote::{
    host,
    journal::Journal,
    protocol::{Action, Error, MAX_DOCUMENT, MAX_INSERT},
};
use std::{
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{symlink, PermissionsExt},
    },
    path::PathBuf,
};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let root = base.join("project");
        fs::create_dir(&root).unwrap();
        Self {
            _temp: temp,
            root,
            state: base.join("state"),
        }
    }
    fn open(&self, text: &str) -> (Journal, u64) {
        fs::write(self.root.join("file"), text).unwrap();
        let mut j = Journal::open(&self.state).unwrap();
        j.workspace(self.root.as_os_str().as_bytes()).unwrap();
        let id = j.open_document(b"file").unwrap().document;
        (j, id)
    }
}
fn contents(j: &mut Journal, id: u64) -> String {
    let mut text = String::new();
    loop {
        let (_, page, eof) = j.read(id, text.len() as u64).unwrap();
        text.push_str(&page);
        if eof {
            return text;
        }
    }
}
fn insert(text: &str) -> Action {
    Action::Edit {
        start: 0,
        end: 0,
        text: text.into(),
    }
}

#[test]
fn recovery_and_undo_survive_missing_source_and_all_client_state() {
    let f = Fixture::new();
    let (mut j, id) = f.open("a\r\nب🦀z");
    j.apply(
        id,
        1,
        0,
        &Action::Edit {
            start: 3,
            end: 5,
            text: "שלום".into(),
        },
    )
    .unwrap();
    let edited = contents(&mut j, id);
    drop(j);
    fs::remove_dir_all(&f.root).unwrap();
    let mut j = Journal::open(&f.state).unwrap();
    assert_eq!(j.list().unwrap()[0].document, id);
    assert_eq!(contents(&mut j, id), edited);
    j.apply(id, 2, 1, &Action::Undo {}).unwrap();
    assert_eq!(contents(&mut j, id), "a\r\nب🦀z");
    drop(j);
    let mut j = Journal::open(&f.state).unwrap();
    j.apply(id, 3, 2, &Action::Redo {}).unwrap();
    assert_eq!(contents(&mut j, id), edited);
}

#[test]
fn lost_ack_retry_is_deduplicated_and_changed_payload_is_rejected() {
    let f = Fixture::new();
    let (mut j, id) = f.open("base");
    let info = j.apply(id, 1, 0, &insert("once")).unwrap();
    drop(j);
    let mut j = Journal::open(&f.state).unwrap();
    assert_eq!(j.apply(id, 1, 0, &insert("once")).unwrap(), info);
    assert_eq!(contents(&mut j, id), "oncebase");
    assert_eq!(j.apply(id, 1, 0, &insert("twice")), Err(Error::Conflict));
    assert_eq!(j.apply(id, 2, 0, &insert("stale")), Err(Error::Conflict));
    assert_eq!(j.apply(id, 3, 1, &insert("gap")), Err(Error::Conflict));
    assert_eq!(contents(&mut j, id), "oncebase");
}

#[test]
fn second_journal_cannot_write_and_epoch_survives_restart() {
    let f = Fixture::new();
    let (mut j, _) = f.open("");
    let epoch = j.acquire().unwrap();
    assert!(matches!(Journal::open(&f.state), Err(Error::Busy)));
    drop(j);
    let mut j = Journal::open(&f.state).unwrap();
    assert_eq!(j.acquire().unwrap(), epoch + 1);
}

#[test]
fn invalid_ranges_and_size_limits_leave_the_document_unchanged() {
    let f = Fixture::new();
    let (mut j, id) = f.open("🦀");
    for action in [
        Action::Edit {
            start: 2,
            end: 2,
            text: "x".into(),
        },
        Action::Edit {
            start: 0,
            end: 0,
            text: "x".repeat(MAX_INSERT + 1),
        },
        Action::Undo {},
        Action::Redo {},
    ] {
        assert!(j.apply(id, 1, 0, &action).is_err());
        assert_eq!(contents(&mut j, id), "🦀");
        assert_eq!(j.info(id).unwrap().revision, 0);
    }
    assert!(j.apply(u64::MAX, 1, 0, &insert("x")).is_err());
    assert!(j.apply(id, u64::MAX, 0, &insert("x")).is_err());
    fs::write(f.root.join("huge"), vec![b'x'; MAX_DOCUMENT + 1]).unwrap();
    assert_eq!(j.open_document(b"huge"), Err(Error::Limit));
}

#[test]
fn paging_control_characters_and_unicode_respects_frame_boundaries() {
    let f = Fixture::new();
    let baseline = "\u{0001}\r\nשלום 🦀".repeat(1500);
    let (mut j, id) = f.open(&baseline);
    assert_eq!(contents(&mut j, id), baseline);
    assert_eq!(j.read(id, 4).err(), Some(Error::Conflict)); // Inside Hebrew UTF-8.
    assert!(j.read(id, u64::MAX).is_err());
}

#[test]
fn paths_cannot_escape_root_follow_links_or_block_on_fifos() {
    let f = Fixture::new();
    let (mut j, _) = f.open("safe");
    fs::write(f.root.parent().unwrap().join("outside"), "secret").unwrap();
    symlink("../outside", f.root.join("link")).unwrap();
    symlink(f.root.parent().unwrap(), f.root.join("directory-link")).unwrap();
    assert!(std::process::Command::new("mkfifo")
        .arg(f.root.join("fifo"))
        .status()
        .unwrap()
        .success());
    for path in [
        b"../outside".as_slice(),
        b"/etc/passwd",
        b"link",
        b"directory-link/outside",
        b"fifo",
        b"file\0other",
    ] {
        assert!(j.open_document(path).is_err(), "{path:?}");
    }
    assert_eq!(j.list().unwrap().len(), 1);
}

#[test]
fn recovery_directory_database_and_sidecar_symlinks_are_rejected() {
    let f = Fixture::new();
    let target = f.root.join("target");
    fs::create_dir(&target).unwrap();
    symlink(&target, &f.state).unwrap();
    assert!(Journal::open(&f.state).is_err());
    fs::remove_file(&f.state).unwrap();
    fs::create_dir(&f.state).unwrap();
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(f.root.join("untouched"), "sentinel").unwrap();
    for name in [
        "recovery.sqlite3",
        "recovery.sqlite3-wal",
        "recovery.sqlite3-shm",
        "recovery.sqlite3-journal",
    ] {
        symlink(f.root.join("untouched"), f.state.join(name)).unwrap();
        assert!(Journal::open(&f.state).is_err(), "{name}");
        fs::remove_file(f.state.join(name)).unwrap();
    }
    assert_eq!(
        fs::read_to_string(f.root.join("untouched")).unwrap(),
        "sentinel"
    );
}

#[test]
fn insecure_permissions_and_root_replacement_are_rejected() {
    let f = Fixture::new();
    let (j, _) = f.open("text");
    drop(j);
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(Journal::open(&f.state), Err(Error::Permission)));
    fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
    fs::rename(&f.root, f.root.with_file_name("old")).unwrap();
    fs::create_dir(&f.root).unwrap();
    let mut j = Journal::open(&f.state).unwrap();
    assert_eq!(
        j.workspace(f.root.as_os_str().as_bytes()),
        Err(Error::Conflict)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn non_utf8_filenames_are_literal_remote_bytes() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let f = Fixture::new();
    let (mut j, _) = f.open("text");
    let name = b"odd-\xff\n--file";
    fs::write(f.root.join(OsString::from_vec(name.to_vec())), "unicode 🦀").unwrap();
    let id = j.open_document(name).unwrap().document;
    assert_eq!(contents(&mut j, id), "unicode 🦀");
}

#[test]
fn randomized_edits_match_character_model_and_recover_every_commit() {
    let f = Fixture::new();
    let (mut j, id) = f.open("initial\r\n🦀");
    let mut model: Vec<char> = "initial\r\n🦀".chars().collect();
    let mut seed = 71_u64;
    for sequence in 1..=200 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let start = (seed as usize) % (model.len() + 1);
        let end = (start + ((seed >> 32) as usize) % 4).min(model.len());
        let replacement = match sequence % 4 {
            0 => "🦀",
            1 => "ب\r\n",
            2 => "שלום",
            _ => "",
        };
        model.splice(start..end, replacement.chars());
        j.apply(
            id,
            sequence,
            sequence - 1,
            &Action::Edit {
                start: start as u64,
                end: end as u64,
                text: replacement.into(),
            },
        )
        .unwrap();
        assert_eq!(contents(&mut j, id), model.iter().collect::<String>());
        if sequence % 17 == 0 {
            drop(j);
            j = Journal::open(&f.state).unwrap();
        }
    }
    drop(j);
    fs::remove_file(f.root.join("file")).unwrap();
    let mut j = Journal::open(&f.state).unwrap();
    assert_eq!(contents(&mut j, id), model.iter().collect::<String>());
    assert_eq!(j.info(id).unwrap().revision, 200);
}

#[test]
fn private_directory_has_no_group_or_other_access() {
    let f = Fixture::new();
    let (j, _) = f.open("private");
    use std::os::unix::fs::MetadataExt;
    for entry in fs::read_dir(&f.state).unwrap() {
        assert_eq!(entry.unwrap().metadata().unwrap().mode() & 0o077, 0);
    }
    assert_eq!(fs::metadata(&f.state).unwrap().mode() & 0o077, 0);
    drop(j);
    assert!(host::private_directory(&f.state).is_ok());
}

#[test]
fn undo_restores_a_large_deletion_after_restart_without_exceeding_insert_policy() {
    let f = Fixture::new();
    let original = "🦀".repeat(MAX_DOCUMENT / 4);
    let (mut j, id) = f.open(&original);
    j.apply(
        id,
        1,
        0,
        &Action::Edit {
            start: 0,
            end: (MAX_DOCUMENT / 4) as u64,
            text: String::new(),
        },
    )
    .unwrap();
    assert_eq!(contents(&mut j, id), "");
    drop(j);
    fs::remove_file(f.root.join("file")).unwrap();
    let mut j = Journal::open(&f.state).unwrap();
    j.apply(id, 2, 1, &Action::Undo {}).unwrap();
    assert_eq!(contents(&mut j, id), original);
}

#[test]
fn source_saves_preserve_permissions_conflicts_and_undo_across_restart() {
    let f = Fixture::new();
    let (mut j, id) = f.open("base\n");
    fs::set_permissions(f.root.join("file"), fs::Permissions::from_mode(0o640)).unwrap();
    j.apply(id, 1, 0, &insert("edited ")).unwrap();
    let saved = j.save_document(id, 1, &"a".repeat(64), None).unwrap();
    assert!(!saved.dirty);
    assert_eq!(
        fs::read_to_string(f.root.join("file")).unwrap(),
        "edited base\n"
    );
    assert_eq!(
        fs::metadata(f.root.join("file"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o640
    );
    j.apply(id, 2, 1, &Action::Undo {}).unwrap();
    assert!(j.info(id).unwrap().dirty);
    j.apply(id, 3, 2, &Action::Redo {}).unwrap();
    assert!(!j.info(id).unwrap().dirty);
    drop(j);
    let mut j = Journal::open(&f.state).unwrap();
    j.workspace(f.root.as_os_str().as_bytes()).unwrap();
    assert_eq!(contents(&mut j, id), "edited base\n");
    assert!(!j.info(id).unwrap().dirty);
    fs::write(f.root.join("file"), "external writer").unwrap();
    assert!(j.changed(id).unwrap());
    j.apply(id, 4, 3, &insert("pending ")).unwrap();
    assert_eq!(
        j.save_document(id, 4, &"b".repeat(64), None),
        Err(Error::Conflict)
    );
    assert_eq!(
        fs::read_to_string(f.root.join("file")).unwrap(),
        "external writer"
    );
    assert_eq!(
        j.reload_document(id, true, Some("wrong preview")),
        Err(Error::Conflict)
    );
    assert!(contents(&mut j, id).starts_with("pending "));
}

#[test]
fn compaction_recovers_text_recent_receipts_and_bounded_history() {
    let f = Fixture::new();
    let (mut j, id) = f.open("base");
    for sequence in 1..=600 {
        j.apply(id, sequence, sequence - 1, &insert("x")).unwrap();
    }
    j.compact(id).unwrap();
    drop(j);
    let mut j = Journal::open(&f.state).unwrap();
    assert_eq!(contents(&mut j, id), format!("{}base", "x".repeat(600)));
    j.apply(id, 600, 599, &insert("x")).unwrap();
    assert_eq!(j.info(id).unwrap().revision, 600);
    assert_eq!(j.apply(id, 1, 0, &insert("x")), Err(Error::Conflict));
    j.apply(id, 601, 600, &Action::Undo {}).unwrap();
    assert_eq!(contents(&mut j, id), format!("{}base", "x".repeat(599)));
}

#[test]
fn scratchpads_and_rename_intents_recover_without_original_paths() {
    let f = Fixture::new();
    let (mut j, id) = f.open("base");
    let scratch = j.scratch().unwrap();
    j.apply(scratch.document, 1, 0, &insert("HOST ONLY"))
        .unwrap();
    let operation = j.prepare_move(b"file", b"renamed").unwrap();
    fs::rename(f.root.join("file"), f.root.join("renamed")).unwrap();
    drop(j);
    let mut j = Journal::open(&f.state).unwrap();
    j.workspace(f.root.as_os_str().as_bytes()).unwrap();
    assert_eq!(j.info(id).unwrap().path, b"renamed");
    assert_eq!(contents(&mut j, scratch.document), "HOST ONLY");
    // An already reconciled move is never repeated.
    assert_eq!(j.finish_move(&operation), Err(Error::Missing));
    j.save_document(scratch.document, 1, &"c".repeat(64), Some(b"scratch.txt"))
        .unwrap();
    assert_eq!(
        fs::read_to_string(f.root.join("scratch.txt")).unwrap(),
        "HOST ONLY"
    );
}

#[test]
fn save_as_refuses_existing_files_and_symlink_destinations() {
    let f = Fixture::new();
    let (mut j, id) = f.open("base");
    fs::write(f.root.join("other"), "sentinel").unwrap();
    symlink("other", f.root.join("link")).unwrap();
    for (name, operation) in [
        (b"other".as_slice(), "d".repeat(64)),
        (b"link".as_slice(), "e".repeat(64)),
    ] {
        assert!(j.save_document(id, 0, &operation, Some(name)).is_err());
    }
    assert_eq!(
        fs::read_to_string(f.root.join("other")).unwrap(),
        "sentinel"
    );
    assert_eq!(contents(&mut j, id), "base");
}

#[test]
#[cfg(target_os = "macos")]
fn recovery_acl_grants_are_rejected_even_with_private_mode_bits() {
    for kind in 0..3 {
        let f = Fixture::new();
        let (j, _) = f.open("private");
        drop(j);
        let target = match kind {
            0 => f.state.clone(),
            1 => f.state.join("recovery.sqlite3"),
            _ => f.state.parent().unwrap().to_owned(),
        };
        fs::set_permissions(
            &target,
            fs::Permissions::from_mode(if target.is_dir() { 0o700 } else { 0o600 }),
        )
        .unwrap();
        let status = std::process::Command::new("/bin/chmod")
            .args([
                "+a",
                "everyone allow read,readattr,readextattr,readsecurity",
            ])
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o077,
            0
        );
        assert!(matches!(Journal::open(&f.state), Err(Error::Permission)));
    }
}
