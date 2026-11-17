//! settings.json: migration, saving, reloading, and broken files.
use super::tests::{finish_app_job, frame, key, MemoryStorage};
use super::*;
use eframe::Storage;
use std::path::Path;

/// An app whose settings live in `<directory>/config` for this test's thread.
fn app_in(directory: &Path, storage: Option<&dyn eframe::Storage>) -> (egui::Context, HeditApp) {
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = Some(directory.join("config")));
    let (workspace, _) = Workspace::restore(directory.join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, storage, workspace, vec![], vec![]);
    if app.terminal_onboarding_due() {
        app.choose_terminal(false);
    }
    (ctx, app)
}

fn refocus(ctx: &egui::Context, app: &mut HeditApp) {
    let input = egui::RawInput {
        events: vec![egui::Event::WindowFocused(true)],
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        eframe::App::update(app, ctx, &mut fake_frame())
    });
}

fn fake_frame() -> eframe::Frame {
    eframe::Frame::_new_kittest()
}

#[test]
fn first_run_imports_older_preferences_into_settings_files() {
    let directory = tempfile::tempdir().unwrap();
    let mut storage = MemoryStorage::default();
    eframe::set_value(&mut storage, "wrap", &true);
    eframe::set_value(&mut storage, "ruler_column", &80u16);
    eframe::set_value(&mut storage, "font_size", &17.0f32);
    storage.set_string("debugger", "/opt/cuda/bin/cuda-gdb".into());
    let session = directory.path().join("session");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("editor-settings.json"),
        r#"{"defaults":{"formatting":{"indent_size":2,"trim_trailing_whitespace":true},
            "languages":{"python":{"indent_style":"spaces"}}},
            "keybindings":[{"command":"save","shortcut":"Primary+Alt+S"}]}"#,
    )
    .unwrap();
    let (_ctx, app) = app_in(directory.path(), Some(&storage));
    assert_eq!(app.error, None);
    let config = directory.path().join("config");
    let (settings, warnings) =
        hedit::settings::Settings::load(&config.join("settings.json")).unwrap();
    assert!(warnings.is_empty());
    assert!(settings.word_wrap);
    assert_eq!(settings.rulers, [80]);
    assert_eq!(settings.font_size, 17.0);
    assert_eq!(settings.cuda_gdb_path, "/opt/cuda/bin/cuda-gdb");
    assert_eq!(settings.format.tab_size, Some(2));
    assert_eq!(
        settings.format.trim_trailing_white_space_on_save,
        Some(true)
    );
    assert_eq!(
        settings.syntax_specific["python"].translate_tabs_to_spaces,
        Some(true)
    );
    assert_eq!(app.settings, settings);
    assert!(std::fs::read_to_string(config.join("keybindings.json"))
        .unwrap()
        .contains("Primary+Alt+S"));
    assert!(config.join("snippets.json").is_file());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn changes_are_written_after_they_settle_and_reloaded_after_outside_edits() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path(), None);
    let path = app.settings_path().unwrap();
    app.settings.show_minimap = true;
    app.persist_settings();
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"show_minimap\": true"),
        "a change is not written until it settles"
    );
    assert!(app.settings_due().is_some());
    app.flush_settings();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("\"show_minimap\": true"));
    assert_eq!(app.settings_due(), None);
    // Another editor changes the file; returning to hedit picks it up.
    thread::sleep(Duration::from_millis(20));
    std::fs::write(&path, "{ \"font_size\": 20, // bigger\n }").unwrap();
    refocus(&ctx, &mut app);
    assert_eq!(app.settings.font_size, 20.0);
    assert!(
        !app.settings.show_minimap,
        "missing keys return to defaults"
    );
    assert_eq!(app.notice, "Settings reloaded.");
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn failed_settings_writes_remain_pending_and_retry_after_storage_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path(), None);
    let path = app.settings_path().unwrap();
    let saved = app.settings_files.saved.clone();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    app.settings.word_wrap = !saved.word_wrap;
    app.settings.terminal_font = Some(hedit::settings::TerminalFont::default());
    app.flush_settings();
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Cannot save settings")));
    assert_eq!(
        app.settings_files.saved, saved,
        "a failed write was marked saved"
    );
    assert!(app
        .settings_due()
        .is_some_and(|delay| delay > Duration::ZERO));
    app.settings_files.changed_at = Some(Instant::now() - Duration::from_secs(1));
    app.font_applied = "previous font".into();
    app.apply_editor_style(&ctx);
    assert_eq!(
        app.font_applied, app.settings.font_face,
        "failed saving blocked editor font changes"
    );
    assert_eq!(
        app.terminal_font_applied.as_deref(),
        Some(""),
        "failed saving blocked terminal font changes"
    );
    std::fs::remove_dir(&path).unwrap();
    app.persist_settings();
    let (loaded, _) = hedit::settings::Settings::load(&path).unwrap();
    assert_eq!(loaded.word_wrap, !saved.word_wrap);
    assert_eq!(app.settings_files.saved, app.settings);
    assert!(app.settings_due().is_none());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn failed_config_writes_remain_pending_and_retry_after_storage_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (_ctx, mut app) = app_in(directory.path(), None);
    let path = directory.path().join("config/snippets.json");
    let saved = app.settings_files.config_saved.clone();
    assert!(!saved.snippets.is_empty());
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    app.quality.config.snippets.clear();
    app.flush_settings();
    assert!(app
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Cannot save key bindings or snippets")));
    assert_eq!(
        app.settings_files.config_saved, saved,
        "a partial config write was marked saved"
    );
    assert!(app
        .settings_due()
        .is_some_and(|delay| delay > Duration::ZERO));
    std::fs::remove_dir(&path).unwrap();
    app.settings_files.config_changed_at = Some(Instant::now() - Duration::from_secs(1));
    app.persist_settings();
    let snippets: Vec<hedit::intelligence::Snippet> =
        hedit::projects::read_json(&path, 4 * 1024 * 1024).unwrap();
    assert!(snippets.is_empty());
    assert_eq!(app.settings_files.config_saved, app.quality.config);
    assert!(app.settings_due().is_none());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn oversized_config_files_are_rejected_before_any_preferences_are_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let (_ctx, mut app) = app_in(directory.path(), None);
    let config = directory.path().join("config");
    let files = ["keybindings.json", "snippets.json", "macros.json"];
    let original: Vec<_> = files
        .iter()
        .map(|name| std::fs::read(config.join(name)).ok())
        .collect();
    let previous = app.quality.config.clone();
    for (index, file) in files.iter().enumerate() {
        let mut candidate = previous.clone();
        candidate.keybindings.push(quality::Binding {
            command: Command::Save,
            shortcut: "Primary+Alt+S".into(),
            editor_only: false,
        });
        match index {
            0 => candidate.keybindings[0].shortcut = "x".repeat(1024 * 1024),
            1 => candidate.snippets[0].body = "\"".repeat(2 * 1024 * 1024),
            _ => candidate.macros.push(macros::Macro {
                name: "large".into(),
                steps: vec![macros::Step::Text("x".repeat(4 * 1024 * 1024))],
            }),
        }
        let error = app
            .save_config(candidate)
            .expect_err("saved config that cannot be reloaded");
        assert!(error.contains(file), "{error}");
        assert_eq!(app.quality.config, previous);
        for (name, bytes) in files.iter().zip(&original) {
            assert_eq!(
                &std::fs::read(config.join(name)).ok(),
                bytes,
                "{name} changed while rejecting {file}"
            );
        }
    }
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn a_broken_settings_file_is_reported_and_never_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    let broken = "{ \"font_size\": 16, oops }";
    std::fs::write(config.join("settings.json"), broken).unwrap();
    let (ctx, mut app) = app_in(directory.path(), None);
    assert!(app.error.as_deref().unwrap().contains("will not overwrite"));
    app.settings.word_wrap = true;
    app.flush_settings();
    assert_eq!(
        std::fs::read_to_string(config.join("settings.json")).unwrap(),
        broken
    );
    // Fixing the file brings it back into use.
    thread::sleep(Duration::from_millis(20));
    std::fs::write(config.join("settings.json"), "{ \"font_size\": 16 }").unwrap();
    refocus(&ctx, &mut app);
    assert_eq!(app.settings.font_size, 16.0);
    app.settings.word_wrap = true;
    app.flush_settings();
    assert!(std::fs::read_to_string(config.join("settings.json"))
        .unwrap()
        .contains("\"word_wrap\": true"));
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn saving_settings_json_in_hedit_applies_it() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path(), None);
    app.open_settings_file(&ctx);
    finish_app_job(&mut app, &ctx);
    let resolve = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
    assert_eq!(
        app.active()
            .page
            .info
            .path
            .as_deref()
            .map(PathBuf::from)
            .map(resolve),
        app.settings_path().map(resolve)
    );
    let text = app
        .active()
        .page
        .text
        .replace("\"line_numbers\": true", "\"line_numbers\": false");
    let index = app.workspace.active;
    let previous = std::mem::replace(&mut app.workspace.open[index].page.text, text);
    app.workspace.edit(index, &previous).unwrap();
    app.execute(&ctx, Command::Save);
    finish_app_job(&mut app, &ctx);
    frame(&ctx, &mut app, vec![]);
    assert!(!app.settings.line_numbers);
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn recorded_shortcuts_parse_back_to_the_same_keys() {
    for (modifiers, key) in [
        (Modifiers::COMMAND, Key::K),
        (
            Modifiers {
                shift: true,
                ..Modifiers::COMMAND
            },
            Key::F5,
        ),
        (Modifiers::ALT, Key::Comma),
        (Modifiers::NONE, Key::F9),
    ] {
        let text = settings_pages::binding_text(modifiers, key);
        let parsed = quality_windows::shortcut(&text).unwrap().unwrap();
        assert_eq!(parsed.logical_key, key, "{text}");
        assert_eq!(parsed.modifiers.command, modifiers.command, "{text}");
        assert_eq!(parsed.modifiers.shift, modifiers.shift, "{text}");
        assert_eq!(parsed.modifiers.alt, modifiers.alt, "{text}");
    }
}

