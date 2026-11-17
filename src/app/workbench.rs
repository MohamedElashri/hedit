//! Workbench font loading is single-flight and never owns document jobs.
use super::*;

#[derive(Default)]
pub(super) struct FontState {
    pub data: Option<(Vec<u8>, u32)>,
    requested: Option<String>,
    loading: Option<u64>,
    generation: u64,
}
impl HeditApp {
    pub(super) fn workbench_preferences(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Interface typography and density")
            .id_salt("workbench-preferences")
            .show(ui, |ui| self.interface_settings(ui));
    }
    pub(super) fn apply_workbench_style(&mut self, ctx: &egui::Context) {
        hedit::ui_style::apply(ctx, self.settings.ui_font_size, self.settings.ui_density);
        let wanted = self.settings.ui_font_face.trim();
        if self.ui_font.requested.as_deref() == Some(wanted)
            || self.ui_font.loading.is_some()
            || self
                .settings_files
                .changed_at
                .is_some_and(|at| at.elapsed() < settings_store::SETTLE)
        {
            return;
        }
        self.ui_font.requested = Some(wanted.to_owned());
        self.ui_font.generation = self.ui_font.generation.wrapping_add(1);
        if wanted.is_empty() {
            if self.ui_font.data.take().is_some() {
                self.rebuild_fonts(ctx);
            }
            return;
        }
        let name = wanted.to_owned();
        let generation = self.ui_font.generation;
        self.ui_font.loading = Some(generation);
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = hedit::fonts::load(&name);
            let _ = tx.send(Ok(Action::UiFont(generation, name, result)));
            ctx.request_repaint();
        });
    }
    pub(super) fn finish_ui_font(
        &mut self,
        ctx: &egui::Context,
        generation: u64,
        name: String,
        result: Result<(Vec<u8>, u32), String>,
    ) {
        if self.ui_font.loading == Some(generation) {
            self.ui_font.loading = None;
        }
        if generation != self.ui_font.generation || self.settings.ui_font_face.trim() != name {
            return;
        }
        match result {
            Ok(font) => self.ui_font.data = Some(font),
            Err(error) => {
                self.ui_font.data = None;
                self.notice = format!("UI font: {error} Using built-in Noto Sans.");
            }
        }
        self.rebuild_fonts(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::frame;
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
        let app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        (dir, ctx, app)
    }
    #[test]
    fn ui_font_results_reject_stale_choices_and_preserve_document_jobs() {
        let (_dir, ctx, mut app) = app();
        app.settings.ui_font_face = " newest ".into();
        app.ui_font.generation = 2;
        app.ui_font.loading = Some(2);
        app.busy = true;
        let focus = egui::Id::new("document-job");
        app.job_editor_focus = Some(focus);
        app.tx
            .send(Ok(Action::UiFont(
                1,
                "old".into(),
                Err("stale error".into()),
            )))
            .unwrap();
        app.receive(&ctx);
        assert_eq!(app.ui_font.loading, Some(2));
        assert!(!app.notice.contains("stale error"));
        app.tx
            .send(Ok(Action::UiFont(
                2,
                "newest".into(),
                Ok((
                    include_bytes!("../../assets/fonts/NotoSans-Regular.ttf").to_vec(),
                    0,
                )),
            )))
            .unwrap();
        app.receive(&ctx);
        assert!(app.ui_font.data.is_some());
        assert_eq!(app.ui_font.loading, None);
        assert!(app.busy);
        assert_eq!(app.job_editor_focus, Some(focus));
        app.busy = false;
    }
    #[test]
    fn ui_metrics_follow_theme_changes_and_leave_document_preferences_unchanged() {
        let (_dir, ctx, mut app) = app();
        let before = app.settings.font_size;
        app.settings.ui_font_size = 18.0;
        app.settings.ui_density = hedit::ui_style::Density::Comfortable;
        for theme in [Theme::default(), Theme::paper()] {
            app.set_theme(&ctx, theme);
            frame(&ctx, &mut app, vec![]);
            assert_eq!(ctx.style().text_styles[&egui::TextStyle::Body].size, 18.0);
            assert_eq!(app.settings.font_size, before);
            assert!(app.settings.terminal_font.is_none());
        }
    }
    #[test]
    fn failed_ui_font_falls_back_without_repeated_requests() {
        let (_dir, ctx, mut app) = app();
        app.settings.ui_font_face = "missing".into();
        app.ui_font.generation = 1;
        app.ui_font.requested = Some("missing".into());
        app.ui_font.loading = Some(1);
        app.finish_ui_font(&ctx, 1, "missing".into(), Err("unavailable".into()));
        app.apply_workbench_style(&ctx);
        assert!(app.ui_font.data.is_none());
        assert_eq!(app.ui_font.loading, None);
        assert!(app.notice.contains("unavailable"));
    }

    #[test]
    fn settled_workbench_frames_do_not_reupload_fonts_or_icons() {
        let (_dir, ctx, mut app) = app();
        app.settings.caret_blink = false;
        app.settings.auto_complete = false;
        app.settings.show_folders = false;
        for _ in 0..5 {
            frame(&ctx, &mut app, vec![]);
        }
        for _ in 0..30 {
            let output = frame(&ctx, &mut app, vec![]);
            assert!(
                output.textures_delta.set.is_empty(),
                "idle workbench uploaded textures again"
            );
        }
        assert!(app.ui_font.loading.is_none());
    }

    #[test]
    fn folder_toolbar_controls_stay_visible_and_clickable_with_larger_ui_fonts() {
        use crate::app::tests::{click_at, frame_size};
        use hedit::{icons::Icon, ui_style::Density};
        let size = egui::vec2(720.0, 480.0);
        for font in [10.0, 13.0, 20.0] {
            for density in [Density::Compact, Density::Default, Density::Comfortable] {
                let (_dir, ctx, mut app) = app();
                app.settings.ui_font_size = font;
                app.settings.ui_density = density;
                for _ in 0..4 {
                    frame_size(&ctx, &mut app, vec![], size);
                }
                let out = frame_size(&ctx, &mut app, vec![], size);
                let sidebar =
                    egui::containers::panel::PanelState::load(&ctx, egui::Id::new("buffers"))
                        .unwrap()
                        .rect;
                let mut close = None;
                for icon in [Icon::Add, Icon::Refresh, Icon::Close] {
                    let rect = out
                        .shapes
                        .iter()
                        .find_map(|s| match &s.shape {
                            egui::Shape::Mesh(mesh)
                                if icon.matches_texture(&ctx, mesh.texture_id)
                                    && sidebar.contains_rect(mesh.calc_bounds())
                                    && s.clip_rect.contains_rect(mesh.calc_bounds()) =>
                            {
                                Some(mesh.calc_bounds())
                            }
                            _ => None,
                        })
                        .unwrap_or_else(|| {
                            panic!("folder action {icon:?} clipped at font {font}, {density:?}")
                        });
                    if icon == Icon::Close {
                        close = Some(rect.center());
                    }
                }
                click_at(&ctx, &mut app, close.unwrap(), size);
                assert!(!app.settings.show_folders);
                assert!(app.active().page.text.is_empty());
            }
        }
    }

    #[test]
    fn font_rebuild_refreshes_offscreen_tab_geometry_and_keeps_active_tab_visible() {
        let (_dir, ctx, mut app) = app();
        app.settings.show_sidebar = false;
        for i in 0..100 {
            if i > 0 {
                app.workspace.new_buffer().unwrap();
            }
            app.workspace
                .name_scratchpad(i, &format!("wide-font-tab-{i:03}.rs"))
                .unwrap();
        }
        app.select_tab(99);
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let before = app.tab_strip.width();
        app.settings.ui_font_face = "choice".into();
        app.ui_font.requested = Some("choice".into());
        app.ui_font.generation = 1;
        app.ui_font.loading = Some(1);
        app.finish_ui_font(
            &ctx,
            1,
            "choice".into(),
            Ok((
                include_bytes!("../../assets/fonts/NotoSansArabic.ttf").to_vec(),
                0,
            )),
        );
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        assert!(
            (app.tab_strip.width() - before).abs() > 1.0,
            "all cached widths must reflect the new proportional fallback"
        );
        app.settings.ui_font_size = 20.0;
        app.settings.ui_density = hedit::ui_style::Density::Comfortable;
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        let output = frame(&ctx, &mut app, vec![]);
        assert!(output.shapes.iter().any(|s| super::super::tests::text_rect(
            &s.shape,
            "wide-font-tab-099.rs"
        )
        .is_some_and(|r| r.intersects(s.clip_rect))));
        assert!(app.tab_strip.height() >= 40.0);
    }

    #[test]
    #[ignore = "exports actual workbench UI previews to HEDIT_PREVIEW_DIR"]
    fn export_workbench_previews() {
        use hedit::ui_style::Density;
        let target =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&target).unwrap();
        for (name, light, size, font, density, settings) in [
            (
                "workbench-dark",
                false,
                egui::vec2(1280.0, 820.0),
                13.0,
                Density::Default,
                false,
            ),
            (
                "workbench-light",
                true,
                egui::vec2(1280.0, 820.0),
                13.0,
                Density::Default,
                false,
            ),
            (
                "workbench-compact",
                false,
                egui::vec2(720.0, 480.0),
                10.0,
                Density::Compact,
                false,
            ),
            (
                "workbench-large",
                true,
                egui::vec2(720.0, 480.0),
                20.0,
                Density::Comfortable,
                false,
            ),
            (
                "settings-dark",
                false,
                egui::vec2(1280.0, 820.0),
                13.0,
                Density::Default,
                true,
            ),
            (
                "settings-large",
                false,
                egui::vec2(720.0, 480.0),
                20.0,
                Density::Comfortable,
                true,
            ),
            (
                "settings-compact",
                true,
                egui::vec2(720.0, 480.0),
                13.0,
                Density::Default,
                true,
            ),
        ] {
            let (_dir, ctx, mut app) = app();
            ctx.set_pixels_per_point(1.5);
            app.set_theme(
                &ctx,
                if light {
                    Theme::paper()
                } else {
                    Theme::default()
                },
            );
            app.settings.ui_font_size = font;
            app.settings.ui_density = density;
            app.settings.caret_blink = false;
            app.settings.auto_complete = false;
            app.settings.show_folders = false;
            app.workspace.name_scratchpad(0, "main.rs").unwrap();
            app.workspace.open[0].page.text =
                "// A focused native workbench\nfn main() {\n    println!(\"Hello, hedit\");\n}\n"
                    .into();
            app.workspace.edit(0, "").unwrap();
            app.workspace.new_buffer().unwrap();
            app.workspace.name_scratchpad(1, "config.json").unwrap();
            app.select_tab(0);
            if settings {
                app.open_preferences(PreferencesPage::Appearance);
            }
            let mut textures = std::collections::HashMap::new();
            for _ in 0..4 {
                let out = super::super::tests::frame_size(&ctx, &mut app, vec![], size);
                for (id, delta) in &out.textures_delta.set {
                    if let egui::ImageData::Color(image) = &delta.image {
                        if delta.pos.is_none() {
                            textures.insert(*id, (**image).clone());
                        }
                    }
                }
            }
            let out = super::super::tests::frame_size(&ctx, &mut app, vec![], size);
            std::fs::write(
                target.join(format!("{name}.ppm")),
                super::super::rtl_tests::rasterize_with_textures(&ctx, out, size, 1.5, &textures),
            )
            .unwrap();
        }
    }
}
