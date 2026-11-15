use super::*;
use crate::app::tests::{finish_app_job, frame};
fn app(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let dir = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    (dir, ctx, app)
}
#[test]
fn cold_manual_completion_does_not_index_a_large_document_on_the_ui_thread() {
    let text = format!("al\n{}", "alpha alpine\n".repeat(10_000));
    let (_dir, ctx, mut app) = app(&text);
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    app.open_completion(&ctx);
    assert!(app.auto.building.is_some());
    assert_eq!(app.auto.words_len(), 0);
}
#[test]
fn a_popup_from_another_tab_cannot_replace_the_current_tabs_prefix() {
    let (_dir, ctx, mut app) = app("al alpha alpine\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    app.open_completion(&ctx);
    assert!(app.auto.popup.is_some());
    app.execute(&ctx, Command::New);
    finish_app_job(&mut app, &ctx);
    let i = app.workspace.active;
    app.workspace.open[i].page.text = "al".into();
    app.workspace.edit(i, "").unwrap();
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    app.commit_choice(0);
    assert_eq!(app.active().page.text, "al");
}

fn finish_completion(ctx: &egui::Context, app: &mut HeditApp) {
    let start = Instant::now();
    loop {
        frame(ctx, app, vec![]);
        if app.auto.building.is_none() && app.auto.ranking.is_none() {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn large_completion_is_ranked_in_background_and_escape_prevents_a_late_popup() {
    let source = format!(
        "va\n{}",
        (0..20_000)
            .map(|i| format!("value_{i}\n"))
            .collect::<String>()
    );
    let (_dir, ctx, mut app) = app(&source);
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    app.open_completion(&ctx);
    finish_completion(&ctx, &mut app);
    assert!(app.auto.popup.is_some());
    let query = app.completion_query().unwrap();
    app.auto.dismiss();
    app.auto.intent = Some(query);
    app.refresh_choices(&ctx);
    assert!(app.auto.ranking.is_some());
    frame(
        &ctx,
        &mut app,
        vec![crate::app::tests::key(Key::Escape, Modifiers::NONE)],
    );
    finish_completion(&ctx, &mut app);
    assert!(!app.auto.active());
}
#[test]
fn cancelled_builds_keep_their_slot_until_finished_and_cannot_publish_stale_words() {
    let (_dir, ctx, mut app) = app("new_prefix\nne");
    app.set_selections(vec![hedit::advanced::Selection::caret(13)]);
    app.auto.wanted = true;
    let key = app.completion_key();
    let (tx, rx) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    app.auto.building_key = key.clone();
    app.auto.building = Some(Job {
        cancel: cancel.clone(),
        rx,
    });
    let old = app.active().page.text.clone();
    app.workspace.open[0].page.text.push('w');
    app.workspace.edit(0, &old).unwrap();
    for _ in 0..20 {
        app.maintain_index(&ctx, true);
        assert!(Arc::ptr_eq(
            &app.auto.building.as_ref().unwrap().cancel,
            &cancel
        ));
    }
    assert!(cancel.load(Ordering::Relaxed));
    tx.send(Build {
        key,
        words: completion::index_cancellable(&["secret_from_stale_scope"], || false),
    })
    .unwrap();
    app.maintain_index(&ctx, true);
    assert!(app.auto.building.is_none());
    assert!(!app
        .auto
        .words
        .iter()
        .any(|w| w.text == "secret_from_stale_scope"));
    assert!(app.auto.words.iter().any(|w| w.text == "new_prefix"));
}
#[test]
fn closing_an_indexed_file_clears_its_words_even_with_suggestions_disabled() {
    let (_dir, ctx, mut app) = app("secret_word_from_closed_file\nal");
    app.set_selections(vec![hedit::advanced::Selection::caret(30)]);
    app.open_completion(&ctx);
    let weak = Arc::downgrade(&app.auto.words);
    app.workspace.new_buffer().unwrap();
    app.workspace.open.remove(0);
    app.workspace.active = 0;
    app.settings.auto_complete = false;
    app.auto.dismiss();
    app.maintain_index(&ctx, false);
    assert_eq!(app.auto.words_len(), 0);
    assert!(weak.upgrade().is_none());
}
#[test]
fn completion_snapshot_budget_prioritizes_the_active_file_and_preserves_utf8() {
    let (_dir, _ctx, mut app) = app("");
    app.workspace.open[0].page.text = "界".repeat(1_500_000);
    app.workspace.edit(0, "").unwrap();
    app.workspace.open[0].page.info.paged = false;
    for _ in 0..2 {
        app.workspace.new_buffer().unwrap();
        let i = app.workspace.active;
        app.workspace.open[i].page.text = "α".repeat(2_200_000);
        app.workspace.edit(i, "").unwrap();
        app.workspace.open[i].page.info.paged = false;
    }
    let key = app.completion_key();
    assert_eq!(key[0].id, app.active().view.id);
    assert!(key.iter().map(|s| s.end).sum::<usize>() <= SNAPSHOT_BYTES);
    assert!(key.len() <= SNAPSHOT_FILES);
    for s in key {
        let b = app
            .workspace
            .open
            .iter()
            .find(|b| b.view.id == s.id)
            .unwrap();
        assert!(b.page.text.is_char_boundary(s.end));
    }
}
#[test]
fn delayed_completion_is_cancelled_by_escape_tab_switch_and_cursor_movement() {
    for action in 0..3 {
        let (_dir, ctx, mut app) = app("alpha alpine\nal");
        let end = app.active().page.text.chars().count();
        app.set_selections(vec![hedit::advanced::Selection::caret(end)]);
        frame(&ctx, &mut app, vec![]);
        app.settings.auto_complete_delay = 500;
        frame(&ctx, &mut app, vec![egui::Event::Text("p".into())]);
        assert!(app.auto.due.is_some());
        if action == 0 {
            frame(
                &ctx,
                &mut app,
                vec![crate::app::tests::key(Key::Escape, Modifiers::NONE)],
            );
        } else if action == 1 {
            app.workspace.new_buffer().unwrap();
        } else {
            frame(
                &ctx,
                &mut app,
                vec![crate::app::tests::key(Key::ArrowLeft, Modifiers::NONE)],
            );
        }
        if let Some(due) = &mut app.auto.due {
            due.at = Instant::now() - Duration::from_millis(1);
        }
        frame(&ctx, &mut app, vec![]);
        assert!(
            !app.auto.active(),
            "action {action}: due={} intent={} popup={}",
            app.auto.due.is_some(),
            app.auto.intent.is_some(),
            app.auto.popup.is_some()
        );
    }
}
#[test]
fn a_revision_or_pane_change_and_batched_text_cannot_accept_an_old_choice() {
    for action in 0..3 {
        let (_dir, ctx, mut app) = app("al alpha alpine\n");
        app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
        app.open_completion(&ctx);
        if action == 0 {
            let old = app.active().page.text.clone();
            app.workspace.open[0].page.text.push('x');
            app.workspace.edit(0, &old).unwrap();
            app.commit_choice(0);
            assert_eq!(app.active().page.text, "al alpha alpine\nx");
        } else if action == 1 {
            app.panes.mode = 1;
            app.panes.focused = 1;
            app.commit_choice(0);
            assert_eq!(app.active().page.text, "al alpha alpine\n");
        } else if action == 2 {
            frame(
                &ctx,
                &mut app,
                vec![
                    egui::Event::Text("z".into()),
                    crate::app::tests::key(Key::Tab, Modifiers::NONE),
                ],
            );
            assert!(!app.active().page.text.starts_with("alpha"));
            assert!(
                app.active().page.text.contains('z'),
                "{:?}",
                app.active().page.text
            );
        }
    }
}

#[test]
fn mouse_clicks_accept_choices_and_explicit_navigation_keeps_the_highlighted_choice() {
    let (_dir, ctx, mut app) = app("al alpha alpine\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    frame(&ctx, &mut app, vec![]);
    app.open_completion(&ctx);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    frame(
        &ctx,
        &mut app,
        vec![crate::app::tests::key(Key::ArrowDown, Modifiers::NONE)],
    );
    let popup = app.auto.popup.as_ref().unwrap();
    let selected = match &popup.choices[popup.selected] {
        Choice::Word(w) => w.clone(),
        _ => panic!(),
    };
    let query = popup.query.clone();
    app.auto.version += 1;
    app.publish_choices(query, vec!["altered_result".into()]);
    let popup = app.auto.popup.as_ref().unwrap();
    assert!(matches!(&popup.choices[popup.selected], Choice::Word(w) if w == &selected));
    let rect = egui::AreaState::load(&ctx, egui::Id::new("completion-popup"))
        .unwrap()
        .rect();
    let pos = rect.left_top() + egui::vec2(20.0, 16.0);
    crate::app::tests::click_at(&ctx, &mut app, pos, egui::vec2(1280.0, 820.0));
    assert_eq!(app.active().page.text, "alpha alpha alpine\n");
    assert!(!app.auto.visible());
}
#[test]
fn ranking_cancellation_retains_one_slot_and_rejects_a_late_answer() {
    let (_dir, ctx, mut app) = app("va value_one value_two\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    app.auto.wanted = true;
    app.maintain_index(&ctx, true);
    let old = app.completion_query().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let cancel = Arc::new(AtomicBool::new(false));
    app.auto.ranking = Some(Job {
        cancel: cancel.clone(),
        rx,
    });
    app.auto.ranking_key = Some((old.clone(), app.auto.version));
    let mut current = old.clone();
    current.typed = "val".into();
    current.head = 3;
    app.auto.intent = Some(current);
    for _ in 0..20 {
        app.refresh_choices(&ctx);
        assert!(Arc::ptr_eq(
            &app.auto.ranking.as_ref().unwrap().cancel,
            &cancel
        ));
    }
    assert!(cancel.load(Ordering::Relaxed));
    tx.send(Ranked {
        query: old,
        version: app.auto.version,
        words: Some(vec!["stale_answer".into()]),
    })
    .unwrap();
    app.refresh_choices(&ctx);
    assert!(app.auto.ranking.is_none());
    assert!(!app
        .auto
        .popup
        .as_ref()
        .unwrap()
        .choices
        .iter()
        .any(|c| matches!(c,Choice::Word(w) if w=="stale_answer")));
}
#[test]
fn snapshot_file_count_is_bounded_and_manual_completion_works_with_auto_disabled() {
    let (_dir, ctx, mut app) = app("first_word\n");
    for _ in 0..SNAPSHOT_FILES + 3 {
        app.workspace.new_buffer().unwrap();
    }
    let i = app.workspace.active;
    app.workspace.open[i].page.text = "al alpha alpine\n".into();
    app.workspace.edit(i, "").unwrap();
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    let key = app.completion_key();
    assert_eq!(key.len(), SNAPSHOT_FILES);
    assert_eq!(key[0].id, app.active().view.id);
    app.settings.auto_complete = false;
    app.open_completion(&ctx);
    assert!(app.auto.popup.is_some());
    assert!(app.auto.limited);
    app.commit_choice(0);
    assert_eq!(app.active().page.text, "alpha alpha alpine\n");
}
#[test]
#[ignore = "manual release profile of completion prefix lookup"]
fn profile_completion_prefix() {
    let text = format!(
        "{}\nvalue_12",
        "let value = compute(item);\n".repeat(60_000)
    );
    let index = editor_layout::TextIndex::new(&text);
    let cursor = index.chars;
    let old = Instant::now();
    for _ in 0..200 {
        std::hint::black_box(completion::typed_word(std::hint::black_box(&text), cursor));
    }
    let old_us = old.elapsed().as_secs_f64() * 1e6 / 200.0;
    let new = Instant::now();
    for _ in 0..10_000 {
        let byte = index.byte(&text, cursor);
        std::hint::black_box(completion::typed_word_at(
            std::hint::black_box(&text),
            byte,
            cursor,
        ));
    }
    eprintln!(
        "Completion prefix, {} bytes: linear {:.2} us, indexed {:.3} us",
        text.len(),
        old_us,
        new.elapsed().as_secs_f64() * 1e6 / 10_000.0
    );
}

#[test]
fn selection_navigation_and_accept_in_one_batch_cannot_commit_a_stale_choice() {
    let (_dir, ctx, mut app) = app("al alpha alpine\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    frame(&ctx, &mut app, vec![]);
    app.open_completion(&ctx);
    frame(
        &ctx,
        &mut app,
        vec![
            crate::app::tests::key(Key::ArrowDown, Modifiers::SHIFT),
            crate::app::tests::key(Key::Tab, Modifiers::NONE),
        ],
    );
    assert!(!app.active().page.text.starts_with("alpha"));
    assert!(!app.auto.visible());
}

#[test]
fn completion_invoked_from_the_command_palette_survives_editor_focus_transfer() {
    let (_dir, ctx, mut app) = app("al alpha alpine\n");
    app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
    frame(&ctx, &mut app, vec![]);
    app.execute(&ctx, Command::Palette);
    frame(&ctx, &mut app, vec![]);
    app.palette_query = "Complete word".into();
    frame(&ctx, &mut app, vec![]);
    frame(
        &ctx,
        &mut app,
        vec![crate::app::tests::key(Key::Enter, Modifiers::NONE)],
    );
    frame(&ctx, &mut app, vec![]);
    assert!(!app.palette);
    assert!(app.auto.popup.is_some());
    frame(
        &ctx,
        &mut app,
        vec![crate::app::tests::key(Key::Tab, Modifiers::NONE)],
    );
    assert_eq!(app.active().page.text, "alpha alpha alpine\n");
}
