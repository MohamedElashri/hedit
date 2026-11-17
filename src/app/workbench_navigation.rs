//! Compact workbench navigation using the same commands and document state.
use super::*;
use hedit::icons::{self, Icon};

impl HeditApp {
    pub(super) fn activity_bar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_activity_bar || self.distraction_free {
            return;
        }
        enum Action {
            Files,
            Git,
            Command(Command),
        }
        let mut action = None;
        let width = (self.settings.ui_font_size + 23.0).max(48.0);
        egui::SidePanel::left("activity-bar")
            .exact_width(width)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(self.theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(6, 6)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    if icons::navigation(
                        ui,
                        Icon::Files,
                        "Files",
                        !self.source_control.open && self.settings.show_sidebar,
                    )
                    .clicked()
                    {
                        action = Some(Action::Files);
                    }
                    if ui
                        .add_enabled_ui(self.command_enabled(Command::SourceControl), |ui| {
                            icons::navigation(
                                ui,
                                Icon::SourceControl,
                                "Source Control",
                                self.source_control.open && self.settings.show_sidebar,
                            )
                        })
                        .inner
                        .clicked()
                    {
                        action = Some(Action::Git);
                    }
                    if ui
                        .add_enabled_ui(self.command_enabled(Command::FindFiles), |ui| {
                            icons::navigation(ui, Icon::Search, "Find in files", false)
                        })
                        .inner
                        .clicked()
                    {
                        action = Some(Action::Command(Command::FindFiles));
                    }
                });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled_ui(self.command_enabled(Command::Settings), |ui| {
                            icons::navigation(ui, Icon::Settings, "Settings", false)
                        })
                        .inner
                        .clicked()
                    {
                        action = Some(Action::Command(Command::Settings));
                    }
                });
            });
        match action {
            Some(Action::Files) => {
                if !self.source_control.open && self.settings.show_sidebar {
                    self.execute(ctx, Command::Sidebar);
                } else {
                    self.source_control.open = false;
                    self.settings.show_sidebar = true;
                    self.focus_editor = true;
                }
            }
            Some(Action::Git) => {
                if self.source_control.open && self.settings.show_sidebar {
                    self.execute(ctx, Command::Sidebar);
                } else {
                    self.open_source_control(ctx);
                }
            }
            Some(Action::Command(command)) => self.execute(ctx, command),
            None => {}
        }
    }
    pub(super) fn workspace_header(&mut self, ui: &mut egui::Ui, git: bool) -> Option<bool> {
        if !self.settings.show_activity_bar {
            return controls::workspace_switcher(
                ui,
                &self.theme,
                git,
                &self.shortcut_text(Command::SourceControl),
            );
        }
        let m = hedit::ui_style::Metrics::for_ui(ui);
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(if git { "Source Control" } else { "Files" })
                    .size(m.small)
                    .strong(),
            )
            .truncate(),
        );
        ui.add_space(6.0);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{click_at, frame_size};
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.auto_complete = false;
        (dir, ctx, app)
    }
    fn icon(output: &egui::FullOutput, ctx: &egui::Context, icon: Icon) -> egui::Rect {
        output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Mesh(m) if icon.matches_texture(ctx, m.texture_id) => {
                    Some(m.calc_bounds())
                }
                _ => None,
            })
            .expect("visible activity action")
    }
    #[test]
    fn activity_navigation_preserves_documents_and_sidebar_state() {
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(720.0, 480.0);
        app.workspace.open[0].page.text = "Keep this unsaved note".into();
        app.workspace.edit(0, "").unwrap();
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let out = frame_size(&ctx, &mut app, vec![], size);
        let files = icon(&out, &ctx, Icon::Files);
        click_at(&ctx, &mut app, files.center(), size);
        assert!(!app.settings.show_sidebar);
        let out = frame_size(&ctx, &mut app, vec![], size);
        click_at(&ctx, &mut app, icon(&out, &ctx, Icon::Files).center(), size);
        assert!(app.settings.show_sidebar);
        let out = frame_size(&ctx, &mut app, vec![], size);
        click_at(
            &ctx,
            &mut app,
            icon(&out, &ctx, Icon::SourceControl).center(),
            size,
        );
        assert!(app.source_control.open);
        let out = frame_size(&ctx, &mut app, vec![], size);
        click_at(&ctx, &mut app, icon(&out, &ctx, Icon::Files).center(), size);
        assert!(!app.source_control.open);
        assert_eq!(app.active().page.text, "Keep this unsaved note");
    }
    #[test]
    fn activity_bar_can_be_hidden_and_distraction_free_keeps_the_preference() {
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(720.0, 480.0);
        app.settings.show_activity_bar = false;
        let out = frame_size(&ctx, &mut app, vec![], size);
        assert!(!out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Mesh(m) if Icon::Search.matches_texture(&ctx,m.texture_id))));
        app.settings.show_activity_bar = true;
        app.execute(&ctx, Command::DistractionFree);
        let out = frame_size(&ctx, &mut app, vec![], size);
        assert!(!out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Mesh(m) if Icon::Search.matches_texture(&ctx,m.texture_id))));
        assert!(app.settings.show_activity_bar);
    }
}
