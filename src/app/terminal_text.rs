//! Display-only terminal BiDi layout and OpenType shaping. Ghostty keeps the
//! original UTF-8 and logical cell coordinates for PTY operations and copying.
use super::*;
use ab_glyph::{Font, FontRef, Glyph, GlyphId, PxScale};
use hedit::{
    bidi,
    terminal::{Cell, Grid},
};
use std::{
    collections::VecDeque,
    hash::{Hash, Hasher},
};

const ARABIC: &[u8] = include_bytes!("../../assets/fonts/NotoSansArabic.ttf");
const CACHE_PIXELS: usize = 4_000_000;

pub(super) struct Run {
    pub start: usize,
    pub width: usize,
    pub text: String,
    clusters: Vec<(char, u32)>,
    pre: String,
    post: String,
}

pub(super) struct Row {
    pub visual_to_logical: Vec<usize>,
    pub logical_to_visual: Vec<usize>,
    rtl: Vec<bool>,
    pub runs: Vec<Run>,
}

impl Row {
    pub fn cursor(&self, cells: &[Cell], column: usize) -> (f32, bool) {
        if column >= cells.len() {
            return (column as f32, false);
        }
        // A shell's insertion cursor after an RTL word belongs at the left
        // edge of the word, even though its next logical cell is blank.
        if cells[column].text.is_empty() && column > 0 && self.rtl[column - 1] {
            return (self.logical_to_visual[column - 1] as f32, true);
        }
        let x = self.logical_to_visual[column] as f32;
        (
            x + if self.rtl[column] { 1.0 } else { 0.0 },
            self.rtl[column],
        )
    }
}

pub(super) fn layout(grid: &Grid, enabled: bool) -> Vec<Row> {
    let mut result = Vec::with_capacity(grid.rows.len());
    let mut first = 0;
    while first < grid.rows.len() {
        let mut end = first + 1;
        while end < grid.rows.len() && grid.continuations[end] {
            end += 1;
        }
        let mut text = String::new();
        let mut sources = Vec::new();
        let mut ranges = Vec::new();
        for row in &grid.rows[first..end] {
            let start = sources.len();
            for (column, cell) in row.iter().enumerate() {
                if column > 0 && row[column - 1].wide {
                    continue;
                }
                if cell.text.is_empty() {
                    text.push(' ');
                    sources.push(column);
                } else {
                    for c in cell.text.chars() {
                        text.push(c);
                        sources.push(column);
                    }
                }
            }
            ranges.push(start..sources.len());
        }
        let chars: Vec<_> = text.chars().collect();
        let bidi =
            (enabled && bidi::has_rtl(&text)).then(|| bidi::Line::with_base(&text, Some(false)));
        for (offset, range) in ranges.into_iter().enumerate() {
            let cells = &grid.rows[first + offset];
            let mut rtl = vec![false; cells.len()];
            let order = if let Some(bidi) = &bidi {
                for (index, direction) in
                    bidi.row_rtl(range.start, range.end).into_iter().enumerate()
                {
                    rtl[sources[range.start + index]] |= direction;
                }
                bidi.visual(range.start, range.end)
            } else {
                range.clone().collect()
            };
            let mut visited = vec![false; cells.len()];
            let mut visual_to_logical = Vec::with_capacity(cells.len());
            for index in order {
                let col = sources[index];
                if visited[col] {
                    continue;
                }
                visited[col] = true;
                visual_to_logical.push(col);
                if cells[col].wide && col + 1 < cells.len() {
                    visited[col + 1] = true;
                    visual_to_logical.push(col + 1);
                    rtl[col + 1] = rtl[col];
                }
            }
            let mut logical_to_visual = vec![0; cells.len()];
            for (visual, &logical) in visual_to_logical.iter().enumerate() {
                logical_to_visual[logical] = visual;
            }
            let mut runs = Vec::new();
            let mut visual = 0;
            while visual < visual_to_logical.len() {
                let logical = visual_to_logical[visual];
                if !rtl[logical] || cells[logical].wide || (logical > 0 && cells[logical - 1].wide)
                {
                    visual += 1;
                    continue;
                }
                let begin = visual;
                visual += 1;
                while visual < visual_to_logical.len() {
                    let previous = visual_to_logical[visual - 1];
                    let next = visual_to_logical[visual];
                    if !rtl[next]
                        || next + 1 != previous
                        || cells[next].wide
                        || (next > 0 && cells[next - 1].wide)
                    {
                        break;
                    }
                    visual += 1;
                }
                let low = *visual_to_logical[begin..visual].iter().min().unwrap();
                let high = *visual_to_logical[begin..visual].iter().max().unwrap() + 1;
                let mut run_text = String::new();
                let mut clusters = Vec::new();
                for (col, cell) in cells.iter().enumerate().take(high).skip(low) {
                    let content = if cell.text.is_empty() {
                        " "
                    } else {
                        &cell.text
                    };
                    run_text.push_str(content);
                    clusters.extend(content.chars().map(|c| (c, (col - low) as u32)));
                }
                let start_char = range
                    .clone()
                    .find(|&i| sources[i] >= low)
                    .unwrap_or(range.start);
                let end_char = range
                    .clone()
                    .rfind(|&i| sources[i] < high)
                    .map_or(range.end, |i| i + 1);
                runs.push(Run {
                    start: begin,
                    width: visual - begin,
                    text: run_text,
                    clusters,
                    pre: chars[..start_char]
                        .iter()
                        .rev()
                        .take(32)
                        .copied()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect(),
                    post: chars[end_char..].iter().take(32).collect(),
                });
            }
            result.push(Row {
                visual_to_logical,
                logical_to_visual,
                rtl,
                runs,
            });
        }
        first = end;
    }
    result
}

