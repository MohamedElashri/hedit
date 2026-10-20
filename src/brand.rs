//! The same geometric mark as assets/hedit.svg, drawn without raster assets.
use eframe::egui::{self, Color32};

const STEMS: [[f32; 4]; 3] = [
    [14.0, 14.0, 8.0, 36.0],
    [22.0, 27.0, 13.0, 8.0],
    [33.0, 33.0, 8.0, 17.0],
];
const SHOULDER: [[f32; 2]; 4] = [[33.0, 27.0], [35.0, 27.0], [41.0, 33.0], [33.0, 35.0]];
const LINES: [[f32; 4]; 2] = [[31.0, 14.0, 19.0, 4.0], [38.0, 21.0, 12.0, 4.0]];

pub fn paint(ui: &mut egui::Ui, size: f32, accent: Color32, secondary: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let point = |x: f32, y: f32| rect.min + egui::vec2(x, y) * (size / 64.0);
    for [x, y, w, h] in STEMS {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(point(x, y), point(x + w, y + h)),
            0.0,
            accent,
        );
    }
    ui.painter().add(egui::Shape::convex_polygon(
        SHOULDER.into_iter().map(|[x, y]| point(x, y)).collect(),
        accent,
        egui::Stroke::NONE,
    ));
    for [x, y, w, h] in LINES {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(point(x, y), point(x + w, y + h)),
            0.0,
            secondary,
        );
    }
    response.on_hover_text("hedit · keep your train of thought")
}
fn in_rect(x: f32, y: f32, rect: [f32; 4]) -> bool {
    x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
}
fn foreground(x: f32, y: f32) -> bool {
    STEMS.into_iter().any(|r| in_rect(x, y, r))
        || ((33.0..=41.0).contains(&x)
            && y >= 27.0 + (x - 35.0).max(0.0)
            && y <= 35.0 - (x - 33.0) / 4.0)
}
pub fn icon() -> egui::IconData {
    const SIZE: usize = 128;
    let mut rgba = vec![0; SIZE * SIZE * 4];
    // Supersample the vector geometry for clean small desktop icons.
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut channels = [0u32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let px = (x as f32 + (sx as f32 + 0.5) / 4.0) / 2.0;
                    let py = (y as f32 + (sy as f32 + 0.5) / 4.0) / 2.0;
                    let cx = px.clamp(14.0, 50.0);
                    let cy = py.clamp(14.0, 50.0);
                    let inside = (px - cx).powi(2) + (py - cy).powi(2) <= 14.0f32.powi(2);
                    if inside {
                        let color = if foreground(px, py) {
                            [100, 216, 203, 255]
                        } else if LINES.into_iter().any(|r| in_rect(px, py, r)) {
                            [154, 235, 223, 255]
                        } else {
                            [16, 22, 31, 255]
                        };
                        for i in 0..4 {
                            channels[i] += color[i];
                        }
                    }
                }
            }
            let alpha = channels[3] / 16;
            let at = (y * SIZE + x) * 4;
            if alpha > 0 {
                for i in 0..3 {
                    rgba[at + i] = (channels[i] * 255 / channels[3]) as u8;
                }
            }
            rgba[at + 3] = alpha as u8;
        }
    }
    egui::IconData {
        rgba,
        width: SIZE as u32,
        height: SIZE as u32,
    }
}
