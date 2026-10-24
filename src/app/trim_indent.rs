//! Indentation that Enter adds is taken away again when its line stays empty
//! and the cursor moves on (`trim_automatic_white_space`).
use super::*;

/// The line, in a view, that holds only indentation Enter added.
pub(super) struct AutoIndent {
    view: u64,
    line: usize,
}

/// Byte range of a 0-based line's text, without its line break.
fn line_range(text: &str, line: usize) -> Option<(usize, usize)> {
    let start = if line == 0 {
        0
    } else {
        memchr::memchr_iter(b'\n', text.as_bytes()).nth(line - 1)? + 1
    };
    let end = text[start..].find('\n').map_or(text.len(), |n| start + n);
    let end = if text[..end].ends_with('\r') {
        end - 1
    } else {
        end
    };
    Some((start, end))
}

/// Remove a line's text when it is only spaces and tabs. Returns the new text,
/// where the line starts (in characters), and how many characters went.
pub(super) fn trim_blank_line(text: &str, line: usize) -> Option<(String, usize, usize)> {
    let (start, end) = line_range(text, line)?;
    let content = &text[start..end];
    if content.is_empty() || !content.chars().all(|c| c == ' ' || c == '\t') {
        return None;
    }
    let mut trimmed = String::with_capacity(text.len());
    trimmed.push_str(&text[..start]);
    trimmed.push_str(&text[end..]);
    Some((
        trimmed,
        text[..start].chars().count(),
        content.chars().count(),
    ))
}

impl HeditApp {
    /// After Enter: trim the line it left if that line was only indentation,
    /// and remember the new line if it is.
    pub(super) fn after_newline(&mut self, old_line: usize, batch: &mut hedit::advanced::Batch) {
        self.auto_indent = None;
        if !self.settings.trim_automatic_white_space || batch.selections.len() != 1 {
            return;
        }
        if let Some((text, start, removed)) = trim_blank_line(&batch.text, old_line) {
            batch.text = text;
            for s in &mut batch.selections {
                if s.head > start {
                    s.head -= removed;
                    s.anchor = s.anchor.saturating_sub(removed);
                }
            }
        }
        let head = batch.selections[0].head;
        let at = hedit::advanced::byte(&batch.text, head);
        let line = memchr::memchr_iter(b'\n', &batch.text.as_bytes()[..at]).count();
        if trim_blank_line(&batch.text, line).is_some() {
            self.auto_indent = Some(AutoIndent {
                view: self.active().view.id,
                line,
            });
        }
    }
    /// Each frame: once the cursor has left the line Enter indented, take the
    /// indentation away if the line is still otherwise empty.
    pub(super) fn trim_auto_indent(&mut self) {
        let Some(pending) = &self.auto_indent else {
            return;
        };
        let b = self.active();
        if !self.settings.trim_automatic_white_space
            || pending.view != b.view.id
            || !b.view.selections.is_empty()
            || b.page.info.paged
        {
            self.auto_indent = None;
            return;
        }
        let line = pending.line;
        if self.cursor_line() == line {
            return;
        }
        let trimmed = trim_blank_line(&b.page.text, line);
        let (anchor, head) = (b.view.anchor, b.view.head);
        self.auto_indent = None;
        let Some((text, start, removed)) = trimmed else {
            return;
        };
        let shift = |p: usize| if p > start { p - removed } else { p };
        let (anchor, head) = (shift(anchor), shift(head));
        self.apply_edit(hedit::editing::Edit { text, anchor, head }, None);
    }
}
