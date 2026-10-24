//! Spell checking as you type (F6): a worker thread checks the active file a
//! moment after edits stop, and the editor underlines what it found.
use super::*;
use hedit::syntax::grammar::Grammar;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::mpsc,
};

/// Check again this long after the last edit.
const DELAY: Duration = Duration::from_millis(300);
/// Larger files are not checked.
const MAX_TEXT: usize = 8 * 1024 * 1024;

struct Request {
    view: u64,
    revision: u64,
    text: Arc<String>,
    language: syntax::Language,
    known: Arc<HashSet<String>>,
}

struct Reply {
    view: u64,
    revision: u64,
    text: Arc<String>,
    ranges: Vec<Range<usize>>,
}

/// Misspelled words in one view, as character ranges.
struct Checked {
    revision: u64,
    /// The text that was checked, to move the ranges after later edits.
    text: Arc<String>,
    ranges: Vec<Range<usize>>,
    /// `ranges` moved to a later revision.
    moved: Option<(u64, Vec<Range<usize>>)>,
}

/// What the right-click menu offers for a misspelled word.
pub(super) struct WordMenu {
    view: u64,
    range: Range<usize>,
    pub word: String,
    pub suggestions: Vec<String>,
}

pub(super) enum SpellAction {
    Replace(String),
    Add,
    Ignore,
}

#[derive(Default)]
pub(super) struct SpellCheck {
    worker: Option<(mpsc::Sender<Request>, mpsc::Receiver<Reply>)>,
    results: HashMap<u64, Checked>,
    /// The view and revision being checked now.
    pub(super) pending: Option<(u64, u64)>,
    /// The revision seen last and when it changed.
    seen: (u64, u64, Option<Instant>),
    /// Words ignored until hedit quits.
    pub ignored: HashSet<String>,
    /// Added and ignored words the results were made with.
    known: Arc<HashSet<String>>,
    pub menu: Option<WordMenu>,
}

