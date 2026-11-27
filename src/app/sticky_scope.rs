//! The declaration immediately above the visible source range.
use super::*;
use egui_commonmark::pulldown_cmark::{Event, Parser, Tag};

const MAX_BYTES: usize = 1024 * 1024;

struct Scope {
    start: usize,
    end: usize,
    title: String,
}

fn title(line: &str) -> String {
    line.trim().chars().take(180).collect()
}

fn code_scopes(text: &str, tab_width: usize) -> Vec<Scope> {
    let mut line_starts = vec![0];
    let mut scalar = 0;
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    for line in &lines {
        scalar += line.chars().count();
        line_starts.push(scalar);
    }
    let symbols = hedit::intelligence::symbols(text, std::path::Path::new(""));
    hedit::advanced::indentation_folds(text, tab_width)
        .into_iter()
        .filter_map(|fold| {
            let start = line_starts.partition_point(|&at| at < fold.start);
            let end = line_starts.partition_point(|&at| at < fold.end);
            if symbols
                .binary_search_by_key(&start, |symbol| symbol.line)
                .is_err()
            {
                return None;
            }
            Some(Scope {
                start,
                end,
                title: title(lines.get(start.checked_sub(1)?)?),
            })
        })
        .collect()
}

fn markdown_scopes(text: &str) -> Vec<Scope> {
    let lines: Vec<_> = std::iter::once(0)
        .chain(memchr::memchr_iter(b'\n', text.as_bytes()).map(|index| index + 1))
        .collect();
    let mut scopes: Vec<Scope> = Vec::new();
    let mut levels = Vec::<(usize, usize)>::new();
    for (event, range) in Parser::new(text).into_offset_iter() {
        let Event::Start(Tag::Heading { level, .. }) = event else {
            continue;
        };
        let index = lines.partition_point(|&start| start <= range.start);
        let level = level as usize;
        while levels
            .last()
            .is_some_and(|&(previous, _)| previous >= level)
        {
            let (_, section) = levels.pop().unwrap();
            scopes[section].end = index.saturating_sub(1);
        }
        let heading = text[range].lines().next().unwrap_or("").trim_start();
        let heading = heading.trim_start_matches('#').trim_end_matches('#');
        levels.push((level, scopes.len()));
        scopes.push(Scope {
            start: index,
            end: lines.len(),
            title: title(heading),
        });
    }
    scopes
}

#[derive(Default)]
pub(super) struct Cache {
    key: Option<(u64, u64, syntax::Language, usize)>,
    scopes: Vec<Scope>,
}

impl Cache {
    pub(super) fn refresh(&mut self, buffer: &Buffer) -> bool {
        let language = buffer.language();
        if buffer.page.info.paged
            || buffer.follow
            || buffer.page.text.len() > MAX_BYTES
            || language == syntax::Language::PlainText
        {
            return false;
        }
        let width = usize::from(buffer.view.tab_size.unwrap_or(4));
        let key = (buffer.view.id, buffer.page.info.revision, language, width);
        if self.key != Some(key) {
            let source = &buffer.page.text;
            self.scopes = if language == syntax::Language::Markdown {
                markdown_scopes(source)
            } else {
                code_scopes(source, width)
            };
            self.key = Some(key);
        }
        !self.scopes.is_empty()
    }

    fn at(&self, visible_top: usize) -> Option<&Scope> {
        let end = self
            .scopes
            .partition_point(|scope| scope.start < visible_top);
        self.scopes[..end]
            .iter()
            .rev()
            .find(|scope| visible_top <= scope.end)
    }

    pub(super) fn show(
        &self,
        ui: &mut egui::Ui,
        mut rect: egui::Rect,
        visible_top: usize,
        theme: &Theme,
    ) -> Option<usize> {
        let scope = self.at(visible_top)?;
        rect.max.y =
            (rect.min.y + ui.text_style_height(&egui::TextStyle::Monospace) + 6.0).min(rect.max.y);
        if rect.height() < 8.0 {
            return None;
        }
        ui.put(
            rect,
            egui::Button::new(RichText::new(&scope.title).monospace())
                .min_size(rect.size())
                .fill(theme.panel.color())
                .truncate(),
        )
        .on_hover_text(format!("Go to line {}", scope.start))
        .clicked()
        .then_some(scope.start)
    }
}

#[cfg(test)]
#[test]
fn sticky_scope_uses_the_deepest_enclosing_heading_or_declaration() {
    let md = "# Outer\n\n\x60\x60\x60\n# ignored\n\x60\x60\x60\n## Inner\n\nbody\n# Next\n";
    let cache = Cache {
        key: None,
        scopes: markdown_scopes(md),
    };
    assert_eq!(cache.at(4).unwrap().title, "Outer");
    assert_eq!(cache.at(8).unwrap().title, "Inner");
    assert!(cache.at(9).is_none());
    let source = "mod outer {\n    fn inner() {\n        let x = 1;\n    }\n}\n";
    let cache = Cache {
        key: None,
        scopes: code_scopes(source, 4),
    };
    assert_eq!(cache.at(3).unwrap().title, "fn inner() {");
    assert_eq!(cache.at(4).unwrap().title, "mod outer {");
}
