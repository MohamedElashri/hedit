use super::tests::frame;
#[cfg(feature = "terminal")]
use super::tests::key;
use super::*;

fn app_in(directory: &std::path::Path) -> (egui::Context, HeditApp) {
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = Some(directory.join("config")));
    let (workspace, _) = Workspace::restore(directory.join("session")).unwrap();
    let ctx = egui::Context::default();
    let app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    (ctx, app)
}

#[test]
fn terminal_setup_is_opt_in_persisted_and_does_not_start_a_shell() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    assert!(!app.settings.terminal_enabled);
    assert!(!app.terminal.open);
    assert_eq!(app.terminal_onboarding_due(), cfg!(feature = "terminal"));
    app.execute(&ctx, Command::Terminal);
    assert!(
        !app.terminal.open,
        "opening is disabled until enabled in settings"
    );
    app.choose_terminal(false);
    drop(app);
    let (_, mut app) = app_in(directory.path());
    assert!(!app.terminal_onboarding_due());
    assert!(!app.settings.terminal_enabled);
    app.choose_terminal(true);
    drop(app);
    let (_, app) = app_in(directory.path());
    assert!(app.settings.terminal_enabled);
    assert!(!app.terminal.open);
    #[cfg(feature = "terminal")]
    assert!(app.terminal.tabs.is_empty());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn existing_installations_do_not_get_first_install_setup() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("config")).unwrap();
    std::fs::write(
        directory.path().join("config/settings.json"),
        "{\"font_size\":16}",
    )
    .unwrap();
    let (_, app) = app_in(directory.path());
    assert!(!app.terminal_onboarding_due());
    assert!(!app.settings.terminal_enabled);
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn terminal_settings_are_available_and_render_in_all_builds() {
    let directory = tempfile::tempdir().unwrap();
    let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    let ctx = egui::Context::default();
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.execute(&ctx, Command::TerminalSettings);
    assert!(app.preferences_open);
    assert_eq!(app.preferences_page, PreferencesPage::Terminal);
    frame(&ctx, &mut app, vec![]);
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_hide_keeps_session_and_disabling_ends_it_without_editing_buffer() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.execute(&ctx, Command::Terminal);
    assert!(app.terminal.open, "{:?}", app.error);
    assert!(!app.terminal.tabs.is_empty());
    let before = app.active().page.text.clone();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 700.0),
        )),
        focused: true,
        ..Default::default()
    };
    let _ = ctx.run(input, |ctx| {
        app.chrome(ctx);
        app.terminal_panel(ctx);
        app.editor(ctx);
    });
    // Wait for a real command response before sending control keys. On Windows
    // those keys can affect cmd.exe startup before its input handler is ready.
    app.terminal
        .active_session_mut()
        .unwrap()
        .write(b"echo HEDIT_SHELL_READY\r".to_vec())
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let session = app.terminal.active_session_mut().unwrap();
        session.poll();
        let grid = session.grid().unwrap();
        let lines: Vec<String> = grid
            .rows
            .iter()
            .map(|row| row.iter().map(|c| c.text.as_str()).collect())
            .collect();
        if lines.iter().any(|line| line.trim() == "HEDIT_SHELL_READY") {
            break;
        }
        assert!(
            Instant::now() < until && session.running,
            "shell did not become ready: {} ({lines:?})",
            session.status
        );
        thread::sleep(Duration::from_millis(10));
    }
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            events: vec![
                key(Key::Escape, Modifiers::NONE),
                key(Key::A, Modifiers::CTRL),
            ],
            ..Default::default()
        },
        |ctx| app.shortcuts(ctx),
    );
    assert_eq!(app.active().page.text, before);
    assert!(app.terminal.open, "Escape belongs to the terminal");
    assert!(!app.focus_editor);
    // winit turns Ctrl+C into Copy on Linux/Windows. That event must still
    // interrupt the shell; ordinary Text and Enter must reach the PTY.
    for (modifiers, events) in [
        (Modifiers::CTRL, vec![egui::Event::Copy]),
        (
            Modifiers::NONE,
            vec![
                egui::Event::Text("echo HEDIT_UI_KEY_INPUT".into()),
                key(Key::Enter, Modifiers::NONE),
            ],
        ),
    ] {
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                modifiers,
                events,
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("integrated-terminal-input")));
                app.shortcuts(ctx);
            },
        );
    }
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let session = app.terminal.active_session_mut().unwrap();
        session.poll();
        let grid = session.grid().unwrap();
        let text: String = grid
            .rows
            .iter()
            .flat_map(|r| r.iter().map(|c| c.text.as_str()))
            .collect();
        if text.contains("HEDIT_UI_KEY_INPUT") {
            break;
        }
        assert!(
            Instant::now() < until,
            "text did not reach the shell: {}",
            session.status
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        app.terminal.active_session().unwrap().running,
        "interrupting must leave the interactive shell running"
    );
    assert_eq!(app.active().page.text, before);
    app.terminal
        .active_session_mut()
        .unwrap()
        .vt
        .vt_write(b"PRESERVED_SESSION");
    app.toggle_terminal(&ctx);
    assert!(!app.terminal.open);
    assert!(!app.terminal.tabs.is_empty());
    app.toggle_terminal(&ctx);
    let grid = app.terminal.active_session_mut().unwrap().grid().unwrap();
    let text: String = grid
        .rows
        .iter()
        .flat_map(|r| r.iter().map(|c| c.text.as_str()))
        .collect();
    assert!(text.contains("PRESERVED_SESSION"));
    // A rebound toggle works while the shell has focus, including a chord.
    app.quality.config.keybindings = vec![quality::Binding {
        command: Command::Terminal,
        shortcut: "Primary+K, Primary+T".into(),
        editor_only: false,
    }];
    for pressed in [Key::K, Key::T] {
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![key(pressed, Modifiers::COMMAND)],
                ..Default::default()
            },
            |ctx| {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new("integrated-terminal-input")));
                app.shortcuts(ctx);
            },
        );
    }
    assert!(!app.terminal.open);
    assert!(!app.terminal.tabs.is_empty());
    app.settings.terminal_enabled = false;
    app.poll_terminal(&ctx);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.open);
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[cfg(feature = "terminal")]
#[test]
#[ignore = "exports terminal and onboarding previews to /tmp"]
fn export_terminal_previews() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    let size = egui::vec2(1100.0, 760.0);
    let mut textures = std::collections::HashMap::new();
    for name in ["onboarding", "panel", "settings", "compact", "light"] {
        if name == "panel" {
            app.choose_terminal(true);
            app.toggle_terminal(&ctx);
            app.terminal.active_session_mut().unwrap().vt.reset();
            app.terminal.active_session_mut().unwrap().vt.vt_write(
                b"\x1b[32mproject\x1b[0m $ cargo check\r\n\x1b[32m    Finished\x1b[0m dev profile in 0.42s\r\n\r\n\x1b[32mproject\x1b[0m $ ");
            app.terminal.tabs[0].name = "Project".into();
            app.new_terminal(&ctx);
            app.terminal.tabs[1].name = "العربية".into();
            let session = app.terminal.active_session_mut().unwrap();
            session.vt.reset();
            session.vt.vt_write("\x1b[32mproject\x1b[0m $ printf 'مرحبا بالعالم'\r\nمرحبا بالعالم\r\nالسَّلَامُ عَلَيْكُمْ\r\nملف 123 (report.txt)\r\nשלום עולם\r\n\r\n\x1b[32mproject\x1b[0m $ ".as_bytes());
        }
        if name == "settings" {
            app.open_preferences(PreferencesPage::Terminal);
        }
        if matches!(name, "compact" | "light") {
            app.preferences_open = false;
            app.terminal.focus = true;
        }
        if name == "compact" {
            app.terminal.tabs[0].name = "Build · a project with a long terminal title".into();
        }
        if name == "light" {
            app.terminal.tabs[0].name = "Project".into();
            app.set_theme(&ctx, hedit::theme::builtin(hedit::theme::PAPER).unwrap());
        }
        let size = if name == "compact" {
            egui::vec2(480.0, 600.0)
        } else {
            size
        };
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    app.chrome(ctx);
                    app.terminal_panel(ctx);
                    app.editor(ctx);
                    app.preferences(ctx);
                    app.terminal_onboarding(ctx);
                },
            ));
            for (id, delta) in &output.as_ref().unwrap().textures_delta.set {
                if delta.pos.is_none() {
                    if let egui::ImageData::Color(image) = &delta.image {
                        textures.insert(*id, (**image).clone());
                    }
                }
            }
        }
        std::fs::write(
            format!("/tmp/hedit-terminal-{name}.ppm"),
            super::rtl_tests::rasterize_with_textures(&ctx, output.unwrap(), size, 1.0, &textures),
        )
        .unwrap();
    }
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[cfg(feature = "terminal")]
fn terminal_frame(
    ctx: &egui::Context,
    app: &mut HeditApp,
    events: Vec<egui::Event>,
    modifiers: Modifiers,
) -> egui::FullOutput {
    terminal_frame_size(ctx, app, events, modifiers, egui::vec2(1000.0, 720.0))
}

