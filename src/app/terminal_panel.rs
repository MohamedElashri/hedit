//! Opt-in setup and the integrated terminal surface.
use super::*;

#[cfg(feature = "terminal")]
pub(super) const MAX_TERMINAL_TABS: usize = 16;

#[cfg(feature = "terminal")]
#[path = "terminal_tabs.rs"]
mod tabs;

#[derive(Default)]
pub(super) struct TerminalState {
    pub open: bool,
    binding_prefix: Option<egui::KeyboardShortcut>,
    #[cfg(feature = "terminal")]
    pub focus: bool,
    #[cfg(feature = "terminal")]
    pub tabs: Vec<TerminalTab>,
    #[cfg(feature = "terminal")]
    pub active: usize,
    #[cfg(feature = "terminal")]
    next_id: u64,
    #[cfg(feature = "terminal")]
    dragged_tab: Option<u64>,
    #[cfg(feature = "terminal")]
    renderer: terminal_text::Renderer,
}

#[cfg(feature = "terminal")]
pub(super) struct TerminalTab {
    pub id: u64,
    pub session: hedit::terminal::Session,
    pub name: String,
    preedit: String,
    seen: u64,
    selection_start: Option<(u16, u32)>,
    scroll_remainder: f32,
}

#[cfg(feature = "terminal")]
impl TerminalState {
    pub(super) fn active_session(&self) -> Option<&hedit::terminal::Session> {
        self.tabs.get(self.active).map(|t| &t.session)
    }
    #[cfg(test)]
    pub(super) fn active_session_mut(&mut self) -> Option<&mut hedit::terminal::Session> {
        self.tabs.get_mut(self.active).map(|t| &mut t.session)
    }
}

fn input_id() -> egui::Id {
    egui::Id::new("integrated-terminal-input")
}

impl HeditApp {
    pub(super) fn terminal_preferences(&mut self, ui: &mut egui::Ui) {
        ui.label("Run your local shell inside hedit, starting in the current project folder.");
        ui.add_space(8.0);
        let before = self.settings.terminal_enabled;
        ui.add_enabled_ui(cfg!(feature = "terminal"), |ui| {
            ui.checkbox(
                &mut self.settings.terminal_enabled,
                "Enable integrated terminal",
            );
        });
        if before != self.settings.terminal_enabled {
            self.settings.terminal_onboarding_complete = true;
            self.sync_terminal();
        }
        ui.weak("Disabled by default. Enabling allows you to open a terminal; it does not start a shell.");
        ui.weak("Hiding the panel keeps the shell running. Stop, disabling the terminal, or quitting hedit ends the session. Terminal sessions are not restored after quitting.");
        ui.checkbox(
            &mut self.settings.terminal_bidi,
            "Bidirectional text and Arabic shaping",
        );
        ui.weak("Display Arabic and Hebrew in reading order. Turn this off for applications that already arrange bidirectional text themselves.");
        if !cfg!(feature = "terminal") {
            ui.label("This build does not include terminal support. Use a build with the terminal feature to enable it.");
        }
        if ui
            .add_enabled(
                self.settings.terminal_enabled && cfg!(feature = "terminal"),
                egui::Button::new("Open terminal"),
            )
            .clicked()
        {
            self.preferences_open = false;
            self.terminal.open = false;
            self.toggle_terminal(ui.ctx());
        }
    }

    pub(super) fn terminal_onboarding_due(&self) -> bool {
        cfg!(feature = "terminal")
            && self.settings_files.dir.is_some()
            && !self.settings.terminal_onboarding_complete
    }

    pub(super) fn choose_terminal(&mut self, enabled: bool) {
        self.settings.terminal_enabled = enabled;
        self.settings.terminal_onboarding_complete = true;
        self.sync_terminal();
        self.flush_settings();
        self.focus_editor = true;
    }

