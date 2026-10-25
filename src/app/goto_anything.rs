//! Go to Anything, as in Sublime Text: in Go to file, `@` jumps to a symbol,
//! `:` to a line, and `#` to text, in the current file or after a file name.
use super::*;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Symbol(String),
    /// A 1-based line and optional 1-based column.
    Line(Option<usize>, Option<usize>),
    Text(String),
}

/// Split `app.rs@save` into the file query and what to go to inside it.
pub(super) fn parse(query: &str) -> (&str, Option<Target>) {
    let Some(at) = query.find(['@', ':', '#']) else {
        return (query, None);
    };
    let (file, rest) = (query[..at].trim(), &query[at + 1..]);
    let target = match query.as_bytes()[at] {
        b'@' => Target::Symbol(rest.trim().to_owned()),
        b':' => {
            let mut parts = rest.trim().splitn(2, ':');
            let number =
                |part: Option<&str>| part.and_then(|p| p.trim().parse().ok()).filter(|&n| n > 0);
            Target::Line(number(parts.next()), number(parts.next()))
        }
        _ => Target::Text(rest.trim().to_owned()),
    };
    (file, Some(target))
}

/// The first line a target names in `text`, as (1-based line, optional column).
pub(super) fn resolve(text: &str, path: &Path, target: &Target) -> Option<(usize, Option<usize>)> {
    match target {
        Target::Line(line, column) => line.map(|l| (l, *column)),
        Target::Symbol(query) => symbol_matches(text, path, query)
            .into_iter()
            .next()
            .map(|(_, line)| (line, None)),
        Target::Text(query) => text_matches(text, query, 1)
            .into_iter()
            .next()
            .map(|(_, line)| (line, None)),
    }
}

/// Symbols matching `query`, best first, as (name, 1-based line).
pub(super) fn symbol_matches(text: &str, path: &Path, query: &str) -> Vec<(String, usize)> {
    let mut found: Vec<(i64, String, usize)> = hedit::intelligence::symbols(text, path)
        .into_iter()
        .filter_map(|s| {
            let score = if query.is_empty() {
                0
            } else {
                hedit::folder::fuzzy_score(&s.name, &query.to_lowercase())?
            };
            Some((score, s.name, s.line))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
    found
        .into_iter()
        .map(|(_, name, line)| (name, line))
        .collect()
}

/// Lines containing `query`, ignoring case, as (trimmed text, 1-based line).
pub(super) fn text_matches(text: &str, query: &str, limit: usize) -> Vec<(String, usize)> {
    if query.is_empty() {
        return vec![];
    }
    let query = query.to_lowercase();
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.to_lowercase().contains(&query))
        .take(limit)
        .map(|(n, line)| (line.trim().chars().take(160).collect(), n + 1))
        .collect()
}

/// Where the cursor was before previewing Go to Anything results.
pub(super) struct Origin {
    id: u64,
    anchor: usize,
    head: usize,
    selections: Vec<hedit::advanced::Selection>,
    scroll_top: f64,
    scroll_left: f64,
}

impl HeditApp {
    /// Move the cursor to a 1-based line and column in the active file.
    pub(super) fn jump_to_line(&mut self, ctx: &egui::Context, line: usize, column: Option<usize>) {
        if self.active().page.info.paged {
            self.goto = line.to_string();
            self.goto_location(ctx, line.max(1) as u64, column.unwrap_or(1).max(1) as u64);
            return;
        }
        let index = self.workspace.active;
        let b = &mut self.workspace.open[index];
        let text = &b.page.text;
        let start = if line <= 1 {
            0
        } else {
            memchr::memchr_iter(b'\n', text.as_bytes())
                .nth(line - 2)
                .map_or(text.len(), |n| n + 1)
        };
        let end = text[start..].find('\n').map_or(text.len(), |n| start + n);
        let column = column.unwrap_or(1).saturating_sub(1);
        let offset = text[start..end]
            .char_indices()
            .nth(column)
            .map_or(end, |(i, _)| start + i);
        let cursor = text[..offset].chars().count();
        b.view.anchor = cursor;
        b.view.head = cursor;
        b.view.selections.clear();
        b.reveal_cursor = true;
        b.restore_cursor = true;
        self.changed();
    }
    /// Preview a line while choosing, remembering where the cursor was.
    pub(super) fn preview_line(&mut self, ctx: &egui::Context, line: usize, column: Option<usize>) {
        if self.goto_origin.is_none() {
            let b = self.active();
            self.goto_origin = Some(Origin {
                id: b.view.id,
                anchor: b.view.anchor,
                head: b.view.head,
                selections: b.view.selections.clone(),
                scroll_top: b.view.scroll_top,
                scroll_left: b.view.scroll_left,
            });
        }
        self.jump_to_line(ctx, line, column);
    }
    /// Put the cursor back after a cancelled preview.
    pub(super) fn cancel_goto_preview(&mut self) {
        let Some(origin) = self.goto_origin.take() else {
            return;
        };
        let Some(index) = self
            .workspace
            .open
            .iter()
            .position(|b| b.view.id == origin.id)
        else {
            return;
        };
        let b = &mut self.workspace.open[index];
        b.view.anchor = origin.anchor;
        b.view.head = origin.head;
        b.view.selections = origin.selections;
        b.view.scroll_top = origin.scroll_top;
        b.view.scroll_left = origin.scroll_left;
        b.restore_cursor = true;
        b.reveal_cursor = false;
        self.changed();
    }
    /// Apply a Go to Anything target after choosing a file: jump in an open
    /// buffer now, or open the file at the resolved line.
    pub(super) fn go_to_target(
        &mut self,
        ctx: &egui::Context,
        path: Option<String>,
        target: Target,
    ) {
        self.goto_origin = None;
        let Some(path) = path else {
            let b = self.active();
            let path = b
                .page
                .info
                .path
                .clone()
                .map(PathBuf::from)
                .unwrap_or_default();
            if let Some((line, column)) = resolve(&b.page.text.clone(), &path, &target) {
                self.navigation_mark();
                self.jump_to_line(ctx, line, column);
            }
            return;
        };
        if let Target::Line(Some(line), column) = target {
            let suffix = column.map_or(String::new(), |c| format!(":{c}"));
            self.pending_paths
                .push_back(PathBuf::from(format!("{path}:{line}{suffix}")));
            return;
        }
        self.job(ctx, move || {
            let file = PathBuf::from(&path);
            let text = hedit::projects::read_text(&file, 32 * 1024 * 1024).unwrap_or_default();
            let line = resolve(&text, &file, &target).map_or(1, |(line, _)| line);
            Ok(Action::OpenAt(PathBuf::from(format!("{path}:{line}"))))
        });
    }
}
