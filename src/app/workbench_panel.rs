//! A single bottom region. Visibility never owns a shell, build, or diagnostic.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Problems,
    Output,
    Terminal,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum OutputChannel {
    #[default]
    Build,
    Ssh,
}

pub(super) struct State {
    pub last: Panel,
    pub maximized: bool,
    pub normal_height: f32,
    output: OutputChannel,
}
impl Default for State {
    fn default() -> Self {
        Self {
            last: Panel::Problems,
            maximized: false,
            normal_height: 230.0,
            output: OutputChannel::Build,
        }
    }
}

impl HeditApp {
    fn active_panel(&self) -> Option<Panel> {
        if self.terminal.open && self.settings.terminal_enabled {
            Some(Panel::Terminal)
        } else if self.build.open {
            Some(Panel::Output)
        } else if self.languages.problems {
            Some(Panel::Problems)
        } else {
            None
        }
    }

    pub(super) fn set_panel(&mut self, panel: Panel) {
        self.panel_state.last = panel;
        self.terminal.open = panel == Panel::Terminal;
        self.build.open = panel == Panel::Output;
        self.languages.problems = panel == Panel::Problems;
        if panel != Panel::Terminal {
            self.terminal.release_input();
            self.focus_editor = true;
        }
    }

    pub(super) fn toggle_panel(&mut self, panel: Panel) {
        if self.active_panel() == Some(panel) {
            self.hide_panel();
        } else {
            self.set_panel(panel);
        }
    }

    pub(super) fn show_ssh_output(&mut self) {
        self.panel_state.output = OutputChannel::Ssh;
        self.set_panel(Panel::Output);
    }

    pub(super) fn show_build_output(&mut self) {
        self.panel_state.output = OutputChannel::Build;
        self.set_panel(Panel::Output);
    }

    pub(super) fn hide_panel(&mut self) {
        if let Some(panel) = self.active_panel() {
            self.panel_state.last = panel;
        }
        self.terminal.open = false;
        self.terminal.release_input();
        self.build.open = false;
        self.languages.problems = false;
        self.focus_editor = true;
    }