    pub(super) fn terminal_onboarding(&mut self, ctx: &egui::Context) {
        if !self.terminal_onboarding_due() || self.close_flow.is_some() || self.closing {
            return;
        }
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("terminal-onboarding")).show(ctx, |ui| {
            ui.set_max_width(440.0);
            ui.heading("An optional terminal for your projects");
            ui.label("Open a local shell in a resizable panel while you edit. The integrated terminal is powered by Ghostty.");
            ui.add_space(8.0);
            ui.label("It starts disabled. If you enable it, a shell starts only when you open the panel from Tools → Toggle terminal.");
            ui.weak("You can change this anytime in Settings → Terminal.");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Keep disabled").clicked() { choice = Some(false); }
                if ui.button("Enable terminal").clicked() { choice = Some(true); }
            });
        });
        if modal.should_close() {
            choice = Some(false);
        }
        if let Some(enabled) = choice {
            self.choose_terminal(enabled);
        }
    }

    fn sync_terminal(&mut self) {
        if !self.settings.terminal_enabled {
            self.focus_editor |= self.terminal.open;
            self.terminal = TerminalState::default();
        }
    }

    pub(super) fn toggle_terminal(&mut self, ctx: &egui::Context) {
        if !self.settings.terminal_enabled {
            return;
        }
        #[cfg(feature = "terminal")]
        {
            if self.terminal.open {
                if let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) {
                    tab.preedit.clear();
                }
                self.terminal.open = false;
                self.terminal.dragged_tab = None;
                self.terminal.binding_prefix = None;
                self.focus_editor = true;
                ctx.memory_mut(|m| m.surrender_focus(input_id()));
                return;
            }
            if self.terminal.tabs.is_empty() {
                self.new_terminal(ctx);
            }
            self.terminal.open = !self.terminal.tabs.is_empty();
            self.terminal.focus = self.terminal.open;
            self.focus_editor = !self.terminal.open;
        }
        #[cfg(not(feature = "terminal"))]
        let _ = ctx;
    }

    #[cfg(feature = "terminal")]
    fn terminal_directory(&self) -> PathBuf {
        let file = self.active().page.info.path.as_ref().map(PathBuf::from);
        self.workspace
            .folders
            .iter()
            .filter(|f| file.as_ref().is_some_and(|p| p.starts_with(&f.path)))
            .max_by_key(|f| PathBuf::from(&f.path).components().count())
            .or_else(|| self.workspace.folders.first())
            .map(|f| PathBuf::from(&f.path))
            .or_else(|| file.and_then(|p| p.parent().map(std::path::Path::to_path_buf)))
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    pub(super) fn new_terminal(&mut self, ctx: &egui::Context) {
        if !self.settings.terminal_enabled {
            return;
        }
        #[cfg(feature = "terminal")]
        {
            if self.terminal.tabs.len() >= MAX_TERMINAL_TABS {
                self.notice =
                    format!("Up to {MAX_TERMINAL_TABS} terminal tabs can be open at once.");
                return;
            }
            let wake = ctx.clone();
            match hedit::terminal::Session::start(&self.terminal_directory(), move || {
                wake.request_repaint()
            }) {
                Ok(session) => {
                    if let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) {
                        tab.preedit.clear();
                    }
                    self.terminal.next_id += 1;
                    self.terminal.dragged_tab = None;
                    self.terminal.tabs.push(TerminalTab {
                        id: self.terminal.next_id,
                        session,
                        name: String::new(),
                        preedit: String::new(),
                        seen: 0,
                        selection_start: None,
                        scroll_remainder: 0.0,
                    });
                    self.terminal.active = self.terminal.tabs.len() - 1;
                    self.terminal.open = true;
                    self.terminal.focus = true;
                    self.terminal.binding_prefix = None;
                    self.focus_editor = false;
                }
                Err(error) => self.error = Some(format!("Cannot open terminal: {error}")),
            }
        }
        #[cfg(not(feature = "terminal"))]
        let _ = ctx;
    }

    #[cfg(feature = "terminal")]
    pub(super) fn close_terminal(&mut self, ctx: &egui::Context, index: usize) {
        #[cfg(feature = "terminal")]
        {
            if index >= self.terminal.tabs.len() {
                return;
            }
            self.terminal.tabs.remove(index);
            self.terminal.dragged_tab = None;
            if index < self.terminal.active {
                self.terminal.active -= 1;
            }
            self.terminal.active = self
                .terminal
                .active
                .min(self.terminal.tabs.len().saturating_sub(1));
            self.terminal.binding_prefix = None;
            self.terminal.open = !self.terminal.tabs.is_empty();
            self.terminal.focus = self.terminal.open;
            self.focus_editor = !self.terminal.open;
            if !self.terminal.open {
                ctx.memory_mut(|m| m.surrender_focus(input_id()));
            }
        }
        #[cfg(not(feature = "terminal"))]
        let _ = (ctx, index);
    }

    pub(super) fn close_active_terminal(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "terminal")]
        self.close_terminal(ctx, self.terminal.active);
        #[cfg(not(feature = "terminal"))]
        let _ = ctx;
    }

    pub(super) fn cycle_terminal(&mut self, forward: bool) {
        #[cfg(feature = "terminal")]
        {
            let count = self.terminal.tabs.len();
            if count == 0 {
                return;
            }
            self.terminal.tabs[self.terminal.active].preedit.clear();
            self.terminal.active =
                (self.terminal.active + if forward { 1 } else { count - 1 }) % count;
            self.terminal.focus = true;
            self.terminal.binding_prefix = None;
            self.terminal.open = true;
            self.focus_editor = false;
        }
        #[cfg(not(feature = "terminal"))]
        let _ = forward;
    }

    pub(super) fn restart_terminal(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "terminal")]
        if let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) {
            let wake = ctx.clone();
            match hedit::terminal::Session::start(&tab.session.directory, move || {
                wake.request_repaint()
            }) {
                Ok(session) => {
                    tab.session = session;
                    tab.seen = 0;
                    tab.selection_start = None;
                    tab.preedit.clear();
                    tab.scroll_remainder = 0.0;
                    self.terminal.focus = true;
                }
                Err(error) => self.error = Some(format!("Cannot restart terminal: {error}")),
            }
        }
        #[cfg(not(feature = "terminal"))]
        let _ = ctx;
    }

    pub(super) fn poll_terminal(&mut self, ctx: &egui::Context) {
        self.sync_terminal();
        #[cfg(feature = "terminal")]
        for tab in &mut self.terminal.tabs {
            if tab.session.poll() {
                ctx.request_repaint();
            }
            // Every shell keeps draining and reports exit even when hidden.
            if tab.session.running {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }
        #[cfg(not(feature = "terminal"))]
        let _ = ctx;
    }

    /// Route terminal input before editor shortcuts, including Ctrl+C and Esc.
    pub(super) fn terminal_shortcuts(&mut self, ctx: &egui::Context) -> bool {
        if self.terminal_onboarding_due() {
            return true;
        }
        if !self.settings.terminal_enabled {
            self.sync_terminal();
            return false;
        }
        if !self.preferences_open
            && !self.palette
            && !self.busy
            && !self.closing
            && !self.overridden(Command::Terminal)
            && ctx.input_mut(|i| {
                i.consume_key(
                    if cfg!(target_os = "macos") {
                        Modifiers::CTRL
                    } else {
                        Modifiers::COMMAND
                    },
                    Key::Backtick,
                )
            })
        {
            self.execute(ctx, Command::Terminal);
            return true;
        }
        // egui only installs a focus lock after one full focused frame. Keep
        // Escape/Tab/arrows in the terminal on the first frame after opening it.
        if self.terminal.open
            && ctx.memory(|m| m.had_focus_last_frame(input_id()) && m.focused().is_none())
            && ctx.input(|i| {
                i.key_pressed(Key::Escape)
                    || i.key_pressed(Key::Tab)
                    || i.key_pressed(Key::ArrowUp)
                    || i.key_pressed(Key::ArrowDown)
                    || i.key_pressed(Key::ArrowLeft)
                    || i.key_pressed(Key::ArrowRight)
            })
        {
            ctx.memory_mut(|m| m.request_focus(input_id()));
        }
        if !self.terminal.open
            || self.preferences_open
            || self.palette
            || self.open_menu.is_some()
            || self.source_control.prompt.is_some()
            || !ctx.memory(|m| m.has_focus(input_id()))
        {
            return false;
        }
        let shift = Modifiers {
            shift: true,
            ..Modifiers::COMMAND
        };
        if self.terminal_bindings(ctx) {
            return true;
        }
        for (modifiers, key, command) in [
            (Modifiers::COMMAND, Key::Comma, Command::Settings),
            (shift, Key::P, Command::Palette),
            (shift, Key::T, Command::NewTerminal),
            (shift, Key::W, Command::CloseTerminal),
            (Modifiers::CTRL, Key::PageUp, Command::PreviousTerminal),
            (Modifiers::CTRL, Key::PageDown, Command::NextTerminal),
        ] {
            if !self.overridden(command) && ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.execute(ctx, command);
                return true;
            }
        }
        #[cfg(feature = "terminal")]
        self.terminal_input(ctx);
        true
    }

    /// Only global terminal/window bindings may run while a shell has focus.
    /// Editor commands, including their chords, stay with the shell.
    fn terminal_bindings(&mut self, ctx: &egui::Context) -> bool {
        let bindings: Vec<_> = self
            .quality
            .config
            .keybindings
            .iter()
            .filter(|b| {
                !b.editor_only
                    && matches!(
                        b.command,
                        Command::Terminal
                            | Command::NewTerminal
                            | Command::CloseTerminal
                            | Command::NextTerminal
                            | Command::PreviousTerminal
                            | Command::RestartTerminal
                            | Command::TerminalSettings
                            | Command::Settings
                            | Command::Palette
                    )
            })
            .filter_map(|b| {
                chords::parse(&b.shortcut)
                    .ok()
                    .map(|keys| (b.command, keys))
            })
            .collect();
        let Some((key, modifiers)) = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        }) else {
            return false;
        };
        let matches = |s: &egui::KeyboardShortcut| {
            s.logical_key == key && modifiers.matches_exact(s.modifiers)
        };
        let mut command = None;
        let used = if let Some(prefix) = self.terminal.binding_prefix.take() {
            command = bindings
                .iter()
                .find_map(|(command, keys)| match keys.as_slice() {
                    [first, second] if *first == prefix && matches(second) => Some(*command),
                    _ => None,
                });
            true
        } else {
            bindings.iter().any(|(bound, keys)| match keys.as_slice() {
                [single] if matches(single) => {
                    command = Some(*bound);
                    true
                }
                [first, _] if matches(first) => {
                    self.terminal.binding_prefix = Some(*first);
                    true
                }
                _ => false,
            })
        };
        if used {
            ctx.input_mut(|i| {
                i.events
                    .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
            });
            if let Some(command) = command {
                self.execute(ctx, command);
            }
        }
        used
    }

    pub(super) fn terminal_panel(&mut self, ctx: &egui::Context) {
        if !self.settings.terminal_enabled || !self.terminal.open {
            return;
        }
        #[cfg(feature = "terminal")]
        self.draw_terminal(ctx);
        #[cfg(not(feature = "terminal"))]
        let _ = ctx;
    }
}

