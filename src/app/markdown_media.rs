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
    /// Distance from the image top to the TeX baseline, in logical points.
    pub baseline: Option<f32>,
}

struct Entry {
    result: Option<Result<Arc<Raster>, String>>,
    tiles: Vec<Tile>,
}

struct Tile {
    texture: egui::TextureHandle,
    rect: egui::Rect,
    #[cfg(test)]
    image: egui::ColorImage,
}

#[derive(Default)]
pub(super) struct Media {
    entries: Mutex<HashMap<Key, Arc<Mutex<Entry>>>>,
    working: Arc<std::sync::atomic::AtomicUsize>,
    font_size: std::sync::atomic::AtomicU32,
}

impl Media {
    pub(super) fn set_font_size(&self, size: f32) {
        if self.font_size.swap(size.to_bits(), Ordering::Relaxed) != size.to_bits() {
            self.entries.lock().unwrap().clear();
        }
    }
    pub(super) fn inline_height(&self, ui: &egui::Ui, document: u64, source: &str) -> (f32, f32) {
        let size = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .round()
            .clamp(8.0, 96.0) as u16;
        let key = Key {
            document,
            kind: Kind::InlineMath,
            source: source.to_owned(),
            size,
            color: ui.visuals().text_color().to_array(),
            dark: ui.visuals().dark_mode,
        };
        let entries = self.entries.lock().unwrap();
        entries
            .get(&key)
            .and_then(|entry| {
                let entry = entry.lock().unwrap();
                let raster = entry.result.as_ref()?.as_ref().ok()?;
                let (height, text_baseline, baseline) = inline_geometry(ui, raster);
                Some((height, (baseline - text_baseline).max(0.0).ceil()))
            })
            .unwrap_or((f32::from(size) * 1.6, 0.0))
    }
    pub(super) fn forget(&self, document: u64) {
        self.entries
            .lock()
            .unwrap()
            .retain(|key, _| key.document != document);
    }

    pub(super) fn retain(&self, document: u64, figures: &[Figure]) {
        let keep: std::collections::HashSet<_> = figures
            .iter()
            .map(|figure| (figure.kind, figure.source.as_str()))
            .collect();
        self.entries.lock().unwrap().retain(|key, _| {
            key.document != document || keep.contains(&(key.kind, key.source.as_str()))
        });
    }

