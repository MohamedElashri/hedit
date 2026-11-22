//! Compact workbench navigation using the same commands and document state.
use super::*;
use hedit::icons::{self, Icon};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Sidebar {
    #[default]
    Files,
    Search,
    SourceControl,
    Remote,
}

impl HeditApp {
    pub(super) fn show_git_sidebar(&mut self, ctx: &egui::Context) {
        self.sidebar = Sidebar::SourceControl;
        self.settings.show_sidebar = true;
        self.distraction_free = false;
        self.prepare_source_control(ctx);
    }
    pub(super) fn show_search_sidebar(&mut self) {
        if !self.quality.folder_initialized {
            if self.quality.folder_query.is_empty() {
                self.quality.folder_query.clone_from(&self.query);
            }
            self.quality.folder_initialized = true;
        }
        self.sidebar = Sidebar::Search;
        self.settings.show_sidebar = true;
        self.distraction_free = false;
        self.quality.folders_open = false;
        self.quality.folder_focus = true;
        self.focus_editor = false;
    }

    pub(super) fn activity_bar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_activity_bar || self.distraction_free {
            return;
        }
        enum Action {
            Files,
            Git,
            Remote,
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
                        self.sidebar == Sidebar::Files && self.settings.show_sidebar,
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
                                self.sidebar == Sidebar::SourceControl
                                    && self.settings.show_sidebar,
                            )
                        })
                        .inner
                        .clicked()
                    {
                        action = Some(Action::Git);
                    }
                    if ui
                        .add_enabled_ui(self.command_enabled(Command::FindFiles), |ui| {
                            icons::navigation(
                                ui,
                                Icon::Search,
                                "Find in files",
                                self.sidebar == Sidebar::Search && self.settings.show_sidebar,
                            )
                        })
                        .inner
                        .clicked()
                    {
                        action = Some(Action::Command(Command::FindFiles));
                    }
                    if icons::navigation(
                        ui,
                        Icon::Remote,
                        "Remote Explorer",
                        self.sidebar == Sidebar::Remote && self.settings.show_sidebar,
                    )
                    .clicked()
                    {
                        action = Some(Action::Remote);
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
                if self.sidebar == Sidebar::Files && self.settings.show_sidebar {
                    self.execute(ctx, Command::Sidebar);
                } else {
                    self.sidebar = Sidebar::Files;
                    self.settings.show_sidebar = true;
                    self.focus_editor = true;
                }
            }
            Some(Action::Git) => {
                if self.sidebar == Sidebar::SourceControl && self.settings.show_sidebar {
                    self.execute(ctx, Command::Sidebar);
                } else {
                    self.show_git_sidebar(ctx);
                }
            }
            Some(Action::Remote) => {
                self.close_transient(ctx);
                if self.source_control.open {
                    self.leave_review();
                    self.focus_editor = true;
                }
                self.show_remote_explorer(ctx);
            }
            Some(Action::Command(command)) => self.execute(ctx, command),
            None => {}
        }
    }
    pub(super) fn workspace_header(&mut self, ui: &mut egui::Ui, git: bool) -> Option<bool> {
        if !self.settings.show_activity_bar {
            let selected = controls::workspace_switcher(
                ui,
                &self.theme,
                git,
                &self.shortcut_text(Command::SourceControl),
            );
            if selected == Some(false) {
                self.sidebar = Sidebar::Files;
            }
            return selected;
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
    use crate::app::tests::{click_at, frame_size, text_rect};
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
        assert!(!out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Remote Explorer").is_some()));
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
        assert_eq!(app.sidebar, Sidebar::SourceControl);
        assert!(!app.source_control.open);
        let out = frame_size(&ctx, &mut app, vec![], size);
        assert!(!out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Remote Explorer").is_some()));
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