fn spawn_worker(ctx: &egui::Context) -> (mpsc::Sender<Request>, mpsc::Receiver<Reply>) {
    let (tx, requests) = mpsc::channel::<Request>();
    let (replies, rx) = mpsc::channel();
    let ctx = ctx.clone();
    thread::spawn(move || {
        // Each view keeps its highlighting state, so rechecks only parse changes.
        let mut grammars: HashMap<u64, Grammar> = HashMap::new();
        while let Ok(mut request) = requests.recv() {
            // Only the newest request matters.
            while let Ok(newer) = requests.try_recv() {
                request = newer;
            }
            if grammars.len() > 4 && !grammars.contains_key(&request.view) {
                grammars.clear();
            }
            let grammar = grammars.entry(request.view).or_default();
            let regions = hedit::spelling::regions(&request.text, request.language, grammar);
            let ranges = hedit::spelling::misspelled(&request.text, &regions, &request.known);
            let reply = Reply {
                view: request.view,
                revision: request.revision,
                text: request.text,
                ranges,
            };
            if replies.send(reply).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    });
    (tx, rx)
}

/// Move ranges found in `old` to `new`: ranges before or after the edited
/// part shift, and ranges touching it are dropped until the next check.
fn move_ranges(old: &str, new: &str, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let prefix = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .count();
    let (old_len, new_len) = (old.chars().count(), new.chars().count());
    let room = old_len.min(new_len) - prefix;
    let suffix = old
        .chars()
        .rev()
        .zip(new.chars().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    let old_end = old_len - suffix;
    ranges
        .iter()
        .filter_map(|r| {
            if r.end < prefix {
                Some(r.clone())
            } else if r.start > old_end {
                Some(r.start + new_len - old_len..r.end + new_len - old_len)
            } else {
                None
            }
        })
        .collect()
}

impl HeditApp {
    /// The words that count as spelled: added in settings, or ignored.
    fn known_words(&self) -> HashSet<String> {
        self.settings
            .added_words
            .iter()
            .chain(&self.spell.ignored)
            .map(|w| w.to_lowercase())
            .collect()
    }
    /// Each frame: collect results and ask for a check when the file changed.
    pub(super) fn update_spelling(&mut self, ctx: &egui::Context) {
        if !self.settings.spell_check {
            self.spell.results.clear();
            self.spell.menu = None;
            return;
        }
        if self.spell.worker.is_none() {
            self.spell.worker = Some(spawn_worker(ctx));
        }
        let replies: Vec<Reply> = self
            .spell
            .worker
            .as_ref()
            .map(|(_, rx)| rx.try_iter().collect())
            .unwrap_or_default();
        for reply in replies {
            if self.spell.pending == Some((reply.view, reply.revision)) {
                self.spell.pending = None;
            }
            self.spell.results.insert(
                reply.view,
                Checked {
                    revision: reply.revision,
                    text: reply.text,
                    ranges: reply.ranges,
                    moved: None,
                },
            );
        }
        let known = self.known_words();
        if *self.spell.known != known {
            self.spell.known = Arc::new(known);
            self.spell.results.clear();
            self.spell.pending = None;
        }
        let b = &self.workspace.open[self.workspace.active];
        let (view, revision) = (b.view.id, b.page.info.revision);
        if b.page.info.paged || b.page.text.len() > MAX_TEXT {
            return;
        }
        if (self.spell.seen.0, self.spell.seen.1) != (view, revision) {
            self.spell.seen = (view, revision, Some(Instant::now()));
        }
        let current = self
            .spell
            .results
            .get(&view)
            .is_some_and(|c| c.revision == revision);
        if current || self.spell.pending == Some((view, revision)) {
            return;
        }
        // A file not checked yet goes at once; edits wait for a pause.
        let first = !self.spell.results.contains_key(&view);
        let waited = self.spell.seen.2.map_or(DELAY, |at| at.elapsed());
        if !first && waited < DELAY {
            ctx.request_repaint_after(DELAY - waited);
            return;
        }
        let request = Request {
            view,
            revision,
            text: Arc::new(b.page.text.clone()),
            language: b.language(),
            known: Arc::clone(&self.spell.known),
        };
        let sent = self
            .spell
            .worker
            .as_ref()
            .is_some_and(|(tx, _)| tx.send(request).is_ok());
        if sent {
            self.spell.pending = Some((view, revision));
        }
    }
    /// The revision a view's results are for.
    #[cfg(test)]
    pub(super) fn spell_checked(&self, view: u64) -> Option<u64> {
        self.spell.results.get(&view).map(|c| c.revision)
    }
    /// Misspelled character ranges for a view, moved to its current text.
    pub(super) fn misspellings(&mut self, view: u64) -> Option<&[Range<usize>]> {
        let b = self.workspace.open.iter().find(|b| b.view.id == view)?;
        let checked = self.spell.results.get_mut(&view)?;
        if checked.revision == b.page.info.revision {
            return Some(&checked.ranges);
        }
        let revision = b.page.info.revision;
        if checked.moved.as_ref().is_none_or(|(r, _)| *r != revision) {
            let moved = move_ranges(&checked.text, &b.page.text, &checked.ranges);
            checked.moved = Some((revision, moved));
        }
        checked.moved.as_ref().map(|(_, ranges)| ranges.as_slice())
    }
    /// The misspelled word at a character position, for the right-click menu.
    pub(super) fn word_menu_at(&mut self, view: u64, at: usize) {
        self.spell.menu = None;
        let Some(range) = self.misspellings(view).and_then(|ranges| {
            ranges
                .iter()
                .find(|r| r.start <= at && at <= r.end)
                .cloned()
        }) else {
            return;
        };
        let Some(b) = self.workspace.open.iter().find(|b| b.view.id == view) else {
            return;
        };
        let word: String = b
            .page
            .text
            .chars()
            .skip(range.start)
            .take(range.end - range.start)
            .collect();
        let suggestions = hedit::spelling::suggestions(&word);
        self.spell.menu = Some(WordMenu {
            view,
            range,
            word,
            suggestions,
        });
    }
    /// Apply a choice from the right-click menu's spelling entries.
    pub(super) fn spell_action(&mut self, action: SpellAction) {
        let Some(menu) = self.spell.menu.take() else {
            return;
        };
        match action {
            SpellAction::Replace(word) => {
                if self.active().view.id != menu.view {
                    return;
                }
                let batch = hedit::advanced::replace(
                    &self.active().page.text,
                    &[hedit::advanced::Selection {
                        anchor: menu.range.start,
                        head: menu.range.end,
                    }],
                    &word,
                );
                self.batch(batch);
            }
            SpellAction::Add => {
                if !self.settings.added_words.contains(&menu.word) {
                    self.settings.added_words.push(menu.word);
                }
            }
            SpellAction::Ignore => {
                self.spell.ignored.insert(menu.word);
            }
        }
    }
    /// Ctrl+F6 / Ctrl+Shift+F6: select the next or previous misspelled word.
    pub(super) fn next_misspelling(&mut self, forward: bool) {
        let view = self.active().view.id;
        let head = self.active().view.head;
        let Some(ranges) = self.misspellings(view).map(<[_]>::to_vec) else {
            self.notice = "Spelling has not been checked yet.".into();
            return;
        };
        let next = if forward {
            ranges.iter().find(|r| r.start > head).or(ranges.first())
        } else {
            ranges.iter().rev().find(|r| r.end < head).or(ranges.last())
        };
        match next.cloned() {
            Some(range) => {
                self.navigation_mark();
                self.set_selections(vec![hedit::advanced::Selection {
                    anchor: range.start,
                    head: range.end,
                }]);
            }
            None => self.notice = "No misspelled words.".into(),
        }
    }
}
