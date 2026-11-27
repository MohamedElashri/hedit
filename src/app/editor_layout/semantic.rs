use super::*;
use hedit::advanced::{Fold, Projection};
use hedit::language_server::text::{SemanticKind, SemanticToken};

pub(super) struct Colors {
    projection: Arc<Projection>,
    source: Arc<Vec<SemanticToken>>,
    ranges: Vec<(std::ops::Range<usize>, SemanticKind)>,
}

impl Colors {
    pub(super) fn current(
        &self,
        projection: &Arc<Projection>,
        source: &Arc<Vec<SemanticToken>>,
    ) -> bool {
        Arc::ptr_eq(&self.projection, projection) && Arc::ptr_eq(&self.source, source)
    }

    pub(super) fn matches_text(&self, text: &str) -> bool {
        self.projection.text == text
    }

    pub(super) fn new(
        projection: Arc<Projection>,
        source: Arc<Vec<SemanticToken>>,
        folds: &[Fold],
    ) -> Self {
        let mut chars = projection.text.char_indices().enumerate().peekable();
        let mut byte_at = |target| {
            while chars.peek().is_some_and(|(scalar, _)| *scalar < target) {
                chars.next();
            }
            chars
                .peek()
                .map_or(projection.text.len(), |(_, (byte, _))| *byte)
        };
        let mut ranges = Vec::new();
        let mut folds = folds.iter().peekable();
        for token in source.iter() {
            while folds.peek().is_some_and(|fold| fold.end <= token.start) {
                folds.next();
            }
            if folds.peek().is_some_and(|fold| fold.start < token.end) {
                continue;
            }
            let start = byte_at(projection.display(token.start));
            let end = byte_at(projection.display(token.end));
            if start < end {
                ranges.push((start..end, token.kind));
            }
        }
        Self {
            projection,
            source,
            ranges,
        }
    }

    pub(super) fn apply(&self, job: &mut egui::text::LayoutJob, theme: &Theme) {
        use egui::text::{ByteIndex, LayoutSection};
        let mut overlays = self.ranges.iter().peekable();
        let mut sections = Vec::with_capacity(job.sections.len() + self.ranges.len());
        for section in std::mem::take(&mut job.sections) {
            let mut start = section.byte_range.start.0;
            let end = section.byte_range.end.0;
            let mut add = |first, last, color| {
                if first < last {
                    let mut format = section.format.clone();
                    format.color = color;
                    sections.push(LayoutSection {
                        leading_space: if first == section.byte_range.start.0 {
                            section.leading_space
                        } else {
                            0.0
                        },
                        byte_range: ByteIndex(first)..ByteIndex(last),
                        format,
                    });
                }
            };
            while let Some((range, kind)) = overlays.peek() {
                if range.start >= end {
                    break;
                }
                if start < range.start {
                    add(start, range.start, section.format.color);
                }
                let colored_end = end.min(range.end);
                add(start.max(range.start), colored_end, color(*kind, theme));
                start = start.max(colored_end);
                if range.end <= end {
                    overlays.next();
                } else {
                    break;
                }
            }
            add(start, end, section.format.color);
        }
        job.sections = sections;
    }
}

fn color(kind: SemanticKind, theme: &Theme) -> egui::Color32 {
    match kind {
        SemanticKind::Type => theme.type_name,
        SemanticKind::Parameter => theme.constant,
        SemanticKind::Property => theme.property,
        SemanticKind::Function => theme.function,
        SemanticKind::Keyword => theme.keyword,
        SemanticKind::Comment => theme.comment,
        SemanticKind::String => theme.string,
        SemanticKind::Number => theme.number,
        SemanticKind::Operator => theme.operator,
    }
    .color()
}
