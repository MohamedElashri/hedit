//! settings.json: migration, saving, reloading, and broken files.
use super::tests::{finish_app_job, frame, MemoryStorage};
use super::*;
use eframe::Storage;
use std::path::Path;

/// An app whose settings live in `<directory>/config` for this test's thread.
fn app_in(directory: &Path, storage: Option<&dyn eframe::Storage>) -> (egui::Context, HeditApp) {
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = Some(directory.join("config")));
    let (workspace, _) = Workspace::restore(directory.join("session")).unwrap();
    let ctx = egui::Context::default();
    let app = HeditApp::from_context(&ctx, storage, workspace, vec![], vec![]);
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
    assert_eq!(
        app.active().page.info.path.as_deref().map(PathBuf::from),
        app.settings_path()
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
