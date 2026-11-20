use super::*;
use crate::app::{
    extensions::tests::app_with,
    tests::{click_label, frame},
};
use serde_json::{json, Value};
use std::fs;
use std::time::{Duration, Instant};

const SAMPLE: &str = include_str!("../../../../examples/extensions/markdown-quote.json");

#[track_caller]
fn finish(app: &mut HeditApp, ctx: &egui::Context) {
    frame(ctx, app, vec![]);
    let start = Instant::now();
    while app.extension_manager.pending.is_some() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "Extension operation did not finish"
        );
        thread::sleep(Duration::from_millis(5));
        app.poll_extension_manager();
    }
    if let Some(Feedback::Error(error)) = &app.extension_manager.feedback {
        panic!("Extension action failed: {error}");
    }
    frame(ctx, app, vec![]);
}

fn show_picked_manifest(app: &mut HeditApp, source: &Path, config: &Path) {
    let review = ImportPreview::open(source.to_owned())
        .unwrap()
        .review(config)
        .unwrap();
    // Bypass the platform file picker and deliver the same reviewed
    // result the picker worker sends when the user selects a file.
    let (tx, rx) = mpsc::channel();
    app.extension_manager.pending = Some(rx);
    app.extension_manager.discard_file_review = false;
    tx.send(Ok(Update::File(Some(review)))).unwrap();
}

#[test]
fn extension_settings_reviews_install_update_command_approval_and_uninstall() {
    let config = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let original = source.path().join("original.json");
    let updated = source.path().join("updated.json");
    let installed = config.path().join("extensions/markdown-quote.json");
    let mut manifest: Value = serde_json::from_str(SAMPLE).unwrap();
    let mut second = manifest["commands"][0].clone();
    second["id"] = json!("another");
    second["title"] = json!("Quote another selection");
    manifest["commands"].as_array_mut().unwrap().push(second);
    let sample = manifest.to_string();
    let update = sample.replace("Quote selection as Markdown", "Quote updated text");
    fs::write(&original, &sample).unwrap();
    fs::write(&updated, &update).unwrap();

    let (_session, ctx, mut app) = app_with("", Some(config.path()));
    app.open_preferences(PreferencesPage::Extensions);
    finish(&mut app, &ctx);

    show_picked_manifest(&mut app, &original, config.path());
    finish(&mut app, &ctx);
    assert!(!installed.exists());
    click_label(&ctx, &mut app, "Install");
    finish(&mut app, &ctx);
    assert_eq!(fs::read_to_string(&installed).unwrap(), sample);
    assert!(app.extensions.is_empty());

    if crate::plugin_runtime::SUPPORTED {
        click_label(&ctx, &mut app, "Review permissions…");
        finish(&mut app, &ctx);
        assert!(app.extensions.is_empty());
        click_label(&ctx, &mut app, "Cancel");
        assert!(app.extension_manager.review.is_none());
        assert!(app.extensions.is_empty());

        click_label(&ctx, &mut app, "Review permissions…");
        finish(&mut app, &ctx);
        click_label(&ctx, &mut app, "Approve command");
        finish(&mut app, &ctx);
        assert_eq!(app.extensions.len(), 1);
        assert_eq!(app.extensions[0].id, "markdown-quote.quote");

        click_label(&ctx, &mut app, "Revoke approval");
        finish(&mut app, &ctx);
        assert!(app.extensions.is_empty());
        click_label(&ctx, &mut app, "Review permissions…");
        finish(&mut app, &ctx);
        click_label(&ctx, &mut app, "Approve command");
        finish(&mut app, &ctx);
        assert_eq!(app.extensions.len(), 1);
    }

    show_picked_manifest(&mut app, &updated, config.path());
    finish(&mut app, &ctx);
    assert_eq!(fs::read_to_string(&installed).unwrap(), sample);
    click_label(&ctx, &mut app, "Update");
    finish(&mut app, &ctx);
    assert_eq!(fs::read_to_string(&installed).unwrap(), update);
    assert!(app.extensions.is_empty());

    click_label(&ctx, &mut app, "Uninstall…");
    finish(&mut app, &ctx);
    assert!(installed.exists());
    click_label(&ctx, &mut app, "Cancel");
    assert!(installed.exists());

    click_label(&ctx, &mut app, "Uninstall…");
    finish(&mut app, &ctx);
    click_label(&ctx, &mut app, "Uninstall");
    finish(&mut app, &ctx);
    assert!(!installed.exists());
    assert!(app.extensions.is_empty());
    assert_eq!(fs::read_to_string(&original).unwrap(), sample);
    assert_eq!(fs::read_to_string(&updated).unwrap(), update);
}

#[test]
fn leaving_extensions_discards_a_file_picker_result() {
    let config = tempfile::tempdir().unwrap();
    let source = config.path().join("manifest.json");
    fs::write(&source, SAMPLE).unwrap();
    let (_session, ctx, mut app) = app_with("", Some(config.path()));
    app.open_preferences(PreferencesPage::Extensions);
    finish(&mut app, &ctx);

    show_picked_manifest(&mut app, &source, config.path());
    app.open_preferences(PreferencesPage::Editor);
    finish(&mut app, &ctx);
    app.open_preferences(PreferencesPage::Extensions);
    finish(&mut app, &ctx);
    assert!(app.extension_manager.review.is_none());
    assert!(app.extensions.is_empty());
}
