//! Native tree rows with small vector icons, independent of installed fonts.
use super::*;

pub(super) struct Row<'a> {
    pub name: &'a str,
    pub path: &'a str,
    pub directory: bool,
    pub expanded: bool,
    pub selected: bool,
    pub depth: usize,
    pub symlink: bool,
}

pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, row: Row<'_>) -> egui::Response {
    ui.push_id(row.path, |ui| {
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width(),
                hedit::ui_style::Metrics::for_ui(ui).row,
            ),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                row.selected,
                row.name,
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
            use hedit::icons::Icon;
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
            let icon = if row.directory {
                if row.expanded {
                    Icon::FolderOpen
                } else {
                    Icon::Folder
                }
            } else {
                Icon::File
            };
            icon.paint(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(left + 14.0, cy - 8.0),
                    egui::vec2(16.0, 16.0),
                ),
                if row.selected {
                    theme.text.color()
                } else {
                    color
                },
            );
            let name = hedit::bidi::display(row.name);
            let name = if row.symlink {
                format!("{name} ↗")
            } else {
                name.into_owned()
            };
            let galley = egui::WidgetText::from(name).into_galley(
                ui,
                Some(egui::TextWrapMode::Truncate),
                (rect.right() - left - 38.0).max(0.0),
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
        response.on_hover_text(row.path)
    })
    .inner
}
