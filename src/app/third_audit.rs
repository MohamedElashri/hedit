use super::tests::{finish_app_job, frame};
use super::*;
use hedit::{
    advanced::{self, Selection as S},
    search,
};
use std::{fs, path::Path};

fn app_with(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let dir = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.focus_editor = true;
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    (dir, ctx, app)
}

fn range(anchor: usize, head: usize) -> S {
    S { anchor, head }
}

fn open(app: &mut HeditApp, ctx: &egui::Context, path: &Path) {
    app.open(ctx, Some(path.to_owned()));
    finish_app_job(app, ctx);
}

#[test]
fn paste_indented_keeps_nonbreaking_space_on_its_own_line() {
    assert_eq!(
        clipboard_lines::reindent_paste("first\n\u{a0}\nlast", "", true, 4),
        "first\n\u{a0}\nlast"
    );
}

#[test]
fn paste_indented_preserves_relative_tab_width_at_a_different_tab_stop() {
    let output = clipboard_lines::reindent_paste("first\n\tsecond", "  ", true, 4);
    let indent: String = output
        .lines()
        .nth(1)
        .unwrap()
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect();
    assert_eq!(
        advanced::line_column(&indent, indent.chars().count(), 4).1,
        6,
        "{output:?}"
    );
}

#[test]
fn quick_find_moves_in_a_dense_document_beyond_the_highlight_limit() {
    let (_dir, _ctx, mut app) = app_with(&"item ".repeat(search::MATCH_LIMIT + 2));
    app.set_selections(vec![range(0, 4)]);
    app.quick_find(true);
    assert_eq!(app.selections(), [range(5, 9)]);
}

#[test]
fn select_all_find_does_not_use_stale_selection_scope_from_another_tab() {
    let (dir, ctx, mut app) = app_with("item item");
    let prior = app.active().view.id;
    let path = dir.path().join("named.txt");
    fs::write(&path, "item item").unwrap();
    open(&mut app, &ctx, &path);
    app.quality.selection_only = true;
    app.quality.scope = Some((prior, 0, 4));
    app.query = "item".into();
    app.set_selections(vec![S::caret(1)]);
    app.select_all_matches();
    assert_eq!(app.selections(), [S::caret(1)]);
}

#[test]
fn macro_paste_timeout_does_not_change_the_next_ordinary_paste() {
    let (_dir, ctx, mut app) = app_with("");
    app.play_macro(vec![macros::Step::PasteIndent]);
    let _ = ctx.run(Default::default(), |ctx| app.play_step(ctx));
    assert!(app.paste_indent);
    thread::sleep(Duration::from_millis(1050));
    let _ = ctx.run(Default::default(), |ctx| app.play_step(ctx));
    assert!(!app.paste_indent);
}

#[test]
fn ctrl_tab_starts_relative_to_the_tab_selected_since_the_last_frame() {
    let (dir, ctx, mut app) = app_with("scratch");
    let first = dir.path().join("a.txt");
    let second = dir.path().join("b.txt");
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    open(&mut app, &ctx, &first);
    open(&mut app, &ctx, &second);
    frame(&ctx, &mut app, vec![]);
    let a = app
        .workspace
        .open
        .iter()
        .position(|b| b.view.name == "a.txt")
        .unwrap();
    app.select_tab(a);
    app.cycle_recent(false);
    assert_eq!(app.active().view.name, "b.txt");
}

#[test]
fn quoted_location_round_trips_an_at_sign_after_a_literal_quote() {
    let (dir, ctx, mut app) = app_with("");
    let path = dir.path().join("sales\"@sun.txt");
    if !cfg!(unix) {
        return;
    }
    fs::write(&path, "\nprice\n").unwrap();
    open(&mut app, &ctx, &path);
    app.set_selections(vec![S::caret(3)]);
    let output = ctx.run(Default::default(), |ctx| app.copy_file_location(ctx));
    let copied = output
        .platform_output
        .commands
        .into_iter()
        .find_map(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                Some(text)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(
        goto_anything::parse_file_query(&copied),
        (
            app.active().page.info.path.as_deref().unwrap().into(),
            Some(goto_anything::Target::Line(Some(2), Some(3)))
        )
    );
}

#[test]
fn navigation_back_skips_an_explicitly_discarded_tab() {
    let (_dir, ctx, mut app) = app_with("abc def");
    let kept = app.active().view.id;
    app.set_selections(vec![S::caret(2)]);
    app.navigation_mark();
    app.execute(&ctx, Command::New);
    let discarded = app.active().view.id;
    app.navigation_mark();
    app.execute(&ctx, Command::New);
    app.navigation_mark();
    let index = app
        .workspace
        .open
        .iter()
        .position(|b| b.view.id == discarded)
        .unwrap();
    app.workspace.forget(index).unwrap();
    app.navigate(&ctx, false);
    finish_app_job(&mut app, &ctx);
    assert_eq!(app.active().view.id, kept);
    assert_eq!(app.active().view.head, 2);
}
