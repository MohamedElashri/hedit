//! Completions offered while typing, in a list at the cursor. Words come from
//! an index of the open files, built on another thread a moment after edits
//! stop, so typing never waits for it.
use super::*;
use hedit::completion::{self, Word};
use std::sync::mpsc;

/// Rows shown in the list.
const SHOWN: usize = 12;
/// Index the open files again this long after the last edit.
const REINDEX_AFTER: Duration = Duration::from_millis(750);
/// Letters typed before the list opens by itself.
const MIN_TYPED: usize = 2;

#[derive(Clone, Debug)]
pub(super) enum Choice {
    Word(String),
    Snippet(hedit::intelligence::Snippet),
}

pub(super) struct Popup {
    view: u64,
    /// Where the typed word starts, as a character index.
    start: usize,
    typed: String,
    pub(super) choices: Vec<Choice>,
    pub(super) selected: usize,
}

/// The files and revisions indexed, the merged words, and each changed
/// file's own words.
type Built = (Vec<(u64, u64)>, Vec<Word>, Vec<(u64, u64, Arc<Vec<Word>>)>);

#[derive(Default)]
pub(super) struct AutoComplete {
    words: Arc<Vec<Word>>,
    /// Each file's words by file ID, with the revision they came from, so only
    /// changed files are indexed again.
    files: std::collections::HashMap<u64, (u64, Arc<Vec<Word>>)>,
    /// The open files and revisions `words` came from.
    indexed: Vec<(u64, u64)>,
    /// The files and revisions seen last, and when they last changed.
    seen: (Vec<(u64, u64)>, Option<Instant>),
    pub(super) building: Option<mpsc::Receiver<Built>>,
    /// Open the list at this time, after a word character was typed.
    due: Option<Instant>,
    /// Words were typed, so the index is worth keeping; nothing is indexed
    /// before that.
    pub(super) wanted: bool,
    pub(super) popup: Option<Popup>,
}

impl AutoComplete {
    #[cfg(test)]
    pub(super) fn words_len(&self) -> usize {
        self.words.len()
    }
}

