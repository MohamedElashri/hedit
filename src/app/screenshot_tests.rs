//! The README screenshots, drawn headlessly from a real hedit window.
use super::tests::{finish_app_job, key};
use super::*;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 780.0);
/// Pixels per point, for sharp text on high-density screens.
const SCALE: f32 = 2.0;

const MAIN_RS: &str = r#"use std::collections::HashMap;

/// Count how often each word appears in the text.
// يحسب عدد مرات ظهور كل كلمة في النص
fn word_counts(text: &str) -> HashMap<&str, usize> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    counts
}

fn main() {
    let text = std::fs::read_to_string("notes.txt").unwrap_or_default();
    let counts = word_counts(&text);
    // Print the most frequent words first, once we recieve them.
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    for (word, count) in sorted.iter().take(10) {
        println!("{count:>5} {word}");
    }
    
}
"#;

fn run(ctx: &egui::Context, app: &mut HeditApp, events: Vec<egui::Event>) -> egui::FullOutput {
    app.project.poll();
    app.receive(ctx);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
        focused: true,
        events,
        ..Default::default()
    };
    ctx.run(input, |ctx| {
        app.shortcuts(ctx);
        app.chrome(ctx);
        app.search_panel(ctx);
        app.build_panel(ctx);
        app.editor(ctx);
        app.quality_windows(ctx);
    })
}

/// A small project, open in hedit with completions showing, in `theme`, as
/// a binary PPM image.
fn scene(theme: &str) -> Vec<u8> {
    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("wordcount");
    for (path, text) in [
        ("src/main.rs", MAIN_RS),
        ("src/report.rs", "pub fn report() {}\n"),
        (
            "kernels/vector_add.cu",
            include_str!("../../examples/cuda/vector_add.cu"),
        ),
        (
            "Cargo.toml",
            "[package]\nname = \"wordcount\"\nversion = \"0.1.0\"\n",
        ),
        ("README.md", "# wordcount\n"),
    ] {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let ctx = egui::Context::default();
    ctx.set_pixels_per_point(SCALE);
    settings_store::fallback_fonts();
    settings_store::set_editor_font(&ctx, None);
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_theme(&ctx, hedit::theme::builtin(theme).unwrap());
    app.settings.caret_blink = false;
    app.settings.auto_complete_delay = 0;
    app.settings.spell_check = true;
    app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
    app.project
        .expanded
        .insert(root.join("src").to_string_lossy().into_owned());
    for file in ["kernels/vector_add.cu", "src/report.rs", "src/main.rs"] {
        app.open(&ctx, Some(root.join(file)));
        finish_app_job(&mut app, &ctx);
    }
    // The empty file every new session starts with is not part of the scene.
    if let Some(index) = app
        .workspace
        .open
        .iter()
        .position(|b| b.page.info.path.is_none())
    {
        app.workspace.close(index).unwrap();
        app.workspace.closed.clear();
    }
    // Index words, then type “wor” on the empty line to show completions.
    app.auto.wanted = true;
    let caret = MAIN_RS
        .rfind("    \n}")
        .map(|b| MAIN_RS[..b + 4].chars().count())
        .unwrap();
    app.set_selections(vec![hedit::advanced::Selection::caret(caret)]);
    for _ in 0..40 {
        run(&ctx, &mut app, vec![]);
        thread::sleep(Duration::from_millis(20));
    }
    for c in "wor".chars() {
        run(&ctx, &mut app, vec![egui::Event::Text(c.into())]);
    }
    for _ in 0..10 {
        run(&ctx, &mut app, vec![]);
        thread::sleep(Duration::from_millis(20));
    }
    assert!(app.auto.popup.is_some(), "completions are showing");
    // Move off the first entry so the list shows it can be chosen.
    run(&ctx, &mut app, vec![key(Key::ArrowDown, Modifiers::NONE)]);
    // Recovery is up to date, as it is a moment after typing.
    app.workspace.checkpoint().unwrap();
    app.persisted = app.generation;
    run(&ctx, &mut app, vec![]);
    let output = run(&ctx, &mut app, vec![]);
    super::rtl_tests::rasterize(&ctx, output, SIZE, SCALE)
}

#[test]
#[ignore = "writes the README screenshots to HEDIT_PREVIEW_DIR"]
fn export_readme_screenshots() {
    let output =
        PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    for (theme, name) in [("One Dark", "dark"), ("One Light", "light")] {
        std::fs::write(output.join(format!("screenshot-{name}.ppm")), scene(theme)).unwrap();
    }
}