    pub(super) fn show(&self, ui: &mut egui::Ui, document: u64, source: &str, kind: Kind) {
        let block = kind != Kind::InlineMath;
        let color = ui.visuals().text_color();
        // Font size and line height are different metrics. Using the latter
        // enlarged equations whenever the reader's line spacing increased.
        let size = egui::TextStyle::Body
            .resolve(ui.style())
            .size
            .round()
            .clamp(8.0, 96.0) as u16;
        let key = Key {
            document,
            kind,
            source: source.to_owned(),
            size,
            color: color.to_array(),
            dark: ui.visuals().dark_mode,
        };
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|old, _| {
            old.document != document || (old.color == key.color && old.dark == key.dark)
        });
        let entry = if let Some(entry) = entries.get(&key) {
            Some(entry.clone())
        } else if self.working.load(Ordering::Relaxed) < 2 {
            let entry = Arc::new(Mutex::new(Entry {
                result: None,
                tiles: vec![],
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
        let wide_inline = !block
            && result
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .is_some_and(|r| r.size.x > ui.max_rect().width());
        let contents = |ui: &mut egui::Ui| match &result {
            Some(Ok(raster)) => {
                let mut entry = entry.as_ref().unwrap().lock().unwrap();
                if entry.tiles.is_empty() {
                    entry.tiles = upload(ui.ctx(), raster, egui::Id::new(&key).value());
                }
                let response = if block {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(raster.size.x.ceil(), raster.size.y.ceil()),
                        egui::Sense::click(),
                    );
                    paint(ui, &entry.tiles, rect.min);
                    response
                } else {
                    inline_image(ui, &entry.tiles, raster)
                };
                response.on_hover_text(source).context_menu(|ui| {
                    if ui.button("Copy source").clicked() {
                        ui.ctx().copy_text(source.to_owned());
                        ui.close_menu();
                    }
                });
            }
            Some(Err(error)) => {
                if block {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        if kind == Kind::Mermaid {
                            "Mermaid could not be rendered"
                        } else {
                            "Math could not be rendered"
                        },
                    )
                    .on_hover_text(error);
                }
                ui.label(
                    RichText::new(source.trim_end_matches('\n'))
                        .monospace()
                        .text_style(egui::TextStyle::Monospace),
                )
                .on_hover_text(error);
            }
            None => {
                if block {
                    ui.set_min_height(56.0);
                    ui.spinner();
                    ui.weak(if kind == Kind::Mermaid {
                        "Rendering diagram…"
                    } else {
                        "Rendering equation…"
                    });
                } else {
                    ui.label(RichText::new(source).monospace());
                }
            }
        };
        if block || wide_inline {
            if wide_inline {
                ui.end_row();
            }
            let width = ui.available_width().max(1.0);
            let body = egui::TextStyle::Body.resolve(ui.style()).size;
            let height = match result.as_ref() {
                Some(Ok(raster)) => raster.size.y.min(480.0),
                Some(Err(_)) => {
                    let code = egui::TextStyle::Monospace.resolve(ui.style()).size;
                    (body * 1.6
                        + code * 1.6 * source.trim_end_matches('\n').lines().count().max(1) as f32
                        + ui.spacing().item_spacing.y)
                        .clamp(56.0, 480.0)
                }
                None => (body * 3.2).max(56.0),
            };
            // Wide equations retain their font size and scroll locally instead
            // of shrinking into illegible text or shifting the whole page.
            ui.allocate_ui_with_layout(
                egui::vec2(width, height + 16.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(width);
                    ui.add_space(8.0);
                    egui::ScrollArea::both()
                        .id_salt(("figure", ui.id(), &key))
                        .auto_shrink([false, true])
                        .max_height(480.0)
                        .show(ui, |ui| {
                            let natural_width = result
                                .as_ref()
                                .and_then(|r| r.as_ref().ok())
                                .map_or(width, |r| r.size.x);
                            ui.horizontal_top(|ui| {
                                ui.add_space(((width - natural_width) / 2.0).max(0.0).round());
                                ui.vertical(|ui| contents(ui));
                            });
                        });
                    ui.add_space(8.0);
                },
            );
            if wide_inline {
                ui.end_row();
                ui.set_row_height(f32::from(size) * 1.6);
            }
        } else {
            contents(ui);
        }
    }

    #[cfg(test)]
    pub(super) fn rendered_count(&self) -> usize {
        self.entries
            .lock()
            .unwrap()
            .values()
            .filter(|entry| !entry.lock().unwrap().tiles.is_empty())
            .count()
    }

    #[cfg(test)]
    pub(super) fn baselines(&self) -> HashMap<egui::TextureId, f32> {
        self.entries
            .lock()
            .unwrap()
            .values()
            .flat_map(|entry| {
                let entry = entry.lock().unwrap();
                let baseline = entry
                    .result
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .and_then(|r| r.baseline);
                entry
                    .tiles
                    .iter()
                    .filter_map(|tile| Some((tile.texture.id(), baseline? - tile.rect.top())))
                    .collect::<Vec<_>>()
            })
            .collect()
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
            .flat_map(|entry| {
                entry
                    .lock()
                    .unwrap()
                    .tiles
                    .iter()
                    .map(|tile| (tile.texture.id(), tile.image.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// Align the TeX baseline with the surrounding reader font's actual baseline.
/// Center-aligned flow reserves enough ascent AND descent, including fractions.
fn inline_image(ui: &mut egui::Ui, tiles: &[Tile], raster: &Raster) -> egui::Response {
    let (height, text_baseline, baseline) = inline_geometry(ui, raster);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(raster.size.x.ceil(), height.ceil()),
        egui::Sense::click(),
    );
    let image = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.top() + text_baseline - baseline),
        raster.size,
    );
    paint(ui, tiles, image.min);
    response
}

/// GPU texture limits apply to each side, independently of the pixel budget.
/// Tile the raster while retaining its logical dimensions and sampling density.
fn upload(ctx: &egui::Context, raster: &Raster, id: u64) -> Vec<Tile> {
    let side = ctx.input(|i| i.max_texture_side).max(1);
    let [width, height] = raster.image.size;
    let mut tiles = vec![];
    for y in (0..height).step_by(side) {
        for x in (0..width).step_by(side) {
            let w = side.min(width - x);
            let h = side.min(height - y);
            let pixels = (y..y + h)
                .flat_map(|row| {
                    raster.image.pixels[row * width + x..row * width + x + w]
                        .iter()
                        .copied()
                })
                .collect();
            let image = egui::ColorImage {
                size: [w, h],
                pixels,
            };
            #[cfg(test)]
            let retained = image.clone();
            let texture = ctx.load_texture(
                format!("markdown-media-{id}-{x}-{y}"),
                image,
                egui::TextureOptions::LINEAR,
            );
            let at = |x: usize, y: usize| {
                egui::pos2(
                    raster.size.x * x as f32 / width as f32,
                    raster.size.y * y as f32 / height as f32,
                )
            };
            tiles.push(Tile {
                texture,
                rect: egui::Rect::from_min_max(at(x, y), at(x + w, y + h)),
                #[cfg(test)]
                image: retained,
            });
        }
    }
    tiles
}

fn paint(ui: &egui::Ui, tiles: &[Tile], origin: egui::Pos2) {
    let uv = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0));
    for tile in tiles {
        ui.painter().image(
            tile.texture.id(),
            tile.rect.translate(origin.to_vec2()),
            uv,
            egui::Color32::WHITE,
        );
    }
}

fn inline_geometry(ui: &egui::Ui, raster: &Raster) -> (f32, f32, f32) {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::simple(
        "x".into(),
        font.clone(),
        ui.visuals().text_color(),
        f32::INFINITY,
    );
    job.sections[0].format.line_height = Some(font.size * 1.6);
    let reference = ui.fonts(|fonts| fonts.layout_job(job));
    let glyph = &reference.rows[0].glyphs[0];
    let offset = glyph.pos.y - reference.size().y / 2.0;
    let baseline = raster.baseline.unwrap_or(raster.size.y * 0.75);
    let height =
        (2.0 * (baseline - offset).max(raster.size.y - baseline + offset)).max(reference.size().y);
    (height.ceil(), glyph.pos.y, baseline)
}

fn render(key: &Key) -> Result<Raster, String> {
    let (svg, baseline) = match key.kind {
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
            let ast = latex_rust::parse(&key.source).map_err(|e| e.to_string())?;
            let tree = latex_rust::layout(
                &ast,
                font,
                if options.display {
                    latex_rust::MathStyle::Display
                } else {
                    latex_rust::MathStyle::Text
                },
            )
            .map_err(|e| e.to_string())?;
            let baseline = f32::from_bits(tree.height.to_ieee32_bits()) * f32::from(key.size);
            (
                latex_rust::render_svg(&tree, font, &options).map_err(|e| e.to_string())?,
                Some(baseline),
            )
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
            (
                mermaid_rs_renderer::render_strict(&key.source, options)
                    .map_err(|e| e.to_string())?,
                None,
            )
        }
    };
    let mut raster = rasterize(&svg)?;
    raster.baseline = baseline;
    Ok(raster)
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
        baseline: None,
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
            (r"\text{energy} = \frac{1}{2}mv^2", Kind::InlineMath),
            (
                r"\begin{bmatrix} a & b \\ c & d \end{bmatrix}",
                Kind::DisplayMath,
            ),
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
