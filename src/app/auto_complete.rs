//! Bounded completion indexing and ranking, with guarded background results.
use super::*;
use hedit::completion::{self, Word};
use std::sync::mpsc;
const SHOWN: usize = 12;
const REINDEX_AFTER: Duration = Duration::from_millis(750);
const MIN_TYPED: usize = 2;
const INLINE_TEXT: usize = 64 * 1024;
const INLINE_WORDS: usize = 4096;
const SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
const SNAPSHOT_FILES: usize = 32;
#[derive(Clone, Debug)]
pub(super) enum Choice {
    Word(String),
    Language(language_servers::Completion),
    Snippet(hedit::intelligence::Snippet),
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Query {
    group: u64,
    view: u64,
    revision: u64,
    pane: usize,
    start: usize,
    head: usize,
    typed: String,
}
pub(super) struct Popup {
    query: Query,
    version: u64,
    locked: bool,
    pub(super) choices: Vec<Choice>,
    pub(super) selected: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Source {
    id: u64,
    revision: u64,
    start: u64,
    end: usize,
}
type IndexKey = Vec<Source>;
#[derive(Clone, Copy)]
struct Scheduled {
    group: u64,
    at: Instant,
    view: u64,
    pane: usize,
    revision: u64,
    head: usize,
}

pub(super) struct Job<T> {
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<T>,
}
impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
pub(super) struct Build {
    key: IndexKey,
    words: Option<completion::Indexed>,
}
struct Ranked {
    query: Query,
    version: u64,
    words: Option<Vec<String>>,
}
struct WordCache {
    view: u64,
    revision: u64,
    head: usize,
    prefix: Option<(usize, String)>,
}
#[derive(Default)]
pub(super) struct AutoComplete {
    words: Arc<Vec<Word>>,
    indexed: Option<IndexKey>,
    seen: Option<(IndexKey, Instant)>,
    pub(super) building: Option<Job<Build>>,
    building_key: IndexKey,
    ranking: Option<Job<Ranked>>,
    ranking_key: Option<(Query, u64)>,
    version: u64,
    due: Option<Scheduled>,
    intent: Option<Query>,
    word: Option<WordCache>,
    pub(super) wanted: bool,
    pub(super) popup: Option<Popup>,
    limited: bool,
    started: bool,
}
impl AutoComplete {
    #[cfg(test)]
    pub(super) fn words_len(&self) -> usize {
        self.words.len()
    }
    pub(super) fn active(&self) -> bool {
        self.visible() || self.due.is_some()
    }
    pub(super) fn visible(&self) -> bool {
        self.popup.is_some() || self.intent.is_some()
    }
    pub(super) fn dismiss(&mut self) {
        self.popup = None;
        self.intent = None;
        self.due = None;
        if let Some(job) = &self.ranking {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }
}
impl HeditApp {
    pub(super) fn editor_id(&self) -> egui::Id {
        let b = self.active();
        let id = egui::Id::new(("editor", b.page.info.id, b.page.start));
        if self.panes.mode != 0 && self.panes.focused > 0 {
            id.with(("pane", self.panes.focused))
        } else {
            id
        }
    }
    fn completion_available(&self) -> bool {
        !self.busy
            && !self.closing
            && !self.source_control.open
            && !self.palette
            && !self.preferences_open
            && !self.goto_open
            && !self.about_open
            && !self.guide.open
            && self.languages.info.is_none()
            && self.rename_target.is_none()
            && !self.shelf_open
            && !self.bookmarks_open
            && !self.updates.open
            && self.project.create.is_none()
            && self.quality.file_action.is_none()
            && self.workspace.remote.as_ref().is_none_or(|r| r.writable())
    }
    /// Active file first, with one global snapshot budget across all open files.
    fn completion_key(&self) -> IndexKey {
        let active = self.active().view.id;
        let mut left = SNAPSHOT_BYTES;
        std::iter::once(self.active())
            .chain(self.workspace.open.iter().filter(|b| b.view.id != active))
            .filter(|b| !b.page.info.paged && !b.follow)
            .take(SNAPSHOT_FILES)
            .filter_map(|b| {
                if left == 0 {
                    return None;
                }
                let mut end = b.page.text.len().min(completion::MAX_TEXT).min(left);
                while !b.page.text.is_char_boundary(end) {
                    end -= 1;
                }
                left -= end;
                Some(Source {
                    id: b.view.id,
                    revision: b.page.info.revision,
                    start: b.page.start,
                    end,
                })
            })
            .collect()
    }
    fn publish_index(&mut self, key: IndexKey, index: completion::Indexed) {
        self.auto.limited |= index.limited;
        self.auto.words = Arc::new(index.words);
        self.auto.indexed = Some(key);
        self.auto.version = self.auto.version.wrapping_add(1);
    }
    fn build_index(&mut self, ctx: &egui::Context, key: IndexKey) {
        self.auto.started = true;
        let snapshots: Vec<String> = key
            .iter()
            .filter_map(|source| {
                self.workspace
                    .open
                    .iter()
                    .find(|b| b.view.id == source.id)
                    .map(|b| b.page.text[..source.end].to_owned())
            })
            .collect();
        self.auto.limited = key.len()
            < self
                .workspace
                .open
                .iter()
                .filter(|b| !b.page.info.paged && !b.follow)
                .count()
            || key.iter().any(|s| {
                self.workspace
                    .open
                    .iter()
                    .any(|b| b.view.id == s.id && b.page.text.len() > s.end)
            });
        if snapshots.iter().map(String::len).sum::<usize>() <= INLINE_TEXT {
            self.publish_index(
                key,
                completion::index_cancellable(&snapshots, || false).unwrap(),
            );
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let ctx = ctx.clone();
        self.auto.building_key = key.clone();
        thread::spawn(move || {
            let words = completion::index_cancellable(&snapshots, || stop.load(Ordering::Relaxed));
            let _ = tx.send(Build { key, words });
            ctx.request_repaint();
        });
        self.auto.building = Some(Job { cancel, rx });
    }
    fn maintain_index(&mut self, ctx: &egui::Context, force: bool) {
        let key = self.completion_key();
        let first = !self.auto.started;
        if self.auto.seen.as_ref().is_none_or(|(old, _)| old != &key) {
            let scope_changed = self.auto.seen.as_ref().is_some_and(|(old, _)| {
                old.iter()
                    .map(|s| (s.id, s.start))
                    .ne(key.iter().map(|s| (s.id, s.start)))
            });
            if scope_changed {
                self.auto.words = Arc::default();
                self.auto.indexed = None;
                self.auto.word = None;
                self.auto.version = self.auto.version.wrapping_add(1);
                self.auto.popup = None;
            }
            self.auto.seen = Some((key.clone(), Instant::now()));
        }
        let enabled =
            (self.settings.auto_complete && self.auto.wanted) || self.auto.intent.is_some();
        if let Some(job) = &self.auto.building {
            if !enabled || self.auto.building_key != key {
                job.cancel.store(true, Ordering::Relaxed);
            }
            match job.rx.try_recv() {
                Ok(built) => {
                    let cancelled = job.cancel.load(Ordering::Relaxed);
                    self.auto.building = None;
                    if enabled && !cancelled && built.key == key {
                        if let Some(words) = built.words {
                            self.publish_index(built.key, words);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.auto.building = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if !enabled {
            if self.auto.indexed.is_some() || !self.auto.words.is_empty() {
                self.auto.words = Arc::default();
                self.auto.indexed = None;
                self.auto.word = None;
                self.auto.version = self.auto.version.wrapping_add(1);
            }
            return;
        }
        if self.auto.indexed.as_ref() == Some(&key) || self.auto.building.is_some() {
            return;
        }
        let since = self
            .auto
            .seen
            .as_ref()
            .map_or(REINDEX_AFTER, |(_, at)| at.elapsed());
        if force || first || since >= REINDEX_AFTER {
            self.build_index(ctx, key);
        } else {
            ctx.request_repaint_after(REINDEX_AFTER - since);
        }
    }
    fn completion_choices(&self, typed: &str, words: Vec<String>) -> Vec<Choice> {
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
        choices.extend(words.into_iter().map(Choice::Word));
        choices.truncate(SHOWN);
        choices
    }
    fn completion_word(&mut self) -> Option<(usize, String)> {
        let b = self.active();
        if b.page.info.paged
            || b.follow
            || !b.view.selections.is_empty()
            || b.view.anchor != b.view.head
        {
            return None;
        }
        let key = (b.view.id, b.page.info.revision, b.view.head);
        if let Some(cached) = &self.auto.word {
            if (cached.view, cached.revision, cached.head) == key {
                return cached.prefix.clone();
            }
        }
        let i = self.workspace.active;
        let b = &self.workspace.open[i];
        let pane = if self.panes.mode == 0 {
            0
        } else {
            self.panes.focused
        };
        let index = self.projections[pane].source_index(&b.page.text);
        let head = b.view.head.min(index.chars);
        let word = completion::typed_word_at(&b.page.text, index.byte(&b.page.text, head), head);
        self.auto.word = Some(WordCache {
            view: key.0,
            revision: key.1,
            head: key.2,
            prefix: word.clone(),
        });
        word
    }
    fn completion_query(&mut self) -> Option<Query> {
        if !self.completion_available() {
            return None;
        }
        let (start, typed) = self.completion_word()?;
        if typed.is_empty() {
            return None;
        }
        let b = self.active();
        Some(Query {
            group: self.panes.focused_id(),
            view: b.view.id,
            revision: b.page.info.revision,
            pane: if self.panes.mode == 0 {
                0
            } else {
                self.panes.focused
            },
            start,
            head: b.view.head,
            typed,
        })
    }
    fn publish_choices(&mut self, query: Query, words: Vec<String>) {
        if self
            .auto
            .popup
            .as_ref()
            .is_some_and(|p| p.query == query && p.locked)
        {
            return;
        }
        let mut choices = self.completion_choices(&query.typed, words);
        if let Some(popup) = &self.auto.popup {
            if popup.query == query {
                let language: Vec<_> = popup
                    .choices
                    .iter()
                    .filter(|c| matches!(c, Choice::Language(_)))
                    .cloned()
                    .collect();
                if !language.is_empty() {
                    choices.splice(0..0, language);
                    choices.truncate(SHOWN);
                }
            }
        }
        if choices.is_empty() {
            self.auto.popup = None;
            if !self.language_completion_pending() {
                self.auto.intent = None;
            }
        } else {
            self.auto.popup = Some(Popup {
                query,
                version: self.auto.version,
                choices,
                selected: 0,
                locked: false,
            });
        }
    }
    fn refresh_choices(&mut self, ctx: &egui::Context) {
        if self.auto.intent.is_some() {
            self.request_language_completion(ctx);
        }
        if let Some(job) = &self.auto.ranking {
            let current = self.auto.ranking_key.as_ref().is_some_and(|(q, v)| {
                self.auto.intent.as_ref() == Some(q) && *v == self.auto.version
            });
            if !current {
                job.cancel.store(true, Ordering::Relaxed);
            }
            match job.rx.try_recv() {
                Ok(ranked) => {
                    let cancelled = job.cancel.load(Ordering::Relaxed);
                    self.auto.ranking = None;
                    self.auto.ranking_key = None;
                    if current
                        && !cancelled
                        && ranked.version == self.auto.version
                        && self.auto.intent.as_ref() == Some(&ranked.query)
                    {
                        if let Some(words) = ranked.words {
                            self.publish_choices(ranked.query, words);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.auto.ranking = None;
                    self.auto.ranking_key = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let Some(query) = self.auto.intent.clone() else {
            return;
        };
        if self
            .auto
            .popup
            .as_ref()
            .is_some_and(|p| p.query == query && (p.version == self.auto.version || p.locked))
        {
            return;
        }
        if self.auto.indexed.is_none() {
            // Snippets are immediately available even while word indexing runs.
            let choices = self.completion_choices(&query.typed, vec![]);
            if !choices.is_empty() && self.auto.popup.is_none() {
                self.auto.popup = Some(Popup {
                    query,
                    version: self.auto.version.wrapping_sub(1),
                    choices,
                    selected: 0,
                    locked: false,
                });
            }
            return;
        }
        if self.auto.words.len() <= INLINE_WORDS {
            let words = completion::rank(&self.auto.words, &query.typed, SHOWN)
                .into_iter()
                .map(str::to_owned)
                .collect();
            self.publish_choices(query, words);
        } else if self.auto.ranking.is_none() {
            let words = Arc::clone(&self.auto.words);
            let version = self.auto.version;
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = Arc::clone(&cancel);
            let (tx, rx) = mpsc::sync_channel(1);
            let ctx = ctx.clone();
            self.auto.ranking_key = Some((query.clone(), version));
            thread::spawn(move || {
                let words = completion::rank_cancellable(&words, &query.typed, SHOWN, || {
                    stop.load(Ordering::Relaxed)
                })
                .map(|words| words.into_iter().map(str::to_owned).collect());
                let _ = tx.send(Ranked {
                    query,
                    version,
                    words,
                });
                ctx.request_repaint();
            });
            self.auto.ranking = Some(Job { cancel, rx });
        }
    }
    #[cfg(test)]
    pub(super) fn prepare_completion_index(&mut self, ctx: &egui::Context) {
        self.auto.wanted = true;
        self.maintain_index(ctx, true);
    }
    pub(super) fn open_completion(&mut self, ctx: &egui::Context) {
        self.auto.dismiss();
        self.reset_language_completion();
        let Some(query) = self.completion_query() else {
            self.notice = "No completions here.".into();
            return;
        };
        // Menus and the command palette run after the editor. Transfer focus
        // now so the next frame preserves this explicit completion request.
        let id = self.editor_id();
        if !ctx.memory(|m| m.has_focus(id)) {
            ctx.memory_mut(|m| m.request_focus(id));
        }
        self.focus_editor = true;
        self.auto.wanted = true;
        self.auto.intent = Some(query);
        self.maintain_index(ctx, true);
        self.refresh_choices(ctx);
        if !self.auto.visible() {
            self.notice = "No completions here.".into();
        }
        ctx.request_repaint();
    }
    pub(super) fn update_completion(&mut self, ctx: &egui::Context) {
        let focused = ctx.memory(|m| m.has_focus(self.editor_id()));
        let popup_rect =
            egui::AreaState::load(ctx, egui::Id::new("completion-popup")).map(|area| area.rect());
        // A pressed choice temporarily owns focus until its release commits it.
        let choosing = self.auto.visible()
            && ctx.input(|i| {
                (i.pointer.primary_down() || i.pointer.primary_released())
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|pos| popup_rect.is_some_and(|rect| rect.contains(pos)))
            });
        if (!focused && !choosing)
            || !self.completion_available()
            || !self.macros.playing.is_empty()
        {
            self.auto.dismiss();
        }
        let navigating = ctx.input(|i| {
            i.events.iter().any(|e| match e {
                egui::Event::PointerButton {
                    pos, pressed: true, ..
                } => popup_rect.is_none_or(|area| !area.contains(*pos)),
                egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } => {
                    matches!(
                        key,
                        Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::Home
                            | Key::End
                            | Key::PageUp
                            | Key::PageDown
                    ) || (matches!(key, Key::ArrowUp | Key::ArrowDown)
                        && *modifiers != Modifiers::NONE)
                        || (self.auto.popup.is_none()
                            && matches!(key, Key::ArrowUp | Key::ArrowDown | Key::Tab | Key::Enter))
                }
                _ => false,
            })
        });
        if navigating {
            self.auto.dismiss();
        }
        if let Some(previous) = self.auto.intent.clone() {
            match self.completion_query() {
                Some(now)
                    if now.view == previous.view
                        && now.pane == previous.pane
                        && now.start == previous.start
                        && (now.revision != previous.revision || now.head == previous.head) =>
                {
                    if now != previous {
                        self.auto.popup = None;
                        self.auto.intent = Some(now);
                    }
                }
                _ => self.auto.dismiss(),
            }
        }
        self.maintain_index(ctx, false);
        if let Some(mut due) = self.auto.due {
            let b = self.active();
            let pane = if self.panes.mode == 0 {
                0
            } else {
                self.panes.focused
            };
            if due.group != self.panes.focused_id()
                || due.view != b.view.id
                || due.pane != pane
                || (due.revision == b.page.info.revision && due.head != b.view.head)
            {
                self.auto.dismiss();
            } else {
                due.revision = b.page.info.revision;
                due.head = b.view.head;
                self.auto.due = Some(due);
                if Instant::now() >= due.at {
                    self.auto.due = None;
                    self.auto.intent = self
                        .completion_query()
                        .filter(|q| q.typed.chars().count() >= MIN_TYPED);
                } else {
                    ctx.request_repaint_after(due.at - Instant::now());
                }
            }
        }
        self.refresh_choices(ctx);
        if !self.settings.auto_complete
            || !focused
            || !self.completion_available()
            || !self.macros.playing.is_empty()
        {
            return;
        }
        let typed = ctx.input(|i| {
            let mut any = false;
            let mut word = true;
            for event in &i.events {
                if let egui::Event::Text(t) | egui::Event::Ime(egui::ImeEvent::Commit(t)) = event {
                    any = true;
                    word &= t.chars().all(completion::is_word_char);
                }
            }
            any.then_some(word)
        });
        match typed {
            Some(true) => {
                self.auto.wanted = true;
                if self.auto.intent.is_none() {
                    let delay = Duration::from_millis(self.settings.auto_complete_delay.into());
                    let b = self.active();
                    self.auto.due = Some(Scheduled {
                        group: self.panes.focused_id(),
                        at: Instant::now() + delay,
                        view: b.view.id,
                        pane: if self.panes.mode == 0 {
                            0
                        } else {
                            self.panes.focused
                        },
                        revision: b.page.info.revision,
                        head: b.view.head,
                    });
                    ctx.request_repaint_after(delay);
                }
            }
            Some(false) => self.auto.dismiss(),
            None => {}
        }
    }
    pub(super) fn completion_keys(&mut self, ctx: &egui::Context) {
        // A choice describes the previous editor frame. Let TextEdit handle
        // concurrent text, paste, pointer and navigation events in their order.
        let changing = ctx.input(|i| {
            i.events.iter().any(|event| match event {
                egui::Event::Text(_)
                | egui::Event::Paste(_)
                | egui::Event::Cut
                | egui::Event::Ime(egui::ImeEvent::Commit(_))
                | egui::Event::PointerButton { pressed: true, .. } => true,
                egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    ..
                } => {
                    matches!(
                        key,
                        Key::ArrowLeft
                            | Key::ArrowRight
                            | Key::Home
                            | Key::End
                            | Key::PageUp
                            | Key::PageDown
                            | Key::Backspace
                            | Key::Delete
                    ) || (matches!(key, Key::ArrowUp | Key::ArrowDown)
                        && *modifiers != Modifiers::NONE)
                }
                _ => false,
            })
        });
        if changing && ctx.input(|i| i.key_pressed(Key::Tab) || i.key_pressed(Key::Enter)) {
            self.auto.dismiss();
            return;
        }
        let Some(popup) = &mut self.auto.popup else {
            return;
        };
        let count = popup.choices.len();
        if count == 0 {
            return;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::ArrowDown) {
            popup.selected = (popup.selected + 1) % count;
            popup.locked = true;
        }
        if tab_switch::take_exact(ctx, Modifiers::NONE, Key::ArrowUp) {
            popup.selected = (popup.selected + count - 1) % count;
            popup.locked = true;
        }
        let enter = !self.settings.auto_complete_commit_on_tab
            && tab_switch::take_exact(ctx, Modifiers::NONE, Key::Enter);
        if enter || tab_switch::take_exact(ctx, Modifiers::NONE, Key::Tab) {
            let selected = popup.selected;
            self.commit_choice(selected);
        } else if self.settings.auto_complete_commit_on_tab
            && ctx.input(|i| i.key_pressed(Key::Enter))
        {
            self.auto.dismiss();
        }
    }
    pub(super) fn commit_choice(&mut self, index: usize) {
        let Some(popup) = self.auto.popup.take() else {
            return;
        };
        self.auto.dismiss();
        let Some(choice) = popup.choices.get(index).cloned() else {
            return;
        };
        if self.completion_query().as_ref() != Some(&popup.query) {
            return;
        }
        match choice {
            Choice::Word(word) => {
                let batch = hedit::advanced::replace(
                    &self.active().page.text,
                    &[hedit::advanced::Selection {
                        anchor: popup.query.start,
                        head: popup.query.head,
                    }],
                    &word,
                );
                self.batch(batch);
            }
            Choice::Language(choice) => {
                match language_servers::LanguageServers::completion_edit(
                    &self.active().page.text,
                    &choice,
                ) {
                    Ok((text, cursor)) => self.batch(hedit::advanced::Batch {
                        text,
                        selections: vec![hedit::advanced::Selection::caret(cursor)],
                    }),
                    Err(e) => self.notice = e,
                }
            }
            Choice::Snippet(snippet) => self.insert_snippet(snippet, popup.query.typed.len()),
        }
        self.focus_editor = true;
    }
    /// Draw the list below the cursor, or above it near the bottom.
    pub(super) fn completion_popup(&mut self, ctx: &egui::Context) {
        let Some(caret) = self.caret_screen else {
            return;
        };
        if !self.auto.visible() {
            return;
        }
        let popup = self.auto.popup.as_ref();
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
                        ui.set_width(280.0_f32.min((ctx.screen_rect().width() - 24.0).max(100.0)));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.visuals_mut().selection.stroke.width = 0.0;
                        if popup.is_none() {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.weak(if self.auto.indexed.is_none() {
                                    "Indexing open files…"
                                } else {
                                    "Finding completions…"
                                });
                            });
                        }
                        for (index, choice) in
                            popup.into_iter().flat_map(|p| p.choices.iter()).enumerate()
                        {
                            let (label, detail) = match choice {
                                Choice::Word(word) => (word.as_str(), ""),
                                Choice::Language(c) => (c.label.as_str(), c.detail.as_str()),
                                Choice::Snippet(s) => (s.trigger.as_str(), s.description.as_str()),
                            };
                            // Button shortcut text never wraps or truncates on its own.
                            // Bound the description before it takes width from the label.
                            let detail_galley = egui::WidgetText::from(detail).into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                ui.available_width() * 0.45,
                                egui::TextStyle::Small,
                            );
                            let response = ui.add(
                                egui::Button::new(RichText::new(label).font(font.clone()))
                                    .selected(popup.is_some_and(|p| index == p.selected))
                                    .shortcut_text(detail_galley)
                                    .frame(false)
                                    .truncate()
                                    .min_size(egui::vec2(ui.available_width(), 24.0)),
                            );
                            if response.clicked() {
                                clicked = Some(index);
                            }
                            if !detail.is_empty() {
                                response.on_hover_text(detail);
                            }
                        }
                        if self.auto.limited {
                            ui.weak("Some open-file words are outside the completion limit.");
                        }
                    });
            });
        if let Some(index) = clicked {
            self.commit_choice(index);
        }
    }
}

#[cfg(test)]
mod redesign_tests {
    use super::*;

