//! Reuse shaped rows across edits. Blocks always end at paragraph boundaries,
//! so wrapping, Arabic joining and bidi resolution see complete paragraphs.
use super::*;
use egui::{
    epaint::text::{ByteIndex, FontsView, LayoutSection},
    text::LayoutJob,
};
use std::ops::Range;
const BLOCK_BYTES: usize = 4096;
#[derive(Clone)]
pub(super) struct Block {
    bytes: Range<usize>,
    galley: Arc<Galley>,
    styles: Vec<LayoutSection>,
    bidi: super::super::bidi_view::BidiRows,
}

impl Block {
    pub(super) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.galley.job.text.len()
            + self.styles.len() * std::mem::size_of::<LayoutSection>()
    }
}

type Previous<'a> = (&'a Galley, &'a [Block]);

fn pieces(text: &str, old: Option<Previous<'_>>) -> Vec<(Range<usize>, Option<usize>)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut tail = Vec::new();
    if let Some((old, blocks)) = old {
        let (prefix, end, inserted) = hedit::workspace::splice(&old.job.text, text);
        for (i, block) in blocks.iter().enumerate() {
            if block.bytes.end > prefix
                || (block.bytes.end == old.job.text.len() && text.len() != old.job.text.len())
            {
                break;
            }
            pieces.push((block.bytes.clone(), Some(i)));
            start = block.bytes.end;
        }
        let delta = inserted.len() as isize - (end - prefix) as isize;
        for (i, block) in blocks.iter().enumerate().rev() {
            if block.bytes.start < end || block.bytes.start < start {
                break;
            }
            let at = (block.bytes.start as isize + delta) as usize;
            if at < start || (at > 0 && text.as_bytes().get(at - 1) != Some(&b'\n')) {
                break;
            }
            tail.push((at..(block.bytes.end as isize + delta) as usize, Some(i)));
        }
        tail.reverse();
    }
    let end = tail.first().map_or(text.len(), |(range, _)| range.start);
    while start < end {
        let target = (start + BLOCK_BYTES).min(end);
        let next = if target == end {
            end
        } else {
            memchr::memchr(b'\n', &text.as_bytes()[target..end]).map_or(end, |n| target + n + 1)
        };
        pieces.push((start..next, None));
        start = next;
    }
    pieces.extend(tail);
    if pieces.is_empty() {
        pieces.push((0..0, None));
    }
    pieces
}
fn styles(job: &LayoutJob, range: &Range<usize>) -> Vec<LayoutSection> {
    let first = job
        .sections
        .partition_point(|s| s.byte_range.end.0 <= range.start);
    let mut styles = Vec::new();
    for section in &job.sections[first..] {
        if section.byte_range.start.0 >= range.end {
            break;
        }
        let mut style = section.clone();
        style.byte_range = ByteIndex(section.byte_range.start.0.max(range.start) - range.start)
            ..ByteIndex(section.byte_range.end.0.min(range.end) - range.start);
        styles.push(style);
    }
    if let Some(section) = job.sections.last().filter(|_| styles.is_empty()) {
        let mut style = section.clone();
        style.byte_range = ByteIndex(0)..ByteIndex(0);
        styles.push(style);
    }
    styles
}

