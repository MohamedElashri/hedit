//! Go to Anything: @symbol, :line, and #text in Go to file.
use super::goto_anything::{parse, Target};
use super::tests::{finish_app_job, frame, key};
use super::*;

#[test]
fn queries_split_into_a_file_and_a_target() {
    assert_eq!(parse("main"), ("main", None));
    assert_eq!(
        parse("app.rs@save"),
        ("app.rs", Some(Target::Symbol("save".into())))
    );
    assert_eq!(parse(":42:7"), ("", Some(Target::Line(Some(42), Some(7)))));
    assert_eq!(
        parse("kernel.cu:12"),
        ("kernel.cu", Some(Target::Line(Some(12), None)))
    );
    assert_eq!(parse(":x"), ("", Some(Target::Line(None, None))));
    assert_eq!(
        parse("#todo item"),
        ("", Some(Target::Text("todo item".into())))
    );
}

fn line_of(app: &HeditApp) -> usize {
    let b = app.active();
    b.page
        .text
        .chars()
        .take(b.view.head)
        .filter(|&c| c == '\n')
        .count()
        + 1
}

fn palette(ctx: &egui::Context, app: &mut HeditApp, query: &str) {
    app.execute(ctx, Command::Switcher);
    frame(ctx, app, vec![]);
    frame(ctx, app, vec![egui::Event::Text(query.into())]);
}

#[test]
fn symbols_lines_and_text_are_previewed_then_chosen_or_cancelled() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text =
        "fn alpha() {\n}\n\nfn beta_value() {\n    // needle here\n}\n".into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    palette(&ctx, &mut app, "@beta");
    assert_eq!(line_of(&app), 4, "the first match is previewed");
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    assert!(!app.palette);
    assert_eq!(line_of(&app), 1, "Escape returns to where you were");
    palette(&ctx, &mut app, "#needle");
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert!(!app.palette);
    assert_eq!(line_of(&app), 5);
    palette(&ctx, &mut app, ":2");
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    assert_eq!(line_of(&app), 2);
    // Back returns to the line before the jump.
    app.execute(&ctx, Command::NavBack);
    finish_app_job(&mut app, &ctx);
    assert_eq!(line_of(&app), 5);
}

#[test]
fn a_file_name_before_the_target_opens_it_there() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("src");
    std::fs::create_dir(&source).unwrap();
    std::fs::write(source.join("other.rs"), "// top\n\nfn gamma() {}\n").unwrap();
    let (workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.set_folder(hedit::folder::read_directory(source.clone()).unwrap());
    frame(&ctx, &mut app, vec![]);
    palette(&ctx, &mut app, "other@gamma");
    // Wait for the folder search to list the file.
    let started = Instant::now();
    while !app.project.search.as_ref().is_some_and(|r| r.is_ok()) {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
        frame(&ctx, &mut app, vec![]);
    }
    frame(&ctx, &mut app, vec![key(Key::Enter, Modifiers::NONE)]);
    finish_app_job(&mut app, &ctx);
    finish_app_job(&mut app, &ctx);
    assert!(app
        .active()
        .page
        .info
        .path
        .as_deref()
        .unwrap()
        .ends_with("other.rs"));
    assert_eq!(line_of(&app), 3);
}

#[test]
fn find_selects_matches_while_typing_and_highlights_them_all() {
    let text = "one two\nthree two\nfour\n";
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    // Find starts searching from the cursor.
    workspace.open[0].view.anchor = 5;
    workspace.open[0].view.head = 5;
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![key(Key::F, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, vec![egui::Event::Text("t".into())]);
    let selected = |app: &HeditApp| (app.active().view.anchor, app.active().view.head);
    assert_eq!(selected(&app), (8, 9), "first “t” after the cursor");
    frame(&ctx, &mut app, vec![egui::Event::Text("wo".into())]);
    assert_eq!(selected(&app), (14, 17), "“two” after the cursor");
    assert_eq!(app.quality.match_status, "2 of 2");
    assert_eq!(
        app.find_highlights().unwrap().as_slice(),
        [(4, 7), (14, 17)]
    );
    frame(&ctx, &mut app, vec![egui::Event::Text("x".into())]);
    assert_eq!(selected(&app), (5, 5), "no match returns to the start");
    assert_eq!(app.quality.match_status, "No matches");
    assert_eq!(app.active().page.text, text);
    // Without Find, selecting a word outlines its other occurrences.
    frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 7 }]);
    assert_eq!(
        app.selection_highlights().unwrap().as_slice(),
        [(4, 7), (14, 17)]
    );
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 6 }]);
    assert!(app.selection_highlights().is_none(), "only whole words");
    app.settings.match_selection = false;
    app.set_selections(vec![hedit::advanced::Selection { anchor: 4, head: 7 }]);
    assert!(app.selection_highlights().is_none());
}
