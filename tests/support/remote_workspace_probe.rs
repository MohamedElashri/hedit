//! Production workspace regression driver for the disposable SSH harness.
use hedit::{
    document::Document,
    remote::{Config, Connection},
};
use hedit_core::backend::{Backend, Call, FsQuery, PtyEvent};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
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
        managed: false,
        takeover: false,
    })?;
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = wakes.clone();
    remote.set_wake(Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    }));
    let path = remote.path("file")?;
    let mut doc = remote.open(1, &path)?;
    assert!(doc.is_ephemeral());
    assert!(doc.info().name == "file");
    let original = doc.text(10 * 1024 * 1024)?;
    if args.get(5).is_some_and(|mode| mode == "language_servers") {
        use hedit::language_server::{
            manager::{Launch, Registration},
            Document as LspDocument, Event, Feature, Operation, Reply,
        };
        remote.trust(true)?;
        let service = remote.language_servers(Arc::new(|| {}));
        let collect = |ticket: u64| -> Result<Vec<Event>, String> {
            let start = Instant::now();
            let mut events = vec![];
            loop {
                let mut completed = false;
                for event in service.poll() {
                    match &event {
                        Event::Disconnected(e) => return Err(e.clone()),
                        Event::Result { ticket: t, result } if *t == ticket => {
                            result.as_ref().map_err(Clone::clone)?;
                            completed = true;
                        }
                        Event::Response { ticket: t, result } if *t == ticket => {
                            result.as_ref().map_err(Clone::clone)?;
                            completed = true;
                        }
                        _ => (),
                    }
                    events.push(event);
                }
                if completed {
                    return Ok(events);
                }
                if start.elapsed() > Duration::from_secs(15) {
                    return Err(format!("LSP bridge timed out: {events:?}"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let registration = Registration {
            languages: vec!["rust".into()],
            external: Some(Launch {
                executable: PathBuf::from("/usr/bin/python3"),
                arguments: vec![
                    format!("{}/fake-language-server.py", remote.config.root),
                    "--encoding".into(),
                    "utf-8".into(),
                ],
            }),
            ..Registration::default()
        };
        let mut registration = registration;
        registration.priority = 30;
        registration
            .environment
            .insert("HEDIT_LSP_TEST_FLAG".into(), "host-value".into());
        registration.root_markers = vec![".hedit-project".into()];
        registration.initialization_options = Some(serde_json::json!({"hostInit":true}));
        registration.arguments = registration.external.as_ref().map(|l| l.arguments.clone());
        let mut companion = registration.clone();
        companion.priority = -1;
        companion.features.hover = false;
        companion.features.completion = false;
        companion.features.definition = false;
        companion.features.references = false;
        service.submit(
            1,
            Operation::Configure {
                id: "fixture".into(),
                registration: Box::new(registration),
            },
        )?;
        collect(1)?;
        service.submit(
            1000,
            Operation::Configure {
                id: "companion".into(),
                registration: Box::new(companion),
            },
        )?;
        collect(1000)?;
        service.submit(1001, Operation::List)?;
        let registered = collect(1001)?;
        assert!(registered.iter().any(|e| matches!(e, Event::Result { result: Ok(value), .. } if value["installed"].as_array().is_some_and(|servers| servers.iter().any(|server| server["id"] == "fixture" && server["registration"]["priority"] == 30 && server["registration"]["environment"]["HEDIT_LSP_TEST_FLAG"] == "host-value" && server["registration"]["initialization_options"]["hostInit"] == true)))));
        let path = remote.path("main.rs")?;
        let document = LspDocument {
            path: path.clone(),
            root: remote.root(),
            language: "rust".into(),
            revision: 1,
            text: "fo".into(),
            dirty: true,
        };
        service.submit(
            2,
            Operation::Sync {
                documents: vec![document.clone()],
            },
        )?;
        let mut events = collect(2)?;
        service.submit(
            3,
            Operation::Request {
                path: path.clone(),
                revision: 1,
                scalar: 2,
                feature: Feature::Hover,
                tab_size: 4,
                spaces: true,
            },
        )?;
        events.extend(collect(3)?);
        assert!(events.iter().any(|event|matches!(event,Event::Response{result:Ok(reply),..} if reply.path==path && reply.value["contents"]["value"]=="Fixture hover information")));
        let started = Instant::now();
        while !events.iter().any(|event| {
            matches!(event,
                Event::Diagnostics { server, root, path: source, .. }
                    if server == "fixture" && *root == remote.root() && *source == path
            )
        }) {
            if started.elapsed() > Duration::from_secs(5) {
                return Err(format!("Missing mapped diagnostic ownership: {events:?}"));
            }
            events.extend(service.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        let until = Instant::now() + Duration::from_secs(5);
        while !events.iter().any(|e| matches!(e, Event::Diagnostics { server, path: source, .. } if server == "companion" && *source == path)) {
            assert!(Instant::now() < until, "missing companion diagnostics over SSH");
            events.extend(service.poll());
            std::thread::sleep(Duration::from_millis(10));
        }
        service.submit(
            4,
            Operation::Request {
                path: path.clone(),
                revision: 1,
                scalar: 2,
                feature: Feature::Definition,
                tab_size: 4,
                spaces: true,
            },
        )?;
        let events = collect(4)?;
        let reply: Reply = events
            .into_iter()
            .find_map(|e| match e {
                Event::Response {
                    result: Ok(reply), ..
                } => Some(reply),
                _ => None,
            })
            .ok_or("No definition response.")?;
        assert_eq!(
            reply.value[0]["targetUri"],
            remote.path("target.rs")?.to_string_lossy().as_ref()
        );
        assert_eq!(reply.value[0]["heditColumn16"], 4);
        service.submit(
            40,
            Operation::Rename {
                path: path.clone(),
                revision: 1,
                scalar: 2,
                name: "renamed".into(),
            },
        )?;
        let renamed = collect(40)?
            .into_iter()
            .find_map(|event| match event {
                Event::Response {
                    result: Ok(reply), ..
                } => Some(reply),
                _ => None,
            })
            .ok_or("No rename response.")?;
        let preview: hedit::language_server::workspace_edit::Preview =
            serde_json::from_value(renamed.value).map_err(|e| e.to_string())?;
        assert_eq!(preview.files.len(), 2);
        for (index, file) in preview.files.into_iter().enumerate() {
            remote.relative(&file.path)?;
            assert!(file.path.to_string_lossy().starts_with("hedit-ssh:"));
            let mut edited = remote.open(70 + index as u64, &file.path)?;
            assert_eq!(edited.text(2 * 1024 * 1024)?, file.original);
            let revision = edited.info().revision;
            edited.replace(
                0,
                file.original.len() as u64,
                file.updated.clone(),
                revision,
            )?;
            assert_eq!(edited.text(2 * 1024 * 1024)?, file.updated);
            let revision = edited.info().revision;
            let undone = edited.history(false, 0, revision)?;
            assert_eq!(undone.text, file.original);
        }
        service.submit(
            41,
            Operation::Request {
                path: path.clone(),
                revision: 1,
                scalar: 2,
                feature: Feature::QuickFixes,
                tab_size: 4,
                spaces: true,
            },
        )?;
        let fixed = collect(41)?
            .into_iter()
            .find_map(|event| match event {
                Event::Response {
                    result: Ok(reply), ..
                } => Some(reply),
                _ => None,
            })
            .ok_or("No quick-fix response.")?;
        let fixes: Vec<hedit::language_server::workspace_edit::QuickFix> =
            serde_json::from_value(fixed.value).map_err(|e| e.to_string())?;
        assert!(fixes[0]
            .preview
            .as_ref()
            .is_some_and(|p| p.files[0].path == path));
        assert!(fixes[1].preview.is_none());
        let mut large = document;
        large.revision = 2;
        large.text = "x".repeat(100_000);
        service.submit(
            5,
            Operation::Sync {
                documents: vec![large],
            },
        )?;
        collect(5)?;
        let mut registration = Registration {
            languages: vec!["rust".into()],
            ..Registration::default()
        };
        registration
            .workspaces
            .insert(remote.root().to_string_lossy().into_owned(), false);
        // Keep the explicit host executable while updating the workspace override.
        registration.external = Some(Launch {
            executable: "/usr/bin/python3".into(),
            arguments: vec![format!("{}/fake-language-server.py", remote.config.root)],
        });
        service.submit(
            6,
            Operation::Configure {
                id: "fixture".into(),
                registration: Box::new(registration),
            },
        )?;
        collect(6)?;
        service.submit(7, Operation::List)?;
        let events = collect(7)?;
        assert!(events.iter().any(|e|matches!(e,Event::Result{result:Ok(value),..} if value["installed"].as_array().is_some_and(|servers| servers.iter().any(|server| server["id"] == "fixture" && server["registration"]["workspaces"][remote.root().to_string_lossy().as_ref()] == false)))));
        service.submit(
            8,
            Operation::Remove {
                id: "fixture".into(),
            },
        )?;
        collect(8)?;
        let script = remote.path("fake-language-server.py")?;
        assert!(Backend::request(
            remote.as_ref(),
            Call::File {
                path: script,
                query: FsQuery::Entry
            },
            &AtomicBool::new(false)
        )
        .is_ok());
        remote.trust(false)?;
        let started = Instant::now();
        loop {
            if service
                .poll()
                .iter()
                .any(|e| matches!(e, Event::Disconnected(_)))
            {
                break;
            }
            if started.elapsed() > Duration::from_secs(5) {
                return Err("LSP bridge ignored trust revocation.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        println!("LANGUAGE_SERVER_REMOTE_OK");
        return Ok(());
    }
    if args.get(5).is_some_and(|mode| mode == "last_tab_refusal") {
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let mut config = remote.config.clone();
        config.takeover = true;
        let (mut workspace, errors) =
            hedit::workspace::Workspace::connect_remote(directory.path().join("client"), config)?;
        assert!(errors.is_empty());
        assert_eq!(workspace.open.len(), 1);
        let owner = workspace.remote.as_ref().unwrap().clone();
        for n in 0..50 {
            let path = owner.path(&format!("shelf-{n}.txt"))?;
            let mut document = owner.open(workspace.editor.id(), &path)?;
            let page = document.page(0)?;
            workspace.add(document, page)?;
            workspace.close(1)?;
        }
        let ids: Vec<_> = workspace.closed.iter().map(|view| view.id).collect();
        assert!(workspace.close(0).is_err());
        assert_eq!(
            workspace
                .closed
                .iter()
                .map(|view| view.id)
                .collect::<Vec<_>>(),
            ids,
            "refusing the last tab must not evict shelf history"
        );
        for id in ids {
            assert!(workspace.editor.get(id).is_ok());
        }
        let id = workspace.open[0].view.id;
        assert!(workspace.forget(0).is_err());
        assert!(
            workspace.editor.get(id).is_ok(),
            "refusing discard must retain the live document"
        );
        workspace.open[0].page.text = format!("LAST_TAB_ALIVE{original}");
        workspace.edit(0, &original)?;
        owner.flush()?;
        workspace
            .editor
            .get(id)?
            .lock()
            .map_err(|e| e.to_string())?
            .save(None)?;
        assert!(!directory.path().join("client").exists());
        println!("LAST_TAB_REFUSAL_OK");
        return Ok(());
    }
    if args
        .get(5)
        .is_some_and(|mode| mode == "recover_pending" || mode == "recover_pending_auto")
    {
        let automatic = args[5] == "recover_pending_auto";
        if automatic {
            remote.trust(true)?;
        }
        doc.replace(0, 0, "PENDING_ONCE".into(), doc.info().revision)?;
        let deadline = Instant::now() + Duration::from_secs(20);
        while remote.status().0 {
            if Instant::now() > deadline {
                return Err("Test transport was not interrupted.".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(remote.status().2, 1);
        if automatic {
            let deadline = Instant::now() + Duration::from_secs(30);
            while !remote.status().0 {
                if Instant::now() > deadline {
                    return Err(format!("Automatic reconnect failed: {}", remote.status().3));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            assert!(!remote.status().1, "reconnect must reset execution trust");
        } else {
            remote.reconnect()?;
        }
        remote.flush()?;
        let recovered = remote.open(2, &path)?.text(10 * 1024 * 1024)?;
        assert_eq!(recovered, format!("PENDING_ONCE{original}"));
        doc.save(None)?;
        println!("PENDING_RECONCILIATION_OK");
        return Ok(());
    }

    if args.get(5).is_some_and(|mode| mode == "unlisted_recovery") {
        let mut draft = remote.scratch(2)?;
        draft.replace(0, 0, "UNLISTED_PRIVATE_DRAFT".into(), draft.info().revision)?;
        remote.flush()?;
        let view = hedit::session::SessionView {
            open: vec![],
            closed: vec![],
            active: None,
            secondary: None,
            folder: None,
            folders: vec![],
        };
        remote.metadata(Some(serde_json::to_value(view).map_err(|e| e.to_string())?))?;
        let directory = tempfile::tempdir().map_err(|e| e.to_string())?;
        let mut config = remote.config.clone();
        config.takeover = true;
        let (workspace, _) =
            hedit::workspace::Workspace::connect_remote(directory.path().join("client"), config)?;
        assert!(workspace
            .open
            .iter()
            .any(|b| b.page.text == "UNLISTED_PRIVATE_DRAFT"));
        assert!(!workspace.open.iter().any(|b| b.page.info.name == "file"));
        assert!(workspace.closed.iter().any(|v| v.name == "file"));
        assert!(
            !directory.path().join("client").exists(),
            "remote recovery must not create client storage"
        );
        println!("UNLISTED_RECOVERY_OK");
        return Ok(());
    }

    if args.get(5).is_some_and(|mode| mode == "barrier_frontier") {
        use std::io::Read;
        println!("FRONTIER_READY");
        std::io::stdin()
            .read_exact(&mut [0u8])
            .map_err(|e| e.to_string())?;
        let reader = remote.clone();
        let metadata = std::thread::spawn(move || reader.metadata(None));
        std::thread::sleep(Duration::from_millis(100));
        let reader = remote.clone();
        let barrier = std::thread::spawn(move || reader.flush());
        std::thread::sleep(Duration::from_millis(100));
        doc.replace(0, 0, "FRONTIER_ONCE".into(), doc.info().revision)?;
        println!("EDIT_QUEUED");
        barrier.join().map_err(|_| "Barrier worker panicked")??;
        metadata.join().map_err(|_| "Metadata worker panicked")??;
        remote.flush()?;
        doc.save(None)?;
        println!("BARRIER_FRONTIER_OK");
        return Ok(());
    }

    // Work receipts wake the UI, while unchanged periodic host checks stay quiet.
    remote.flush()?;
    std::thread::sleep(Duration::from_millis(100));
    let idle_wakes = wakes.load(Ordering::Relaxed);
    assert!(idle_wakes > 0);
    std::thread::sleep(Duration::from_millis(1600));
    assert_eq!(wakes.load(Ordering::Relaxed), idle_wakes);
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
