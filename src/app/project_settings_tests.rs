use super::project_settings::View;
use super::tests::{click_label, frame, frame_size, text_rect};

fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("demo.hedit-project");
    let definition = serde_json::json!({
        "folders": [], "name": "Demo project",
        "settings": {
            "formatting": {"indent_style": "spaces", "indent_size": 4, "trim_trailing_whitespace": true},
            "languages": {"rust": {"indent_size": 2, "insert_final_newline": false, "custom": {"keep": true}}},
            "metadata": {"keep": true}
        }
    });
    hedit::projects::atomic_json(&path, &definition).unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.quality.project_path = Some(path.clone());
    app.quality.project_settings = serde_json::from_value(definition["settings"].clone()).unwrap();
    app.execute(&ctx, Command::ProjectSettings);
    (directory, ctx, app, path)
}

#[test]
fn gui_edits_and_json_round_trip_without_losing_overrides_or_metadata() {
    let (_directory, ctx, mut app, path) = fixture();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "4");
    click_label(&ctx, &mut app, "2");
    assert_eq!(
        app.quality.project_editor.draft.formatting.indent_size,
        Some(2)
    );
    assert_eq!(
        app.quality.project_settings.formatting.indent_size,
        Some(4),
        "draft changes do not apply before Save"
    );
    click_label(&ctx, &mut app, "JSON");
    let mut json: serde_json::Value = serde_json::from_str(&app.quality.settings_json).unwrap();
    assert_eq!(json["formatting"]["indent_size"], 2);
    assert_eq!(json["metadata"]["keep"], true);
    assert_eq!(json["languages"]["rust"]["custom"]["keep"], true);
    json["languages"]["rust"]["trim_trailing_whitespace"] = false.into();
    json["languages"]["custom-syntax"] = serde_json::json!({"indent_size": 6});
    app.quality.settings_json = serde_json::to_string_pretty(&json).unwrap();
    assert!(app.switch_project_settings_view(View::Gui));
    assert_eq!(
        app.quality.project_editor.draft.languages["rust"].trim_trailing_whitespace,
        Some(false)
    );
    app.save_project_settings_draft().unwrap();
    let saved: serde_json::Value = hedit::projects::read_json(&path, 1024 * 1024).unwrap();
    assert_eq!(saved["name"], "Demo project");
    assert_eq!(saved["settings"]["metadata"]["keep"], true);
    assert_eq!(
        saved["settings"]["languages"]["custom-syntax"]["indent_size"],
        6
    );
    assert_eq!(
        saved["settings"]["languages"]["rust"]["custom"]["keep"],
        true
    );
    app.workspace.open[app.workspace.active].set_language(Some(syntax::Language::Rust));
    assert_eq!(
        app.formatting().unwrap().trim_trailing_whitespace,
        Some(false)
    );
    app.quality
        .project_editor
        .draft
        .languages
        .get_mut("rust")
        .unwrap()
        .trim_trailing_whitespace = None;
    app.sync_project_settings_gui();
    app.save_project_settings_draft().unwrap();
    assert_eq!(
        app.formatting().unwrap().trim_trailing_whitespace,
        Some(true),
        "Inherit uses the project default"
    );
}

#[test]
fn closing_discards_an_unsaved_project_settings_draft() {
    let (_directory, ctx, mut app, path) = fixture();
    let before = std::fs::read(&path).unwrap();
    app.quality.project_editor.draft.formatting.indent_size = Some(8);
    app.sync_project_settings_gui();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Close");
    assert!(!app.quality.settings_open);
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert_eq!(app.quality.project_settings.formatting.indent_size, Some(4));
    app.execute(&ctx, Command::ProjectSettings);
    assert_eq!(
        app.quality.project_editor.draft.formatting.indent_size,
        Some(4)
    );
}

#[test]
fn language_overrides_can_be_added_and_removed_in_the_gui() {
    let (_directory, ctx, mut app, _) = fixture();
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    click_label(&ctx, &mut app, "Add language…");
    click_label(&ctx, &mut app, "C++");
    assert_eq!(app.quality.project_editor.scope.as_deref(), Some("cpp"));
    assert!(app
        .quality
        .project_editor
        .draft
        .languages
        .contains_key("cpp"));
    click_label(&ctx, &mut app, "Remove override");
    assert!(app.quality.project_editor.scope.is_none());
    let json: serde_json::Value = serde_json::from_str(&app.quality.settings_json).unwrap();
    assert!(json["languages"].get("cpp").is_none());
    assert_eq!(json["languages"]["rust"]["insert_final_newline"], false);
}

