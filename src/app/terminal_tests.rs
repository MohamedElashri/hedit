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

#[test]
fn invalid_terminal_font_drafts_do_not_block_other_settings_or_reach_disk() {
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(false);
    app.settings.terminal_font = Some(hedit::settings::TerminalFont::default());
    app.open_preferences(PreferencesPage::Terminal);
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    ctx.memory_mut(|m| m.request_focus(egui::Id::new("terminal-font-face")));
    // The limit is in bytes, while TextEdit's character limit counts scalars.
    frame(&ctx, &mut app, vec![egui::Event::Paste("界".repeat(2000))]);
    assert!(
        app.settings.validate().is_ok(),
        "invalid font text reached live settings"
    );
    assert!(app.settings.terminal_font.as_ref().unwrap().face.is_empty());
    app.set_browser_value("font_size", serde_json::json!(20.0))
        .unwrap();
    app.flush_settings();
    let (saved, _) = hedit::settings::Settings::load(&app.settings_path().unwrap()).unwrap();
    assert_eq!(saved.font_size, 20.0);
    assert!(saved.terminal_font.unwrap().face.is_empty());
    app.open_preferences(PreferencesPage::General);
    frame(&ctx, &mut app, vec![]);
    app.open_preferences(PreferencesPage::Terminal);
    frame(&ctx, &mut app, vec![]);
    frame(&ctx, &mut app, vec![]);
    let output = frame(&ctx, &mut app, vec![]);
    assert!(
        output.shapes.iter().any(|shape| super::tests::text_rect(
            &shape.shape,
            "terminal_font needs a valid font name/path and a size between 8 and 40."
        )
        .is_some()),
        "invalid draft lost its visible error after navigation"
    );
    super::tests::click_label(&ctx, &mut app, "Built-in (Hack)");
    let output = frame(&ctx, &mut app, vec![]);
    let choice = output
        .shapes
        .iter()
        .filter_map(|shape| {
            super::tests::text_rect(&shape.shape, "Built-in (Hack)")
                .filter(|rect| shape.clip_rect.contains(rect.center()))
        })
        .next_back()
        .expect("the built-in font choice is visible");
    super::tests::click_at(&ctx, &mut app, choice.center(), egui::vec2(1280.0, 820.0));
    assert!(app.settings_browser.terminal_font_error.is_none());
    assert!(app
        .settings_browser
        .terminal_font_draft
        .as_ref()
        .is_none_or(String::is_empty));
    // Reset also discards an invalid draft while keeping the app usable.
    ctx.memory_mut(|m| m.request_focus(egui::Id::new("terminal-font-face")));
    frame(&ctx, &mut app, vec![egui::Event::Paste("界".repeat(2000))]);
    super::tests::click_label(&ctx, &mut app, "Use app font and size");
    assert!(app.settings.terminal_font.is_none());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
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
        app.workbench_panel(ctx);
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
    let session_id = app.terminal.tabs[0].id;
    app.execute(&ctx, Command::LanguageProblems);
    assert!(app.languages.problems && !app.terminal.open);
    app.terminal
        .active_session_mut()
        .unwrap()
        .write(b"echo HIDDEN_PANEL_OUTPUT\r".to_vec())
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        app.poll_terminal(&ctx);
        let grid = app.terminal.active_session_mut().unwrap().grid().unwrap();
        let lines: Vec<String> = grid
            .rows
            .iter()
            .map(|row| row.iter().map(|c| c.text.as_str()).collect())
            .collect();
        if lines
            .iter()
            .any(|line| line.trim() == "HIDDEN_PANEL_OUTPUT")
        {
            break;
        }
        assert!(Instant::now() < until, "hidden shell output did not drain");
        thread::sleep(Duration::from_millis(10));
    }
    app.execute(&ctx, Command::Terminal);
    assert!(app.terminal.open && !app.languages.problems);
    assert_eq!(app.terminal.tabs[0].id, session_id);
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
    // Preset editor/workbench shortcuts remain shell input while it has focus.
    app.settings.keybinding_preset = hedit::settings::KeybindingPreset::Vscode;
    let sidebar = app.settings.show_sidebar;
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            events: vec![key(Key::B, Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("integrated-terminal-input")));
            app.shortcuts(ctx);
        },
    );
    assert_eq!(app.settings.show_sidebar, sidebar);
    assert!(app.terminal.open);
    // Explorer focus is a workbench action and preserves the live shell.
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            events: vec![key(Key::E, Modifiers::COMMAND | Modifiers::SHIFT)],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|m| m.request_focus(terminal_panel::input_id()));
            app.shortcuts(ctx);
        },
    );
    assert_eq!(
        ctx.memory(|m| m.focused()),
        Some(egui::Id::new("explorer-tree"))
    );
    assert!(app.terminal.open);
    assert_eq!(app.terminal.tabs[0].id, session_id);
    let _ = ctx.run(
        egui::RawInput {
            focused: true,
            events: vec![key(Key::Num1, Modifiers::COMMAND)],
            ..Default::default()
        },
        |ctx| {
            ctx.memory_mut(|m| m.request_focus(terminal_panel::input_id()));
            app.shortcuts(ctx);
        },
    );
    assert!(
        app.focus_editor,
        "Primary+1 focuses the already selected editor group from the terminal"
    );
    assert!(app.terminal.open);
    app.execute(&ctx, Command::FocusTerminal);
    assert!(!app.focus_editor);
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
    for name in [
        "onboarding",
        "panel",
        "settings",
        "compact",
        "light",
        "custom",
        "custom-settings",
    ] {
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
        if matches!(name, "custom" | "custom-settings") {
            app.settings.terminal_font = Some(hedit::settings::TerminalFont {
                face: String::new(),
                size: 18.0,
            });
            let dark = hedit::theme::builtin(hedit::theme::INK).unwrap();
            app.settings.terminal_colors = Some(hedit::settings::TerminalColors {
                ansi: Some(hedit::settings::TerminalColors::theme_palette(&dark)),
                ..hedit::settings::TerminalColors::from_theme(&dark)
            });
            app.apply_editor_style(&ctx);
            if name == "custom-settings" {
                app.open_preferences(PreferencesPage::Terminal);
            }
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
                    app.workbench_panel(ctx);
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
            app.workbench_panel(ctx);
            app.editor(ctx);
        },
    )
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_header_scales_without_changing_shell_or_terminal_typography() {
    use hedit::ui_style::{Density, Metrics};
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.new_terminal(&ctx);
    let id = app.terminal.tabs[0].id;
    let terminal_font = app.terminal_font_id();
    let size = egui::vec2(720.0, 480.0);
    for font in [10.0, 13.0, 20.0] {
        for density in [Density::Compact, Density::Default, Density::Comfortable] {
            app.settings.ui_font_size = font;
            app.settings.ui_density = density;
            for _ in 0..3 {
                terminal_frame_size(&ctx, &mut app, vec![], Modifiers::NONE, size);
            }
            let bar = ctx
                .read_response(egui::Id::new("terminal-tab-bar"))
                .unwrap()
                .rect;
            assert_eq!(bar.height(), Metrics::new(font, density).tab);
            for action in [
                egui::Id::new("terminal-new-tab"),
                egui::Id::new("terminal-hide"),
                egui::Id::new(("terminal-tab-close", id)),
            ] {
                let rect = ctx.read_response(action).unwrap().rect;
                assert!(bar.contains_rect(rect) && rect.width() >= 22.0 && rect.height() >= 22.0);
            }
            assert_eq!(app.terminal.tabs[0].id, id);
            assert_eq!(app.terminal_font_id(), terminal_font);
        }
    }
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
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

#[test]
fn terminal_appearance_is_independent_and_inheritance_can_be_restored() {
    use hedit::settings::{TerminalColors, TerminalFont};
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    let editor_font = app.settings.font_face.clone();
    let editor_size = app.settings.font_size;
    let editor_theme = app.theme.clone();
    assert_eq!(app.terminal_font_id(), FontId::monospace(editor_size));
    assert_eq!(app.terminal_surface_theme().surface, editor_theme.surface);
    app.settings.terminal_font = Some(TerminalFont {
        face: String::new(),
        size: 24.0,
    });
    app.settings.terminal_colors = Some(TerminalColors::from_theme(
        &hedit::theme::builtin(hedit::theme::PAPER).unwrap(),
    ));
    app.apply_editor_style(&ctx);
    assert_eq!(app.terminal_font_id().size, 24.0);
    assert_eq!(
        app.terminal_font_id().family,
        egui::FontFamily::Name("hedit-terminal".into())
    );
    assert_eq!(app.settings.font_face, editor_font);
    assert_eq!(app.settings.font_size, editor_size);
    assert_eq!(app.theme.to_json(), editor_theme.to_json());
    app.settings.terminal_colors.as_mut().unwrap().background = hedit::theme::Rgb::new(12, 13, 14);
    let override_background = app.terminal_surface_theme().surface;
    let new_theme = hedit::theme::builtin(hedit::theme::PAPER).unwrap();
    app.set_theme(&ctx, new_theme);
    assert_eq!(app.terminal_surface_theme().surface, override_background);
    assert!(!app.terminal.open);
    #[cfg(feature = "terminal")]
    assert!(app.terminal.tabs.is_empty());
    app.settings.terminal_font = None;
    app.settings.terminal_colors = None;
    app.apply_editor_style(&ctx);
    assert_eq!(app.terminal_font_id(), FontId::monospace(editor_size));
    assert_eq!(app.terminal_surface_theme().surface, app.theme.surface);
    app.settings
        .save(&directory.path().join("config/settings.json"))
        .unwrap();
    drop(app);
    let (_, app) = app_in(directory.path());
    assert!(app.settings.terminal_font.is_none());
    assert!(app.settings.terminal_colors.is_none());
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn stale_terminal_font_results_do_not_replace_a_new_selection() {
    use hedit::settings::TerminalFont;
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.settings.terminal_font = Some(TerminalFont {
        face: "current".into(),
        size: 20.0,
    });
    app.terminal_font_generation = 2;
    app.busy = true;
    app.job_editor_focus = Some(egui::Id::new("busy-editor"));
    let notice = app.notice.clone();
    app.tx
        .send(Ok(Action::TerminalFont(
            1,
            "old".into(),
            Err("stale error".into()),
        )))
        .unwrap();
    app.receive(&ctx);
    assert!(app.busy);
    assert_eq!(app.job_editor_focus, Some(egui::Id::new("busy-editor")));
    assert_eq!(app.notice, notice);
    assert!(app.terminal_font.is_none());
    app.settings.terminal_font = None;
    app.tx
        .send(Ok(Action::TerminalFont(
            2,
            "current".into(),
            Err("disabled error".into()),
        )))
        .unwrap();
    app.receive(&ctx);
    assert!(app.busy);
    assert_eq!(app.job_editor_focus, Some(egui::Id::new("busy-editor")));
    assert_eq!(app.notice, notice);
    assert!(app.terminal_font.is_none());
    app.settings.terminal_font = Some(TerminalFont {
        face: "current".into(),
        size: 20.0,
    });
    app.tx
        .send(Ok(Action::TerminalFont(
            2,
            "current".into(),
            Err("cannot load selected font".into()),
        )))
        .unwrap();
    app.receive(&ctx);
    assert!(app.busy);
    assert_eq!(app.job_editor_focus, Some(egui::Id::new("busy-editor")));
    assert!(app.notice.contains("cannot load selected font"));
    app.busy = false;
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_custom_colors_and_font_apply_live_without_replacing_the_shell() {
    use hedit::{
        settings::{TerminalColors, TerminalFont},
        theme::Rgb,
    };
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    app.choose_terminal(true);
    app.new_terminal(&ctx);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let id = app.terminal.tabs[0].id;
    let original = app.terminal.active_session().unwrap().vt.cols().unwrap();
    let mut colors = TerminalColors::from_theme(&app.theme);
    colors.foreground = Rgb::new(201, 202, 203);
    colors.background = Rgb::new(11, 12, 13);
    colors.cursor = Rgb::new(100, 101, 102);
    let mut palette = TerminalColors::theme_palette(&app.theme);
    palette[1] = Rgb::new(220, 30, 40);
    colors.ansi = Some(palette);
    app.settings.terminal_colors = Some(colors.clone());
    app.settings.terminal_font = Some(TerminalFont {
        face: String::new(),
        size: 26.0,
    });
    app.apply_editor_style(&ctx);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let session = app.terminal.active_session_mut().unwrap();
    assert!(
        session.vt.cols().unwrap() < original,
        "larger font must resize the existing PTY"
    );
    session.vt.reset();
    session
        .vt
        .vt_write(b"\x1b[2J\x1b[Hplain \x1b[31mred \x1b[38;2;9;8;7mtruecolor\x1b[0m");
    let grid = session.grid().unwrap();
    let rgb = |c: libghostty_vt::style::RgbColor| [c.r, c.g, c.b];
    assert_eq!(rgb(grid.rows[0][0].foreground), colors.foreground.0);
    assert_eq!(rgb(grid.rows[0][0].background), colors.background.0);
    assert_eq!(rgb(grid.rows[0][6].foreground), palette[1].0);
    assert_eq!(rgb(grid.rows[0][10].foreground), [9, 8, 7]);
    assert_eq!(
        rgb(session.vt.cursor_color().unwrap().unwrap()),
        colors.cursor.0
    );
    assert_eq!(app.terminal.tabs[0].id, id);
    assert_eq!(app.terminal.tabs.len(), 1);
    assert!(app.terminal.active_session().unwrap().running);
    // Application palette overrides survive preference changes.
    app.terminal
        .active_session_mut()
        .unwrap()
        .vt
        .vt_write(b"\x1b]4;1;rgb:01/02/03\x1b\\");
    app.settings
        .terminal_colors
        .as_mut()
        .unwrap()
        .ansi
        .as_mut()
        .unwrap()[1] = Rgb::new(33, 44, 55);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    app.terminal
        .active_session_mut()
        .unwrap()
        .vt
        .vt_write(b"\x1b[H\x1b[31mX");
    assert_eq!(
        rgb(app
            .terminal
            .active_session_mut()
            .unwrap()
            .grid()
            .unwrap()
            .rows[0][0]
            .foreground),
        [1, 2, 3]
    );
    app.settings.terminal_font = None;
    app.settings.terminal_colors = None;
    app.apply_editor_style(&ctx);
    terminal_frame(&ctx, &mut app, vec![], Modifiers::NONE);
    let session = app.terminal.active_session_mut().unwrap();
    assert_eq!(session.vt.cols().unwrap(), original);
    assert_eq!(rgb(session.grid().unwrap().background), app.theme.surface.0);
    assert_eq!(app.terminal.tabs[0].id, id);
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn terminal_font_loads_without_changing_the_editor_and_survives_editor_font_changes() {
    use hedit::settings::TerminalFont;
    let directory = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(directory.path());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/NotoSans-Regular.ttf");
    app.settings.terminal_font = Some(TerminalFont {
        face: path.display().to_string(),
        size: 20.0,
    });
    app.apply_editor_style(&ctx);
    let until = Instant::now() + Duration::from_secs(5);
    while app.terminal_font.is_none() {
        app.receive(&ctx);
        assert!(
            Instant::now() < until,
            "terminal font did not load: {}",
            app.notice
        );
        thread::sleep(Duration::from_millis(5));
    }
    let measure = |app: &HeditApp| {
        let mut widths = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            widths = Some(ctx.fonts(|fonts| {
                (
                    fonts.glyph_width(&app.terminal_font_id(), 'i'),
                    fonts.glyph_width(&app.terminal_font_id(), 'W'),
                    fonts.glyph_width(&FontId::monospace(20.0), 'i'),
                    fonts.glyph_width(&FontId::monospace(20.0), 'W'),
                )
            }));
        });
        widths.unwrap()
    };
    let before = measure(&app);
    assert!(
        before.0 < before.1,
        "the selected terminal face must be used"
    );
    assert_eq!(
        before.2, before.3,
        "the editor retains its built-in monospace face"
    );
    app.settings.font_face = "test-editor".into();
    app.tx
        .send(Ok(Action::Font(
            "test-editor".into(),
            include_bytes!("../../assets/fonts/NotoSans-SemiBold.ttf").to_vec(),
            0,
        )))
        .unwrap();
    app.receive(&ctx);
    let after = measure(&app);
    assert_eq!((before.0, before.1), (after.0, after.1));
    assert!(after.2 < after.3, "the editor font changed independently");
    assert!(!app.terminal.open);
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}

#[test]
fn terminal_font_loading_is_single_flight_and_then_loads_the_latest_selection() {
    let dir = tempfile::tempdir().unwrap();
    let (ctx, mut app) = app_in(dir.path());
    app.terminal_font_loading = Some(7);
    app.terminal_font_generation = 7;
    app.terminal_font_applied = Some("older face".into());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/NotoSans-Regular.ttf");
    app.settings.terminal_font = Some(hedit::settings::TerminalFont {
        face: path.display().to_string(),
        size: 18.0,
    });
    for _ in 0..5 {
        app.apply_editor_style(&ctx);
    }
    assert_eq!(
        app.terminal_font_generation, 7,
        "a second load must wait for the in-flight request"
    );
    app.tx
        .send(Ok(Action::TerminalFont(
            7,
            "older face".into(),
            Err("obsolete".into()),
        )))
        .unwrap();
    app.receive(&ctx);
    app.apply_editor_style(&ctx);
    assert_eq!(app.terminal_font_generation, 8);
    let until = Instant::now() + Duration::from_secs(5);
    while app.terminal_font.is_none() {
        app.receive(&ctx);
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    assert!(app.terminal_font_loading.is_none());
    assert!(!app.notice.contains("obsolete"));
    settings_store::TEST_CONFIG_DIR.with(|dir| *dir.borrow_mut() = None);
}
