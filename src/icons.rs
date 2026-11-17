//! Pinned Codicons rendered from embedded vectors, independent of chosen fonts.
use eframe::egui::{self, Color32, TextureHandle};

macro_rules! icons {
    ($($variant:ident => $file:literal),* $(,)?) => {
        #[derive(Clone,Copy,Debug,PartialEq,Eq,Hash)]
        pub enum Icon {$($variant),*}
        impl Icon {
            pub const ALL: &'static [Self]=&[$(Self::$variant),*];
            pub fn svg(self)-> &'static [u8] {
                match self {$(Self::$variant => include_bytes!(concat!("../assets/icons/codicons/",$file,".svg"))),*}
            }
        }
    }
}
icons! {
    Files=>"files", Search=>"search", SourceControl=>"source-control",
    Terminal=>"terminal", Settings=>"settings-gear", Close=>"close", Add=>"add",
    ChevronDown=>"chevron-down", ChevronRight=>"chevron-right", ChevronUp=>"chevron-up",
    More=>"ellipsis", Refresh=>"refresh", Folder=>"folder", FolderOpen=>"folder-opened",
    File=>"file", Split=>"split-horizontal", Sidebar=>"layout-sidebar-left",
    SidebarOff=>"layout-sidebar-left-off", Check=>"check", Warning=>"warning",
    Error=>"error", Debug=>"debug",
}

#[derive(Clone, Default)]
struct Cached(std::sync::Arc<std::sync::Mutex<Vec<(u32, TextureHandle)>>>);
impl Icon {
    fn key(self) -> egui::Id {
        egui::Id::new(("workbench-codicon", self))
    }
    /// Keep at most four pixel sizes per icon/context; no unbounded DPI cache.
    pub fn texture(self, ctx: &egui::Context, points: f32) -> TextureHandle {
        let points = if points.is_finite() { points } else { 16.0 };
        let pixels = (points * ctx.pixels_per_point()).ceil().clamp(8.0, 128.0) as u32;
        let cache = ctx
            .data(|d| d.get_temp::<Cached>(self.key()))
            .unwrap_or_default();
        if let Some((_, texture)) = cache
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|(size, _)| *size == pixels)
        {
            return texture.clone();
        }
        let image = rasterize(self, pixels).expect("embedded Codicons are validated by tests");
        let texture = ctx.load_texture(
            format!("codicon-{self:?}-{pixels}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        {
            let mut entries = cache.0.lock().unwrap();
            if entries.len() >= 4 {
                drop(entries.remove(0));
            }
            entries.push((pixels, texture.clone()));
        }
        ctx.data_mut(|d| d.insert_temp(self.key(), cache));
        texture
    }
    pub fn matches_texture(self, ctx: &egui::Context, id: egui::TextureId) -> bool {
        ctx.data(|d| d.get_temp::<Cached>(self.key()))
            .is_some_and(|c| c.0.lock().unwrap().iter().any(|(_, t)| t.id() == id))
    }
    pub fn paint(self, ui: &egui::Ui, rect: egui::Rect, color: Color32) {
        if ui.is_rect_visible(rect) {
            let texture = self.texture(ui.ctx(), rect.width().max(rect.height()));
            ui.painter().image(
                texture.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                color,
            );
        }
    }
}

fn rasterize(icon: Icon, pixels: u32) -> Result<egui::ColorImage, String> {
    use resvg::{tiny_skia, usvg};
    let options = usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..Default::default()
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(icon.svg(), &options).map_err(|e| e.to_string())?;
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new(pixels, pixels).ok_or("Invalid icon size")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(
            pixels as f32 / size.width(),
            pixels as f32 / size.height(),
        ),
        &mut pixmap.as_mut(),
    );
    Ok(egui::ColorImage::from_rgba_premultiplied(
        [pixels as usize, pixels as usize],
        pixmap.data(),
    ))
}

