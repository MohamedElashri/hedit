//! Native tree rows with small vector icons, independent of installed fonts.
use super::*;
use hedit::icons::{FileIcon, FolderIcon, Icon, FILE_ICON_SIZE};

pub(super) struct Row<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub icon_name: &'a str,
    pub directory: bool,
    pub expanded: bool,
    pub selected: bool,
    pub depth: usize,
    pub symlink: bool,
    pub badge: Option<(&'a str, egui::Color32)>,
    pub badge_hint: Option<&'a str>,
    pub roving: bool,
}

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, row: Row<'_>) -> egui::Response {
    ui.push_id(row.path, |ui| {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                hedit::ui_style::Metrics::for_ui(ui).row,
            ),
            if row.roving {
                egui::Sense::click().difference(egui::Sense::focusable_noninteractive())
            } else {
                egui::Sense::click()
            },
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                row.selected,
                row.badge_hint.map_or_else(
                    || row.name.to_owned(),
                    |hint| format!("{} · {hint}", row.name),
                ),
            )
        });
        if ui.is_rect_visible(rect) {
            let visuals = ui.style().interact_selectable(&response, row.selected);
            if row.selected || response.hovered() || response.has_focus() {
                ui.painter().rect_filled(rect, 0.0, visuals.weak_bg_fill);
            }
            let left = rect.left() + row.depth.min(32) as f32 * 12.0;
            let cy = rect.center().y;
            let color = theme.muted.color();
            if row.directory {
                (if row.expanded {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                })
                .paint(
                    ui,
                    egui::Rect::from_min_size(egui::pos2(left, cy - 6.0), egui::vec2(12.0, 12.0)),
                    color,
                );
            }
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(left + 22.0, cy),
                egui::Vec2::splat(FILE_ICON_SIZE),
            );
            let icon_color = if row.selected {
                theme.text.color()
            } else {
                color
            };
            if row.directory {
                FolderIcon::for_name(row.icon_name).paint(ui, icon_rect, row.expanded, icon_color);
            } else {
                FileIcon::for_name(row.icon_name).paint(ui, icon_rect, icon_color);
            }
            let name = hedit::bidi::display(row.name);
            let name = if row.symlink {
                format!("{name} ↗")
            } else {
                name.into_owned()
            };
            let mut badge_width = 0.0;
            if let Some((label, color)) = row.badge {
                let badge = egui::WidgetText::from(RichText::new(label).color(color)).into_galley(
                    ui,
                    Some(egui::TextWrapMode::Truncate),
                    50.0,
                    egui::TextStyle::Small,
                );
                badge_width = badge.size().x + 8.0;
                ui.painter()
                    .with_clip_rect(rect.intersect(ui.clip_rect()))
                    .galley(
                        egui::pos2(
                            rect.right() - badge.size().x - 4.0,
                            cy - badge.size().y / 2.0,
                        ),
                        badge,
                        color,
                    );
            }
            let galley = egui::WidgetText::from(name).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                (rect.right() - left - 38.0 - badge_width).max(0.0),
                egui::TextStyle::Button,
            );
            ui.painter()
                .with_clip_rect(rect.intersect(ui.clip_rect()))
                .galley(
                    egui::pos2(left + 37.0, cy - galley.size().y / 2.0),
                    galley,
                    theme.text.color(),
                );
        }
        response.on_hover_text(row.badge_hint.map_or_else(
            || row.path.to_owned(),
            |hint| format!("{}\n{hint}", row.path),
        ))
    })
    .inner
}
