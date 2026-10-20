//! Test-only SVG layout export for visual review without a display server.
use eframe::egui::{
    self,
    epaint::{ColorMode, Shape},
    Color32,
};
use std::fmt::Write;

fn color(color: Color32) -> String {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    format!("rgba({r},{g},{b},{:.3})", a as f32 / 255.0)
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn shape(svg: &mut String, s: &Shape) {
    match s {
        Shape::Vec(shapes) => {
            for s in shapes {
                shape(svg, s);
            }
        }
        Shape::Rect(r) => {
            let _=write!(svg,"<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",r.rect.left(),r.rect.top(),r.rect.width(),r.rect.height(),r.corner_radius.nw,color(r.fill),color(r.stroke.color),r.stroke.width);
        }
        Shape::Circle(c) => {
            let _=write!(svg,"<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",c.center.x,c.center.y,c.radius,color(c.fill),color(c.stroke.color),c.stroke.width);
        }
        Shape::LineSegment { points, stroke } => {
            let _ = write!(
                svg,
                "<path d=\"M{} {}L{} {}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"/>",
                points[0].x,
                points[0].y,
                points[1].x,
                points[1].y,
                color(stroke.color),
                stroke.width
            );
        }
        Shape::Path(p) => {
            let points = p
                .points
                .iter()
                .map(|p| format!("{},{}", p.x, p.y))
                .collect::<Vec<_>>()
                .join(" ");
            let stroke = match &p.stroke.color {
                ColorMode::Solid(c) => *c,
                _ => Color32::TRANSPARENT,
            };
            let tag = if p.closed { "polygon" } else { "polyline" };
            let _ = write!(
                svg,
                "<{tag} points=\"{points}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"/>",
                color(p.fill),
                color(stroke),
                p.stroke.width
            );
        }
        Shape::Text(t) => {
            for row in &t.galley.rows {
                for glyph in &row.glyphs {
                    let section = &t.galley.job.sections[glyph.section_index as usize];
                    let c = t.override_text_color.unwrap_or(
                        if section.format.color == Color32::PLACEHOLDER {
                            t.fallback_color
                        } else {
                            section.format.color
                        },
                    );
                    let family = if section.format.font_id.family == egui::FontFamily::Monospace {
                        "DejaVu Sans Mono"
                    } else {
                        "DejaVu Sans"
                    };
                    let _=write!(svg,"<text x=\"{}\" y=\"{}\" fill=\"{}\" font-family=\"{}\" font-size=\"{}\">{}</text>",t.pos.x+glyph.pos.x,t.pos.y+glyph.pos.y,color(c),family,section.format.font_id.size,escape(&glyph.chr.to_string()));
                }
            }
        }
        _ => (),
    }
}
pub fn export(
    shapes: &[egui::epaint::ClippedShape],
    size: [f32; 2],
    background: Color32,
) -> String {
    let mut svg=format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"{}\"/>",size[0],size[1],size[0],size[1],color(background));
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
    for (index, clipped) in shapes.iter().enumerate() {
        let rect = clipped.clip_rect.intersect(screen);
        if !rect.is_positive() {
            continue;
        }
        let _=write!(svg,"<clipPath id=\"c{index}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath><g clip-path=\"url(#c{index})\">",rect.left(),rect.top(),rect.width(),rect.height());
        shape(&mut svg, &clipped.shape);
        svg.push_str("</g>");
    }
    svg.push_str("</svg>");
    svg
}
