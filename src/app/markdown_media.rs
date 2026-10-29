//! Cached native equation and Mermaid rendering, performed off the UI thread.
use super::*;
use std::{collections::HashMap, sync::Mutex};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(super) enum Kind {
    InlineMath,
    DisplayMath,
    Mermaid,
}

#[derive(Clone)]
pub(super) struct Figure {
    pub kind: Kind,
    pub source: String,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Key {
    document: u64,
    kind: Kind,
    source: String,
    size: u16,
    color: [u8; 4],
    dark: bool,
}

pub(super) struct Raster {
    pub image: egui::ColorImage,
    pub size: egui::Vec2,
}

struct Entry {
    result: Option<Result<Arc<Raster>, String>>,
    texture: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub(super) struct Media {
    entries: Mutex<HashMap<Key, Arc<Mutex<Entry>>>>,
    working: Arc<std::sync::atomic::AtomicUsize>,
}

impl Media {
    pub(super) fn forget(&self, document: u64) {
        self.entries
            .lock()
            .unwrap()
            .retain(|key, _| key.document != document);
    }

    pub(super) fn show(&self, ui: &mut egui::Ui, document: u64, source: &str, kind: Kind) {
        let block = kind != Kind::InlineMath;
        if block {
            ui.label("\n");
        }
        let color = ui.visuals().text_color();
        let size = ui.text_style_height(&egui::TextStyle::Body).round() as u16;
        let key = Key {
            document,
            kind,
            source: source.to_owned(),
            size,
            color: color.to_array(),
            dark: ui.visuals().dark_mode,
        };
        let mut entries = self.entries.lock().unwrap();
        let entry = if let Some(entry) = entries.get(&key) {
            Some(entry.clone())
        } else if self.working.load(Ordering::Relaxed) < 2 {
            let entry = Arc::new(Mutex::new(Entry {
                result: None,
                texture: None,
            }));
            entries.insert(key.clone(), entry.clone());
            self.working.fetch_add(1, Ordering::Relaxed);
            let output = entry.clone();
            let working = self.working.clone();
            let ctx = ui.ctx().clone();
            let request = key.clone();
            thread::spawn(move || {
                let result = std::panic::catch_unwind(|| render(&request))
                    .unwrap_or_else(|_| Err("The renderer could not process this input.".into()));
                output.lock().unwrap().result = Some(result.map(Arc::new));
                working.fetch_sub(1, Ordering::Relaxed);
                ctx.request_repaint();
            });
            Some(entry)
        } else {
            None
        };
        drop(entries);
        let result = entry
            .as_ref()
            .and_then(|entry| entry.lock().unwrap().result.clone());
        match result {
            Some(Ok(raster)) => {
                let mut entry = entry.as_ref().unwrap().lock().unwrap();
                let texture = entry.texture.get_or_insert_with(|| {
                    ui.ctx().load_texture(
                        format!("markdown-media-{}", egui::Id::new(&key).value()),
                        raster.image.clone(),
                        egui::TextureOptions::LINEAR,
                    )
                });
                ui.add(
                    egui::Image::from_texture((texture.id(), raster.size))
                        .max_width(ui.available_width().max(1.0)),
                );
            }
            Some(Err(error)) => {
                let label = match kind {
                    Kind::Mermaid => "Mermaid",
                    _ => "Math",
                };
                if block {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!("{label} could not be rendered"),
                    )
                    .on_hover_text(&error);
                    ui.label("\n");
                }
                ui.monospace(source).on_hover_text(error);
            }
            None => {
                if block {
                    ui.spinner();
                    ui.weak(if kind == Kind::Mermaid {
                        "Rendering diagram…"
                    } else {
                        "Rendering equation…"
                    });
                } else {
                    ui.weak(source);
                }
            }
        }
        if block {
            ui.label("\n");
        }
    }

    #[cfg(test)]
    pub(super) fn pending(&self) -> bool {
        self.working.load(Ordering::Relaxed) > 0
    }

