use super::super::tests::{click_label, finish_app_job, frame, frame_size, key, text_rect};
use super::*;

fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp, Git) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let git = Git::default();
    git.command(&root, &["init", "-b", "main"]).unwrap();
    for (key, value) in [
        ("user.name", "Test Author"),
        ("user.email", "test@example.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", "/nonexistent/hedit-test-hooks"),
        ("core.autocrlf", "false"),
    ] {
        git.command(&root, &["config", key, value]).unwrap();
    }
    let base = "use std::path::Path;\n\nfn load(path: &Path) -> String {\n    std::fs::read_to_string(path).unwrap()\n}\n";
    std::fs::write(root.join("main.rs"), base).unwrap();
    git.execute(&root, &Operation::Stage(vec!["main.rs".into()]))
        .unwrap();
    git.execute(
        &root,
        &Operation::Commit {
            message: "Read project notes".into(),
            amend: false,
        },
    )
    .unwrap();
    std::fs::write(
        root.join("main.rs"),
        base.replace("unwrap()", "unwrap_or_default()"),
    )
    .unwrap();
    let (mut workspace, errors) = Workspace::restore(directory.path().join("session")).unwrap();
    workspace.folders = vec![hedit::session::FolderView {
        path: root.to_string_lossy().into_owned(),
        visible: true,
        collapsed: false,
        expanded: vec![],
    }];
    let ctx = egui::Context::default();
    ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
    let mut app = HeditApp::from_context(&ctx, None, workspace, errors, vec![]);
    app.open_source_control(&ctx);
    finish_git(&ctx, &mut app);
    (directory, ctx, app, git)
}
fn finish_git(ctx: &egui::Context, app: &mut HeditApp) {
    let start = Instant::now();
    loop {
        app.poll_source_control(ctx);
        if !app.source_control.pending {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "Git job did not complete"
        );
        thread::sleep(Duration::from_millis(5));
    }
    frame(ctx, app, vec![]);
}
fn visible(output: &egui::FullOutput, label: &str) -> bool {
    output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, label).is_some_and(|r| s.clip_rect.contains_rect(r)))
}

#[test]
fn sidebar_stages_and_commits_saved_files_and_preserves_the_editor() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    let revision = app.active().page.info.revision;
    let output = frame(&ctx, &mut app, vec![]);
    assert!(visible(&output, "Source Control"));
    assert!(!visible(&output, "OPEN FILES"));
    click_label(&ctx, &mut app, "main.rs");
    finish_git(&ctx, &mut app);
    assert!(app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .patch
        .contains("+    std::fs::read_to_string(path).unwrap_or_default()"));
    click_label(&ctx, &mut app, "Stage file");
    finish_git(&ctx, &mut app);
    assert!(git.snapshot(&root, 200).unwrap().files[0].staged());
    app.source_control.draft = "Avoid a panic while loading notes".into();
    click_label(&ctx, &mut app, "Commit staged (1)");
    finish_git(&ctx, &mut app);
    assert!(git.snapshot(&root, 200).unwrap().files.is_empty());
    assert_eq!(
        git.history(&root, 200).unwrap()[0].subject,
        "Avoid a panic while loading notes"
    );
    assert!(app.source_control.draft.is_empty());
    assert_eq!(app.active().page.info.revision, revision);
    click_label(&ctx, &mut app, "Files");
    assert!(!app.source_control.open);
    assert!(
        app.workspace.closed.is_empty(),
        "Git view must not create Closed Files entries"
    );
}

#[test]
fn unsaved_buffers_block_staging_and_worktree_changes_without_losing_text() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.open(&ctx, Some(root.join("main.rs")));
    finish_app_job(&mut app, &ctx);
    let index = app.workspace.active;
    let before = app.active().page.text.clone();
    app.workspace.open[index]
        .page
        .text
        .push_str("\n// unsaved note\n");
    app.workspace.edit(index, &before).unwrap();
    let text = app.active().page.text.clone();
    app.git_action(&ctx, Operation::Stage(vec!["main.rs".into()]));
    assert!(!app.source_control.pending);
    assert!(app
        .source_control
        .error
        .as_ref()
        .unwrap()
        .contains("Save or close"));
    assert!(!git.snapshot(&root, 200).unwrap().files[0].staged());
    app.git_action(&ctx, Operation::CreateBranch("feature".into()));
    assert_eq!(git.snapshot(&root, 200).unwrap().branch, "main");
    assert_eq!(app.active().page.text, text);
    assert!(app.active().page.info.dirty);
}

#[test]
fn branch_prompt_cancel_escape_and_create_have_distinct_outcomes() {
    let (_directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.source_control.view = View::Branches;
    app.source_control.prompt = Some(Prompt::Branch("cancelled".into()));
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Cancel");
    assert!(app.source_control.prompt.is_none());
    assert!(!app.source_control.pending);
    app.source_control.prompt = Some(Prompt::Branch("escaped".into()));
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.source_control.prompt.is_none());
    app.source_control.prompt = Some(Prompt::Branch("feature".into()));
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Create");
    finish_git(&ctx, &mut app);
    assert_eq!(git.snapshot(&root, 200).unwrap().branch, "feature");
}

