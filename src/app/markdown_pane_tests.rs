//! Native split-divider interaction and clipping, in both pane orientations.
use super::markdown_tests::fixture;
use super::tests::{frame_size, key};
use super::*;

fn divider(output: &egui::FullOutput, size: egui::Vec2) -> [egui::Pos2; 2] {
    output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::LineSegment { points, .. } => {
                let vertical = size.x >= 900.0
                    && (points[0].x - points[1].x).abs() < 1.0
                    && points[1].y - points[0].y > size.y * 0.6
                    && points[0].x > 300.0
                    && points[0].x < size.x - 240.0;
                let horizontal = size.x < 900.0
                    && (points[0].y - points[1].y).abs() < 1.0
                    && points[1].x - points[0].x > size.x - 250.0
                    && points[0].y > size.y * 0.25
                    && points[0].y < size.y * 0.75;
                (vertical || horizontal).then_some(*points)
            }
            _ => None,
        })
        .expect("split divider")
}

fn point(line: [egui::Pos2; 2]) -> egui::Pos2 {
    line[0] + (line[1] - line[0]) * 0.5
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn markdown_split_divider_resizes_with_pointer_and_keyboard_without_editing_source() {
    let source = "# Heading\n\nA paragraph with **bold** emphasis.\n";
    for (size, delta, backward) in [
        (
            egui::vec2(1280.0, 1000.0),
            egui::vec2(100.0, 0.0),
            Key::ArrowLeft,
        ),
        (
            egui::vec2(720.0, 1100.0),
            egui::vec2(0.0, 120.0),
            Key::ArrowUp,
        ),
    ] {
        let (_directory, ctx, mut app) = fixture(source);
        app.execute(&ctx, Command::MarkdownSplit);
        for _ in 0..4 {
            frame_size(&ctx, &mut app, vec![], size);
        }
        let output = frame_size(&ctx, &mut app, vec![], size);
        let before = point(divider(&output, size));
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(before), button(before, true)],
            size,
        );
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(before + delta)],
            size,
        );
        frame_size(&ctx, &mut app, vec![button(before + delta, false)], size);
        let output = frame_size(&ctx, &mut app, vec![], size);
        let after = point(divider(&output, size));
        assert!(
            after.distance(before + delta) <= 2.0,
            "{before:?} -> {after:?}"
        );
        // The preview's text and clipping rectangle stay on their own side.
        let preview = output.shapes.iter().find(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "A paragraph with bold emphasis.")).unwrap();
        if size.x >= 900.0 {
            assert!(preview.clip_rect.left() > after.x);
        } else {
            assert!(preview.clip_rect.top() > after.y);
        }
        // Click the divider to give it keyboard focus; an arrow moves it 1%.
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(after), button(after, true)],
            size,
        );
        frame_size(&ctx, &mut app, vec![button(after, false)], size);
        frame_size(&ctx, &mut app, vec![key(backward, Modifiers::NONE)], size);
        let output = frame_size(&ctx, &mut app, vec![], size);
        let keyboard = point(divider(&output, size));
        let change = if size.x >= 900.0 {
            after.x - keyboard.x
        } else {
            after.y - keyboard.y
        };
        assert!(
            change > 7.0 && change < 12.0,
            "keyboard resize moved {change}px"
        );
        assert_eq!(app.active().page.text, source);
    }
}

#[test]
fn markdown_split_divider_keeps_separate_orientation_sizes_and_resets_on_double_click() {
    let source = "# Heading\n\nA paragraph.\n";
    let (_directory, ctx, mut app) = fixture(source);
    app.execute(&ctx, Command::MarkdownSplit);
    let wide = egui::vec2(1280.0, 1000.0);
    let narrow = egui::vec2(720.0, 1100.0);
    for _ in 0..4 {
        frame_size(&ctx, &mut app, vec![], wide);
    }
    let output = frame_size(&ctx, &mut app, vec![], wide);
    let equal = point(divider(&output, wide));
    let target = equal + egui::vec2(150.0, 0.0);
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(equal), button(equal, true)],
        wide,
    );
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(target)],
        wide,
    );
    frame_size(&ctx, &mut app, vec![button(target, false)], wide);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], narrow);
    }
    let output = frame_size(&ctx, &mut app, vec![], narrow);
    let stacked = point(divider(&output, narrow));
    assert!(stacked.y > narrow.y * 0.45 && stacked.y < narrow.y * 0.6);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], wide);
    }
    let output = frame_size(&ctx, &mut app, vec![], wide);
    assert!(point(divider(&output, wide)).distance(target) < 2.0);
    // Two clicks at the resized divider restore its original equal split.
    for _ in 0..2 {
        frame_size(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(target), button(target, true)],
            wide,
        );
        frame_size(&ctx, &mut app, vec![button(target, false)], wide);
    }
    let output = frame_size(&ctx, &mut app, vec![], wide);
    assert!(point(divider(&output, wide)).distance(equal) < 2.0);
    // Dragging beyond the pane and then shrinking the window keeps both usable.
    frame_size(
        &ctx,
        &mut app,
        vec![egui::Event::PointerMoved(equal), button(equal, true)],
        wide,
    );
    let edge = egui::pos2(wide.x - 1.0, equal.y);
    frame_size(&ctx, &mut app, vec![egui::Event::PointerMoved(edge)], wide);
    frame_size(&ctx, &mut app, vec![button(edge, false)], wide);
    let smaller = egui::vec2(1000.0, 1000.0);
    for _ in 0..3 {
        frame_size(&ctx, &mut app, vec![], smaller);
    }
    let output = frame_size(&ctx, &mut app, vec![], smaller);
    let clamped = point(divider(&output, smaller));
    assert!(smaller.x - clamped.x >= 235.0);
    assert!(clamped.x > 450.0);
    assert_eq!(app.active().page.text, source);
}