    #[test]
    fn redesign_completion_stays_on_screen_with_long_snippet_descriptions() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let size = egui::vec2(720.0, 480.0);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        app.workspace.open[0].page.text = "for".into();
        app.workspace.edit(0, "").unwrap();
        app.workspace.open[0].view.anchor = 3;
        app.workspace.open[0].view.head = 3;
        let word = "format_with_a_long_identifier".repeat(20);
        app.settings.font_size = 28.0;
        app.auto.popup = Some(Popup {
            query: Query {
                group: app.panes.focused_id(),
                view: app.active().view.id,
                revision: app.active().page.info.revision,
                pane: 0,
                start: 0,
                head: 3,
                typed: "for".into(),
            },
            version: 0,
            selected: 0,
            locked: false,
            choices: vec![
                Choice::Snippet(hedit::intelligence::Snippet {
                    trigger: "for".into(),
                    description: "A very long snippet description ".repeat(40),
                    body: "for item in items {}".into(),
                    language: None,
                }),
                Choice::Word(word.clone()),
            ],
        });
        app.caret_screen = Some(egui::Rect::from_min_size(
            egui::pos2(680.0, 400.0),
            egui::vec2(2.0, 30.0),
        ));
        for _ in 0..4 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| app.completion_popup(ctx),
            );
        }
        let popup = egui::AreaState::load(&ctx, egui::Id::new("completion-popup"))
            .unwrap()
            .rect();
        assert!(
            screen.contains_rect(popup),
            "completion escaped the window: {popup:?}"
        );
        assert!(
            popup.width() <= 300.0,
            "completion ignored its compact width: {popup:?}"
        );
        // Click the truncated word, then undo: truncation must affect only display.
        let position = egui::pos2(popup.left() + 30.0, popup.bottom() - 20.0);
        for pressed in [true, false] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    focused: true,
                    events: vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ctx| app.completion_popup(ctx),
            );
        }
        assert_eq!(app.active().page.text, word);
        assert!(app.auto.popup.is_none());
        app.history(&ctx, false);
        super::super::tests::finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "for");
    }
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;

