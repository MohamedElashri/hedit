//! A single bottom region. Visibility never owns a shell, build, or diagnostic.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Panel {
    Problems,
    Output,
    Terminal,
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

    pub(super) fn hide_panel(&mut self) {
        self.terminal.open = false;
        self.terminal.release_input();
        self.build.open = false;
        self.languages.problems = false;
        self.focus_editor = true;
    }

    pub(super) fn workbench_panel(&mut self, ctx: &egui::Context) {
        let Some(panel) = self.active_panel() else {
            return;
        };
        let mut select = None;
        let mut hide = false;
        egui::TopBottomPanel::bottom("workbench-panel")
            .resizable(true)
            .default_height(230.0)
            .height_range(90.0..=(ctx.screen_rect().height() * 0.6).max(100.0))
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
                    });
                });
                ui.separator();
                match panel {
                    Panel::Output => self.build_contents(ui),
                    Panel::Problems => {
                        self.build_problem_contents(ui);
                        self.problems_contents(ui);
                    }
                    Panel::Terminal => self.terminal_contents(ui),
                }
            });
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

    fn build_problem_contents(&mut self, ui: &mut egui::Ui) {
        if self.build.locations.is_empty() {
            return;
        }
        let mut jump = None;
        ui.strong(format!(
            "Build · {} {}",
            self.build.locations.len(),
            if self.build.locations.len() == 1 {
                "result"
            } else {
                "results"
            }
        ));
        egui::ScrollArea::vertical()
            .id_salt("build-problems")
            .max_height((ui.available_height() * 0.45).max(30.0))
            .show_rows(
                ui,
                ui.text_style_height(&egui::TextStyle::Body),
                self.build.locations.len(),
                |ui, range| {
                    for index in range {
                        let location = &self.build.locations[index];
                        if ui
                            .selectable_label(
                                self.build.current == Some(index),
                                format!(
                                    "{}:{} · {}",
                                    location.file.display(),
                                    location.line,
                                    location.message
                                ),
                            )
                            .on_hover_text(location.file.display().to_string())
                            .clicked()
                        {
                            jump = Some(index);
                        }
                    }
                },
            );
        if let Some(index) = jump {
            // Keep Problems selected when following a build diagnostic.
            self.go_to_location(index);
            self.set_panel(Panel::Problems);
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
