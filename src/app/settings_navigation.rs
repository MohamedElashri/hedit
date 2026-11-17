//! Settings categories behave as a keyboard-navigable list without capturing text keys.
use super::*;

impl HeditApp {
    pub(super) fn settings_navigation(
        &mut self,
        ui: &mut egui::Ui,
        searching: bool,
        counts: &[usize; 11],
        height: f32,
        width: f32,
    ) {
        let m = hedit::ui_style::Metrics::for_ui(ui);
        let active = if searching {
            self.settings_browser.category
        } else {
            Some(self.preferences_page)
        };
        let token = (
            active,
            searching,
            height.to_bits(),
            width.to_bits(),
            m.row.to_bits(),
        );
        let reveal_id = egui::Id::new("settings-category-reveal");
        let reveal = ui.data_mut(|data| {
            let changed = data.get_temp(reveal_id) != Some(token);
            data.insert_temp(reveal_id, token);
            changed
        });
        ui.set_width(width);
        egui::ScrollArea::vertical()
            .id_salt("settings-categories")
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut entries = Vec::new();
                if searching {
                    entries.push((None, "All results".to_owned()));
                }
                entries.extend(
                    PreferencesPage::ALL
                        .into_iter()
                        .filter(|p| p.available())
                        .map(|p| {
                            (
                                Some(p),
                                if searching {
                                    format!("{} ({})", p.title(), counts[p as usize])
                                } else {
                                    p.title().into()
                                },
                            )
                        }),
                );
                let mut responses = Vec::new();
                for (page, label) in &entries {
                    let selected = if searching {
                        self.settings_browser.category == *page
                    } else {
                        Some(self.preferences_page) == *page
                    };
                    let response = ui
                        .push_id(("settings-category", page.map(|p| p as u8)), |ui| {
                            ui.add(
                                egui::Button::new(label)
                                    .min_size(egui::vec2(ui.available_width(), m.row))
                                    .selected(selected)
                                    .frame(false)
                                    .truncate(),
                            )
                            .on_hover_text(format!(
                                "{label}\nUse Up/Down, Home or End while focused"
                            ))
                        })
                        .inner;
                    if response.clicked() {
                        response.request_focus();
                        self.select_settings_category(*page, searching);
                    }
                    if selected && reveal {
                        response.scroll_to_me_animation(None, egui::style::ScrollAnimation::none());
                    }
                    if response.has_focus() {
                        ui.memory_mut(|memory| {
                            memory.set_focus_lock_filter(
                                response.id,
                                egui::EventFilter {
                                    vertical_arrows: true,
                                    ..Default::default()
                                },
                            )
                        });
                    }
                    responses.push(response);
                }
                if let Some(at) = responses.iter().position(|r| r.has_focus()) {
                    let target = ui.input_mut(|input| {
                        if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                            Some((at + 1).min(entries.len() - 1))
                        } else if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                            Some(at.saturating_sub(1))
                        } else if input.consume_key(Modifiers::NONE, Key::Home) {
                            Some(0)
                        } else if input.consume_key(Modifiers::NONE, Key::End) {
                            Some(entries.len() - 1)
                        } else {
                            None
                        }
                    });
                    if let Some(target) = target {
                        self.select_settings_category(entries[target].0, searching);
                        responses[target].request_focus();
                        responses[target].scroll_to_me(None);
                    }
                }
            });
    }
    fn select_settings_category(&mut self, page: Option<PreferencesPage>, searching: bool) {
        if let Some(page) = page {
            self.preferences_page = page;
        }
        self.settings_browser.category = if searching { page } else { None };
        self.settings_browser.reset_scroll = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{click_at, frame_size, key, text_rect};
    const SIZE: egui::Vec2 = egui::vec2(720.0, 480.0);
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        ctx.style_mut(|s| s.scroll_animation = egui::style::ScrollAnimation::none());
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.auto_complete = false;
        app.font_families = Some(vec![]);
        app.open_preferences(PreferencesPage::General);
        (dir, ctx, app)
    }
    fn visible(output: &egui::FullOutput, label: &str, size: egui::Vec2) -> egui::Rect {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        output
            .shapes
            .iter()
            .find_map(|s| {
                text_rect(&s.shape, label)
                    .filter(|r| screen.contains_rect(*r) && s.clip_rect.contains_rect(*r))
            })
            .unwrap_or_else(|| panic!("missing or clipped {label}"))
    }
    #[test]
    fn settings_header_search_reset_and_footer_fit_the_typography_matrix() {
        use hedit::ui_style::Density;
        for font in [10.0, 13.0, 20.0] {
            for density in [Density::Compact, Density::Default, Density::Comfortable] {
                for dpi in [1.0, 1.5, 2.0] {
                    let (_dir, ctx, mut app) = app();
                    ctx.set_pixels_per_point(dpi);
                    app.set_theme(
                        &ctx,
                        if dpi == 1.5 {
                            Theme::paper()
                        } else {
                            Theme::default()
                        },
                    );
                    app.settings.ui_font_size = font;
                    app.settings.ui_density = density;
                    app.settings_browser.query = "ui_font_size".into();
                    for _ in 0..5 {
                        frame_size(&ctx, &mut app, vec![], SIZE);
                    }
                    let out = frame_size(&ctx, &mut app, vec![], SIZE);
                    for label in ["Clear", "Open settings.json", "UI font size", "Reset"] {
                        visible(&out, label, SIZE);
                    }
                    let search = ctx
                        .read_response(egui::Id::new("settings-search"))
                        .unwrap()
                        .rect;
                    let modified = visible(&out, "Modified", SIZE);
                    assert!(search.width() >= 80.0 && search.right() < modified.left());
                    let footer = visible(&out, "Open settings.json", SIZE);
                    assert!(visible(&out, "UI font size", SIZE).bottom() < footer.top());
                }
            }
        }
    }
    #[test]
    fn category_arrows_home_and_end_navigate_without_capturing_text_field_keys() {
        let (_dir, ctx, mut app) = app();
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], SIZE);
        }
        let out = frame_size(&ctx, &mut app, vec![], SIZE);
        let general = visible(&out, "General", SIZE);
        click_at(&ctx, &mut app, general.center(), SIZE);
        frame_size(
            &ctx,
            &mut app,
            vec![key(Key::ArrowDown, Modifiers::NONE)],
            SIZE,
        );
        assert_eq!(app.preferences_page, PreferencesPage::Terminal);
        frame_size(&ctx, &mut app, vec![key(Key::End, Modifiers::NONE)], SIZE);
        assert_eq!(app.preferences_page, PreferencesPage::Recovery);
        frame_size(&ctx, &mut app, vec![key(Key::Home, Modifiers::NONE)], SIZE);
        assert_eq!(app.preferences_page, PreferencesPage::General);
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("settings-search")));
        frame_size(
            &ctx,
            &mut app,
            vec![key(Key::ArrowDown, Modifiers::NONE)],
            SIZE,
        );
        assert_eq!(app.preferences_page, PreferencesPage::General);
        assert!(app.active().page.text.is_empty());
    }

    #[test]
    fn installed_ui_font_filter_selects_and_restores_without_changing_document_fonts() {
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(1280.0, 820.0);
        app.font_families = Some(vec![
            hedit::fonts::Family {
                name: "Test Sans".into(),
                monospace: false,
                scripts: vec!["Arabic"],
            },
            hedit::fonts::Family {
                name: "Other Family".into(),
                monospace: true,
                scripts: vec![],
            },
        ]);
        let editor = app.settings.font_face.clone();
        let terminal = app.settings.terminal_font.clone();
        app.settings_browser.query = "ui_font_face".into();
        for _ in 0..5 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let out = frame_size(&ctx, &mut app, vec![], size);
        click_at(
            &ctx,
            &mut app,
            visible(&out, "Choose installed font…", size).center(),
            size,
        );
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("ui-font-filter-input")));
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::Text("Arabic".into())],
            size,
        );
        let out = frame_size(&ctx, &mut app, vec![], size);
        assert!(!out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Other Family").is_some()));
        click_at(
            &ctx,
            &mut app,
            visible(&out, "Test Sans", size).center(),
            size,
        );
        assert_eq!(app.settings.ui_font_face, "Test Sans");
        assert_eq!(app.settings.font_face, editor);
        assert_eq!(app.settings.terminal_font, terminal);
        let out = frame_size(&ctx, &mut app, vec![], size);
        click_at(
            &ctx,
            &mut app,
            visible(&out, "Choose installed font…", size).center(),
            size,
        );
        let out = frame_size(&ctx, &mut app, vec![], size);
        click_at(
            &ctx,
            &mut app,
            visible(&out, "Built-in (Noto Sans)", size).center(),
            size,
        );
        assert!(app.settings.ui_font_face.is_empty());
        assert!(app.active().page.text.is_empty());
    }

    #[test]
    fn opening_a_later_category_reveals_it_at_large_ui_sizes() {
        let (_dir, ctx, mut app) = app();
        app.settings.ui_font_size = 20.0;
        app.settings.ui_density = hedit::ui_style::Density::Comfortable;
        app.open_preferences(PreferencesPage::Appearance);
        for _ in 0..5 {
            frame_size(&ctx, &mut app, vec![], SIZE);
        }
        let out = frame_size(&ctx, &mut app, vec![], SIZE);
        assert_eq!(
            out.shapes
                .iter()
                .filter(|s| text_rect(&s.shape, "Appearance")
                    .is_some_and(|r| s.clip_rect.contains_rect(r)))
                .count(),
            2,
            "both the selected category and the content heading are visible"
        );
    }

    #[test]
    fn every_settings_category_keeps_search_and_footer_reachable_at_minimum_size() {
        for (font, density) in [
            (10.0, hedit::ui_style::Density::Compact),
            (20.0, hedit::ui_style::Density::Comfortable),
        ] {
            let (_dir, ctx, mut app) = app();
            app.settings.ui_font_size = font;
            app.settings.ui_density = density;
            for page in PreferencesPage::ALL.into_iter().filter(|p| p.available()) {
                eprintln!("Checking {} at UI size {font}", page.title());
                app.open_preferences(page);
                for _ in 0..5 {
                    frame_size(&ctx, &mut app, vec![], SIZE);
                }
                let out = frame_size(&ctx, &mut app, vec![], SIZE);
                for label in ["Clear", "Open settings.json"] {
                    visible(&out, label, SIZE);
                }
                let search = ctx
                    .read_response(egui::Id::new("settings-search"))
                    .unwrap()
                    .rect;
                assert!(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE).contains_rect(search),
                    "{}: {search:?}",
                    page.title()
                );
            }
        }
    }
}