#[test]
fn save_failure_does_not_apply_the_project_draft() {
    let (directory, _ctx, mut app, _) = fixture();
    app.quality.project_editor.draft.formatting.indent_size = Some(8);
    app.sync_project_settings_gui();
    app.quality.project_path = Some(directory.path().to_owned());
    assert!(app.save_project_settings_draft().is_err());
    assert_eq!(app.quality.project_settings.formatting.indent_size, Some(4));
    assert!(app.quality.project_editor.dirty);
}

#[test]
fn project_utf8_choices_control_the_saved_byte_order_mark() {
    let (directory, ctx, mut app, _) = fixture();
    let path = directory.path().join("notes.txt");
    std::fs::write(&path, b"\xef\xbb\xbfcafe\n").unwrap();
    app.open(&ctx, Some(path.clone()));
    tests::finish_app_job(&mut app, &ctx);
    app.open_project_settings();
    for (charset, expected) in [
        ("utf-8", b"cafe\n".as_slice()),
        ("utf-8-bom", b"\xef\xbb\xbfcafe\n".as_slice()),
    ] {
        app.quality.project_editor.draft.formatting.charset = Some(charset.into());
        app.sync_project_settings_gui();
        app.save_project_settings_draft().unwrap();
        let id = app.active().view.id;
        app.save_to_path(&ctx, id, None);
        tests::finish_app_job(&mut app, &ctx);
        assert_eq!(std::fs::read(&path).unwrap(), expected);
    }
}

#[test]
fn both_project_settings_views_keep_save_visible_at_minimum_size() {
    let (_directory, ctx, mut app, _) = fixture();
    let size = egui::vec2(720.0, 480.0);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    for view in [View::Gui, View::Json] {
        app.switch_project_settings_view(view);
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        for label in ["GUI", "JSON", "Close", "Save settings"] {
            assert!(
                output.shapes.iter().any(|s| text_rect(&s.shape, label)
                    .is_some_and(|r| screen.contains_rect(r) && s.clip_rect.contains_rect(r))),
                "{label} must remain visible"
            );
        }
    }
    app.quality.settings_json = "{ invalid json".into();
    app.switch_project_settings_view(View::Gui);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], size);
    }
    let output = frame_size(&ctx, &mut app, vec![], size);
    assert!(output
        .shapes
        .iter()
        .any(|s| text_rect(&s.shape, "Save settings")
            .is_some_and(|r| screen.contains_rect(r) && s.clip_rect.contains_rect(r))));
}
use super::*;

#[test]
fn invalid_json_is_kept_when_switching_to_gui() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.execute(&ctx, Command::ProjectSettings);
    app.switch_project_settings_view(View::Json);
    for draft in ["{ invalid draft", r#"{"formatting":{"indent_size":0}}"#] {
        app.quality.settings_json = draft.into();
        assert!(!app.switch_project_settings_view(View::Gui));
        assert!(app.quality.project_editor.view == View::Json);
        assert_eq!(app.quality.settings_json, draft);
        assert!(app.quality.project_editor.error.is_some());
        assert!(app.save_project_settings_draft().is_err());
        assert_eq!(app.quality.project_settings.formatting.indent_size, None);
    }
}

#[test]
#[ignore = "writes project-settings SVG previews to HEDIT_PREVIEW_DIR"]
fn export_project_settings_previews() {
    let output = PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").unwrap());
    std::fs::create_dir_all(&output).unwrap();
    for (theme, name) in [(Theme::default(), "ink"), (Theme::paper(), "paper")] {
        for state in ["gui", "rust", "json", "error"] {
            for size in [egui::vec2(720.0, 480.0), egui::vec2(1280.0, 820.0)] {
                let (_directory, ctx, mut app, _) = fixture();
                app.set_theme(&ctx, theme.clone());
                if state == "rust" {
                    app.quality.project_editor.scope = Some("rust".into());
                }
                if matches!(state, "json" | "error") {
                    app.switch_project_settings_view(View::Json);
                }
                if state == "error" {
                    app.quality.settings_json =
                        "{\n  \"formatting\": { \"indent_size\": 0 }\n}".into();
                    app.switch_project_settings_view(View::Gui);
                }
                for _ in 0..4 {
                    frame_size(&ctx, &mut app, vec![], size);
                }
                let output_frame = frame_size(&ctx, &mut app, vec![], size);
                let svg = crate::preview::export(
                    &output_frame.shapes,
                    [size.x, size.y],
                    app.theme.background.color(),
                );
                std::fs::write(
                    output.join(format!("project-{name}-{state}-{}.svg", size.x)),
                    svg,
                )
                .unwrap();
            }
        }
    }
}