pub(super) fn layout(
    fonts: &mut FontsView<'_>,
    job: LayoutJob,
    base: super::super::bidi_view::Base,
    old: Option<Previous<'_>>,
) -> (Arc<Galley>, super::super::bidi_view::BidiRows, Vec<Block>) {
    use super::super::bidi_view::{apply, BidiRows};
    let ranges = pieces(&job.text, old);
    if ranges.len() == 1 {
        let galley = fonts.layout_job(job);
        let (galley, bidi) = apply(fonts, galley, base);
        let block = Block {
            bytes: 0..galley.job.text.len(),
            galley: Arc::clone(&galley),
            styles: galley.job.sections.clone(),
            bidi: bidi.clone(),
        };
        return (galley, bidi, vec![block]);
    }
    let mut blocks = Vec::with_capacity(ranges.len());
    let mut galleys = Vec::with_capacity(ranges.len());
    let mut dirs = Vec::new();
    let mut row_count = 0;
    for (index, (bytes, previous)) in ranges.iter().enumerate() {
        // Concat restores the line break separating adjacent blocks.
        let trim = usize::from(index + 1 < ranges.len() && job.text[bytes.clone()].ends_with('\n'));
        let part = bytes.start..bytes.end - trim;
        let sections = styles(&job, &part);
        let (galley, bidi) = if let Some(block) = previous
            .and_then(|i| old.map(|(_, blocks)| &blocks[i]))
            .filter(|block| block.styles == sections && block.galley.job.text.len() == part.len())
        {
            (Arc::clone(&block.galley), block.bidi.clone())
        } else {
            let mini = LayoutJob {
                text: job.text[part].into(),
                sections: sections.clone(),
                wrap: job.wrap.clone(),
                first_row_min_height: 0.0,
                break_on_newline: job.break_on_newline,
                halign: job.halign,
                justify: job.justify,
                round_output_to_gui: job.round_output_to_gui,
                center_text_in_line_height: job.center_text_in_line_height,
                keep_trailing_whitespace: job.keep_trailing_whitespace,
            };
            let galley = fonts.layout_job(mini);
            apply(fonts, galley, base)
        };
        if !dirs.is_empty() || bidi.any() {
            dirs.resize(row_count, None);
            dirs.extend((0..galley.rows.len()).map(|i| bidi.row(i).cloned()));
        }
        row_count += galley.rows.len();
        galleys.push(Arc::clone(&galley));
        blocks.push(Block {
            bytes: bytes.clone(),
            galley,
            styles: sections,
            bidi,
        });
    }
    let galley = Arc::new(Galley::concat(
        Arc::new(job),
        &galleys,
        fonts.pixels_per_point(),
    ));
    (galley, BidiRows { rows: dirs }, blocks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::bidi_view::Base;

    fn code(source: &str) -> LayoutJob {
        code_job(
            source,
            Language::PlainText,
            Font {
                size: 14.0,
                line_height: Some(19.5),
            },
            f32::INFINITY,
            &Theme::default(),
            None,
        )
    }

    #[test]
    fn repositioning_block_rows_shares_the_existing_glyphs_and_meshes() {
        let source = "value item\n".repeat(3_000);
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            crate::app::tests::run_pass(&ctx, Default::default(), |ctx| {
                ctx.fonts_mut(|fonts| {
                    let (first, _, blocks) = layout(fonts, code(&source), Base::Ltr, None);
                    let edited = format!("inserted line\n{source}");
                    let (next, _, _) =
                        layout(fonts, code(&edited), Base::Ltr, Some((&first, &blocks)));
                    assert_eq!(next.rows.len(), first.rows.len() + 1);
                    let row = first.rows.len() - 2;
                    assert_eq!(first.rows[row].text(), next.rows[row + 1].text());
                    assert!(Arc::ptr_eq(&first.rows[row].row, &next.rows[row + 1].row));
                    assert!(next.rows[row + 1].pos.y > first.rows[row].pos.y);
                })
            });
        }
    }

    #[test]
    fn block_boundaries_preserve_newlines_and_logical_offsets_after_an_edit() {
        let source = "alpha שלום β\n\r\n".repeat(2_000);
        let ctx = egui::Context::default();
        let _ = crate::app::tests::run_pass(&ctx, Default::default(), |ctx| {
            ctx.fonts_mut(|fonts| {
                let (first, _, blocks) = layout(fonts, code(&source), Base::Auto, None);
                let edited = format!("{source}last line");
                let (next, bidi, _) =
                    layout(fonts, code(&edited), Base::Auto, Some((&first, &blocks)));
                assert!(bidi.any());
                assert_eq!(next.end().index.0, edited.chars().count());
                assert_eq!(next.rows.last().unwrap().text(), "last line");
                assert!(next.rows[..next.rows.len() - 1]
                    .iter()
                    .all(|row| row.ends_with_newline));
            })
        });
    }
}
