//! Bounded, passive history. Actions retain the identity of their original work.
use super::*;
const MAX_ENTRIES: usize = 128;
const MAX_BYTES: usize = 256 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    RetrySave {
        view: u64,
        path: Option<String>,
        save_as: bool,
    },
    Output,
    LanguageLog,
    GitLog,
    Checkpoint,
    Search,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Info,
    Success,
    Error,
}
struct Entry {
    id: u64,
    kind: Kind,
    message: String,
    action: Option<Action>,
    count: u32,
    read: bool,
}
#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    entries: VecDeque<Entry>,
    bytes: usize,
    next: u64,
    seen_error: Option<String>,
    seen_notice: String,
    seen_language: Option<String>,
    seen_git: Option<String>,
    seen_search: Option<String>,
    pub pending_save: Option<Action>,
}
impl Action {
    fn label(&self) -> &'static str {
        match self {
            Self::RetrySave { .. } => "Retry save",
            Self::Output => "Open output",
            Self::LanguageLog => "Open server logs",
            Self::GitLog => "Open Git output",
            Self::Checkpoint => "Retry checkpoint",
            Self::Search => "Open Search",
        }
    }
}
impl Entry {
    fn bytes(&self) -> usize {
        self.message.len()
            + match &self.action {
                Some(Action::RetrySave {
                    path: Some(path), ..
                }) => path.len(),
                _ => 0,
            }
    }
}
impl State {
    pub fn seen_error_record(&mut self, error: &str) {
        self.seen_error = Some(error.to_owned());
    }
    pub fn unread(&self) -> usize {
        self.entries.iter().filter(|e| !e.read).count()
    }
    fn push(&mut self, kind: Kind, message: String, action: Option<Action>) {
        let message: String = message.chars().take(4096).collect();
        if message.trim().is_empty() {
            return;
        }
        if let Some(previous) = self.entries.back_mut() {
            if previous.kind == kind && previous.message == message && previous.action == action {
                previous.count = previous.count.saturating_add(1);
                previous.read = self.open;
                return;
            }
        }
        self.next = self.next.wrapping_add(1);
        let entry = Entry {
            id: self.next,
            kind,
            message,
            action,
            count: 1,
            read: self.open,
        };
        self.bytes += entry.bytes();
        self.entries.push_back(entry);
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            if let Some(entry) = self.entries.pop_front() {
                self.bytes = self.bytes.saturating_sub(entry.bytes());
            } else {
                break;
            }
        }
    }
    fn mark_read(&mut self) {
        for entry in &mut self.entries {
            entry.read = true;
        }
    }
    fn remove(&mut self, id: u64) {
        if let Some(index) = self.entries.iter().position(|e| e.id == id) {
            let entry = self.entries.remove(index).unwrap();
            self.bytes = self.bytes.saturating_sub(entry.bytes());
        }
    }
}
impl HeditApp {
    pub(super) fn notify(&mut self, kind: Kind, message: String, action: Option<Action>) {
        self.notifications.push(kind, message, action);
    }
    pub(super) fn observe_notifications(&mut self) {
        if self.notifications.seen_error != self.error {
            self.notifications.seen_error = self.error.clone();
            if let Some(error) = self.error.clone() {
                let action = if error.starts_with(CHECKPOINT_FAILED) {
                    Some(Action::Checkpoint)
                } else if !self.busy {
                    self.notifications.pending_save.take()
                } else {
                    None
                };
                self.notify(Kind::Error, error, action);
            }
        }
        if self.notifications.seen_notice != self.notice {
            self.notifications.seen_notice = self.notice.clone();
            if self.notice != "Ready." && self.notice != "File saved." {
                self.notify(Kind::Info, self.notice.clone(), None);
            }
        }
        let language = self.languages.notification_error().map(str::to_owned);
        if self.notifications.seen_language != language {
            self.notifications.seen_language = language.clone();
            if let Some(error) = language {
                self.notify(
                    Kind::Error,
                    format!("Language server: {error}"),
                    Some(Action::LanguageLog),
                );
            }
        }
        let git = self.source_control.notification_error().map(str::to_owned);
        if self.notifications.seen_git != git {
            self.notifications.seen_git = git.clone();
            if let Some(error) = git {
                self.notify(Kind::Error, format!("Git: {error}"), Some(Action::GitLog));
            }
        }
        let search = match &self.quality.folder_state {
            folder_search::State::Failed(e) => Some(format!("Search failed: {e}")),
            folder_search::State::Cancelled => Some("Search cancelled.".into()),
            folder_search::State::Applied(s) => Some(s.clone()),
            _ => None,
        };
        if self.notifications.seen_search != search {
            self.notifications.seen_search = search.clone();
            if let Some(message) = search {
                let kind = if matches!(self.quality.folder_state, folder_search::State::Failed(_)) {
                    Kind::Error
                } else {
                    Kind::Info
                };
                self.notify(kind, message, Some(Action::Search));
            }
        }
    }
    pub(super) fn toggle_notifications(&mut self) {
        self.notifications.open = !self.notifications.open;
        if self.notifications.open {
            self.notifications.mark_read();
        }
    }
    fn notification_action(&mut self, ctx: &egui::Context, action: Action) {
        if self.busy || self.closing {
            return;
        }
        match action {
            Action::RetrySave {
                view,
                path,
                save_as,
            } => {
                let Some(index) = self
                    .workspace
                    .open
                    .iter()
                    .position(|b| b.view.id == view && b.page.info.path == path)
                else {
                    self.notice = "Open the original file before retrying this save.".into();
                    return;
                };
                self.select_tab(index);
                self.error = None;
                self.save_file(ctx, save_as);
            }
            Action::Output => self.set_panel(workbench_panel::Panel::Output),
            Action::LanguageLog => {
                self.notifications.open = false;
                self.execute(ctx, Command::LanguageServers);
            }
            Action::GitLog => {
                self.show_git_sidebar(ctx);
                self.source_control.open_notification_output();
            }
            Action::Checkpoint => self.checkpoint(ctx),
            Action::Search => self.show_search_sidebar(),
        }
    }
    pub(super) fn notification_center(&mut self, ctx: &egui::Context) {
        if !self.notifications.open {
            return;
        }
        self.notifications.mark_read();
        let mut open = true;
        let mut action = None;
        let mut remove = None;
        let mut clear = false;
        let language = self.languages.notification_progress().to_owned();
        let language_active = self.languages.notification_working();
        let mut cancel_build = false;
        let mut cancel_search = false;
        let mut cancel_language = false;
        egui::Window::new("Notifications")
            .open(&mut open)
            .default_width(570.0)
            .default_height(400.0)
            .show(ctx, |ui| {
                if self.busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Working…");
                    });
                }
                if self.build.running() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Build · {}", self.build.name));
                        cancel_build = ui.small_button("Cancel build").clicked();
                    });
                }
                if self.quality.folder_task.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Searching files…");
                        cancel_search = ui.small_button("Cancel search").clicked();
                    });
                }
                if language_active {
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(if language.is_empty() {
                            "Language server operation…"
                        } else {
                            &language
                        });
                        cancel_language = ui.small_button("Cancel server operation").clicked();
                    });
                }
                ui.horizontal(|ui| {
                    ui.weak(format!(
                        "{} notifications",
                        self.notifications.entries.len()
                    ));
                    clear = ui.small_button("Clear history").clicked();
                });
                ui.separator();
                if self.notifications.entries.is_empty() {
                    ui.weak("No notifications.");
                }
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for entry in self.notifications.entries.iter().rev() {
                            ui.push_id(entry.id, |ui| {
                                let color = if entry.kind == Kind::Error {
                                    self.theme.error.color()
                                } else {
                                    self.theme.text.color()
                                };
                                ui.horizontal_top(|ui| {
                                    let width = (ui.available_width() - 30.0).max(80.0);
                                    ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&entry.message).color(color),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    });
                                    if controls::close_button(ui, "Dismiss notification").clicked()
                                    {
                                        remove = Some(entry.id);
                                    }
                                });
                                if entry.count > 1 {
                                    ui.weak(format!("Repeated {} times", entry.count));
                                }
                                if let Some(a) = &entry.action {
                                    if ui
                                        .add_enabled(
                                            !self.busy && !self.closing,
                                            egui::Button::new(a.label()),
                                        )
                                        .clicked()
                                    {
                                        action = Some(a.clone());
                                    }
                                }
                                ui.separator();
                            });
                        }
                    });
            });
        self.notifications.open = open;
        if clear {
            self.notifications.entries.clear();
            self.notifications.bytes = 0;
        }
        if let Some(id) = remove {
            self.notifications.remove(id);
        }
        if cancel_build {
            self.cancel_build();
        }
        if cancel_search {
            if let Some(task) = &self.quality.folder_task {
                task.cancel.store(true, Ordering::Relaxed);
            }
        }
        if cancel_language {
            self.cancel_language_work();
            self.notify(
                Kind::Info,
                "Language-server operation cancelled. Refresh Language servers to resume.".into(),
                Some(Action::LanguageLog),
            );
        }
        if let Some(action) = action {
            self.notification_action(ctx, action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{click_label, finish_app_job, frame, key};
    fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        settings_store::TEST_CONFIG_DIR.with(|d| *d.borrow_mut() = Some(dir.path().join("config")));
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        (dir, ctx, app)
    }
    #[test]
    fn history_deduplicates_caps_bytes_and_entries_and_marks_read() {
        let mut state = State::default();
        state.push(Kind::Error, "same".into(), Some(Action::Output));
        state.push(Kind::Error, "same".into(), Some(Action::Output));
        assert_eq!(state.entries.len(), 1);
        assert_eq!(state.entries[0].count, 2);
        assert_eq!(state.unread(), 1);
        state.mark_read();
        assert_eq!(state.unread(), 0);
        for i in 0..300 {
            state.push(Kind::Info, format!("{i}:{}", "ع".repeat(5000)), None);
        }
        assert!(state.entries.len() <= MAX_ENTRIES);
        assert!(state.bytes <= MAX_BYTES);
        assert_eq!(
            state.bytes,
            state.entries.iter().map(Entry::bytes).sum::<usize>()
        );
        let id = state.entries[0].id;
        state.remove(id);
        assert_eq!(
            state.bytes,
            state.entries.iter().map(Entry::bytes).sum::<usize>()
        );
    }
    #[test]
    fn passive_errors_do_not_take_focus_and_dismissal_keeps_history() {
        let (_dir, ctx, mut app) = fixture();
        frame(&ctx, &mut app, vec![]);
        let focus = ctx.memory(|m| m.focused());
        app.error = Some("A recoverable problem".into());
        app.observe_notifications();
        assert_eq!(ctx.memory(|m| m.focused()), focus);
        assert!(!app.notifications.open);
        assert_eq!(app.notifications.entries.len(), 1);
        app.error = None;
        app.observe_notifications();
        assert_eq!(app.notifications.entries.len(), 1);
        app.execute(&ctx, Command::Notifications);
        assert!(app.notifications.open);
        assert_eq!(app.notifications.unread(), 0);
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.notifications.open);
        assert_eq!(app.notifications.entries.len(), 1);
    }
    #[test]
    fn retry_save_uses_original_document_identity_after_switching_tabs() {
        let (dir, ctx, mut app) = fixture();
        let path = dir.path().join("original.txt");
        std::fs::write(&path, "original").unwrap();
        app.open(&ctx, Some(path.clone()));
        finish_app_job(&mut app, &ctx);
        let view = app.active().view.id;
        let original_path = app.active().page.info.path.clone();
        app.batch(hedit::advanced::Batch {
            text: "edited".into(),
            selections: vec![hedit::advanced::Selection::caret(0)],
        });
        app.execute(&ctx, Command::New);
        finish_app_job(&mut app, &ctx);
        let scratch = app.active().view.id;
        app.notification_action(
            &ctx,
            Action::RetrySave {
                view,
                path: original_path,
                save_as: false,
            },
        );
        finish_app_job(&mut app, &ctx);
        assert_eq!(std::fs::read_to_string(path).unwrap(), "edited");
        assert_ne!(app.active().view.id, scratch);
        assert_eq!(app.active().view.id, view);
        assert!(app
            .workspace
            .open
            .iter()
            .any(|b| b.view.id == scratch && b.page.text.is_empty()));
    }
    #[test]
    fn stale_retry_cannot_save_another_document_or_a_retargeted_path() {
        let (_dir, ctx, mut app) = fixture();
        let view = app.active().view.id;
        app.notification_action(
            &ctx,
            Action::RetrySave {
                view,
                path: Some("old.txt".into()),
                save_as: false,
            },
        );
        assert!(!app.busy);
        assert!(app.notice.contains("original file"));
        assert!(app.active().page.info.path.is_none());
    }
    #[test]
    fn notification_controls_cancel_only_named_cooperative_work() {
        let (_dir, ctx, mut app) = fixture();
        let cancel = Arc::new(AtomicBool::new(false));
        let (_tx, rx) = mpsc::channel();
        app.quality.folder_task = Some(folder_search::Task {
            cancel: cancel.clone(),
            rx,
        });
        app.execute(&ctx, Command::Notifications);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        click_label(&ctx, &mut app, "Cancel search");
        assert!(cancel.load(Ordering::Relaxed));
        assert!(app.notifications.open);
    }
    #[test]
    fn clearing_history_does_not_dismiss_recovery_errors_or_cancel_work() {
        let (_dir, ctx, mut app) = fixture();
        app.error = Some("Recovery needs attention".into());
        app.observe_notifications();
        app.execute(&ctx, Command::Notifications);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        click_label(&ctx, &mut app, "Clear history");
        assert!(app.notifications.entries.is_empty());
        assert_eq!(app.error.as_deref(), Some("Recovery needs attention"));
        assert!(!app.cancel.load(Ordering::Relaxed));
    }
}