impl HeditApp {
    pub(super) fn publish_language_completions(
        &mut self,
        value: serde_json::Value,
        encoding: hedit::language_server::text::Encoding,
    ) {
        use hedit::language_server::text::{Range, TextEdit};
        let Some(query) = self.auto.intent.clone() else {
            return;
        };
        if self.completion_query().as_ref() != Some(&query)
            || self.auto.popup.as_ref().is_some_and(|p| p.locked)
        {
            return;
        }
        let items = value.as_array().or_else(|| value["items"].as_array());
        let Some(items) = items else {
            if self.auto.popup.is_none() {
                self.auto.dismiss();
            }
            return;
        };
        let source = &self.active().page.text;
        let Ok(start) = encoding.position(source, query.start) else {
            return;
        };
        let Ok(end) = encoding.position(source, query.head) else {
            return;
        };
        let mut choices = vec![];
        for item in items.iter().take(2000) {
            // The client advertises plain-text completions. Never insert
            // unsupported snippet syntax or execute completion commands.
            if item["insertTextFormat"] == 2 {
                continue;
            }
            let Some(label) = item["label"].as_str().filter(|s| s.len() <= 2048) else {
                continue;
            };
            let edit = if let Some(edit) = item.get("textEdit") {
                let Ok(edit) = serde_json::from_value::<TextEdit>(edit.clone()) else {
                    continue;
                };
                edit
            } else {
                TextEdit {
                    range: Range { start, end },
                    new_text: item["insertText"].as_str().unwrap_or(label).to_owned(),
                }
            };
            let mut edits = vec![edit];
            if let Some(additional) = item.get("additionalTextEdits") {
                let Ok(additional) = serde_json::from_value::<Vec<TextEdit>>(additional.clone())
                else {
                    continue;
                };
                if additional.len() > 100 {
                    continue;
                }
                edits.extend(additional);
            }
            if hedit::language_server::text::apply(source, &edits, encoding).is_err() {
                continue;
            }
            choices.push(Choice::Language(language_servers::Completion {
                label: label.into(),
                detail: item["detail"]
                    .as_str()
                    .unwrap_or("Language server")
                    .chars()
                    .take(200)
                    .collect(),
                edits,
                encoding,
            }));
            if choices.len() == SHOWN {
                break;
            }
        }
        if choices.is_empty() {
            if self.auto.popup.is_none() {
                self.auto.dismiss();
            }
            return;
        }
        if let Some(popup) = &self.auto.popup {
            choices.extend(
                popup
                    .choices
                    .iter()
                    .filter(|c| !matches!(c, Choice::Language(_)))
                    .take(SHOWN.saturating_sub(choices.len()))
                    .cloned(),
            );
        }
        self.auto.popup = Some(Popup {
            query,
            version: self.auto.version,
            locked: false,
            choices,
            selected: 0,
        });
    }
}