    #[cfg(test)]
    pub(super) fn textures(&self) -> HashMap<egui::TextureId, egui::ColorImage> {
        self.entries
            .lock()
            .unwrap()
            .values()
            .filter_map(|entry| {
                let entry = entry.lock().unwrap();
                match (&entry.texture, &entry.result) {
                    (Some(texture), Some(Ok(raster))) => Some((texture.id(), raster.image.clone())),
                    _ => None,
                }
            })
            .collect()
    }
}

fn render(key: &Key) -> Result<Raster, String> {
    let svg = match key.kind {
        Kind::InlineMath | Kind::DisplayMath => {
            if key.source.len() > 16 * 1024 {
                return Err("The equation exceeds 16 KiB.".into());
            }
            static FONT: std::sync::OnceLock<Result<latex_rust::MathFont, String>> =
                std::sync::OnceLock::new();
            let font = FONT
                .get_or_init(|| latex_rust::MathFont::stix_two_math().map_err(|e| e.to_string()))
                .as_ref()
                .map_err(Clone::clone)?;
            let options = latex_rust::SvgOptions {
                font_size_pt: latex_rust::Dim::ratio(i64::from(key.size) * 3, 4),
                color: latex_rust::Color::rgb(key.color[0], key.color[1], key.color[2]),
                display: key.kind == Kind::DisplayMath,
            };
            latex_rust::latex_to_svg(&key.source, font, &options).map_err(|e| e.to_string())?
        }
        Kind::Mermaid => {
            if key.source.len() > 64 * 1024 || key.source.lines().count() > 1000 {
                return Err("The diagram exceeds 64 KiB or 1,000 lines.".into());
            }
            let parsed = mermaid_rs_renderer::parse_mermaid_strict(&key.source)
                .map_err(|e| e.to_string())?;
            if parsed.graph.nodes.len() > 256 || parsed.graph.edges.len() > 1024 {
                return Err("The diagram exceeds 256 nodes or 1,024 edges.".into());
            }
            let mut options = mermaid_rs_renderer::RenderOptions::default();
            if key.dark {
                options.theme = mermaid_rs_renderer::Theme::dark();
            }
            options.theme.font_size = f32::from(key.size);
            mermaid_rs_renderer::render_strict(&key.source, options).map_err(|e| e.to_string())?
        }
    };
    rasterize(&svg)
}

fn rasterize(svg: &str) -> Result<Raster, String> {
    use resvg::{tiny_skia, usvg};
    static FONTS: std::sync::OnceLock<Arc<usvg::fontdb::Database>> = std::sync::OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        Arc::new(fonts)
    });
    let options = usvg::Options {
        fontdb: fonts.clone(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..Default::default()
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_data(svg.as_bytes(), &options).map_err(|e| e.to_string())?;
    let natural = tree.size();
    let size = egui::vec2(natural.width(), natural.height());
    // Render at up to 2x density, while bounding memory for wide/tall diagrams.
    let scale = 2.0f32.min((4_000_000.0 / (size.x * size.y)).sqrt());
    if !scale.is_finite() || scale <= 0.0 {
        return Err("Invalid figure dimensions.".into());
    }
    let width = (size.x * scale).ceil().max(1.0) as u32;
    let height = (size.y * scale).ceil().max(1.0) as u32;
    let mut pixels = tiny_skia::Pixmap::new(width, height).ok_or("The figure is too large.")?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixels.as_mut(),
    );
    Ok(Raster {
        image: egui::ColorImage::from_rgba_premultiplied(
            [width as usize, height as usize],
            pixels.data(),
        ),
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(source: &str, kind: Kind) -> Key {
        Key {
            document: 1,
            source: source.into(),
            kind,
            size: 18,
            color: [20, 30, 40, 255],
            dark: false,
        }
    }
    #[test]
    fn native_math_and_mermaid_produce_pixels_and_reject_bad_input() {
        for (source, kind) in [
            (r"\frac{1}{2} + x^2", Kind::DisplayMath),
            ("flowchart LR\n A[Edit] --> B[Preview]", Kind::Mermaid),
            ("sequenceDiagram\n Alice->>Bob: Hello", Kind::Mermaid),
        ] {
            let raster = render(&key(source, kind)).unwrap();
            assert!(raster.size.x > 1.0 && raster.size.y > 1.0);
            assert!(raster.image.pixels.iter().any(|color| color.a() > 0));
        }
        assert!(render(&key(r"\frac{", Kind::DisplayMath)).is_err());
        assert!(render(&key("not a diagram", Kind::Mermaid)).is_err());
    }
}
