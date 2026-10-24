//! Find shortcuts that work from the editor: select every match (Alt+Enter),
//! use the selection as the Find or Replace text (Ctrl+E, Ctrl+Shift+E), and
//! jump to the next copy of the word at the cursor (Ctrl+F3).
use super::*;
use hedit::advanced::{self, Selection};
use hedit::search;

/// Character ranges for sorted byte ranges of `text`, counted in one pass.
fn char_ranges(text: &str, bytes: &[std::ops::Range<usize>]) -> Vec<Selection> {
    let mut out = Vec::with_capacity(bytes.len());
    let (mut at_byte, mut at_char) = (0, 0);
    let mut advance = |to: usize| {
        at_char += text[at_byte..to].chars().count();
        at_byte = to;
        at_char
    };
    for range in bytes {
        let anchor = advance(range.start);
        let head = advance(range.end);
        out.push(Selection { anchor, head });
    }
    out
}

impl HeditApp {
    /// The selected text, or the word at the cursor with its range.
    fn selection_or_word(&self) -> Option<(String, Selection)> {
        let b = self.active();
        let range = if b.view.anchor != b.view.head {
            Selection {
                anchor: b.view.anchor,
                head: b.view.head,
            }
        } else {
            advanced::word(&b.page.text, b.view.head)
        };
        let start = advanced::byte(&b.page.text, range.start());
        let end = start + advanced::byte(&b.page.text[start..], range.end() - range.start());
        let text = &b.page.text[start..end];
        (!text.is_empty()).then(|| (text.to_owned(), range))
    }
    /// Text for the Find field that matches `text` literally, also in regex mode.
    fn literal_query(&self, text: &str) -> String {
        if self.quality.options.regex {
            regex::escape(text)
        } else {
            text.to_owned()
        }
    }
    /// Alt+Enter: a cursor on every match of the Find text.
    pub(super) fn select_all_matches(&mut self) {
        if self.query.is_empty() {
            self.notice = "Enter text in Find first.".into();
            return;
        }
        let b = self.active();
        if b.page.info.paged {
            self.notice = "Selecting all matches needs the whole file loaded.".into();
            return;
        }
        // In “selection only” mode, matches inside the captured selection.
        let scope = match self.quality.scope {
            Some((id, a, z)) if self.quality.selection_only && id == b.view.id => {
                let start = b.page.start as usize;
                a.saturating_sub(start).min(b.page.text.len())
                    ..z.saturating_sub(start).min(b.page.text.len())
            }
            _ => 0..b.page.text.len(),
        };
        match search::matches(&b.page.text, &self.query, &self.quality.options, scope) {
            Ok(found) if found.is_empty() => self.notice = "No matches found.".into(),
            Ok(found) => {
                let total = found.len();
                let ranges = char_ranges(&self.active().page.text, &found);
                self.set_selections(ranges);
                self.find_focus = false;
                self.notice = if total > advanced::MAX_SELECTIONS {
                    format!("Selected the first 1,000 of {total} matches.")
                } else {
                    format!("Selected {total} matches.")
                };
            }
            Err(error) => self.notice = error,
        }
    }
    /// Ctrl+E / Ctrl+Shift+E: the selection, or the word at the cursor, becomes
    /// the Find or Replace text.
    pub(super) fn use_selection(&mut self, replace: bool) {
        let Some((text, _)) = self.selection_or_word() else {
            self.notice = "Select text first.".into();
            return;
        };
        // With regex on, the text must match literally, and `$` in a
        // replacement would otherwise name a capture group.
        let text = match (self.quality.options.regex, replace) {
            (true, false) => regex::escape(&text),
            (true, true) => text.replace('$', "$$"),
            (false, _) => text,
        };
        if replace {
            self.notice = format!("Replace with “{text}”.");
            self.replacement = text;
        } else {
            self.notice = format!("Find “{text}”.");
            self.query = text;
            let b = self.active();
            self.live_find.origin = Some((b.view.id, b.view.anchor.max(b.view.head)));
        }
    }
    /// Ctrl+F3 / Ctrl+Shift+F3: select the word at the cursor, then each press
    /// selects its next (or previous) copy, wrapping around the file.
    pub(super) fn quick_find(&mut self, forward: bool) {
        let Some((needle, range)) = self.selection_or_word() else {
            return;
        };
        let b = self.active();
        let whole_word = b.view.anchor == b.view.head;
        if whole_word {
            // The first press selects the word itself.
            self.query = self.literal_query(&needle);
            self.set_selections(vec![range]);
            return;
        }
        let options = search::Options {
            regex: false,
            case_sensitive: true,
            whole_word: false,
        };
        let text = &b.page.text;
        let Ok(found) = search::matches(text, &needle, &options, 0..text.len()) else {
            return;
        };
        let here = advanced::byte(text, range.start());
        let next = if forward {
            found.iter().find(|m| m.start > here).or(found.first())
        } else {
            found.iter().rev().find(|m| m.start < here).or(found.last())
        };
        if let Some(next) = next.cloned() {
            let selected = char_ranges(text, &[next]);
            self.query = self.literal_query(&needle);
            self.navigation_mark();
            self.set_selections(selected);
        }
    }
}
