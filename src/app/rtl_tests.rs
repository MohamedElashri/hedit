//! Right-to-left text in the editor: logical text, visual caret and clicks.
use super::tests::{click_at, frame, key};
use super::*;

const SIZE: egui::Vec2 = egui::vec2(1280.0, 820.0);

fn app_with(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = text.into();
    workspace.edit(0, "").unwrap();
    let ctx = egui::Context::default();
    settings_store::fallback_fonts();
    settings_store::set_editor_font(&ctx, None);
    let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
    app.focus_editor = true;
    for _ in 0..3 {
        frame(&ctx, &mut app, vec![]);
    }
    (directory, ctx, app)
}

/// Where the editor drew its text, and the galley it drew.
fn drawn(output: &egui::FullOutput, text: &str) -> (egui::Pos2, Arc<egui::Galley>) {
    output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some((t.pos, t.galley.clone())),
            _ => None,
        })
        .expect("editor text is drawn")
}

#[test]
fn arabic_typed_into_the_editor_stays_in_logical_order() {
    let (_directory, ctx, mut app) = app_with("");
    for c in "سلام world".chars() {
        frame(&ctx, &mut app, vec![egui::Event::Text(c.into())]);
    }
    assert_eq!(app.active().page.text, "سلام world");
    assert_eq!(app.active().view.head, 10);
}

#[test]
fn clicks_and_arrows_follow_the_text_on_screen() {
    let text = "مرحبا";
    let (_directory, ctx, mut app) = app_with(text);
    let output = frame(&ctx, &mut app, vec![]);
    let (pos, galley) = drawn(&output, text);
    let row = &galley.rows[0];
    // The first letter is drawn rightmost; clicking at its right edge puts the
    // caret before it (index 0), and at the line's left edge after the last.
    let first = &row.glyphs[0];
    assert!(first.pos.x > row.glyphs[4].pos.x);
    let y = pos.y + row.rect.center().y;
    let right_edge = egui::pos2(pos.x + first.pos.x + first.advance_width - 1.0, y);
    click_at(&ctx, &mut app, right_edge, SIZE);
    assert_eq!(app.active().view.head, 0);
    // Left moves left on screen: further into the word, so forward in the text.
    frame(&ctx, &mut app, vec![key(Key::ArrowLeft, Modifiers::NONE)]);
    assert_eq!(app.active().view.head, 1);
    frame(&ctx, &mut app, vec![key(Key::ArrowRight, Modifiers::NONE)]);
    assert_eq!(app.active().view.head, 0);
    // Shift+Left selects the first letter.
    frame(&ctx, &mut app, vec![key(Key::ArrowLeft, Modifiers::SHIFT)]);
    assert_eq!((app.active().view.anchor, app.active().view.head), (0, 1));

    // Clicking at the line's left edge puts the caret after the last letter.
    let (_directory, ctx, mut app) = app_with(text);
    let left_edge = egui::pos2(pos.x + row.glyphs[4].pos.x + 1.0, y);
    click_at(&ctx, &mut app, left_edge, SIZE);
    assert_eq!(app.active().view.head, 5);
}

