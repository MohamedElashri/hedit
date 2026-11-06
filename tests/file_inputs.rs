#[cfg(unix)]
use hedit::document::Document;
use hedit::projects;
use std::fs;
#[cfg(unix)]
use std::path::Path;

#[test]
fn configuration_reads_enforce_byte_limits() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    fs::write(&path, "{\"text\":\"λ\"}").unwrap();
    let bytes = fs::metadata(&path).unwrap().len();
    assert!(projects::read_json::<serde_json::Value>(&path, bytes).is_ok());
    assert!(projects::read_json::<serde_json::Value>(&path, bytes - 1).is_err());
    assert!(projects::read_text(&path, bytes).is_ok());
    assert!(projects::read_text(&path, bytes - 1).is_err());
}

#[test]
fn opening_and_saving_each_supported_encoding_keeps_all_initial_bytes() {
    let directory = tempfile::tempdir().unwrap();
    for (index, label) in ["UTF-8", "UTF-16LE", "UTF-16BE", "windows-1252"]
        .into_iter()
        .enumerate()
    {
        for bom in [false, true] {
            if label == "windows-1252" && bom {
                continue;
            }
            let path = directory.path().join(format!("{index}-{bom}.txt"));
            let text = "café €\r\nsecond line\r\n";
            let bytes = hedit::text_encoding::encode(text, label, bom).unwrap();
            fs::write(&path, &bytes).unwrap();
            let mut document = if label == "windows-1252" || (label.starts_with("UTF-16") && !bom) {
                hedit::document::Document::open_with_encoding(1, &path, label).unwrap()
            } else {
                hedit::document::Document::open(1, &path).unwrap()
            };
            assert_eq!(document.text(1024).unwrap(), text, "{label}, BOM={bom}");
            document
                .replace(0, 0, "first\r\n".into(), document.info().revision)
                .unwrap();
            document.save(None).unwrap();
            assert_eq!(
                fs::read(&path).unwrap(),
                hedit::text_encoding::encode(&format!("first\r\n{text}"), label, bom).unwrap()
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn document_symlinks_still_save_to_the_target() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("target.txt");
    let link = directory.path().join("linked.txt");
    fs::write(&path, "before\n").unwrap();
    std::os::unix::fs::symlink(&path, &link).unwrap();
    let mut document = Document::open(1, &link).unwrap();
    document.replace(0, 6, "after".into(), 0).unwrap();
    document.save(None).unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&path).unwrap(), "after\n");
}

#[cfg(unix)]
fn fifo_fixture(read: impl FnOnce(&Path) -> bool + Send + 'static, document: bool) {
    use std::{
        io::Write, os::unix::fs::OpenOptionsExt, process::Command, sync::mpsc, thread,
        time::Duration,
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("input");
    assert!(Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap()
        .success());
    let input = path.clone();
    let (tx, rx) = mpsc::channel();
    let worker = thread::spawn(move || tx.send(read(&input)).unwrap());
    let completed = rx.recv_timeout(Duration::from_secs(2));
    // Release an unfixed reader before asserting. The document reader opens
    // twice, so keep its writer open until both opens have finished.
    let mut writer = fs::OpenOptions::new()
        .write(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(&path)
        .ok();
    if let Some(writer) = &mut writer {
        let _ = writer.write_all(b"{}\n");
    }
    if !document {
        drop(writer.take());
    }
    worker.join().unwrap();
    assert!(completed.expect("file input blocked waiting for a FIFO writer"));
}

#[cfg(unix)]
#[test]
fn configuration_fifo_is_rejected_without_waiting() {
    fifo_fixture(|path| projects::read_text(path, 1024).is_err(), false);
}

#[cfg(unix)]
#[test]
fn project_workspace_fifo_is_rejected_without_waiting() {
    fifo_fixture(
        |path| {
            let project = path.with_extension("hedit-project");
            fs::write(&project, "{}").unwrap();
            std::os::unix::fs::symlink(path, project.with_extension("hedit-workspace")).unwrap();
            projects::load(&project).is_err()
        },
        false,
    );
}

#[cfg(unix)]
#[test]
fn editorconfig_fifo_is_rejected_without_waiting() {
    fifo_fixture(
        |path| {
            let directory = path.parent().unwrap();
            std::os::unix::fs::symlink(path, directory.join(".editorconfig")).unwrap();
            projects::formatting(
                &directory.join("notes.txt"),
                "plain",
                &projects::Defaults::default(),
                None,
            )
            .is_err()
        },
        false,
    );
}

#[cfg(unix)]
#[test]
fn document_fifo_is_rejected_without_waiting() {
    fifo_fixture(|path| Document::open(1, path).is_err(), true);
}

#[cfg(unix)]
#[test]
fn recovery_fifo_reports_an_error_without_blocking_startup() {
    fifo_fixture(
        |path| {
            use hedit::workspace::Workspace;
            let session = path.parent().unwrap().join("session");
            let (mut workspace, _) = Workspace::restore(session.clone()).unwrap();
            workspace.open[0].page.text = "unsaved note\n".into();
            workspace.edit(0, "").unwrap();
            workspace.checkpoint().unwrap();
            drop(workspace);
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(session.join("session.json")).unwrap()).unwrap();
            let buffer = session.join(manifest["open"][0]["document"]["buffer"].as_str().unwrap());
            fs::remove_file(&buffer).unwrap();
            std::os::unix::fs::symlink(path, &buffer).unwrap();
            let (_, errors) = Workspace::restore(session).unwrap();
            !errors.is_empty()
        },
        true,
    );
}

#[cfg(unix)]
#[test]
fn configuration_symlinks_to_regular_files_still_work() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    fs::write(&path, "{}").unwrap();
    let link = directory.path().join("linked.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(projects::read_json::<serde_json::Value>(&link, 1024).is_ok());
}

#[cfg(target_os = "linux")]
#[test]
fn configuration_limit_applies_even_when_metadata_reports_zero_bytes() {
    let path = Path::new("/proc/self/cmdline");
    assert_eq!(fs::metadata(path).unwrap().len(), 0);
    assert!(projects::read_text(path, 0).is_err());
}
