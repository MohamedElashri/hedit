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
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 23.0), egui::Sense::click());
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
            let stroke = egui::Stroke::new(1.2_f32, color);
            if row.directory {
                let chevron = if row.expanded {
                    vec![
                        egui::pos2(left + 1.0, cy - 2.0),
                        egui::pos2(left + 5.0, cy + 2.0),
                        egui::pos2(left + 9.0, cy - 2.0),
                    ]
                } else {
                    vec![
                        egui::pos2(left + 3.0, cy - 4.0),
                        egui::pos2(left + 7.0, cy),
                        egui::pos2(left + 3.0, cy + 4.0),
                    ]
                };
                ui.painter().add(egui::Shape::line(
                    chevron,
                    egui::Stroke::new(1.2_f32, theme.muted.color()),
                ));
                let x = left + 14.0;
                let y = cy - 6.0;
                let fill = color.gamma_multiply(0.22);
                for (origin, size) in [
                    (egui::pos2(x, y), egui::vec2(7.0, 5.0)),
                    (egui::pos2(x, y + 2.0), egui::vec2(15.0, 10.0)),
                ] {
                    ui.painter().rect(
                        egui::Rect::from_min_size(origin, size),
                        1.0,
                        fill,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                if row.expanded {
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(x + 2.0, y + 5.0),
                            egui::pos2(x + 17.0, y + 5.0),
                            egui::pos2(x + 14.0, y + 12.0),
                            egui::pos2(x - 1.0, y + 12.0),
                        ],
                        color.gamma_multiply(0.32),
                        stroke,
                    ));
                }
            } else {
                let x = left + 16.0;
                let y = cy - 7.0;
                ui.painter().add(egui::Shape::closed_line(
                    vec![
                        egui::pos2(x, y),
                        egui::pos2(x + 7.0, y),
                        egui::pos2(x + 11.0, y + 4.0),
                        egui::pos2(x + 11.0, y + 14.0),
                        egui::pos2(x, y + 14.0),
                    ],
                    stroke,
                ));
                ui.painter().add(egui::Shape::line(
                    vec![
                        egui::pos2(x + 7.0, y),
                        egui::pos2(x + 7.0, y + 4.0),
                        egui::pos2(x + 11.0, y + 4.0),
                    ],
                    stroke,
                ));
                for dy in [7.0, 10.0] {
                    ui.painter().line_segment(
                        [egui::pos2(x + 3.0, y + dy), egui::pos2(x + 8.0, y + dy)],
                        stroke,
                    );
                }
            }
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