    pub(super) fn toggle_last_panel(&mut self, ctx: &egui::Context) {
        if self.active_panel().is_some() {
            self.hide_panel();
            return;
        }
        match self.panel_state.last {
            Panel::Terminal if cfg!(feature = "terminal") && self.settings.terminal_enabled => {
                self.toggle_terminal(ctx)
            }
            Panel::Terminal => self.set_panel(Panel::Problems),
            panel => self.set_panel(panel),
        }
    }
    pub(super) fn maximize_panel(&mut self, ctx: &egui::Context) {
        if self.active_panel().is_none() {
            self.toggle_last_panel(ctx);
        }
        self.panel_state.maximized = !self.panel_state.maximized;
        if !self.panel_state.maximized {
            let id = egui::Id::new("workbench-panel");
            if let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id) {
                state.rect.min.y = state.rect.max.y - self.panel_state.normal_height;
                ctx.data_mut(|d| d.insert_persisted(id, state));
            }
        }
    }
    pub(super) fn focus_editor_command(&mut self, ctx: &egui::Context) {
        self.terminal.release_input();
        self.focus_editor = true;
        ctx.memory_mut(|m| m.request_focus(self.editor_id()));
    }
    pub(super) fn focus_terminal_command(&mut self, ctx: &egui::Context) {
        if !cfg!(feature = "terminal") || !self.settings.terminal_enabled {
            self.notice = "Enable the integrated terminal in Settings first.".into();
            return;
        }
        if self.active_panel() != Some(Panel::Terminal) {
            self.toggle_terminal(ctx);
        }
        #[cfg(feature = "terminal")]
        {
            self.terminal.focus = true;
            self.focus_editor = false;
            ctx.memory_mut(|m| m.request_focus(terminal_panel::input_id()));
        }
    }
    pub(super) fn workbench_panel(&mut self, ctx: &egui::Context) {
        let Some(panel) = self.active_panel() else {
            return;
        };
        let mut select = None;
        let mut hide = false;
        let mut maximize = false;
        let maximized = self.panel_state.maximized;
        let builder = egui::TopBottomPanel::bottom("workbench-panel");
        let builder = if maximized {
            builder
                .resizable(false)
                .exact_height((ctx.available_rect().height() - 40.0).max(90.0))
        } else {
            builder
                .resizable(true)
                .default_height(self.panel_state.normal_height)
                .height_range(90.0..=(ctx.screen_rect().height() * 0.6).max(100.0))
        };
        let shown = builder
            .frame(
                egui::Frame::new()
                    .fill(self.theme.surface.color())
                    .inner_margin(egui::Margin::symmetric(10, 4)),
            )
            .show(ctx, |ui| {
                ui.set_min_height(ui.available_height());
                ui.horizontal(|ui| {
                    for (tab, label) in [
                        (Panel::Problems, "Problems"),
                        (Panel::Output, "Output"),
                        (Panel::Terminal, "Terminal"),
                    ] {
                        let enabled = tab != Panel::Terminal
                            || (cfg!(feature = "terminal") && self.settings.terminal_enabled);
                        if ui
                            .add_enabled_ui(enabled, |ui| ui.selectable_label(panel == tab, label))
                            .inner
                            .clicked()
                        {
                            select = Some(tab);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        hide = controls::close_button(ui, "Hide panel").clicked();
                        maximize = ui
                            .small_button(if maximized {
                                "Restore panel"
                            } else {
                                "Maximize panel"
                            })
                            .clicked();
                    });
                });
                ui.separator();
                match panel {
                    Panel::Output => {
                        ui.horizontal(|ui| {
                            for (channel, label) in
                                [(OutputChannel::Build, "Build"), (OutputChannel::Ssh, "SSH")]
                            {
                                ui.selectable_value(&mut self.panel_state.output, channel, label);
                            }
                        });
                        match self.panel_state.output {
                            OutputChannel::Build => self.build_contents(ui),
                            OutputChannel::Ssh => self.ssh_output_contents(ui),
                        }
                    }
                    Panel::Problems => {
                        self.problems_contents(ui);
                    }
                    Panel::Terminal => self.terminal_contents(ui),
                }
            });
        if !maximized {
            self.panel_state.normal_height = shown.response.rect.height();
        }
        if maximize {
            self.maximize_panel(ctx);
        }
        if hide {
            self.hide_panel();
        } else if let Some(tab) = select {
            if tab == Panel::Terminal {
                if panel != tab {
                    self.toggle_terminal(ctx);
                }
            } else {
                self.set_panel(tab);
            }
        }
        if !self.terminal.open {
            ctx.memory_mut(|m| m.surrender_focus(terminal_panel::input_id()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{click_label, frame, text_rect};

    #[test]
    fn tab_switches_and_escape_keep_the_editor_and_output_intact() {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.workspace.open[0].page.text = "Keep this editor draft".into();
        app.workspace.edit(0, "").unwrap();
        let id = app.active().view.id;
        app.build.output = "Only visible in Output\n".into();
        app.execute(&ctx, Command::BuildResults);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let out = frame(&ctx, &mut app, vec![]);
        assert!(out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Only visible in Output").is_some()));
        click_label(&ctx, &mut app, "Problems");
        assert_eq!(app.active_panel(), Some(Panel::Problems));
        let out = frame(&ctx, &mut app, vec![]);
        assert!(!out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Only visible in Output").is_some()));
        click_label(&ctx, &mut app, "Output");
        assert_eq!(app.active_panel(), Some(Panel::Output));
        assert_eq!(app.build.output, "Only visible in Output\n");
        app.execute(&ctx, Command::LanguageProblems);
        frame(
            &ctx,
            &mut app,
            vec![crate::app::tests::key(Key::Escape, Modifiers::NONE)],
        );
        assert_eq!(app.active_panel(), None);
        assert_eq!(app.active().view.id, id);
        assert_eq!(app.active().page.text, "Keep this editor draft");
        #[cfg(feature = "terminal")]
        assert!(
            app.terminal.tabs.is_empty(),
            "Visiting Problems and Output never starts a shell"
        );
    }
}
#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::app::tests::frame;
    fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        settings_store::TEST_CONFIG_DIR.with(|d| *d.borrow_mut() = Some(dir.path().join("config")));
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        (dir, ctx, app)
    }
    #[test]
    fn last_panel_toggle_preserves_output_and_uses_problems_when_terminal_is_disabled() {
        let (_dir, ctx, mut app) = fixture();
        app.build.output = "preserved".into();
        app.set_panel(Panel::Output);
        app.toggle_last_panel(&ctx);
        assert_eq!(app.active_panel(), None);
        app.toggle_last_panel(&ctx);
        assert_eq!(app.active_panel(), Some(Panel::Output));
        assert_eq!(app.build.output, "preserved");
        app.hide_panel();
        app.panel_state.last = Panel::Terminal;
        app.toggle_last_panel(&ctx);
        assert_eq!(app.active_panel(), Some(Panel::Problems));
        #[cfg(feature = "terminal")]
        assert!(app.terminal.tabs.is_empty());
    }
    #[test]
    fn maximize_restore_and_hide_keep_the_normal_height_and_active_document() {
        let (_dir, ctx, mut app) = fixture();
        let id = app.active().view.id;
        app.set_panel(Panel::Output);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let height = app.panel_state.normal_height;
        app.maximize_panel(&ctx);
        frame(&ctx, &mut app, vec![]);
        assert!(app.panel_state.maximized);
        assert_eq!(app.panel_state.normal_height, height);
        let maximized =
            egui::containers::panel::PanelState::load(&ctx, egui::Id::new("workbench-panel"))
                .unwrap()
                .size()
                .y;
        assert!(maximized > height);
        app.hide_panel();
        app.toggle_last_panel(&ctx);
        assert!(app.panel_state.maximized);
        app.maximize_panel(&ctx);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(!app.panel_state.maximized);
        assert!(
            (app.panel_state.normal_height - height).abs() < 3.0,
            "{} vs {height}",
            app.panel_state.normal_height
        );
        assert_eq!(app.active().view.id, id);
    }
    #[test]
    fn focus_commands_preserve_search_inputs_panel_and_editor_text() {
        let (_dir, ctx, mut app) = fixture();
        app.quality.folder_query = "keep query".into();
        app.quality.folder_initialized = true;
        app.set_panel(Panel::Output);
        app.execute(&ctx, Command::FocusExplorer);
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("explorer-tree"))
        );
        app.execute(&ctx, Command::FocusSearch);
        assert_eq!(app.sidebar, workbench_navigation::Sidebar::Search);
        assert_eq!(app.quality.folder_query, "keep query");
        app.execute(&ctx, Command::FocusEditor);
        assert!(app.focus_editor);
        assert_eq!(app.active_panel(), Some(Panel::Output));
        assert!(app.active().page.text.is_empty());
    }
}