#[cfg(feature = "terminal")]
mod surface {
    use super::*;
    use libghostty_vt::{
        key,
        selection::{FormatOptions, Selection},
        style::{RgbColor, Underline},
        terminal::{Point, PointCoordinate, ScrollViewport},
    };

    fn color(c: RgbColor) -> egui::Color32 {
        egui::Color32::from_rgb(c.r, c.g, c.b)
    }
    fn rgb(c: egui::Color32) -> RgbColor {
        RgbColor {
            r: c.r(),
            g: c.g(),
            b: c.b(),
        }
    }

    fn terminal_key(key: Key) -> Option<key::Key> {
        use key::Key as K;
        Some(match key {
            Key::ArrowUp => K::ArrowUp,
            Key::ArrowDown => K::ArrowDown,
            Key::ArrowLeft => K::ArrowLeft,
            Key::ArrowRight => K::ArrowRight,
            Key::Enter => K::Enter,
            Key::Tab => K::Tab,
            Key::Escape => K::Escape,
            Key::Backspace => K::Backspace,
            Key::Delete => K::Delete,
            Key::Insert => K::Insert,
            Key::Home => K::Home,
            Key::End => K::End,
            Key::PageUp => K::PageUp,
            Key::PageDown => K::PageDown,
            Key::Space => K::Space,
            Key::Backtick => K::Backquote,
            Key::Num0 => K::Digit0,
            Key::Num1 => K::Digit1,
            Key::Num2 => K::Digit2,
            Key::Num3 => K::Digit3,
            Key::Num4 => K::Digit4,
            Key::Num5 => K::Digit5,
            Key::Num6 => K::Digit6,
            Key::Num7 => K::Digit7,
            Key::Num8 => K::Digit8,
            Key::Num9 => K::Digit9,
            Key::Minus => K::Minus,
            Key::Period => K::Period,
            Key::Quote => K::Quote,
            Key::Comma => K::Comma,
            Key::Semicolon => K::Semicolon,
            Key::Equals | Key::Plus => K::Equal,
            Key::A => K::A,
            Key::B => K::B,
            Key::C => K::C,
            Key::D => K::D,
            Key::E => K::E,
            Key::F => K::F,
            Key::G => K::G,
            Key::H => K::H,
            Key::I => K::I,
            Key::J => K::J,
            Key::K => K::K,
            Key::L => K::L,
            Key::M => K::M,
            Key::N => K::N,
            Key::O => K::O,
            Key::P => K::P,
            Key::Q => K::Q,
            Key::R => K::R,
            Key::S => K::S,
            Key::T => K::T,
            Key::U => K::U,
            Key::V => K::V,
            Key::W => K::W,
            Key::X => K::X,
            Key::Y => K::Y,
            Key::Z => K::Z,
            Key::OpenBracket => K::BracketLeft,
            Key::CloseBracket => K::BracketRight,
            Key::Backslash => K::Backslash,
            Key::Slash => K::Slash,
            Key::F1 => K::F1,
            Key::F2 => K::F2,
            Key::F3 => K::F3,
            Key::F4 => K::F4,
            Key::F5 => K::F5,
            Key::F6 => K::F6,
            Key::F7 => K::F7,
            Key::F8 => K::F8,
            Key::F9 => K::F9,
            Key::F10 => K::F10,
            Key::F11 => K::F11,
            Key::F12 => K::F12,
            _ => return None,
        })
    }

