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

#[derive(Clone, Copy)]
pub(super) enum PickerLine {
    File(usize, Option<usize>),
    Page(usize),
}

/// Split `app.rs@save` into the file query and what to go to inside it.
pub(super) fn parse(query: &str) -> (&str, Option<Target>) {
    let (file, suffix) = if let Some((quoted, suffix)) = query
        .strip_prefix('"')
        .and_then(|text| text.rsplit_once('"'))
        .filter(|(_, suffix)| suffix.is_empty() || suffix.starts_with(['@', ':', '#']))
    {
        (quoted, suffix)
    } else if let Some(at) = query.find(['@', ':', '#']) {
        (query[..at].trim(), &query[at..])
    } else {
        (query, "")
    };
    let Some((delimiter, rest)) = suffix.split_at_checked(1) else {
        return (file, None);
    };
    let target = match delimiter.as_bytes()[0] {
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
    group: u64,
    id: u64,
    anchor: usize,
    head: usize,
    selections: Vec<hedit::advanced::Selection>,
    scroll_top: f64,
    scroll_left: f64,
    markdown_view: hedit::session::MarkdownView,
    page: Option<Page>,
}

impl HeditApp {
    /// Move the cursor to a 1-based line and column in the active file.
    pub(super) fn jump_to_line(&mut self, ctx: &egui::Context, line: usize, column: Option<usize>) {
        if self.active().page.info.paged {
            self.reveal_markdown_source(ctx);
            self.goto = line.to_string();
            self.goto_location(ctx, line.max(1) as u64, column.unwrap_or(1).max(1) as u64);
            return;
        }
        self.jump_to_page_line(ctx, line, column);
    }

    fn jump_to_page_line(&mut self, ctx: &egui::Context, line: usize, column: Option<usize>) {
        self.reveal_markdown_source(ctx);
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
        let line_end = text[start..].find('\n').map_or(text.len(), |n| start + n);
        let line = &text[start..line_end];
        let line = if line_end < text.len() {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        let mut remaining = column.unwrap_or(1).saturating_sub(1);
        let column = line
            .chars()
            .take_while(|ch| {
                let width = ch.len_utf16();
                if width > remaining {
                    return false;
                }
                remaining -= width;
                true
            })
            .count();
        let cursor = text[..start].chars().count() + column;
        b.view.anchor = cursor;
        b.view.head = cursor;
        b.view.selections.clear();
        b.reveal_cursor = true;
        b.restore_cursor = true;
        self.changed();
    }

    pub(super) fn jump_to_picker_line(&mut self, ctx: &egui::Context, line: PickerLine) {
        match line {
            PickerLine::File(line, column) => self.jump_to_line(ctx, line, column),
            PickerLine::Page(line) => self.jump_to_page_line(ctx, line, None),
        }
    }

    pub(super) fn restore_goto_page_for_matches(&mut self) {
        let original = self
            .goto_origin
            .as_ref()
            .and_then(|origin| origin.page.as_ref());
        if original.is_some_and(|page| page.start != self.active().page.start) {
            self.cancel_goto_preview();
        }
    }

    /// Preview a line while choosing, remembering where the cursor was.
    pub(super) fn preview_line(&mut self, ctx: &egui::Context, line: PickerLine) {
        if self.goto_origin.is_none() {
            self.goto_preview_epoch = self.goto_preview_epoch.wrapping_add(1);
            let b = self.active();
            self.goto_origin = Some(Origin {
                group: self.panes.focused_id(),
                id: b.view.id,
                anchor: b.view.anchor,
                head: b.view.head,
                selections: b.view.selections.clone(),
                scroll_top: b.view.scroll_top,
                scroll_left: b.view.scroll_left,
                markdown_view: b.view.markdown_view,
                page: b.page.info.paged.then(|| b.page.clone()),
            });
        }
        let PickerLine::File(file_line, column) = line else {
            // A completed file-line read must not replace a page-local match.
            self.goto_preview_epoch = self.goto_preview_epoch.wrapping_add(1);
            self.jump_to_picker_line(ctx, line);
            ctx.request_repaint();
            return;
        };
        if !self.active().page.info.paged {
            self.jump_to_picker_line(ctx, line);
            ctx.request_repaint();
        } else if !self.goto_preview_running {
            self.goto_preview_running = true;
            let epoch = self.goto_preview_epoch;
            let id = self.active().page.info.id;
            let editor = self.workspace.editor.clone();
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let read = || {
                    let shared = editor.get(id)?;
                    let mut doc = shared.lock().map_err(|e| e.to_string())?;
                    opening::page_at_location(
                        &mut doc,
                        file_line as u64,
                        column.unwrap_or(1) as u64,
                    )
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(read))
                    .unwrap_or_else(|_| Err("The line preview could not be loaded.".into()));
                let _ = tx.send(Ok(Action::GotoPreview(epoch, result)));
                ctx.request_repaint();
            });
        }
    }

    pub(super) fn finish_goto_preview(
        &mut self,
        epoch: u64,
        result: Result<(Page, usize), String>,
    ) {
        self.goto_preview_running = false;
        let Some(origin) = &self.goto_origin else {
            return;
        };
        if epoch != self.goto_preview_epoch
            || origin.group != self.panes.focused_id()
            || origin.id != self.active().view.id
        {
            return;
        }
        let Ok((page, cursor)) = result else {
            return;
        };
        let buffer = &mut self.workspace.open[self.workspace.active];
        if page.info.revision != buffer.page.info.revision
            || page.info.path != buffer.page.info.path
        {
            return;
        }
        buffer.set_page(page);
        buffer.view.anchor = cursor;
        buffer.view.head = cursor;
        buffer.reveal_cursor = true;
        self.changed();
    }
    /// Put the cursor back after a cancelled preview.
    pub(super) fn cancel_goto_preview(&mut self) {
        let Some(origin) = self.goto_origin.take() else {
            return;
        };
        if !self.group_contains(origin.group, origin.id) {
            return;
        }
        let Some(index) = self
            .workspace
            .open
            .iter()
            .position(|b| b.view.id == origin.id)
        else {
            return;
        };
        let b = &mut self.workspace.open[index];
        if let Some(page) = origin.page {
            if b.page.info.revision != page.info.revision || b.page.info.path != page.info.path {
                return;
            }
            if b.page.start != page.start {
                b.set_page(page);
            }
        }
        let canonical = (origin.group != self.panes.focused_id())
            .then(|| (panes::CursorView::get(b), b.restore_cursor, b.reveal_cursor));
        b.view.anchor = origin.anchor;
        b.view.head = origin.head;
        b.view.selections = origin.selections;
        b.view.scroll_top = origin.scroll_top;
        b.view.scroll_left = origin.scroll_left;
        b.view.markdown_view = origin.markdown_view;
        b.restore_cursor = true;
        b.reveal_cursor = false;
        self.save_view_in_group(origin.group, index);
        if let Some((cursor, restore, reveal)) = canonical {
            let b = &mut self.workspace.open[index];
            cursor.put(b);
            b.restore_cursor = restore;
            b.reveal_cursor = reveal;
        }
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
        let label = Path::new(&path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let remote = self.workspace.remote.clone();
        let editor = self.workspace.editor.clone();
        self.read_open(ctx, label, move || {
            let file = PathBuf::from(&path);
            let mut doc = if let Some(remote) = remote {
                remote.open(editor.id(), &file)?
            } else {
                Document::open_or_new(editor.id(), &file)?
            };
            let text = doc.text(32 * 1024 * 1024).unwrap_or_default();
            let (line, column) = resolve(&text, &file, &target).unwrap_or((1, None));
            let (page, cursor) =
                opening::page_at_location(&mut doc, line as u64, column.unwrap_or(1) as u64)?;
            Ok(Action::Add(Box::new(doc), page, cursor))
        });
    }
}
