//! Standard buttons shared by panels and dialogs, so closing, confirming and
//! cancelling look and behave the same everywhere.
use super::*;

/// A font-independent split-pane icon that remains available when the sidebar
/// is hidden. Use a regular button for keyboard activation and focus feedback.
pub(super) fn sidebar_button(
    ui: &mut egui::Ui,
    _theme: &Theme,
    shown: bool,
    shortcut: &str,
) -> egui::Response {
    use hedit::icons::{self, Icon};
    let label = if shown {
        "Hide sidebar"
    } else {
        "Show sidebar"
    };
    icons::toggle(
        ui,
        if shown {
            Icon::Sidebar
        } else {
            Icon::SidebarOff
        },
        label,
        shown,
    )
    .on_hover_text(format!("{label} · {shortcut}"))
}

/// The same workspace selector in Files and Source Control. Labels stay visible
/// in a narrow sidebar, and the entire segment is a keyboard-accessible target.
pub(super) fn workspace_switcher(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: Option<bool>,
    shortcut: &str,
) -> Option<bool> {
    use hedit::icons::Icon;
    let m = hedit::ui_style::Metrics::for_ui(ui);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), m.toolbar),
        egui::Sense::hover(),
    );
    let width = |text: &str| {
        ui.fonts(|f| {
            f.layout_no_wrap(
                text.into(),
                FontId::proportional(m.font),
                theme.text.color(),
            )
            .size()
            .x
        })
    };
    let file_width = width("Files");
    let source_label = if rect.width() >= file_width + width("Source Control") + 32.0 {
        "Source Control"
    } else {
        "Git"
    };
    let show_icons = rect.width() >= file_width + width(source_label) + 32.0 + 44.0;
    let first_width =
        (file_width + 16.0 + if show_icons { 22.0 } else { 0.0 }).min(rect.width() * 0.6);
    let split = rect.left() + first_width;
    let mut changed = None;
    for (source, label, icon, part) in [
        (
            false,
            "Files",
            Icon::Files,
            egui::Rect::from_min_max(rect.min, egui::pos2(split, rect.bottom())),
        ),
        (
            true,
            source_label,
            Icon::SourceControl,
            egui::Rect::from_min_max(egui::pos2(split, rect.top()), rect.max),
        ),
    ] {
        let response = ui.interact(
            part,
            ui.id().with(("workspace-mode", source)),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected == Some(source),
                label,
            )
        });
        let active = selected == Some(source);
        if response.hovered() {
            ui.painter().rect_filled(part, 0.0, theme.hover().color());
        }
        if active {
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(part.left() + 6.0, part.bottom() - 2.0),
                    egui::pos2(part.right() - 6.0, part.bottom()),
                ),
                0.0,
                theme.accent.color(),
            );
        }
        if response.has_focus() {
            ui.painter().rect_stroke(
                part.shrink(1.0),
                2.0,
                egui::Stroke::new(1.0_f32, theme.accent.color()),
                egui::StrokeKind::Inside,
            );
        }
        let color = if active {
            theme.text.color()
        } else {
            theme.muted.color()
        };
        let show_icon = show_icons;
        let left = part.left() + 8.0;
        if show_icon {
            icon.paint(
                ui,
                egui::Rect::from_center_size(
                    egui::pos2(left + 8.0, part.center().y),
                    egui::vec2(16.0, 16.0),
                ),
                color,
            );
        }
        let text_left = left + if show_icon { 22.0 } else { 0.0 };
        let galley = egui::WidgetText::from(RichText::new(label).size(m.font)).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            (part.right() - text_left - 6.0).max(0.0),
            egui::TextStyle::Button,
        );
        ui.painter()
            .with_clip_rect(part.intersect(ui.clip_rect()))
            .galley(
                egui::pos2(text_left, part.center().y - galley.size().y / 2.0),
                galley,
                color,
            );
        if response.clicked() {
            changed = Some(source);
        }
        response.on_hover_text(format!("{label} · {shortcut}"));
    }
    changed
}

/// A shared panel heading and explanation.
pub(super) fn info_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    subtitle: &str,
    loading: bool,
) {
    ui.horizontal(|ui| {
        brand::paint(ui, 48.0, theme.accent.color(), theme.muted.color());
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .size(hedit::ui_style::Metrics::for_ui(ui).font + 5.0)
                    .strong(),
            );
            ui.label(RichText::new(subtitle).small().color(theme.muted.color()));
        });
        if loading {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spinner();
            });
        }
    });
}

pub(super) fn primary_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    label: &str,
    enabled: bool,
) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).color(if enabled {
            theme.accent_text().color()
        } else {
            theme.muted.color()
        }))
        .fill(if enabled {
            theme.accent.color()
        } else {
            theme.elevated.color()
        })
        .min_size(egui::vec2(
            72.0,
            hedit::ui_style::Metrics::for_ui(ui).control,
        )),
    )
}

