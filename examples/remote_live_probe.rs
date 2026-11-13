//! Integration driver run by scripts/test-remote-live.py against disposable SSH.
use hedit::{
    document::Document,
    remote::{Config, Connection},
};
use hedit_core::backend::{Backend, Call, FsQuery, PtyEvent};
use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};
fn main() {
    if let Err(e) = run() {
        eprintln!("REMOTE PROBE FAILED: {e}");
        std::process::exit(1)
    }
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if ![5, 6].contains(&args.len()) {
        return Err("Expected SSH config, helper, state, root".into());
    }
    let remote = Connection::connect(Config {
        target: hedit_remote::client::SshTarget {
            host: "lab".into(),
            user: None,
            port: args[4].parse().map_err(|_| "Invalid port")?,
            config: Some(PathBuf::from(&args[0])),
            helper: args[1].clone(),
            state: args[2].clone(),
        },
        root: args[3].clone(),
        takeover: false,
    })?;
    let path = remote.path("file")?;
    let mut doc = remote.open(1, &path)?;
    assert!(doc.is_ephemeral());
    assert!(doc.info().name == "file");
    let original = doc.text(10 * 1024 * 1024)?;
    if args.get(5).is_some_and(|mode| mode == "recover_pending") {
        doc.replace(0, 0, "PENDING_ONCE".into(), doc.info().revision)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while remote.status().0 {
            if Instant::now() > deadline {
                return Err("Test transport was not interrupted.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(remote.status().2, 1);
        remote.reconnect()?;
        remote.flush()?;
        let recovered = remote.open(2, &path)?.text(10 * 1024 * 1024)?;
        assert_eq!(recovered, format!("PENDING_ONCE{original}"));
        doc.save(None)?;
        println!("PENDING_RECONCILIATION_OK");
        return Ok(());
    }

    for _ in 0..24 {
        doc.replace(0, 0, "ب🦀".into(), doc.info().revision)?;
        std::thread::sleep(Duration::from_millis(2));
    }
    remote.flush()?;
    let text = doc.text(10 * 1024 * 1024)?;
    assert_eq!(text, format!("{}{}", "ب🦀".repeat(24), original));
    doc.save(None)?;
    assert!(!doc.info().dirty);
    doc.history(false, 0, doc.info().revision)?;
    assert!(doc.info().dirty);
    remote.flush()?;
    doc.history(true, 0, doc.info().revision)?;
    assert!(!doc.info().dirty);
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let editor = hedit::Editor::default();
    let page = doc.page(0)?;
    let buffer = hedit::workspace::Buffer::new(page, None);
    editor.insert(doc)?;
    let view = hedit::session::SessionView {
        open: vec![buffer.view],
        closed: vec![],
        active: Some(1),
        secondary: None,
        folder: None,
        folders: vec![],
    };
    assert!(hedit::session::SessionSnapshot::capture(&editor, view).is_err());
    assert!(std::fs::read_dir(tmp.path()).unwrap().next().is_none());
    assert!(Document::open(9, &path).is_err());
    let listing = hedit::folder::read_directory(remote.root())?;
    assert!(listing
        .entries
        .iter()
        .any(|e| e.path == path.to_string_lossy()));
    let created = hedit::folder::create_file(
        remote.root().to_string_lossy().into_owned(),
        "percent%41 space.rs".into(),
    )?;
    let mut newdoc = remote.open(2, Path::new(&created))?;
    assert_eq!(newdoc.info().name, "percent%41 space.rs");
    newdoc.replace(0, 0, "needle remote\n".into(), 0)?;
    remote.flush()?;
    newdoc.save(None)?;
    let results = hedit::search::folders(
        &[remote.root()],
        "",
        "needle",
        None,
        &Default::default(),
        &Default::default(),
        &AtomicBool::new(false),
    )?;
    assert_eq!(results.hits.len(), 1);
    assert_eq!(results.hits[0].path, PathBuf::from(&created));
    let big = remote.path("large.txt")?;
    let large = remote.open(50, &big)?;
    assert_eq!(large.info().size, 2 * 1024 * 1024);
    assert_eq!(large.text(4 * 1024 * 1024)?.len(), 2 * 1024 * 1024);
    let overlay = std::collections::HashMap::from([(
        big.clone(),
        format!("BULK_OVERLAY_CANARY\n{}", "x".repeat(2 * 1024 * 1024)),
    )]);
    let results: hedit::search::Results = serde_json::from_value(remote.request(
        Call::Search {
            roots: vec![remote.root()],
            filter: String::new(),
            query: "BULK_OVERLAY_CANARY".into(),
            replacement: None,
            options: Default::default(),
            open: overlay,
        },
        &AtomicBool::new(false),
    )?)
    .map_err(|e| e.to_string())?;
    assert_eq!(results.hits.len(), 1);
    assert_eq!(results.hits[0].path, big);
    let mut stale = remote.open(51, &path)?;
    {
        let shared = editor.get(1)?;
        let mut doc = shared.lock().map_err(|e| e.to_string())?;
        let revision = doc.info().revision;
        doc.replace(0, 0, "temporary".into(), revision)?;
    }
    remote.flush()?;
    let revision = stale.info().revision;
    assert!(stale
        .replace(0, 0, "STALE MUST FAIL".into(), revision)
        .is_err());
    {
        let shared = editor.get(1)?;
        let mut doc = shared.lock().map_err(|e| e.to_string())?;
        let revision = doc.info().revision;
        doc.history(false, 0, revision)?;
    }
    remote.flush()?;
    let git = hedit::git::Git {
        cancel: Arc::new(AtomicBool::new(false)),
    };
    assert!(git.snapshot(&remote.root(), 100).is_err());
    remote.trust(true)?;
    let root = git.discover(&remote.root())?;
    assert_eq!(root, remote.root());
    let snapshot = git.snapshot(&root, 100)?;
    assert_eq!(snapshot.root, root);
    git.execute(
        &root,
        &hedit::git::Operation::Stage(vec!["file".into(), "percent%41 space.rs".into()]),
    )?;
    git.execute(
        &root,
        &hedit::git::Operation::Commit {
            message: "Remote integration".into(),
            amend: false,
        },
    )?;
    let run = hedit::build::Running::start("printf BUILD_ON_HOST; pwd", &remote.root(), || {})?;
    let mut out = String::new();
    loop {
        match run
            .events
            .recv_timeout(Duration::from_secs(20))
            .map_err(|e| e.to_string())?
        {
            hedit::build::Event::Output(s) => out.push_str(&s),
            hedit::build::Event::Finished(code, _) => {
                assert_eq!(code, Some(0));
                break;
            }
        }
    }
    assert!(out.contains("BUILD_ON_HOST"));
    assert!(out.contains(&args[3]));
    let terminal = remote.terminal(&remote.root())?;
    terminal
        .input
        .send(b"printf 'PTY_TAIL_CANARY\\n'; exit\n".to_vec())
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if Instant::now() > deadline {
            return Err("PTY timed out".into());
        }
        match terminal
            .output
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?
        {
            PtyEvent::Bytes(b) => bytes.extend(b),
            PtyEvent::Exit => break,
            PtyEvent::Error(e) => return Err(e),
        }
    }
    assert!(String::from_utf8_lossy(&bytes).contains("PTY_TAIL_CANARY"));
    let mut scratch = remote.scratch(3)?;
    scratch.replace(0, 0, "SCRATCH_HOST_ONLY".into(), 0)?;
    remote.flush()?;
    let target = remote.path("scratch-saved.txt")?;
    scratch.save(Some(target))?;
    remote.reconnect()?;
    assert!(!remote.status().1);
    assert!(git.snapshot(&root, 10).is_err());
    let memory = remote.open(4, &path)?.text(10 * 1024 * 1024)?;
    assert_eq!(memory, text);
    assert!(remote
        .request(
            Call::File {
                path: remote.root().join("%2e%2e/escape"),
                query: FsQuery::Entry
            },
            &AtomicBool::new(false)
        )
        .is_err());
    let metadata = serde_json::json!({"bulk_view_state":"x".repeat(100_000)});
    assert_eq!(remote.metadata(Some(metadata.clone()))?, metadata);
    let expected = "x".repeat(2 * 1024 * 1024);
    let reloaded = remote.reload(&big, false, Some(&expected), 0)?;
    assert_eq!(reloaded.text, expected);
    println!("EDITOR_REMOTE_OK: edit/save/undo/redo/privacy/files/search/Git/build/PTY/scratch/reconnect");
    Ok(())
}
