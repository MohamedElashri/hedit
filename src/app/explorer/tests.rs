use super::*;
use crate::app::tests::{finish_app_job, frame, key};
fn paint(ctx: &egui::Context, app: &mut HeditApp, events: Vec<egui::Event>) {
    app.project.poll();
    frame(ctx, app, events);
}
fn settle(ctx: &egui::Context, app: &mut HeditApp) {
    let at = Instant::now();
    loop {
        paint(ctx, app, vec![]);
        if app
            .project
            .tree
            .as_ref()
            .is_some_and(|t| t.missing.is_empty())
        {
            paint(ctx, app, vec![]);
            return;
        }
        assert!(at.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
}
fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    settings_store::TEST_CONFIG_DIR.with(|p| *p.borrow_mut() = Some(dir.path().join("config")));
    let root = dir.path().join("root");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("alpha.txt"), "alpha contents\n").unwrap();
    std::fs::write(root.join("beta.txt"), "beta contents\n").unwrap();
    std::fs::write(root.join("src/helper.rs"), "fn helper() {}\n").unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.choose_terminal(false);
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    settle(&ctx, &mut app);
    (dir, ctx, app, root)
}
#[test]
fn keyboard_navigation_and_typeahead_do_not_edit_the_document() {
    let (_dir, ctx, mut app, root) = fixture();
    app.focus_explorer(&ctx);
    paint(&ctx, &mut app, vec![]);
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new("explorer-tree"))));
    paint(&ctx, &mut app, vec![key(Key::End, Modifiers::NONE)]);
    assert_eq!(
        app.explorer.selected.as_deref(),
        root.join("beta.txt").to_str()
    );
    paint(&ctx, &mut app, vec![key(Key::ArrowUp, Modifiers::NONE)]);
    assert_eq!(
        app.explorer.selected.as_deref(),
        root.join("alpha.txt").to_str()
    );
    assert!(
        ctx.memory(|m| m.has_focus(egui::Id::new("explorer-tree"))),
        "arrow navigation must retain tree focus"
    );
    paint(&ctx, &mut app, vec![egui::Event::Text("be".into())]);
    assert_eq!(
        app.explorer.selected.as_deref(),
        root.join("beta.txt").to_str()
    );
    assert!(app.active().page.text.is_empty());
    paint(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().page.text, "beta contents\n");
    assert!(app.project.create.is_none());
}
#[test]
fn inline_creation_preserves_existing_unsaved_text_and_opens_the_new_file() {
    let (_dir, ctx, mut app, root) = fixture();
    app.workspace.open[0].page.text = "keep unsaved".into();
    app.workspace.edit(0, "").unwrap();
    let original = app.active().view.id;
    app.start_explorer_entry(&ctx, root.to_string_lossy().into_owned(), false, false);
    settle(&ctx, &mut app);
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new("explorer-entry-name"))));
    app.project.create.as_mut().unwrap().name = "new.rs".into();
    app.submit_explorer_entry(&ctx);
    finish_app_job(&mut app, &ctx);
    assert!(app.project.create.is_none());
    assert!(root.join("new.rs").is_file());
    assert_eq!(app.active().page.info.name, "new.rs");
    let prior = app
        .workspace
        .open
        .iter()
        .find(|b| b.view.id == original)
        .unwrap();
    assert_eq!(prior.page.text, "keep unsaved");
    assert!(prior.page.info.dirty);
}
#[test]
fn invalid_and_conflicting_inline_names_keep_the_draft_and_existing_file() {
    let (_dir, ctx, mut app, root) = fixture();
    app.start_explorer_entry(&ctx, root.to_string_lossy().into_owned(), false, false);
    app.project.create.as_mut().unwrap().name = "../escape".into();
    app.submit_explorer_entry(&ctx);
    assert!(!app.busy);
    assert!(app.project.create.as_ref().unwrap().error.is_some());
    app.project.create.as_mut().unwrap().name = "alpha.txt".into();
    app.submit_explorer_entry(&ctx);
    finish_app_job(&mut app, &ctx);
    let draft = app.project.create.as_ref().unwrap();
    assert_eq!(draft.name, "alpha.txt");
    assert!(draft.error.is_some());
    assert!(!draft.pending);
    assert_eq!(
        std::fs::read_to_string(root.join("alpha.txt")).unwrap(),
        "alpha contents\n"
    );
    assert!(!root.parent().unwrap().join("escape").exists());
}
#[test]
fn inline_rename_retargets_a_dirty_document_without_saving_its_text() {
    let (_dir, ctx, mut app, root) = fixture();
    let path = root.join("alpha.txt");
    app.open(&ctx, Some(path.clone()));
    finish_app_job(&mut app, &ctx);
    let id = app.active().view.id;
    let before = app.active().page.text.clone();
    app.workspace.open[app.workspace.active].page.text = "unsaved edits".into();
    app.workspace.edit(app.workspace.active, &before).unwrap();
    app.start_explorer_entry(&ctx, path.to_string_lossy().into_owned(), false, true);
    app.project.create.as_mut().unwrap().name = "renamed.rs".into();
    app.submit_explorer_entry(&ctx);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.id, id);
    assert!(app.active().page.info.dirty);
    assert_eq!(app.active().page.text, "unsaved edits");
    assert_eq!(
        app.active().page.info.path.as_deref(),
        root.join("renamed.rs").to_str()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("renamed.rs")).unwrap(),
        "alpha contents\n"
    );
    assert!(!path.exists());
}
#[test]
fn f2_renames_the_explorer_selection_and_escape_cancels_the_inline_field() {
    let (_dir, ctx, mut app, root) = fixture();
    app.focus_explorer(&ctx);
    app.explorer.selected = Some(root.join("alpha.txt").to_string_lossy().into_owned());
    paint(&ctx, &mut app, vec![]);
    paint(&ctx, &mut app, vec![key(Key::F2, Modifiers::NONE)]);
    paint(&ctx, &mut app, vec![]);
    assert_eq!(
        app.project.create.as_ref().unwrap().rename.as_deref(),
        Some(root.join("alpha.txt").as_path())
    );
    paint(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(app.project.create.is_none());
    assert!(root.join("alpha.txt").exists());
    assert!(app.active().page.text.is_empty());
}
#[test]
fn automatic_reveal_keeps_the_search_sidebar_and_its_inputs_intact() {
    let (_dir, ctx, mut app, root) = fixture();
    app.sidebar = workbench_navigation::Sidebar::Search;
    app.quality.folder_query = "keep this query".into();
    app.open(&ctx, Some(root.join("src/helper.rs")));
    finish_app_job(&mut app, &ctx);
    paint(&ctx, &mut app, vec![]);
    assert_eq!(app.sidebar, workbench_navigation::Sidebar::Search);
    assert_eq!(app.quality.folder_query, "keep this query");
    app.sidebar = workbench_navigation::Sidebar::Files;
    settle(&ctx, &mut app);
    assert_eq!(
        app.explorer.selected.as_deref(),
        root.join("src/helper.rs").to_str()
    );
    assert!(app
        .project
        .expanded
        .contains(root.join("src").to_str().unwrap()));
    app.settings.explorer_auto_reveal = false;
    app.explorer.selected = Some(root.to_string_lossy().into_owned());
    app.open(&ctx, Some(root.join("beta.txt")));
    finish_app_job(&mut app, &ctx);
    paint(&ctx, &mut app, vec![]);
    assert_eq!(app.explorer.selected.as_deref(), root.to_str());
}
#[test]
fn inline_names_accept_direct_ime_commits_while_a_git_review_is_open() {
    let (_dir, ctx, mut app, root) = fixture();
    app.start_explorer_entry(&ctx, root.to_string_lossy().into_owned(), false, false);
    settle(&ctx, &mut app);
    app.source_control.open = true;
    paint(
        &ctx,
        &mut app,
        "مرحبا"
            .chars()
            .map(|c| egui::Event::Ime(egui::ImeEvent::Commit(c.to_string())))
            .collect(),
    );
    assert_eq!(app.project.create.as_ref().unwrap().name, "مرحبا");
    assert!(app.active().page.text.is_empty());
}
#[test]
fn compact_folders_keep_cached_chain_identity_and_collapse_all_segments() {
    let (_dir, ctx, mut app, root) = fixture();
    let src = root.join("src");
    let nested = src.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::rename(src.join("helper.rs"), nested.join("helper.rs")).unwrap();
    app.settings.explorer_compact_folders = true;
    app.refresh_folder();
    app.project
        .expanded
        .insert(src.to_string_lossy().into_owned());
    settle(&ctx, &mut app);
    let tree = app.project.tree.clone().unwrap();
    let node = tree
        .rows
        .iter()
        .find(|n| n.path == nested.to_string_lossy())
        .unwrap();
    assert_eq!(node.name, "src / nested");
    assert!(node.expanded);
    app.toggle_explorer_node(node);
    assert!(!app.project.expanded.contains(src.to_str().unwrap()));
    assert!(!app.project.expanded.contains(nested.to_str().unwrap()));
    settle(&ctx, &mut app);
    assert!(
        !app.project
            .tree
            .as_ref()
            .unwrap()
            .rows
            .iter()
            .find(|n| n.path == nested.to_string_lossy())
            .unwrap()
            .expanded
    );
}
#[test]
fn explorer_badges_do_not_start_language_servers() {
    let (_dir, ctx, mut app, _root) = fixture();
    assert!(app.languages.service.is_none());
    app.poll_explorer_badges(&ctx);
    assert!(app.languages.service.is_none());
    app.settings.explorer_git_badges = false;
    app.poll_explorer_badges(&ctx);
    assert!(app.explorer.badges.files.is_empty());
    assert!(app.explorer.badges.metadata.is_empty());
}

#[test]
fn global_shortcuts_win_when_navigation_arrives_in_the_same_input_batch() {
    let (_dir, ctx, mut app, _root) = fixture();
    app.focus_explorer(&ctx);
    paint(&ctx, &mut app, vec![]);
    paint(&ctx, &mut app, vec![]);
    paint(
        &ctx,
        &mut app,
        vec![
            key(Key::P, Modifiers::COMMAND),
            key(Key::ArrowDown, Modifiers::NONE),
        ],
    );
    assert!(app.palette);
    assert!(app.project.create.is_none());
}
#[test]
fn personal_bindings_override_contextual_f2_and_tab_can_leave_the_tree() {
    let (_dir, ctx, mut app, _root) = fixture();
    app.focus_explorer(&ctx);
    paint(&ctx, &mut app, vec![]);
    paint(&ctx, &mut app, vec![]);
    paint(&ctx, &mut app, vec![key(Key::Tab, Modifiers::NONE)]);
    assert!(!ctx.memory(|m| m.has_focus(egui::Id::new("explorer-tree"))));
    app.quality.config.keybindings.push(quality::Binding {
        command: Command::New,
        shortcut: "F2".into(),
        editor_only: false,
    });
    app.focus_explorer(&ctx);
    paint(&ctx, &mut app, vec![]);
    let before = app.active().view.id;
    paint(&ctx, &mut app, vec![key(Key::F2, Modifiers::NONE)]);
    assert!(app.project.create.is_none());
    assert_ne!(app.active().view.id, before);
}

#[test]
fn compact_folder_refresh_keeps_selection_and_an_inline_rename_target() {
    let (_dir, ctx, mut app, root) = fixture();
    let src = root.join("src");
    let nested = src.join("nested");
    let deeper = nested.join("deeper");
    std::fs::create_dir_all(&deeper).unwrap();
    std::fs::rename(src.join("helper.rs"), deeper.join("helper.rs")).unwrap();
    app.explorer.selected = Some(src.to_string_lossy().into_owned());
    app.settings.explorer_compact_folders = true;
    app.refresh_folder();
    settle(&ctx, &mut app);
    assert_eq!(app.explorer.selected.as_deref(), deeper.to_str());
    app.start_explorer_entry(&ctx, nested.to_string_lossy().into_owned(), true, true);
    settle(&ctx, &mut app);
    assert!(app
        .project
        .tree
        .as_ref()
        .unwrap()
        .rows
        .iter()
        .any(|n| n.path == nested.to_string_lossy()));
    assert!(ctx.memory(|m| m.has_focus(egui::Id::new("explorer-entry-name"))));
}
