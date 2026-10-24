//! Moving between changed blocks (the gutter's change markers) and putting a
//! block back as it was in Git's HEAD or the saved file.
use super::*;
use hedit::intelligence::{self, Change};

impl HeditApp {
    /// The active file's changed blocks, when markers are known for it.
    fn active_changes(&self) -> Option<Vec<Change>> {
        let b = self.active();
        if b.page.info.paged {
            return None;
        }
        let base = self.quality.git_base.get(&b.view.id)?;
        Some(intelligence::changes(base, &b.page.text))
    }
    /// The 0-based line of the cursor.
    pub(super) fn cursor_line(&self) -> usize {
        let b = self.active();
        let at = hedit::advanced::byte(&b.page.text, b.view.head);
        memchr::memchr_iter(b'\n', &b.page.text.as_bytes()[..at]).count()
    }
    /// Ctrl+K, Ctrl+. / Ctrl+K, Ctrl+,: the next or previous changed block.
    pub(super) fn next_change(&mut self, ctx: &egui::Context, forward: bool) {
        let Some(changes) = self.active_changes() else {
            self.notice = "No change markers for this file.".into();
            return;
        };
        let line = self.cursor_line();
        let next = if forward {
            changes.iter().find(|c| c.line > line).or(changes.first())
        } else {
            changes
                .iter()
                .rev()
                .find(|c| c.line < line)
                .or(changes.last())
        };
        match next {
            Some(change) => {
                let line = change.line + 1;
                self.navigation_mark();
                self.jump_to_line(ctx, line, None);
            }
            None => self.notice = "No changes.".into(),
        }
    }
    /// Ctrl+K, Ctrl+Z: put the changed block at the cursor back as it was.
    pub(super) fn revert_change(&mut self) {
        let Some(changes) = self.active_changes() else {
            self.notice = "No change markers for this file.".into();
            return;
        };
        let text = &self.active().page.text;
        let head = hedit::advanced::byte(text, self.active().view.head);
        let line_start = text[..head].rfind('\n').map_or(0, |n| n + 1);
        // The block on the cursor's line: one it covers, or a deletion there.
        let Some(change) = changes.into_iter().find(|c| {
            (c.bytes.start <= head && head < c.bytes.end)
                || (line_start <= c.bytes.start && c.bytes.start <= head.max(line_start))
        }) else {
            self.notice = "The cursor is not on a change.".into();
            return;
        };
        let mut reverted = String::with_capacity(text.len());
        reverted.push_str(&text[..change.bytes.start]);
        reverted.push_str(&change.old);
        reverted.push_str(&text[change.bytes.end..]);
        let cursor = text[..change.bytes.start].chars().count();
        self.apply_edit(
            hedit::editing::Edit {
                text: reverted,
                anchor: cursor,
                head: cursor,
            },
            None,
        );
    }
}