#[derive(PartialEq, Eq)]
struct Key {
    text: String,
    pre: String,
    post: String,
    clusters: Vec<(char, u32)>,
    font: u64,
    width: u32,
    height: u32,
    size: u32,
}

struct Cached {
    key: Key,
    texture: egui::TextureHandle,
    pixels: usize,
}

#[derive(Default)]
pub(super) struct Renderer {
    cache: VecDeque<Cached>,
    pixels: usize,
}

impl Renderer {
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        run: &Run,
        cell: egui::Vec2,
        font_size: f32,
        editor: Option<&(Vec<u8>, u32)>,
    ) -> Option<egui::TextureHandle> {
        let candidates = editor
            .map(|(data, index)| (data.as_slice(), *index))
            .into_iter()
            .chain(std::iter::once((ARABIC, 0)))
            .chain(
                settings_store::fallback_fonts()
                    .iter()
                    .map(|(_, data, index)| (*data, *index)),
            );
        let (data, index) = candidates.into_iter().find(|(data, index)| {
            rustybuzz::Face::from_slice(data, *index).is_some_and(|face| {
                run.text
                    .chars()
                    .all(|c| c.is_whitespace() || is_control(c) || face.glyph_index(c).is_some())
            })
        })?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (data.as_ptr() as usize, data.len(), index).hash(&mut hash);
        let scale = ctx.pixels_per_point();
        let key = Key {
            text: run.text.clone(),
            pre: run.pre.clone(),
            post: run.post.clone(),
            clusters: run.clusters.clone(),
            font: hash.finish(),
            width: (run.width as f32 * cell.x * scale)
                .round()
                .clamp(1.0, 8192.0) as u32,
            height: (cell.y * scale).ceil().clamp(1.0, 256.0) as u32,
            size: (font_size * scale * 64.0).round() as u32,
        };
        if let Some(index) = self.cache.iter().position(|c| c.key == key) {
            let cached = self.cache.remove(index).unwrap();
            let texture = cached.texture.clone();
            self.cache.push_back(cached);
            return Some(texture);
        }
        let image = shape_image(
            data,
            index,
            run,
            key.width as usize,
            key.height as usize,
            key.size as f32 / 64.0,
        )?;
        let pixels = image.pixels.len();
        let texture = ctx.load_texture("terminal-shaped-run", image, egui::TextureOptions::LINEAR);
        self.pixels += pixels;
        self.cache.push_back(Cached {
            key,
            texture: texture.clone(),
            pixels,
        });
        while self.pixels > CACHE_PIXELS || self.cache.len() > 128 {
            if let Some(old) = self.cache.pop_front() {
                self.pixels -= old.pixels;
            } else {
                break;
            }
        }
        Some(texture)
    }
}