#[test]
fn source_control_history_and_compact_layout_keep_actions_visible() {
    let (_directory, ctx, mut app, _git) = fixture();
    for name in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(name).unwrap());
        for size in [egui::vec2(1280.0, 820.0), egui::vec2(800.0, 600.0)] {
            let output = frame_size(&ctx, &mut app, vec![], size);
            assert!(visible(&output, "Commit staged (0)"));
            assert!(visible(&output, "Source Control"));
        }
    }
    app.source_control.view = View::History;
    let commit = app.source_control.snapshot.as_ref().unwrap().commits[0]
        .oid
        .clone();
    app.source_control.commit = Some(commit);
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    assert_eq!(app.source_control.files.len(), 1);
    assert!(app.source_control.body.contains("Read project notes"));
    click_label(&ctx, &mut app, "main.rs");
    finish_git(&ctx, &mut app);
    assert!(app
        .source_control
        .diff
        .as_ref()
        .unwrap()
        .patch
        .contains("+use std::path::Path;"));
}

#[test]
fn shortcut_close_and_commit_drafts_preserve_editor_state() {
    #[derive(Default)]
    struct Storage(BTreeMap<String, String>);
    impl eframe::Storage for Storage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn flush(&mut self) {}
    }
    let (_directory, ctx, mut app, _git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    app.source_control.draft = "Keep this message across restarts".into();
    let mut storage = Storage::default();
    app.source_control.save(&mut storage);
    assert_eq!(
        SourceControl::load(Some(&storage))
            .drafts
            .get(&root)
            .unwrap(),
        "Keep this message across restarts"
    );
    let count = app.workspace.open.len();
    frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
    assert!(!app.source_control.open);
    assert_eq!(app.workspace.open.len(), count);
    frame(
        &ctx,
        &mut app,
        vec![key(Key::G, Modifiers::COMMAND | Modifiers::SHIFT)],
    );
    assert!(app.source_control.open);
    assert!(!app.goto_open);
    finish_git(&ctx, &mut app);
    app.source_control.draft.clear();
    app.source_control.save(&mut storage);
    assert!(!SourceControl::load(Some(&storage))
        .drafts
        .contains_key(&root));
}

#[test]
fn workspace_switcher_preserves_drafts_and_files_across_themes_and_small_sidebars() {
    let (_directory, ctx, mut app, _git) = fixture();
    app.source_control.draft = "Keep my commit message".into();
    let id = app.active().view.id;
    for theme in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(theme).unwrap());
        click_label(&ctx, &mut app, "Files");
        assert!(!app.source_control.open);
        // Repaint after the mode switch so the Files panel has settled.
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "Source Control");
        finish_git(&ctx, &mut app);
        assert!(app.source_control.open);
        assert_eq!(app.source_control.draft, "Keep my commit message");
        assert_eq!(app.active().view.id, id);
        assert!(app.workspace.closed.is_empty());
    }
    // Both targets remain entirely inside a sidebar at its minimum width.
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(160.0, 80.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                controls::workspace_switcher(ui, &app.theme, false, "Ctrl+Shift+G");
            });
        },
    );
    assert!(visible(&output, "Files"));
    assert!(visible(&output, "Git"));
}

#[test]
#[ignore = "writes native source control preview images"]
fn source_control_visual_preview() {
    let (directory, ctx, mut app, git) = fixture();
    let root = app.source_control.root.clone().unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(
        root.join("src/settings.rs"),
        "pub const AUTO_SAVE: bool = true;\n",
    )
    .unwrap();
    git.execute(&root, &Operation::Stage(vec!["src/settings.rs".into()]))
        .unwrap();
    std::fs::write(
        root.join("README.md"),
        "# Project notes\n\nA small native project.\n",
    )
    .unwrap();
    app.source_control.draft =
        "Handle missing notes gracefully\n\nUse an empty document when a file cannot be read."
            .into();
    app.source_control.request(&ctx, None);
    finish_git(&ctx, &mut app);
    app.source_control.select(
        &ctx,
        Selection {
            path: "main.rs".into(),
            kind: DiffKind::Working,
        },
    );
    finish_git(&ctx, &mut app);
    let destination = std::env::var_os("HEDIT_PREVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| directory.path().into());
    std::fs::create_dir_all(&destination).unwrap();
    for name in ["Hedit Ink", "Hedit Paper"] {
        app.set_theme(&ctx, hedit::theme::builtin(name).unwrap());
        let output = frame(&ctx, &mut app, vec![]);
        let bytes =
            super::super::rtl_tests::rasterize(&ctx, output, egui::vec2(1280.0, 820.0), 1.0);
        std::fs::write(
            destination.join(format!(
                "git-{}.ppm",
                if name == "Hedit Ink" { "dark" } else { "light" }
            )),
            bytes,
        )
        .unwrap();
    }
}