impl HeditApp {
    /// The focused pane's editor widget.
    pub(super) fn editor_id(&self) -> egui::Id {
        let b = self.active();
        let id = egui::Id::new(("editor", b.page.info.id, b.page.start));
        if self.panes.mode != 0 && self.panes.focused > 0 {
            id.with(self.panes.focused)
        } else {
            id
        }
    }
    fn completion_key(&self) -> Vec<(u64, u64)> {
        self.workspace
            .open
            .iter()
            .filter(|b| !b.page.info.paged)
            .map(|b| (b.page.info.id, b.page.info.revision))
            .collect()
    }
    /// Index changed files on another thread and merge them with the rest,
    /// the current file first.
    fn build_index(&mut self, ctx: &egui::Context, key: Vec<(u64, u64)>) {
        let active = self.active().page.info.id;
        let mut order = vec![];
        let mut changed = vec![];
        for b in std::iter::once(self.active()).chain(
            self.workspace
                .open
                .iter()
                .filter(|b| b.page.info.id != active),
        ) {
            let (id, revision) = (b.page.info.id, b.page.info.revision);
            if b.page.info.paged {
                continue;
            }
            order.push(id);
            if self
                .auto
                .files
                .get(&id)
                .is_some_and(|(r, _)| *r == revision)
            {
                continue;
            }
            let mut end = b.page.text.len().min(completion::MAX_TEXT);
            while !b.page.text.is_char_boundary(end) {
                end -= 1;
            }
            changed.push((id, revision, b.page.text[..end].to_owned()));
        }
        self.auto.files.retain(|id, _| order.contains(id));
        let known = self.auto.files.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let fresh: Vec<(u64, u64, Arc<Vec<Word>>)> = changed
                .into_iter()
                .map(|(id, revision, text)| (id, revision, Arc::new(completion::index(&[text]))))
                .collect();
            let lists: Vec<&[Word]> = order
                .iter()
                .filter_map(|id| {
                    fresh
                        .iter()
                        .find(|(f, _, _)| f == id)
                        .map(|(_, _, words)| words.as_slice())
                        .or_else(|| known.get(id).map(|(_, words)| words.as_slice()))
                })
                .collect();
            let merged = completion::merge(&lists);
            let _ = tx.send((key, merged, fresh));
            ctx.request_repaint();
        });
        self.auto.building = Some(rx);
    }
    /// Keep the word index current: rebuild it a moment after edits stop.
    fn maintain_index(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.auto.building {
            match rx.try_recv() {
                Ok((key, words, fresh)) => {
                    self.auto.words = Arc::new(words);
                    self.auto.indexed = key;
                    self.auto.building = None;
                    for (id, revision, words) in fresh {
                        self.auto.files.insert(id, (revision, words));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.auto.building = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !self.settings.auto_complete || !self.auto.wanted {
            return;
        }
        let key = self.completion_key();
        if key != self.auto.seen.0 {
            self.auto.seen = (key.clone(), Some(Instant::now()));
        }
        if key == self.auto.indexed || self.auto.building.is_some() {
            return;
        }
        let since = self.auto.seen.1.map_or(REINDEX_AFTER, |at| at.elapsed());
        if since >= REINDEX_AFTER || self.auto.indexed.is_empty() {
            self.build_index(ctx, key);
        } else {
            ctx.request_repaint_after(REINDEX_AFTER - since);
        }
    }
    /// Snippets whose trigger starts with the typed word, then indexed words.
    fn completion_choices(&self, typed: &str) -> Vec<Choice> {
        let language = self.active().language().id();
        let lower = typed.to_lowercase();
        let mut choices: Vec<Choice> = self
            .quality
            .config
            .snippets
            .iter()
            .filter(|s| {
                !s.trigger.is_empty()
                    && s.trigger.to_lowercase().starts_with(&lower)
                    && s.language.as_deref().is_none_or(|l| l == language)
            })
            .take(3)
            .cloned()
            .map(Choice::Snippet)
            .collect();
        choices.extend(
            completion::rank(&self.auto.words, typed, SHOWN)
                .into_iter()
                .map(|w| Choice::Word(w.to_owned())),
        );
        choices.truncate(SHOWN);
        choices
    }
    /// The typed word at the only cursor, or None where completion does not apply.
    fn completion_word(&self) -> Option<(usize, String)> {
        let b = self.active();
        if b.page.info.paged
            || b.follow
            || !b.view.selections.is_empty()
            || b.view.anchor != b.view.head
        {
            return None;
        }
        Some(completion::typed_word(&b.page.text, b.view.head))
    }
    /// Open the list now, as Complete word (Ctrl+Space) does.
    pub(super) fn open_completion(&mut self, ctx: &egui::Context) {
        self.auto.wanted = true;
        if self.auto.words.is_empty() {
            // Nothing indexed yet: index the current file right away.
            let b = self.active();
            let mut end = b.page.text.len().min(completion::MAX_TEXT);
            while !b.page.text.is_char_boundary(end) {
                end -= 1;
            }
            self.auto.words = Arc::new(completion::index(&[&b.page.text[..end]]));
        }
        if !self.show_completion(1) {
            self.notice = "No completions here.".into();
        }
        ctx.request_repaint();
    }
    /// Show the list for the typed word, when there is anything to offer.
    fn show_completion(&mut self, min_typed: usize) -> bool {
        self.auto.due = None;
        let Some((start, typed)) = self.completion_word() else {
            return false;
        };
        if typed.chars().count() < min_typed {
            return false;
        }
        let choices = self.completion_choices(&typed);
        if choices.is_empty() {
            self.auto.popup = None;
            return false;
        }
        self.auto.popup = Some(Popup {
            view: self.active().view.id,
            start,
            typed,
            choices,
            selected: 0,
        });
        true
    }
    /// Each frame, before the editor: follow edits made last frame and note
    /// what is typed now.
    pub(super) fn update_completion(&mut self, ctx: &egui::Context) {
        self.maintain_index(ctx);
        if let Some(popup) = &self.auto.popup {
            let (view, start, typed) = (popup.view, popup.start, popup.typed.clone());
            match self.completion_word() {
                Some((now, word))
                    if view == self.active().view.id && now == start && !word.is_empty() =>
                {
                    if word != typed {
                        let choices = self.completion_choices(&word);
                        if choices.is_empty() {
                            self.auto.popup = None;
                        } else if let Some(popup) = &mut self.auto.popup {
                            popup.typed = word;
                            popup.choices = choices;
                            popup.selected = 0;
                        }
                    }
                }
                _ => self.auto.popup = None,
            }
        }
        if let Some(due) = self.auto.due {
            if Instant::now() >= due {
                self.show_completion(MIN_TYPED);
            } else {
                ctx.request_repaint_after(due - Instant::now());
            }
        }
        // Only typing in the editor counts, not in Find or other fields.
        if !self.settings.auto_complete || !ctx.memory(|m| m.has_focus(self.editor_id())) {
            return;
        }
        // Typing a word character schedules the list; anything else closes it.
        let typed: Vec<String> = ctx.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Text(t) | egui::Event::Ime(egui::ImeEvent::Commit(t)) => {
                        Some(t.clone())
                    }
                    _ => None,
                })
                .collect()
        });
        if typed.is_empty() {
            return;
        }
        if typed
            .iter()
            .all(|t| t.chars().all(completion::is_word_char))
        {
            self.auto.wanted = true;
            if self.auto.popup.is_none() {
                let delay = Duration::from_millis(self.settings.auto_complete_delay.into());
                self.auto.due = Some(Instant::now() + delay);
                ctx.request_repaint_after(delay);
            }
        } else {
            self.auto.popup = None;
            self.auto.due = None;
        }
    }
    /// Keys for the open list; they are taken before the editor sees them.
    pub(super) fn completion_keys(&mut self, ctx: &egui::Context) {
        let Some(popup) = &mut self.auto.popup else {
            return;
        };
        let count = popup.choices.len();
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::ArrowDown) {
            popup.selected = (popup.selected + 1) % count;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::ArrowUp) {
            popup.selected = (popup.selected + count - 1) % count;
        }
        let enter = !self.settings.auto_complete_commit_on_tab
            && tab_switch::take_exact(ctx, Modifiers::NONE, Key::Enter);
        if enter || tab_switch::take_exact(ctx, Modifiers::NONE, Key::Tab) {
            let selected = popup.selected;
            self.commit_choice(selected);
        } else if self.settings.auto_complete_commit_on_tab
            && ctx.input(|i| i.key_pressed(Key::Enter))
        {
            self.auto.popup = None;
        }
    }
    /// Replace the typed word with a choice from the list.
    pub(super) fn commit_choice(&mut self, index: usize) {
        let Some(popup) = self.auto.popup.take() else {
            return;
        };
        self.auto.due = None;
        let Some(choice) = popup.choices.get(index).cloned() else {
            return;
        };
        if self.completion_word().map(|(start, _)| start) != Some(popup.start) {
            return;
        }
        match choice {
            Choice::Word(word) => {
                let head = self.active().view.head;
                let batch = hedit::advanced::replace(
                    &self.active().page.text,
                    &[hedit::advanced::Selection {
                        anchor: popup.start,
                        head,
                    }],
                    &word,
                );
                self.batch(batch);
            }
            Choice::Snippet(snippet) => self.insert_snippet(snippet, popup.typed.len()),
        }
        self.focus_editor = true;
    }
    /// Draw the list below the cursor, or above it near the bottom.
    pub(super) fn completion_popup(&mut self, ctx: &egui::Context) {
        let (Some(popup), Some(caret)) = (&self.auto.popup, self.caret_screen) else {
            return;
        };
        let below = caret.bottom() < ctx.screen_rect().height() * 0.6;
        let (pivot, at) = if below {
            (
                egui::Align2::LEFT_TOP,
                caret.left_bottom() + egui::vec2(0.0, 2.0),
            )
        } else {
            (
                egui::Align2::LEFT_BOTTOM,
                caret.left_top() - egui::vec2(0.0, 2.0),
            )
        };
        let theme = self.theme.clone();
        let mut clicked = None;
        let font = FontId::monospace(self.settings.font_size - 1.0);
        egui::Area::new(egui::Id::new("completion-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .pivot(pivot)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.panel.color())
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.set_min_width(220.0);
                        for (index, choice) in popup.choices.iter().enumerate() {
                            let (label, detail) = match choice {
                                Choice::Word(word) => (word.as_str(), ""),
                                Choice::Snippet(s) => (s.trigger.as_str(), s.description.as_str()),
                            };
                            let response = ui.add(
                                egui::Button::new(RichText::new(label).font(font.clone()))
                                    .selected(index == popup.selected)
                                    .shortcut_text(detail)
                                    .frame(index == popup.selected)
                                    .min_size(egui::vec2(ui.available_width(), 0.0)),
                            );
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
            });
        if let Some(index) = clicked {
            self.commit_choice(index);
        }
    }
}
