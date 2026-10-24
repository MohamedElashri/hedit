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
    fn cursor_line(&self) -> usize {
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
            changes
                .iter()
                .find(|c| c.new.start > line)
                .or(changes.first())
        } else {
            changes
                .iter()
                .rev()
                .find(|c| c.new.end.max(c.new.start + 1) <= line)
                .or(changes.last())
        };
        match next {
            Some(change) => {
                let line = change.new.start + 1;
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
        let line = self.cursor_line();
        let Some(change) = changes
            .into_iter()
            .find(|c| c.new.contains(&line) || (c.new.is_empty() && c.new.start == line))
        else {
            self.notice = "The cursor is not on a change.".into();
            return;
        };
        let text = &self.active().page.text;
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let start: usize = lines[..change.new.start].iter().map(|l| l.len()).sum();
        let end: usize = start
            + lines[change.new.clone()]
                .iter()
                .map(|l| l.len())
                .sum::<usize>();
        let mut reverted = String::with_capacity(text.len());
        reverted.push_str(&text[..start]);
        reverted.push_str(&change.old);
        reverted.push_str(&text[end..]);
        let cursor = text[..start].chars().count();
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