#[cfg(feature = "terminal")]
fn terminal_frame_size(
    ctx: &egui::Context,
    app: &mut HeditApp,
    events: Vec<egui::Event>,
    modifiers: Modifiers,
    size: egui::Vec2,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            focused: true,
            events,
            modifiers,
            ..Default::default()
        },
        |ctx| {
            app.shortcuts(ctx);
            app.chrome(ctx);
            app.terminal_panel(ctx);
            app.editor(ctx);
        },
    )
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_tabs_switch_close_restart_and_route_their_own_input() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.new_terminal(&ctx);
    let first = app.terminal.tabs[0].id;
    app.terminal.tabs[0].name = "Project".into();
    app.new_terminal(&ctx);
    let second = app.terminal.tabs[1].id;
    for (index, marker) in [(0, "HEDIT_TAB_ONE"), (1, "HEDIT_TAB_TWO")] {
        app.terminal.tabs[index]
            .session
            .paste(&format!("echo {marker}\n"))
            .unwrap();
    }
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        app.poll_terminal(&ctx);
        let texts: Vec<String> = app
            .terminal
            .tabs
            .iter_mut()
            .map(|t| {
                t.session
                    .grid()
                    .unwrap()
                    .rows
                    .iter()
                    .flat_map(|r| r.iter().map(|c| c.text.as_str()))
                    .collect()
            })
            .collect();
        assert!(!texts[0].contains("HEDIT_TAB_TWO"));
        assert!(!texts[1].contains("HEDIT_TAB_ONE"));
        if texts[0].contains("HEDIT_TAB_ONE") && texts[1].contains("HEDIT_TAB_TWO") {
            break;
        }
        assert!(
            Instant::now() < until,
            "tabs did not receive their own output"
        );
        thread::sleep(Duration::from_millis(10));
    }
    app.toggle_terminal(&ctx);
    assert!(!app.terminal.open);
    assert_eq!(app.terminal.tabs.len(), 2);
    app.toggle_terminal(&ctx);
    app.execute(&ctx, Command::PreviousTerminal);
    assert_eq!(app.terminal.tabs[app.terminal.active].id, first);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::COMMAND
    };
    terminal_frame(&ctx, &mut app, vec![key(Key::T, shift)], shift);
    assert_eq!(app.terminal.tabs.len(), 3);
    assert_eq!(app.terminal.active, 2);
    let third = app.terminal.tabs[2].id;
    terminal_frame(
        &ctx,
        &mut app,
        vec![key(Key::PageUp, Modifiers::CTRL)],
        Modifiers::CTRL,
    );
    assert_eq!(app.terminal.tabs[app.terminal.active].id, second);
    terminal_frame(&ctx, &mut app, vec![key(Key::W, shift)], shift);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.terminal.tabs[app.terminal.active].id, third);
    app.close_terminal(&ctx, 0);
    assert_eq!(app.terminal.active, 0);
    assert_eq!(app.terminal.tabs[0].id, third);
    app.terminal.tabs[0].name = "العربية".into();
    app.terminal.tabs[0].session.vt.vt_write(b"BEFORE_RESTART");
    app.terminal.tabs[0].session.stop();
    assert!(!app.terminal.tabs[0].session.running);
    let text: String = app.terminal.tabs[0]
        .session
        .grid()
        .unwrap()
        .rows
        .iter()
        .flat_map(|r| r.iter().map(|c| c.text.as_str()))
        .collect();
    assert!(text.contains("BEFORE_RESTART"));
    app.execute(&ctx, Command::RestartTerminal);
    assert!(app.terminal.tabs[0].session.running);
    assert_eq!(app.terminal.tabs[0].id, third);
    assert_eq!(app.terminal.tabs[0].name, "العربية");
    app.execute(&ctx, Command::CloseTerminal);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.open);
    assert_eq!(
        app.workspace.open.len(),
        1,
        "terminal shortcuts must not close or reopen editor tabs"
    );
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[cfg(feature = "terminal")]
#[test]
fn arabic_terminal_selection_copies_logical_unicode_after_visual_drag() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.new_terminal(&ctx);
    let session = app.terminal.active_session_mut().unwrap();
    session.vt.reset();
    session.vt.vt_write("سَلَام".as_bytes());
    for _ in 0..2 {
        terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    }
    let rect = ctx
        .read_response(egui::Id::new("integrated-terminal-input"))
        .unwrap()
        .rect;
    let cell = ctx.fonts(|f| {
        egui::vec2(
            f.glyph_width(&FontId::monospace(app.settings.font_size), ' '),
            f.row_height(&FontId::monospace(app.settings.font_size)),
        )
    });
    let left = rect.min + egui::vec2(cell.x * 0.5, cell.y * 0.5);
    let right = rect.min + egui::vec2(cell.x * 3.5, cell.y * 0.5);
    terminal_frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(left),
            egui::Event::PointerButton {
                pos: left,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
        Modifiers::NONE,
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(right)],
        Modifiers::NONE,
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerButton {
            pos: right,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
        Modifiers::NONE,
    );
    let output = terminal_frame(
        &ctx,
        &mut app,
        vec![egui::Event::Copy],
        Modifiers {
            shift: true,
            ..Modifiers::CTRL
        },
    );
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "سَلَام")),
        "copy should preserve logical letters and harakat"
    );
    assert_eq!(app.active().page.text, "");
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_ime_preedit_stays_out_of_the_shell_until_committed() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.new_terminal(&ctx);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let output = terminal_frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Preedit("echo مرحبا".into())),
            key(Key::Enter, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    assert!(
        output.platform_output.ime.is_some(),
        "native input methods need the terminal cursor rectangle"
    );
    let text: String = app
        .terminal
        .active_session_mut()
        .unwrap()
        .grid()
        .unwrap()
        .rows
        .iter()
        .flat_map(|r| r.iter().map(|c| c.text.as_str()))
        .collect();
    assert!(
        !text.contains("مرحبا"),
        "preedit must remain a display-only overlay"
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Commit("echo مرحبا".into())),
            key(Key::Enter, Modifiers::NONE),
        ],
        Modifiers::NONE,
    );
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        app.poll_terminal(&ctx);
        let text: String = app
            .terminal
            .active_session_mut()
            .unwrap()
            .grid()
            .unwrap()
            .rows
            .iter()
            .flat_map(|r| r.iter().map(|c| c.text.as_str()))
            .collect();
        if text.contains("مرحبا") {
            break;
        }
        assert!(
            Instant::now() < until,
            "the IME commit did not reach the shell"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.active().page.text, "");
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[cfg(feature = "terminal")]
#[test]
fn compact_terminal_tabs_support_pointer_creation_reordering_and_middle_close() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.new_terminal(&ctx);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let input = egui::Id::new("integrated-terminal-input");
    let bar = ctx
        .read_response(egui::Id::new("terminal-tab-bar"))
        .unwrap()
        .rect;
    assert!(
        bar.height() <= 32.0,
        "terminal chrome fits in a single compact row"
    );
    assert!(ctx.read_response(input).unwrap().rect.top() <= bar.bottom() + 8.0);
    let point = ctx
        .read_response(egui::Id::new("terminal-new-tab"))
        .unwrap()
        .rect
        .center();
    let button = |pos, button, pressed| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    };
    terminal_frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(point),
            button(point, egui::PointerButton::Primary, true),
        ],
        Modifiers::NONE,
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![button(point, egui::PointerButton::Primary, false)],
        Modifiers::NONE,
    );
    assert_eq!(app.terminal.tabs.len(), 2);
    app.new_terminal(&ctx);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let ids: Vec<_> = app.terminal.tabs.iter().map(|t| t.id).collect();
    let source = ctx
        .read_response(egui::Id::new(("terminal-tab", ids[0])))
        .unwrap()
        .rect
        .center();
    let target = ctx
        .read_response(egui::Id::new(("terminal-tab", ids[1])))
        .unwrap()
        .rect
        .center();
    terminal_frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(source),
            button(source, egui::PointerButton::Primary, true),
        ],
        Modifiers::NONE,
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(target)],
        Modifiers::NONE,
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![button(target, egui::PointerButton::Primary, false)],
        Modifiers::NONE,
    );
    assert_eq!(
        app.terminal.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
        [ids[1], ids[0], ids[2]]
    );
    assert_eq!(
        app.terminal.tabs[app.terminal.active].id, ids[2],
        "reordering preserves the active shell"
    );
    // egui's response geometry can be one pass old after moving widgets.
    for _ in 0..2 {
        terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    }
    let first = ctx
        .read_response(egui::Id::new(("terminal-tab", ids[1])))
        .unwrap()
        .rect
        .center();
    terminal_frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::PointerMoved(first),
            button(first, egui::PointerButton::Middle, true),
        ],
        Modifiers::NONE,
    );
    terminal_frame(
        &ctx,
        &mut app,
        vec![button(first, egui::PointerButton::Middle, false)],
        Modifiers::NONE,
    );
    assert_eq!(
        app.terminal.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
        [ids[0], ids[2]]
    );
    assert_eq!(app.terminal.tabs[app.terminal.active].id, ids[2]);
    assert_eq!(app.workspace.open.len(), 1);
    ctx.style_mut(|style| style.scroll_animation = egui::style::ScrollAnimation::none());
    app.terminal.focus = false;
    for _ in 0..3 {
        terminal_frame_size(
            &ctx,
            &mut app,
            vec![],
            Modifiers::NONE,
            egui::vec2(480.0, 600.0),
        );
    }
    let close = ctx
        .read_response(egui::Id::new(("terminal-tab-close", ids[2])))
        .unwrap()
        .rect;
    let new = ctx
        .read_response(egui::Id::new("terminal-new-tab"))
        .unwrap()
        .rect;
    assert!(
        close.right() <= new.left(),
        "overflow scrolling must reveal the active tab's close button: close={close:?}, new={new:?}"
    );
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}