fn is_control(c: char) -> bool {
    matches!(c, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{061c}')
}

/// Shape a complete directional run, including marks and ligatures, then fit
/// each HarfBuzz cluster to its original number of terminal cells. Colors can
/// split a ligature at cell boundaries without breaking its joining geometry.
fn shape_image(
    data: &[u8],
    index: u32,
    run: &Run,
    width: usize,
    height: usize,
    font_size: f32,
) -> Option<egui::ColorImage> {
    let face = rustybuzz::Face::from_slice(data, index)?;
    let font = FontRef::try_from_slice_and_index(data, index).ok()?;
    let mut input = rustybuzz::UnicodeBuffer::new();
    for &(c, cluster) in &run.clusters {
        input.add(c, cluster);
    }
    input.set_direction(rustybuzz::Direction::RightToLeft);
    input.set_pre_context(&run.pre);
    input.set_post_context(&run.post);
    input.guess_segment_properties();
    let glyphs = rustybuzz::shape(&face, &[], input);
    let infos = glyphs.glyph_infos();
    let positions = glyphs.glyph_positions();
    let mut image = egui::ColorImage::new([width, height], egui::Color32::TRANSPARENT);
    let cell_width = width as f32 / run.width as f32;
    let y_scale = font_size / face.units_per_em() as f32;
    let baseline = (height as f32 + font_size) * 0.5 - font_size * 0.2;
    let mut raster_glyphs = Vec::new();
    let mut begin = 0;
    while begin < infos.len() {
        let cluster = infos[begin].cluster as usize;
        let mut end = begin + 1;
        while end < infos.len() && infos[end].cluster == infos[begin].cluster {
            end += 1;
        }
        let next = infos
            .iter()
            .map(|g| g.cluster as usize)
            .filter(|&c| c > cluster)
            .min()
            .unwrap_or(run.width);
        let columns = next.saturating_sub(cluster).max(1);
        let advance: i32 = positions[begin..end].iter().map(|p| p.x_advance).sum();
        if advance <= 0 {
            begin = end;
            continue;
        }
        let x_scale = columns as f32 * cell_width / advance as f32;
        let origin = (run.width - next) as f32 * cell_width;
        let mut pen = 0;
        for (info, position) in infos[begin..end].iter().zip(&positions[begin..end]) {
            let glyph = Glyph {
                id: GlyphId(info.glyph_id as u16),
                scale: PxScale {
                    x: font.height_unscaled() * x_scale,
                    y: font.height_unscaled() * y_scale,
                },
                position: ab_glyph::point(
                    origin + (pen + position.x_offset) as f32 * x_scale,
                    baseline - position.y_offset as f32 * y_scale,
                ),
            };
            raster_glyphs.push(glyph);
            pen += position.x_advance;
        }
        begin = end;
    }
    // Tall stacked marks must fit the terminal row rather than being clipped
    // by the texture. Keep x geometry and the cluster/cell mapping unchanged.
    let (top, bottom) = raster_glyphs
        .iter()
        .filter_map(|g| font.outline_glyph(g.clone()))
        .map(|g| g.px_bounds())
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), r| {
            (a.min(r.min.y), b.max(r.max.y))
        });
    let adjust = height > 2 && top.is_finite() && (top < 0.0 || bottom > height as f32);
    let fit = if adjust {
        ((height as f32 - 2.0) / (bottom - top)).min(1.0)
    } else {
        1.0
    };
    let offset = if adjust { 1.0 - top * fit } else { 0.0 };
    for mut glyph in raster_glyphs {
        glyph.scale.y *= fit;
        glyph.position.y = glyph.position.y * fit + offset;
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|x, y, coverage| {
                let x = x as i32 + bounds.min.x as i32;
                let y = y as i32 + bounds.min.y as i32;
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    let pixel = &mut image.pixels[y as usize * width + x as usize];
                    let alpha = (coverage * 255.0).round() as u8;
                    let combined = alpha.saturating_add(
                        ((u16::from(pixel.a()) * u16::from(255 - alpha)) / 255) as u8,
                    );
                    *pixel = egui::Color32::from_white_alpha(combined);
                }
            });
        }
    }
    Some(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::style::{RgbColor, Style};

    fn cells(text: &str, padding: usize) -> Vec<Cell> {
        let mut cells: Vec<Cell> = Vec::new();
        for c in text.chars() {
            if matches!(c as u32, 0x064b..=0x065f) && !cells.is_empty() {
                cells.last_mut().unwrap().text.push(c);
            } else {
                cells.push(Cell {
                    text: c.to_string(),
                    foreground: RgbColor::default(),
                    background: RgbColor::default(),
                    style: Style::default(),
                    wide: false,
                    selected: false,
                });
            }
        }
        for _ in 0..padding {
            cells.push(Cell {
                text: String::new(),
                foreground: RgbColor::default(),
                background: RgbColor::default(),
                style: Style::default(),
                wide: false,
                selected: false,
            });
        }
        cells
    }

    fn grid(rows: &[&str], continuation: &[bool]) -> Grid {
        Grid {
            rows: rows.iter().map(|r| cells(r, 2)).collect(),
            continuations: continuation.to_vec(),
            background: RgbColor::default(),
            cursor: None,
        }
    }

    #[test]
    fn arabic_cell_maps_and_cursor_keep_the_source_logical() {
        let grid = grid(&["سلام"], &[false]);
        let rows = layout(&grid, true);
        assert_eq!(rows[0].visual_to_logical, [3, 2, 1, 0, 4, 5]);
        assert_eq!(rows[0].logical_to_visual, [3, 2, 1, 0, 4, 5]);
        assert_eq!(rows[0].cursor(&grid.rows[0], 4), (0.0, true));
        assert_eq!(rows[0].cursor(&grid.rows[0], 1), (3.0, true));
        assert_eq!(
            grid.rows[0]
                .iter()
                .map(|c| c.text.as_str())
                .collect::<String>(),
            "سلام"
        );
        assert_eq!(
            layout(&grid, false)[0].visual_to_logical,
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn mixed_text_preserves_prompt_numbers_marks_and_wide_cells() {
        let mut grid = grid(&["$ بَيت 123 中"], &[false]);
        let wide = grid.rows[0].iter().position(|c| c.text == "中").unwrap();
        grid.rows[0][wide].wide = true;
        grid.rows[0].insert(wide + 1, cells("", 1).remove(0));
        let rows = layout(&grid, true);
        assert_eq!(&rows[0].visual_to_logical[..2], &[0, 1]);
        let arabic: Vec<_> = rows[0]
            .visual_to_logical
            .iter()
            .copied()
            .filter(|i| (2..5).contains(i))
            .collect();
        assert_eq!(arabic, [4, 3, 2]);
        let digits: Vec<_> = rows[0]
            .visual_to_logical
            .iter()
            .copied()
            .filter(|&i| grid.rows[0][i].text.chars().any(|c| c.is_ascii_digit()))
            .collect();
        assert_eq!(digits, [6, 7, 8]);
        assert_eq!(
            rows[0].logical_to_visual[wide + 1],
            rows[0].logical_to_visual[wide] + 1
        );
        let mut mapped = rows[0].visual_to_logical.clone();
        mapped.sort_unstable();
        assert_eq!(mapped, (0..grid.rows[0].len()).collect::<Vec<_>>());
        assert!(rows[0].runs.iter().any(|run| run.text.contains("بَيت")));
    }

    #[test]
    fn open_type_shaping_joins_letters_and_positions_harakat() {
        for text in ["ببب", "سَلَام", "لا", "لَا", "پژوهش"] {
            let grid = grid(&[text], &[false]);
            let rows = layout(&grid, true);
            let run = &rows[0].runs[0];
            let image = shape_image(ARABIC, 0, run, run.width * 24, 36, 28.0).unwrap();
            assert!(image.pixels.iter().any(|p| p.a() > 0), "{text}");
            if text == "ببب" {
                for border in [24, 48] {
                    assert!(
                        (0..36).any(|y| image.pixels[y * 72 + border - 1].a() > 10
                            && image.pixels[y * 72 + border].a() > 10),
                        "joining gap at {border}"
                    );
                }
            }
        }
        let unmarked = layout(&grid(&["سلام"], &[false]), true);
        let marked = layout(&grid(&["سَلَام"], &[false]), true);
        let a = shape_image(ARABIC, 0, &unmarked[0].runs[0], 96, 36, 28.0).unwrap();
        let b = shape_image(ARABIC, 0, &marked[0].runs[0], 96, 36, 28.0).unwrap();
        assert_ne!(a.pixels, b.pixels, "diacritics must be rendered");
    }

    #[test]
    fn soft_wrapping_supplies_arabic_joining_context_without_moving_rows() {
        let mut grid = grid(&["ب", "ب"], &[false, true]);
        grid.rows[0].truncate(1);
        let rows = layout(&grid, true);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].runs[0].post.contains('ب'));
        assert!(rows[1].runs[0].pre.contains('ب'));
        assert_eq!(rows[0].visual_to_logical.len(), grid.rows[0].len());
        assert_eq!(rows[1].visual_to_logical.len(), grid.rows[1].len());
    }

    #[test]
    fn wide_characters_inside_rtl_runs_keep_both_cells() {
        let mut grid = grid(&["ع中ب"], &[false]);
        grid.rows[0][1].wide = true;
        grid.rows[0].insert(2, cells("", 1).remove(0));
        let rows = layout(&grid, true);
        assert_eq!(
            rows[0].logical_to_visual[2],
            rows[0].logical_to_visual[1] + 1
        );
        assert!(rows[0].runs.iter().all(|r| !r.text.contains('中')));
    }
}