/// The close button for panels, bars and banners: an × drawn like the one in
/// window title bars, highlighted on hover.
pub(super) fn close_button(ui: &mut egui::Ui, hint: &str) -> egui::Response {
    close_control(ui, hint, false)
}

/// Keep the dirty marker and close icon in the same hit target, without moving
/// the title when hovering an unsaved tab.
pub(super) fn tab_close_button(ui: &mut egui::Ui, dirty: bool) -> egui::Response {
    close_control(ui, "Close tab", dirty)
}

fn close_control(ui: &mut egui::Ui, hint: &str, dirty: bool) -> egui::Response {
    let side = hedit::ui_style::Metrics::for_ui(ui).control;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), hint));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
        }
        let cross = egui::Rect::from_center_size(rect.center(), egui::vec2(16.0, 16.0));
        let color = if response.hovered() || response.has_focus() {
            visuals.fg_stroke.color
        } else {
            ui.visuals().weak_text_color()
        };
        if dirty && !response.hovered() && !response.has_focus() {
            ui.painter().circle_filled(rect.center(), 3.0, color);
        } else {
            hedit::icons::Icon::Close.paint(ui, cross, color);
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::Default)
        .on_hover_text(hint)
}

/// What the buttons at the foot of a dialog asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Choice {
    #[default]
    None,
    Confirm,
    Cancel,
}

/// The row at the foot of a dialog, aligned right: Cancel, then the default
/// action, highlighted, last, as on macOS and GNOME. Pressing Enter in the
/// dialog's field (`enter`) confirms too, when the action is enabled.
pub(super) fn dialog_buttons(
    ui: &mut egui::Ui,
    theme: &Theme,
    confirm: &str,
    enabled: bool,
    enter: bool,
) -> Choice {
    let mut choice = Choice::None;
    ui.add_space(4.0);
    // Inside a horizontal strip, so the right-aligned row keeps one line's height.
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let primary = egui::Button::new(RichText::new(confirm).color(if enabled {
                theme.accent_text().color()
            } else {
                theme.muted.color()
            }))
            .fill(if enabled {
                theme.accent.color()
            } else {
                theme.elevated.color()
            })
            .min_size(egui::vec2(72.0, 0.0));
            if ui
                .add_enabled(enabled, primary)
                .on_hover_text("Enter")
                .clicked()
                || (enabled && enter)
            {
                choice = Choice::Confirm;
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(72.0, 0.0)))
                .on_hover_text("Esc")
                .clicked()
            {
                choice = Choice::Cancel;
            }
        })
    });
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        mut add: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
            events,
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| add(ui));
        })
    }

    #[test]
    fn close_button_is_a_small_square_that_clicks() {
        let ctx = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        run(&ctx, vec![], |ui| rect = close_button(ui, "Close").rect);
        assert_eq!(rect.width(), rect.height());
        assert!(
            rect.width() >= 22.0,
            "close targets must stay large enough to hit"
        );
        let pos = rect.center();
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut clicked = false;
        for events in [
            vec![egui::Event::PointerMoved(pos), button(true)],
            vec![button(false)],
        ] {
            run(&ctx, events, |ui| {
                clicked |= close_button(ui, "Close").clicked()
            });
        }
        assert!(clicked);
    }

    #[test]
    fn dirty_tab_marker_becomes_a_large_cross_on_hover_without_resizing() {
        let ctx = egui::Context::default();
        let mut rect = egui::Rect::NOTHING;
        let output = run(&ctx, vec![], |ui| {
            rect = tab_close_button(ui, true).rect;
        });
        let original = rect;
        assert!(
            output.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Circle(circle) if circle.center == rect.center() && circle.radius == 3.0))
        );
        let output = run(&ctx, vec![egui::Event::PointerMoved(rect.center())], |ui| {
            let response = tab_close_button(ui, true);
            assert_eq!(response.rect, original);
            assert!(response.hovered());
        });
        assert!(!output.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Circle(circle) if circle.center == rect.center())));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape,
            egui::Shape::Mesh(mesh) if hedit::icons::Icon::Close.matches_texture(&ctx,mesh.texture_id)
                && original.contains_rect(mesh.calc_bounds()))));
    }

    #[test]
    fn enter_confirms_only_an_enabled_action() {
        let ctx = egui::Context::default();
        let theme = hedit::theme::builtins()[0].clone();
        for (enabled, expected) in [(false, Choice::None), (true, Choice::Confirm)] {
            let mut choice = Choice::None;
            run(&ctx, vec![], |ui| {
                choice = dialog_buttons(ui, &theme, "Create", enabled, true);
            });
            assert_eq!(choice, expected);
        }
    }
}
