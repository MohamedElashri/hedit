//! Shared workbench typography and geometry, independent of document/terminal fonts.
use eframe::egui::{self, FontId, TextStyle};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Density {
    Compact,
    #[default]
    Default,
    Comfortable,
}

#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub font: f32,
    pub small: f32,
    pub row: f32,
    pub control: f32,
    pub toolbar: f32,
    pub tab: f32,
    pub gap: f32,
    pub padding: f32,
    pub icon: f32,
}
impl Metrics {
    pub fn new(size: f32, density: Density) -> Self {
        let font = if size.is_finite() {
            size.clamp(10.0, 20.0)
        } else {
            13.0
        };
        let base = (font + 9.0).max(22.0);
        let extra = match density {
            Density::Compact => 0.0,
            Density::Default => 2.0,
            Density::Comfortable => 6.0,
        };
        Self {
            font,
            small: (font - 1.0).max(10.0),
            row: base + extra,
            control: base + extra,
            toolbar: base + extra + 8.0,
            tab: base + extra + 8.0,
            gap: 4.0 + extra / 2.0,
            padding: 3.0 + extra / 2.0,
            icon: 16.0,
        }
    }
    pub fn for_ui(ui: &egui::Ui) -> Self {
        let font = ui.style().text_styles[&TextStyle::Body].size;
        let density = ui
            .ctx()
            .data(|d| d.get_temp::<Density>(egui::Id::new("workbench-density")))
            .unwrap_or_default();
        Self::new(font, density)
    }
}

/// Theme changes may replace egui's styles. Reapply our metrics only when needed.
pub fn apply(ctx: &egui::Context, size: f32, density: Density) {
    let id = egui::Id::new("workbench-density");
    if ctx.data(|d| d.get_temp::<Density>(id)) != Some(density) {
        ctx.data_mut(|d| d.insert_temp(id, density));
    }
    let m = Metrics::new(size, density);
    let style = ctx.style();
    if style.text_styles[&TextStyle::Body] == FontId::proportional(m.font)
        && style.text_styles[&TextStyle::Button] == FontId::proportional(m.font)
        && style.text_styles[&TextStyle::Small] == FontId::proportional(m.small)
        && style.spacing.interact_size.y == m.control
        && style.spacing.button_padding == egui::vec2(8.0, m.padding)
        && style.spacing.item_spacing == egui::vec2(8.0, m.gap)
    {
        return;
    }
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(m.font));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(m.font));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(m.small));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(m.font + 5.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(m.font));
        style.spacing.interact_size.y = m.control;
        style.spacing.button_padding = egui::vec2(8.0, m.padding);
        style.spacing.item_spacing = egui::vec2(8.0, m.gap);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn density_and_font_size_expand_targets_without_touching_editor_fonts() {
        let ctx = egui::Context::default();
        for font in [10.0, 13.0, 20.0] {
            let mut last = 0.0;
            for density in [Density::Compact, Density::Default, Density::Comfortable] {
                apply(&ctx, font, density);
                let style = ctx.style();
                let m = Metrics::new(font, density);
                assert!(m.row >= 22.0 && m.row > last);
                assert_eq!(style.text_styles[&TextStyle::Body].size, font);
                assert_eq!(style.spacing.interact_size.y, m.control);
                last = m.row;
            }
        }
    }
    #[test]
    fn invalid_programmatic_sizes_remain_bounded_and_theme_colors_survive() {
        let ctx = egui::Context::default();
        let color = ctx.style().visuals.selection.bg_fill;
        for font in [f32::NAN, f32::INFINITY, -4.0, 100.0] {
            apply(&ctx, font, Density::Default);
            assert!((10.0..=20.0).contains(&ctx.style().text_styles[&TextStyle::Body].size));
            assert_eq!(ctx.style().visuals.selection.bg_fill, color);
        }
    }
}