    fn is_printable(key: Key) -> bool {
        key.name().len() == 1 || key == Key::Space
    }

    impl HeditApp {
        pub(super) fn terminal_input(&mut self, ctx: &egui::Context) {
            let events = ctx.input(|i| i.events.clone());
            let modifiers = ctx.input(|i| i.modifiers);
            let has_text = events.iter().any(|e| matches!(e, egui::Event::Text(_)));
            let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) else {
                return;
            };
            let session = &mut tab.session;
            let preedit = &mut tab.preedit;
            for event in events {
                let result = match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit(text)) => {
                        *preedit = text;
                        Ok(())
                    }
                    egui::Event::Ime(egui::ImeEvent::Disabled) => {
                        preedit.clear();
                        Ok(())
                    }
                    egui::Event::Text(_) if !preedit.is_empty() => Ok(()),
                    egui::Event::Key { .. } if !preedit.is_empty() => Ok(()),
                    egui::Event::Paste(text) => session.paste(&text),
                    egui::Event::Copy
                        if !cfg!(target_os = "macos") && modifiers.ctrl && !modifiers.shift =>
                    {
                        session.write(vec![3])
                    }
                    egui::Event::Cut
                        if !cfg!(target_os = "macos") && modifiers.ctrl && !modifiers.shift =>
                    {
                        session.write(vec![24])
                    }
                    egui::Event::Copy => {
                        copy_selection(ctx, session);
                        Ok(())
                    }
                    egui::Event::Text(text) | egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                        preedit.clear();
                        let mut event = key::Event::new().unwrap();
                        event.set_action(key::Action::Press).set_utf8(Some(text));
                        if modifiers.alt && !modifiers.ctrl && !cfg!(target_os = "macos") {
                            event.set_mods(key::Mods::ALT);
                        }
                        session.key(&event)
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        // Printable keys arrive as Text; only encode their key event with
                        // Ctrl/Alt. This avoids sending ordinary characters twice.
                        let printable = is_printable(key);
                        if printable && !modifiers.ctrl && !modifiers.alt {
                            continue;
                        }
                        if printable && !modifiers.ctrl && has_text {
                            continue;
                        }
                        let Some(mapped) = terminal_key(key) else {
                            continue;
                        };
                        let mut mods = key::Mods::empty();
                        mods.set(key::Mods::CTRL, modifiers.ctrl);
                        mods.set(key::Mods::ALT, modifiers.alt);
                        mods.set(key::Mods::SHIFT, modifiers.shift);
                        mods.set(key::Mods::SUPER, modifiers.mac_cmd);
                        let mut event = key::Event::new().unwrap();
                        event
                            .set_action(key::Action::Press)
                            .set_key(mapped)
                            .set_mods(mods);
                        if printable {
                            let text = if modifiers.shift {
                                key.name().to_owned()
                            } else {
                                key.name().to_lowercase()
                            };
                            event.set_utf8(Some(if key == Key::Space { " ".into() } else { text }));
                        }
                        session.key(&event)
                    }
                    _ => Ok(()),
                };
                if let Err(error) = result {
                    session.status = error;
                }
            }
            ctx.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(
                        e,
                        egui::Event::Key { .. }
                            | egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Copy
                            | egui::Event::Cut
                            | egui::Event::Ime(_)
                    )
                })
            });
        }

        pub(super) fn draw_terminal(&mut self, ctx: &egui::Context) {
            let mut actions = tabs::Actions::default();
            egui::TopBottomPanel::bottom("terminal-panel")
                .resizable(true)
                .default_height(250.0)
                .height_range(90.0..=(ctx.screen_rect().height() * 0.65).max(100.0))
                .frame(egui::Frame::new().fill(self.theme.surface.color()))
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    actions = tabs::bar(ui, &mut self.terminal, &self.theme);
                    if actions.new {
                        self.new_terminal(ctx);
                    }
                    if let Some(index) = actions.activate {
                        self.terminal.tabs[self.terminal.active].preedit.clear();
                        self.terminal.active = index;
                        self.terminal.focus = true;
                        self.terminal.binding_prefix = None;
                    }
                    if let Some((id, target)) = actions.reorder {
                        let active = self.terminal.tabs[self.terminal.active].id;
                        if let Some(source) = self.terminal.tabs.iter().position(|tab| tab.id == id)
                        {
                            let tab = self.terminal.tabs.remove(source);
                            let target = target.min(self.terminal.tabs.len());
                            self.terminal.tabs.insert(target, tab);
                            self.terminal.active = self
                                .terminal
                                .tabs
                                .iter()
                                .position(|tab| tab.id == active)
                                .unwrap_or(0);
                            self.terminal.focus = true;
                        }
                    }
                    let TerminalState {
                        tabs,
                        active,
                        renderer,
                        focus,
                        ..
                    } = &mut self.terminal;
                    if let Some(tab) = tabs.get_mut(*active) {
                        tab.seen = tab.session.output_generation;
                        egui::Frame::new()
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                draw_surface(
                                    ui,
                                    tab,
                                    renderer,
                                    focus,
                                    SurfaceStyle {
                                        theme: &self.theme,
                                        font_size: self.settings.font_size,
                                        bidi: self.settings.terminal_bidi,
                                        editor: self.editor_font.as_ref(),
                                    },
                                );
                            });
                        if ctx.memory(|m| m.has_focus(input_id())) {
                            self.focus_editor = false;
                        }
                    }
                });
            if actions.hide {
                self.terminal.dragged_tab = None;
                if let Some(tab) = self.terminal.tabs.get_mut(self.terminal.active) {
                    tab.preedit.clear();
                }
                self.terminal.open = false;
                self.terminal.binding_prefix = None;
                self.focus_editor = true;
                ctx.memory_mut(|m| m.surrender_focus(input_id()));
            }
            if let Some(index) = actions.stop {
                if let Some(tab) = self.terminal.tabs.get_mut(index) {
                    tab.session.stop();
                }
                self.terminal.focus = true;
            }
            if actions.copy {
                if let Some(session) = self.terminal.active_session() {
                    copy_selection(ctx, session);
                }
                self.terminal.focus = true;
            }
            self.terminal.focus |= actions.focus;
            if let Some(index) = actions.close {
                self.close_terminal(ctx, index);
            } else if let Some(index) = actions.restart {
                self.terminal.active = index;
                self.restart_terminal(ctx);
            }
        }
    }

    struct SurfaceStyle<'a> {
        theme: &'a Theme,
        font_size: f32,
        bidi: bool,
        editor: Option<&'a (Vec<u8>, u32)>,
    }

    fn draw_surface(
        ui: &mut egui::Ui,
        tab: &mut TerminalTab,
        renderer: &mut terminal_text::Renderer,
        focus: &mut bool,
        style: SurfaceStyle<'_>,
    ) {
        let SurfaceStyle {
            theme,
            font_size,
            bidi,
            editor,
        } = style;
        let ctx = ui.ctx().clone();
        let session = &mut tab.session;
        let font = FontId::monospace(font_size);
        let cell = ui.fonts(|f| {
            egui::vec2(
                f.glyph_width(&font, ' ').max(1.0),
                f.row_height(&font).max(1.0),
            )
        });
        let available = ui.available_size().max(cell);
        let cols = (available.x / cell.x).floor().clamp(2.0, 500.0) as u16;
        let rows = (available.y / cell.y).floor().clamp(1.0, 300.0) as u16;
        let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());
        let response = ui.interact(rect, input_id(), egui::Sense::click_and_drag());
        if std::mem::take(focus) || response.clicked() || response.drag_started() {
            response.request_focus();
        }
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    input_id(),
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }
        if !response.has_focus() {
            tab.preedit.clear();
        }
        let scale = ctx.pixels_per_point();
        if let Err(error) =
            session.resize(cols, rows, (cell.x * scale) as u32, (cell.y * scale) as u32)
        {
            session.status = error;
        }
        let _ = session
            .vt
            .set_default_fg_color(Some(rgb(theme.text.color())));
        let _ = session
            .vt
            .set_default_bg_color(Some(rgb(theme.surface.color())));
        if response.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                tab.scroll_remainder -= scroll / cell.y;
                let lines = tab.scroll_remainder.trunc() as isize;
                tab.scroll_remainder -= lines as f32;
                session.vt.scroll_viewport(ScrollViewport::Delta(lines));
                ctx.input_mut(|i| i.smooth_scroll_delta = egui::Vec2::ZERO);
            }
        }
        let mut grid = match session.grid() {
            Ok(grid) => grid,
            Err(error) => {
                session.status = format!("Terminal rendering: {error}");
                return;
            }
        };
        let layout = terminal_text::layout(&grid, bidi);
        let logical_position = |pos: egui::Pos2| {
            let y = ((pos.y - rect.top()) / cell.y)
                .floor()
                .clamp(0.0, f32::from(rows - 1)) as usize;
            let visual = ((pos.x - rect.left()) / cell.x)
                .floor()
                .clamp(0.0, f32::from(cols - 1)) as usize;
            let x = layout
                .get(y)
                .and_then(|row| row.visual_to_logical.get(visual))
                .copied()
                .unwrap_or(visual);
            (x as u16, y as u32)
        };
        if response.clicked() {
            let _ = session.vt.set_selection(None);
        }
        if let Some(pos) = response.interact_pointer_pos() {
            if response.drag_started() {
                tab.selection_start = Some(logical_position(
                    ctx.input(|i| i.pointer.press_origin()).unwrap_or(pos),
                ));
            }
            if response.dragged() {
                if let Some(start) = tab.selection_start {
                    let point = |(x, y)| Point::Viewport(PointCoordinate { x, y });
                    if let (Ok(a), Ok(b)) = (
                        session.vt.grid_ref(point(start)),
                        session.vt.grid_ref(point(logical_position(pos))),
                    ) {
                        let _ = session.vt.set_selection(Some(&Selection::new(a, b, false)));
                    }
                }
            }
            // Refresh selection flags in the same frame as the drag.
            if let Ok(updated) = session.grid() {
                grid = updated;
            }
        }
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0.0, color(grid.background));
        for (y, (cells, row)) in grid.rows.iter().zip(&layout).enumerate() {
            for (visual, &logical) in row.visual_to_logical.iter().enumerate() {
                let c = &cells[logical];
                let pos = rect.min + egui::vec2(visual as f32 * cell.x, y as f32 * cell.y);
                let bg = if c.selected {
                    theme.selection.color()
                } else if c.style.inverse {
                    color(c.foreground)
                } else {
                    color(c.background)
                };
                painter.rect_filled(egui::Rect::from_min_size(pos, cell), 0.0, bg);
            }
            let mut shaped = vec![false; cells.len()];
            for run in &row.runs {
                if let Some(texture) = renderer.texture(&ctx, run, cell, font_size, editor) {
                    let bounds = egui::Rect::from_min_size(
                        rect.min + egui::vec2(run.start as f32 * cell.x, y as f32 * cell.y),
                        egui::vec2(run.width as f32 * cell.x, cell.y),
                    );
                    for visual in run.start..run.start + run.width {
                        shaped[visual] = true;
                        let c = &cells[row.visual_to_logical[visual]];
                        if c.style.invisible {
                            continue;
                        }
                        let (fg, _) = cell_colors(c, theme);
                        let clip = egui::Rect::from_min_size(
                            rect.min + egui::vec2(visual as f32 * cell.x, y as f32 * cell.y),
                            cell,
                        );
                        painter.with_clip_rect(rect.intersect(clip)).image(
                            texture.id(),
                            bounds,
                            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                            fg,
                        );
                    }
                }
            }
            for (visual, &logical) in row.visual_to_logical.iter().enumerate() {
                let c = &cells[logical];
                let pos = rect.min + egui::vec2(visual as f32 * cell.x, y as f32 * cell.y);
                let (fg, _) = cell_colors(c, theme);
                if !shaped[visual] && !c.style.invisible && !c.text.is_empty() {
                    painter.text(pos, egui::Align2::LEFT_TOP, &c.text, font.clone(), fg);
                }
                let width = if c.wide { cell.x * 2.0 } else { cell.x };
                if c.style.underline != Underline::None {
                    painter.line_segment(
                        [
                            pos + egui::vec2(0.0, cell.y - 1.0),
                            pos + egui::vec2(width, cell.y - 1.0),
                        ],
                        egui::Stroke::new(1.0, fg),
                    );
                }
                if c.style.strikethrough {
                    painter.line_segment(
                        [
                            pos + egui::vec2(0.0, cell.y * 0.5),
                            pos + egui::vec2(width, cell.y * 0.5),
                        ],
                        egui::Stroke::new(1.0, fg),
                    );
                }
            }
        }
        if response.has_focus() && session.running {
            if let Some((x, y)) = grid.cursor {
                if let (Some(row), Some(cells)) =
                    (layout.get(y as usize), grid.rows.get(y as usize))
                {
                    let (x, rtl) = row.cursor(cells, x as usize);
                    let pos = rect.min + egui::vec2(x * cell.x, f32::from(y) * cell.y);
                    ui.output_mut(|o| {
                        o.ime = Some(egui::output::IMEOutput {
                            rect,
                            cursor_rect: egui::Rect::from_min_size(pos, egui::vec2(1.0, cell.y)),
                        });
                        o.mutable_text_under_cursor = response.hovered();
                    });
                    if !tab.preedit.is_empty() {
                        let galley = ui.fonts(|f| {
                            f.layout_no_wrap(
                                hedit::bidi::display(&tab.preedit).into_owned(),
                                font.clone(),
                                theme.text.color(),
                            )
                        });
                        let origin = egui::pos2(
                            pos.x.min(rect.right() - galley.size().x).max(rect.left()),
                            pos.y,
                        );
                        let bounds = egui::Rect::from_min_size(origin, galley.size());
                        painter.rect_filled(bounds, 0.0, theme.surface.color());
                        painter.galley(origin, galley, theme.text.color());
                        painter.line_segment(
                            [bounds.left_bottom(), bounds.right_bottom()],
                            egui::Stroke::new(1.0, theme.text.color()),
                        );
                    }
                    if rtl {
                        painter.line_segment(
                            [pos, pos + egui::vec2(0.0, cell.y)],
                            egui::Stroke::new(1.5, theme.text.color()),
                        );
                    } else {
                        painter.rect_stroke(
                            egui::Rect::from_min_size(pos, cell),
                            0.0,
                            egui::Stroke::new(1.0, theme.text.color()),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
    }

    fn cell_colors(c: &hedit::terminal::Cell, theme: &Theme) -> (egui::Color32, egui::Color32) {
        let (mut fg, mut bg) = (color(c.foreground), color(c.background));
        if c.style.inverse {
            std::mem::swap(&mut fg, &mut bg);
        }
        if c.selected {
            bg = theme.selection.color();
        }
        if c.style.faint {
            fg = fg.lerp_to_gamma(bg, 0.5);
        }
        (fg, bg)
    }

    fn copy_selection(ctx: &egui::Context, session: &hedit::terminal::Session) {
        let mut buffer = vec![0; hedit::terminal::PASTE_LIMIT];
        if let Ok(Some(len)) = session.vt.format_selection_buf(
            FormatOptions::new().with_trim(true).with_unwrap(true),
            &mut buffer,
        ) {
            ctx.copy_text(String::from_utf8_lossy(&buffer[..len]).into_owned());
        }
    }
}

#[cfg(all(test, feature = "terminal"))]
mod directory_tests {
    use super::*;

    #[test]
    fn terminals_use_the_innermost_project_folder_in_any_root_order() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dir.path().join("outer");
        let inner = outer.join("inner");
        std::fs::create_dir_all(&inner).unwrap();
        let file = inner.join("file.txt");
        std::fs::write(&file, "nested project\n").unwrap();
        let ctx = egui::Context::default();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        for root in [&outer, &inner] {
            app.set_folder(hedit::folder::read_directory(root.clone()).unwrap());
        }
        app.open(&ctx, Some(file));
        tests::finish_app_job(&mut app, &ctx);
        for _ in 0..2 {
            assert_eq!(app.terminal_directory(), inner.canonicalize().unwrap());
            app.workspace.folders.reverse();
        }
    }
}