/// Draw a whole frame in software, as the GPU would: tessellate the shapes and
/// fill each textured triangle. `size` is in points and `scale` is pixels per
/// point, which must match the context's. Writes a binary PPM.
pub(super) fn rasterize(
    ctx: &egui::Context,
    output: egui::FullOutput,
    size: egui::Vec2,
    scale: f32,
) -> Vec<u8> {
    let size = size * scale;
    let (width, height) = (size.x as usize, size.y as usize);
    let font = ctx.fonts(|f| f.image());
    let primitives = ctx.tessellate(output.shapes, scale);
    let mut canvas = vec![[0f32; 3]; width * height];
    for primitive in primitives {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
            continue;
        };
        let clip = egui::Rect::from_min_max(
            primitive.clip_rect.min * scale,
            primitive.clip_rect.max * scale,
        );
        for tri in mesh.indices.chunks(3) {
            let v = [0, 1, 2].map(|k| {
                let mut vertex = mesh.vertices[tri[k] as usize];
                vertex.pos = (vertex.pos.to_vec2() * scale).to_pos2();
                vertex
            });
            let (min, max) = (
                v.iter()
                    .fold(egui::pos2(f32::MAX, f32::MAX), |m, v| m.min(v.pos)),
                v.iter()
                    .fold(egui::pos2(f32::MIN, f32::MIN), |m, v| m.max(v.pos)),
            );
            let area = (v[1].pos - v[0].pos).x * (v[2].pos - v[0].pos).y
                - (v[1].pos - v[0].pos).y * (v[2].pos - v[0].pos).x;
            if area.abs() < 1e-6 {
                continue;
            }
            let (x0, x1) = (
                min.x.max(clip.min.x).max(0.0) as usize,
                max.x.min(clip.max.x).min(size.x) as usize,
            );
            let (y0, y1) = (
                min.y.max(clip.min.y).max(0.0) as usize,
                max.y.min(clip.max.y).min(size.y) as usize,
            );
            for y in y0..y1 {
                for x in x0..x1 {
                    let p = egui::pos2(x as f32 + 0.5, y as f32 + 0.5);
                    let w = |a: egui::Pos2, b: egui::Pos2| {
                        (b - a).x * (p - a).y - (b - a).y * (p - a).x
                    };
                    let (w0, w1, w2) = (
                        w(v[1].pos, v[2].pos) / area,
                        w(v[2].pos, v[0].pos) / area,
                        w(v[0].pos, v[1].pos) / area,
                    );
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let mix = |f: &dyn Fn(&egui::epaint::Vertex) -> f32| {
                        w0 * f(&v[0]) + w1 * f(&v[1]) + w2 * f(&v[2])
                    };
                    let color: [f32; 4] =
                        [0, 1, 2, 3].map(|c| mix(&|v| v.color.to_array()[c] as f32 / 255.0));
                    let coverage = if mesh.texture_id == egui::TextureId::default() {
                        let (u, t) = (mix(&|v| v.uv.x), mix(&|v| v.uv.y));
                        let (u, t) = (
                            (u * font.size[0] as f32) as usize,
                            (t * font.size[1] as f32) as usize,
                        );
                        font.pixels
                            [t.min(font.size[1] - 1) * font.size[0] + u.min(font.size[0] - 1)]
                    } else {
                        1.0
                    };
                    let pixel = &mut canvas[y * width + x];
                    let alpha = color[3] * coverage;
                    for c in 0..3 {
                        pixel[c] = color[c] * coverage + pixel[c] * (1.0 - alpha);
                    }
                }
            }
        }
    }
    let mut ppm = format!("P6 {width} {height} 255\n").into_bytes();
    for pixel in canvas {
        ppm.extend(pixel.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8));
    }
    ppm
}

#[test]
#[ignore = "writes an editor screenshot with RTL text to HEDIT_PREVIEW_DIR"]
fn export_rtl_editor() {
    let output_dir = std::path::PathBuf::from(
        std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"),
    );
    let text = "// تعليق بالعربية: comment\nlet name = \"مرحبا\"; // greeting\n\nسلام عليكم ورحمة الله وبركاته\nالعدد 42 والنص English في السطر.\nfn main() {}\n";
    for (name, language) in [
        ("text", hedit::syntax::Language::PlainText),
        ("code", hedit::syntax::Language::Rust),
    ] {
        let (_directory, ctx, mut app) = app_with(text);
        app.set_syntax(Some(language));
        app.settings.caret_blink = false;
        // Select «م ورحمة الل» in line 4, with an extra caret in line 5.
        let view = &mut app.workspace.open[app.workspace.active].view;
        (view.anchor, view.head) = (69, 80);
        view.selections = vec![hedit::advanced::Selection::caret(96)];
        let size = egui::vec2(900.0, 360.0);
        let mut output = None;
        for _ in 0..3 {
            output = Some(super::tests::frame_size(&ctx, &mut app, vec![], size));
        }
        let ppm = rasterize(&ctx, output.unwrap(), size, 1.0);
        std::fs::write(output_dir.join(format!("rtl-editor-{name}.ppm")), ppm).unwrap();
    }
}

#[test]
fn input_method_commits_without_composing_all_reach_the_editor() {
    // GNOME's Arabic layout on Wayland: one commit per letter, no preedit.
    let (_directory, ctx, mut app) = app_with("");
    for c in "سلام".chars() {
        frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::Ime(egui::ImeEvent::Commit(c.into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
        );
    }
    assert_eq!(app.active().page.text, "سلام");
    // A composing input method (Chinese, Japanese) still works as before.
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("ni".into())),
        ],
    );
    frame(
        &ctx,
        &mut app,
        vec![
            egui::Event::Ime(egui::ImeEvent::Commit("你".into())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ],
    );
    assert_eq!(app.active().page.text, "سلام你");
}
