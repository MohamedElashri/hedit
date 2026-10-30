//! Standard buttons shared by panels and dialogs, so closing, confirming and
//! cancelling look and behave the same everywhere.
use super::*;

/// Compact identity shared by the About and Update windows.
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
            ui.label(RichText::new(title).size(22.0).strong());
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
            theme.background.color()
        } else {
            theme.muted.color()
        }))
        .fill(if enabled {
            theme.accent.color()
        } else {
            theme.elevated.color()
        })
        .min_size(egui::vec2(72.0, 28.0)),
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
    let side = 22.0;
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close"));
    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
        }
        let cross = rect.shrink(5.0);
        let color = if response.hovered() || response.has_focus() {
            visuals.fg_stroke.color
        } else {
            ui.visuals().weak_text_color()
        };
        if dirty && !response.hovered() && !response.has_focus() {
            ui.painter().circle_filled(rect.center(), 3.0, color);
        } else {
            let stroke = egui::Stroke::new(1.7, color);
            ui.painter()
                .line_segment([cross.left_top(), cross.right_bottom()], stroke);
            ui.painter()
                .line_segment([cross.right_top(), cross.left_bottom()], stroke);
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
                theme.background.color()
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
        assert_eq!(rect.width(), 22.0);
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
        assert_eq!(
            output
                .shapes
                .iter()
                .filter(|s| matches!(&s.shape,
            egui::Shape::LineSegment { points, stroke } if stroke.width == 1.7
                && (points[1].x - points[0].x).abs() == 12.0
                && (points[1].y - points[0].y).abs() == 12.0))
                .count(),
            2
        );
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