/// Flat toolbar action with a predictable hit target and visible keyboard focus.
pub fn button(ui: &mut egui::Ui, icon: Icon, label: &str) -> egui::Response {
    control(ui, icon, label, None, None)
}
pub fn toggle(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    control(ui, icon, label, Some(selected), None)
}
pub fn navigation(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> egui::Response {
    let side = crate::ui_style::Metrics::for_ui(ui).control.max(36.0);
    control(ui, icon, label, Some(selected), Some((side, 24.0)))
}
fn control(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    state: Option<bool>,
    size: Option<(f32, f32)>,
) -> egui::Response {
    let selected = state.unwrap_or(false);
    let m = crate::ui_style::Metrics::for_ui(ui);
    let (side, glyph) = size.unwrap_or((m.control, m.icon));
    let (_, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    response.widget_info(|| match state {
        Some(selected) => {
            egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
        }
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label),
    });
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(response.rect, 2.0, visuals.weak_bg_fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(0.5),
            2.0,
            egui::Stroke::new(1.0_f32, ui.visuals().widgets.active.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    let color = if !ui.is_enabled() {
        ui.visuals().weak_text_color().gamma_multiply(0.5)
    } else {
        visuals.fg_stroke.color
    };
    icon.paint(
        ui,
        egui::Rect::from_center_size(response.rect.center(), egui::vec2(glyph, glyph)),
        color,
    );
    response.on_hover_text(label)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_embedded_icon_renders_visible_white_alpha_at_multiple_dpis() {
        for &icon in Icon::ALL {
            let source = std::str::from_utf8(icon.svg()).unwrap();
            assert!(
                !source.contains("href=") && !source.contains("<text") && !source.contains("url(")
            );
            for pixels in [16, 24, 32, 64] {
                let image = rasterize(icon, pixels).unwrap();
                assert_eq!(image.size, [pixels as usize, pixels as usize]);
                let visible: Vec<_> = image.pixels.iter().filter(|c| c.a() > 0).collect();
                assert!(visible.len() > 4, "{icon:?}");
                assert!(
                    visible
                        .iter()
                        .all(|c| c.r() == c.g() && c.g() == c.b() && c.r() == c.a()),
                    "tintable premultiplied white: {icon:?}"
                );
            }
        }
    }
    #[test]
    fn cache_reuses_textures_and_replaces_them_on_dpi_changes() {
        let ctx = egui::Context::default();
        let mut first = None;
        let _ = ctx.run(Default::default(), |ctx| {
            let a = Icon::Settings.texture(ctx, 16.0);
            let b = Icon::Settings.texture(ctx, 16.0);
            assert_eq!(a.id(), b.id());
            first = Some(a.id());
        });
        ctx.set_pixels_per_point(2.0);
        let _ = ctx.run(Default::default(), |ctx| {
            let c = Icon::Settings.texture(ctx, 16.0);
            assert_ne!(Some(c.id()), first);
            assert!(Icon::Settings.matches_texture(ctx, c.id()));
        });
    }

    #[test]
    fn mixed_tree_and_toolbar_sizes_reuse_a_bounded_cache() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            let a = Icon::ChevronDown.texture(ctx, 12.0);
            let b = Icon::ChevronDown.texture(ctx, 16.0);
            assert_eq!(Icon::ChevronDown.texture(ctx, 12.0).id(), a.id());
            assert_eq!(Icon::ChevronDown.texture(ctx, 16.0).id(), b.id());
            for pixels in 20..100 {
                drop(Icon::ChevronDown.texture(ctx, pixels as f32));
            }
            let cache = ctx
                .data(|d| d.get_temp::<Cached>(Icon::ChevronDown.key()))
                .unwrap();
            assert!(cache.0.lock().unwrap().len() <= 4);
        });
    }

    #[test]
    fn toolbar_actions_keep_keyboard_focus_and_disabled_activation_semantics() {
        let ctx = egui::Context::default();
        let theme = crate::theme::Theme::default();
        theme.apply(&ctx);
        fn draw(
            ctx: &egui::Context,
            events: Vec<egui::Event>,
            enabled: bool,
        ) -> (egui::FullOutput, egui::Response) {
            let mut response = None;
            let out = ctx.run(
                egui::RawInput {
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| {
                    crate::ui_style::apply(ctx, 13.0, crate::ui_style::Density::Default);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        response = Some(
                            ui.add_enabled_ui(enabled, |ui| {
                                button(ui, Icon::Search, "Search workspace")
                            })
                            .inner,
                        );
                    });
                },
            );
            (out, response.unwrap())
        }
        let (_, first) = draw(&ctx, vec![], true);
        ctx.memory_mut(|m| m.request_focus(first.id));
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let (out, focused) = draw(&ctx, vec![key(true)], true);
        assert!(focused.clicked());
        assert!(out.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Rect(r) if r.rect==focused.rect.shrink(0.5) && r.stroke.color==theme.accent.color())));
        let pos = first.rect.center();
        let pointer = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        draw(
            &ctx,
            vec![key(false), egui::Event::PointerMoved(pos), pointer(true)],
            false,
        );
        let (_, disabled) = draw(&ctx, vec![pointer(false)], false);
        assert!(!disabled.clicked());
        assert_eq!(first.rect.size(), disabled.rect.size());
        assert_eq!(first.rect.width(), first.rect.height());
    }

    #[test]
    fn primary_action_foregrounds_remain_readable_for_every_palette_and_custom_accents() {
        for theme in crate::theme::builtins()
            .iter()
            .map(|b| crate::theme::builtin(&b.name).unwrap())
        {
            assert!(crate::theme::Theme::contrast(theme.accent, theme.accent_text()) >= 4.5);
        }
        let mut theme = crate::theme::Theme::default();
        for rgb in [
            [0, 0, 0],
            [255, 255, 255],
            [0, 120, 212],
            [120, 120, 120],
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
        ] {
            theme.accent = crate::theme::Rgb(rgb);
            assert!(crate::theme::Theme::contrast(theme.accent, theme.accent_text()) >= 4.5);
        }
    }
}