#[test]
fn key_bindings_are_recorded_by_pressing_keys_and_saved() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path(), None);
    app.open_preferences(PreferencesPage::Keys);
    frame(&ctx, &mut app, vec![]);
    // Escape while recording cancels the recording, not the Settings window.
    app.key_capture = Some(Command::Goto);
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert_eq!(app.key_capture, None);
    assert!(app.preferences_open);
    assert!(!app.overridden(Command::Goto));
    // A shortcut that another command uses moves, and that command is unbound.
    app.key_capture = Some(Command::Goto);
    frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
    // With no second key, the single key is used once the chord wait ends.
    app.capture_first = app
        .capture_first
        .take()
        .map(|(first, _)| (first, Instant::now() - Duration::from_secs(5)));
    frame(&ctx, &mut app, vec![]);
    assert!(!app.find_open, "the recorded keys do not run Find");
    assert_eq!(app.shortcut_text(Command::Goto), Command::Find.shortcut());
    assert_eq!(app.shortcut_text(Command::Find), "");
    assert!(app.notice.contains("Find"));
    app.flush_settings();
    let saved = std::fs::read_to_string(directory.path().join("config/keybindings.json")).unwrap();
    assert!(
        saved.contains("\"goto\"") && saved.contains("Primary+F"),
        "{saved}"
    );
    // Reset restores the default.
    app.set_binding(Command::Find, None);
    assert_eq!(app.shortcut_text(Command::Find), Command::Find.shortcut());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn settings_choose_themes_by_name() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path(), None);
    let path = app.settings_path().unwrap();
    // Choosing a theme in the app records its name.
    app.choose_theme(&ctx, hedit::theme::builtin("Nord").unwrap());
    app.flush_settings();
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("\"color_theme\": \"Nord\""));
    // Naming a theme in the file switches to it.
    thread::sleep(Duration::from_millis(20));
    std::fs::write(&path, r#"{ "color_theme": "Dracula" }"#).unwrap();
    app.reload_settings(&ctx);
    assert_eq!(app.theme.name, "Dracula");
    assert_eq!(ctx.style().visuals.panel_fill, app.theme.panel.color());
    // An unknown name is reported and the current theme stays.
    thread::sleep(Duration::from_millis(20));
    std::fs::write(&path, r#"{ "color_theme": "No Such Theme" }"#).unwrap();
    app.reload_settings(&ctx);
    assert_eq!(app.theme.name, "Dracula");
    assert!(app
        .error
        .take()
        .unwrap()
        .contains("Unknown theme “No Such Theme”"));
    // Following the system uses the dark and light names.
    thread::sleep(Duration::from_millis(20));
    std::fs::write(
        &path,
        r#"{ "follow_system_appearance": true, "light_color_theme": "GitHub Light", "dark_color_theme": "One Dark" }"#,
    )
    .unwrap();
    app.reload_settings(&ctx);
    assert!(app.appearance.follow_system);
    assert_eq!(app.appearance.light.name, "GitHub Light");
    assert_eq!(app.appearance.dark.name, "One Dark");
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn editor_font_results_trim_names_and_failures_preserve_document_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(dir.path(), None);
    let bytes = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf").to_vec();
    app.settings.font_face = " selected font ".into();
    app.busy = true;
    let focus = egui::Id::new("document-job");
    app.job_editor_focus = Some(focus);
    app.tx
        .send(Ok(Action::Font("selected font".into(), bytes, 0)))
        .unwrap();
    app.receive(&ctx);
    assert!(
        app.editor_font.is_some(),
        "a trimmed font result must match the selected family"
    );
    app.tx
        .send(Ok(Action::FontFailed(
            "selected font".into(),
            "missing font".into(),
        )))
        .unwrap();
    app.receive(&ctx);
    assert!(app.busy);
    assert_eq!(app.job_editor_focus, Some(focus));
    assert!(app.editor_font.is_none());
    assert!(app.notice.contains("missing font"));
    let notice = app.notice.clone();
    app.tx
        .send(Ok(Action::FontFailed(
            "old selection".into(),
            "stale failure".into(),
        )))
        .unwrap();
    app.receive(&ctx);
    assert_eq!(app.notice, notice);
    assert!(app.busy);
    app.busy = false;
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}
