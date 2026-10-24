//! Text copied or cut in hedit, to paste again from a list (Ctrl+K, Ctrl+V).
use super::*;

/// How many copies are kept.
const LIMIT: usize = 15;

/// A one-line label for a history entry, and its size.
pub(super) fn label(text: &str) -> (String, String) {
    let first = text.trim().lines().next().unwrap_or("");
    let mut shown: String = first.chars().take(80).collect();
    if first.chars().count() > 80 {
        shown.push('…');
    }
    let lines = text.lines().count();
    let detail = if lines > 1 {
        format!("{lines} lines")
    } else {
        format!("{} characters", text.chars().count())
    };
    (shown, detail)
}

impl HeditApp {
    /// Remember text copied or cut this frame, newest first.
    pub(super) fn record_copies(&mut self, ctx: &egui::Context) {
        let copied: Vec<String> = ctx.output(|o| {
            o.commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(text) if !text.trim().is_empty() => {
                        Some(text.clone())
                    }
                    _ => None,
                })
                .collect()
        });
        for text in copied {
            self.paste_history.retain(|t| *t != text);
            self.paste_history.insert(0, text);
            self.paste_history.truncate(LIMIT);
        }
    }
    /// Paste an earlier copy at every cursor. It also goes back on the
    /// clipboard, so it becomes the newest entry.
    pub(super) fn paste_entry(&mut self, ctx: &egui::Context, text: String) {
        if self.active().follow || self.active().page.info.paged {
            return;
        }
        let batch = hedit::advanced::replace(&self.active().page.text, &self.selections(), &text);
        self.batch(batch);
        ctx.copy_text(text);
    }
}
