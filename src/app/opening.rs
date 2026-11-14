//! Bounded, cancellable file reads. Workers own document snapshots; applying a
//! result belongs to the UI thread and never releases another operation's lock.
use super::*;
use std::collections::HashMap;
const WORKERS: usize = 2;
const LIMIT: usize = 16;
/// Navigation adds literal suffixes to an encoded remote URI. Colons in host
/// filenames are percent-encoded, so parsing never consults the local filesystem.
pub(super) fn remote_location(path: &std::path::Path) -> (PathBuf, Option<(u64, u64)>) {
    let Some(text) = path.to_str() else {
        return (path.to_owned(), None);
    };
    let Some((prefix, last)) = text.rsplit_once(':') else {
        return (path.to_owned(), None);
    };
    let Some(last) = last.parse::<u64>().ok().filter(|n| *n > 0) else {
        return (path.to_owned(), None);
    };
    if let Some((file, line)) = prefix.rsplit_once(':') {
        if let Some(line) = line.parse::<u64>().ok().filter(|n| *n > 0) {
            return (file.into(), Some((line, last)));
        }
    }
    (prefix.into(), Some((last, 1)))
}
type Work = Box<dyn FnOnce() -> Result<Action, String> + Send>;
#[derive(Clone, PartialEq)]
struct Origin {
    view: u64,
    revision: u64,
    anchor: usize,
    head: usize,
    scroll: (f64, f64),
}
impl Origin {
    fn capture(app: &HeditApp) -> Self {
        let b = app.active();
        Self {
            view: b.view.id,
            revision: b.page.info.revision,
            anchor: b.view.anchor,
            head: b.view.head,
            scroll: (b.view.scroll_top, b.view.scroll_left),
        }
    }
}
pub(super) struct Request {
    id: u64,
    origin: Origin,
    epoch: u64,
    label: String,
    started: Instant,
    pub preview: bool,
    cancel: Arc<AtomicBool>,
}
struct Task {
    request: Request,
    work: Work,
}
struct Completed {
    request: Request,
    result: Result<Action, String>,
}
pub(super) struct Opens {
    next: u64,
    latest: u64,
    epoch: u64,
    running: HashMap<u64, Arc<AtomicBool>>,
    queued: VecDeque<Task>,
    ready: VecDeque<Completed>,
    tx: Sender<Completed>,
    rx: Receiver<Completed>,
    visible: HashMap<u64, (String, Instant)>,
    reopening: HashMap<u64, u64>,
}
impl Default for Opens {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            next: 0,
            latest: 0,
            epoch: 0,
            running: HashMap::new(),
            queued: VecDeque::new(),
            ready: VecDeque::new(),
            tx,
            rx,
            visible: HashMap::new(),
            reopening: HashMap::new(),
        }
    }
}
impl Drop for Opens {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Opens {
    pub fn pending(&self) -> bool {
        !self.visible.is_empty()
    }
    pub fn reopening(&self, document: u64) -> bool {
        self.reopening.values().any(|id| *id == document)
    }
    pub fn room(&self) -> bool {
        self.visible.len() < LIMIT
    }
    pub fn interrupt_focus(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn cancel(&mut self) {
        self.interrupt_focus();
        for cancel in self.running.values() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.queued.clear();
        self.ready.clear();
        self.visible.clear();
        self.reopening.clear();
    }
    pub(super) fn pump(&mut self, ctx: &egui::Context) {
        while let Ok(completed) = self.rx.try_recv() {
            self.running.remove(&completed.request.id);
            if !completed.request.cancel.load(Ordering::Relaxed) {
                self.ready.push_back(completed);
            }
        }
        while self.running.len() < WORKERS {
            let Some(Task { request, work }) = self.queued.pop_front() else {
                break;
            };
            self.running.insert(request.id, request.cancel.clone());
            let (tx, ctx) = (self.tx.clone(), ctx.clone());
            thread::spawn(move || {
                let result = if request.cancel.load(Ordering::Relaxed) {
                    Ok(Action::Nothing)
                } else {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                        .unwrap_or_else(|_| Err("The file reader stopped unexpectedly.".into()))
                };
                let _ = tx.send(Completed { request, result });
                ctx.request_repaint();
            });
        }
    }
    pub fn take(&mut self) -> Option<(Request, Result<Action, String>)> {
        let completed = self.ready.pop_front()?;
        self.visible.remove(&completed.request.id);
        self.reopening.remove(&completed.request.id);
        Some((completed.request, completed.result))
    }
    fn wants_focus(&self, request: &Request, app: &HeditApp) -> bool {
        request.id == self.latest
            && request.epoch == self.epoch
            && request.origin == Origin::capture(app)
    }
}
impl HeditApp {
    pub(super) fn read_open(
        &mut self,
        ctx: &egui::Context,
        label: String,
        work: impl FnOnce() -> Result<Action, String> + Send + 'static,
    ) {
        if self.closing {
            return;
        }
        if !self.opens.room() {
            self.notice = "Too many files are opening. Wait or cancel pending opens.".into();
            return;
        }
        self.opens.next = self.opens.next.wrapping_add(1);
        let id = self.opens.next;
        self.opens.latest = id;
        let request = Request {
            id,
            origin: Origin::capture(self),
            epoch: self.opens.epoch,
            label,
            started: Instant::now(),
            preview: std::mem::take(&mut self.quality.preview_request),
            cancel: Arc::new(AtomicBool::new(false)),
        };
        self.opens
            .visible
            .insert(id, (request.label.clone(), request.started));
        self.opens.queued.push_back(Task {
            request,
            work: Box::new(work),
        });
        self.opens.pump(ctx);
        ctx.request_repaint_after(Duration::from_millis(150));
    }
    pub(super) fn reopen_closed(&mut self, ctx: &egui::Context, index: usize) -> bool {
        let Some(view) = self.workspace.closed.get(index) else {
            return false;
        };
        let (id, start, label) = (view.id, view.page_start, view.name.clone());
        if self.closing || !self.opens.room() || self.opens.reopening(id) {
            return false;
        }
        let editor = self.workspace.editor.clone();
        self.read_open(ctx, label, move || {
            let mut doc = editor.get(id)?.lock().map_err(|e| e.to_string())?.clone();
            Ok(Action::ReopenRead(id, doc.page(start)?, None))
        });
        self.opens.reopening.insert(self.opens.latest, id);
        true
    }
    pub(super) fn cancel_opens(&mut self) {
        self.opens.cancel();
        self.pending_paths.clear();
        if self.quality.incoming.is_some() {
            self.quality.incoming_error = Some("File opening cancelled.".into());
        }
    }
    pub(super) fn opening_bar(&mut self, ctx: &egui::Context) {
        let Some((label, started)) = self
            .opens
            .visible
            .iter()
            .min_by_key(|(id, _)| *id)
            .map(|(_, v)| v.clone())
        else {
            return;
        };
        if started.elapsed() < Duration::from_millis(150) {
            ctx.request_repaint_after(Duration::from_millis(150).saturating_sub(started.elapsed()));
            return;
        }
        let mut cancel = false;
        egui::TopBottomPanel::top("opening-files")
            .frame(
                egui::Frame::new()
                    .fill(self.theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(10, 2)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    let text = format!(
                        "Opening {}{}",
                        label,
                        if self.opens.visible.len() > 1 {
                            format!(" ({} files)", self.opens.visible.len())
                        } else {
                            String::new()
                        }
                    );
                    ui.add_sized(
                        [(ui.available_width() - 125.0).max(20.0), 20.0],
                        egui::Label::new(text).truncate(),
                    )
                    .on_hover_text(&label);
                    cancel = ui
                        .small_button("Cancel opening")
                        .on_hover_text("Esc after dismissing menus or dialogs")
                        .clicked();
                });
            });
        if cancel {
            self.cancel_opens();
        }
    }
    /// Record intent before applying worker results, so a read completing in
    /// the same frame as a keystroke cannot redirect that keystroke.
    pub(super) fn note_open_input(&mut self, ctx: &egui::Context) {
        self.note_find_input(ctx);
        if self.opens.pending()
            && ctx.input(|input| {
                input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Text(_)
                            | egui::Event::Paste(_)
                            | egui::Event::Cut
                            | egui::Event::Copy
                            | egui::Event::Ime(
                                egui::ImeEvent::Preedit(_) | egui::ImeEvent::Commit(_)
                            )
                            | egui::Event::Key { pressed: true, .. }
                            | egui::Event::PointerButton { pressed: true, .. }
                            | egui::Event::MouseWheel { .. }
                            | egui::Event::Touch { .. }
                    )
                })
            })
        {
            self.opens.interrupt_focus();
        }
    }
    pub(super) fn open_focus(&self, request: &Request) -> bool {
        self.opens.wants_focus(request, self)
    }
    pub(super) fn apply_reopen_read(
        &mut self,
        _ctx: &egui::Context,
        id: u64,
        page: Page,
        cursor: Option<usize>,
        focus: bool,
    ) {
        let current = self
            .workspace
            .editor
            .get(id)
            .ok()
            .and_then(|doc| doc.lock().ok().map(|doc| doc.info()));
        if current
            .as_ref()
            .is_none_or(|info| info.revision != page.info.revision || info.path != page.info.path)
        {
            self.notice = "The file changed while reopening. Open it again.".into();
            return;
        }
        if let Some(index) = self.workspace.closed.iter().position(|view| view.id == id) {
            let view = self.workspace.closed.remove(index);
            self.workspace.open.push(Buffer::new(page, Some(view)));
            self.workspace.active = self.workspace.open.len() - 1;
            if let Some(cursor) = cursor {
                let b = &mut self.workspace.open[self.workspace.active];
                b.view.anchor = cursor;
                b.view.head = cursor;
                b.reveal_cursor = true;
            }
            self.changed();
            if focus {
                self.focus_editor = true;
                self.finish_preview();
                self.navigation_mark();
            }
        } else if focus {
            if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
                self.select_tab(index);
            }
        }
    }
    /// A concurrent read may find a file which another request already opened.
    /// Keep the existing document and its unsaved text, not the duplicate disk snapshot.
    pub(super) fn reconcile_open(&self, action: Action) -> Action {
        if let Action::Add(doc, page, cursor) = action {
            if let Some(path) = page.info.path.as_deref() {
                if let Some(buffer) = self
                    .workspace
                    .open
                    .iter()
                    .find(|b| b.page.info.path.as_deref() == Some(path))
                {
                    return Action::Activate(buffer.view.id);
                }
            }
            if let Some(path) = page.info.path.as_deref() {
                if self.workspace.closed.iter().any(|view| {
                    self.workspace
                        .editor
                        .get(view.id)
                        .ok()
                        .and_then(|doc| {
                            doc.lock()
                                .ok()
                                .map(|doc| doc.info().path.as_deref() == Some(path))
                        })
                        .unwrap_or(false)
                }) {
                    return Action::Nothing;
                }
            }
            Action::Add(doc, page, cursor)
        } else {
            action
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{finish_app_job, frame};
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.focus_editor = true;
        frame(&ctx, &mut app, vec![]);
        (dir, ctx, app)
    }
    fn added(id: u64) -> Action {
        let mut doc = Document::empty(id);
        let page = doc.page(0).unwrap();
        Action::Add(doc, page, 0)
    }
    #[test]
    fn typing_during_a_stalled_open_keeps_focus_and_applies_the_file_in_background() {
        let (_dir, ctx, mut app) = app();
        let original = app.active().view.id;
        let id = app.workspace.editor.id();
        let (tx, rx) = mpsc::channel();
        app.read_open(&ctx, "slow.txt".into(), move || {
            rx.recv().unwrap();
            Ok(added(id))
        });
        assert!(!app.busy);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("kept typing 🦀".into())],
        );
        tx.send(()).unwrap();
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().view.id, original);
        assert_eq!(app.active().page.text, "kept typing 🦀");
        assert!(app.workspace.open.iter().any(|b| b.view.id == id));
    }
    #[test]
    fn a_read_finishing_in_the_same_frame_as_typing_cannot_redirect_the_keystroke() {
        let (_dir, ctx, mut app) = app();
        let original = app.active().view.id;
        let id = app.workspace.editor.id();
        app.read_open(&ctx, "done.txt".into(), move || Ok(added(id)));
        let until = Instant::now() + Duration::from_secs(5);
        while app.opens.ready.is_empty() {
            app.opens.pump(&ctx);
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![egui::Event::Text("original".into())],
                ..Default::default()
            },
            |ctx| {
                app.note_open_input(ctx);
                app.receive(ctx);
                app.shortcuts(ctx);
                app.chrome(ctx);
                app.editor(ctx);
            },
        );
        assert_eq!(app.active().view.id, original);
        assert_eq!(app.active().page.text, "original");
        assert!(app
            .workspace
            .open
            .iter()
            .any(|b| b.view.id == id && b.page.text.is_empty()));
    }

    #[test]
    fn failed_reads_cannot_turn_the_next_explicit_open_into_a_preview() {
        let (_dir, ctx, mut app) = app();
        app.quality.preview_request = true;
        app.read_open(&ctx, "failed.txt".into(), || Err("Read failed".into()));
        let until = Instant::now() + Duration::from_secs(5);
        while app.opens.pending() {
            app.receive(&ctx);
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        assert_eq!(app.error.as_deref(), Some("Read failed"));
        assert!(!app.quality.preview_request);
    }

    #[test]
    fn read_results_wait_for_mutations_and_never_release_their_lock() {
        let (_dir, ctx, mut app) = app();
        let id = app.workspace.editor.id();
        app.read_open(&ctx, "read.txt".into(), move || Ok(added(id)));
        app.busy = true;
        let until = Instant::now() + Duration::from_secs(5);
        while app.opens.ready.is_empty() {
            app.receive(&ctx);
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        assert!(app.busy);
        assert!(!app.workspace.open.iter().any(|b| b.view.id == id));
        app.tx.send(Ok(Action::Nothing)).unwrap();
        finish_app_job(&mut app, &ctx);
        assert!(app.workspace.open.iter().any(|b| b.view.id == id));
    }
    #[test]
    fn cancellation_discards_late_results_and_never_starts_queued_reads() {
        let (_dir, ctx, mut app) = app();
        let (ready, started) = mpsc::channel();
        let mut releases = vec![];
        for _ in 0..3 {
            let id = app.workspace.editor.id();
            let (tx, rx) = mpsc::channel();
            releases.push(tx);
            let ready = ready.clone();
            app.read_open(&ctx, "blocked.txt".into(), move || {
                ready.send(()).unwrap();
                rx.recv().unwrap();
                Ok(added(id))
            });
        }
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(app.opens.running.len(), 2);
        assert_eq!(app.opens.queued.len(), 1);
        app.cancel_opens();
        releases[0].send(()).unwrap();
        releases[1].send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !app.opens.running.is_empty() {
            app.receive(&ctx);
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        assert!(started.try_recv().is_err());
        assert_eq!(app.workspace.open.len(), 1);
        assert!(!app.opens.pending());
    }
    #[test]
    fn concurrent_alias_opens_share_the_existing_document_and_keep_unsaved_text() {
        let (dir, ctx, mut app) = app();
        let path = dir.path().join("file.txt");
        std::fs::write(&path, "disk").unwrap();
        app.open(&ctx, Some(path.clone()));
        finish_app_job(&mut app, &ctx);
        let id = app.active().view.id;
        let old = app.active().page.text.clone();
        let index = app.workspace.active;
        app.workspace.open[index].page.text = "new unsaved text".into();
        app.workspace.edit(index, &old).unwrap();
        let mut duplicate = Document::open(app.workspace.editor.id(), &path).unwrap();
        let page = duplicate.page(0).unwrap();
        app.read_open(&ctx, "alias".into(), move || {
            Ok(Action::Add(duplicate, page, 0))
        });
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().view.id, id);
        assert_eq!(app.active().page.text, "new unsaved text");
        assert_eq!(app.workspace.open.len(), 2);
    }
    #[test]
    fn stale_location_pages_cannot_replace_newer_edits() {
        let (_dir, ctx, mut app) = app();
        let page = app.active().page.clone();
        let id = page.info.id;
        let (tx, rx) = mpsc::channel();
        app.read_open(&ctx, "location".into(), move || {
            rx.recv().unwrap();
            Ok(Action::Page(id, page, Some((0, 0))))
        });
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("new content".into())],
        );
        tx.send(()).unwrap();
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "new content");
        assert!(app.notice.contains("changed while opening"));
    }
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use crate::app::tests::finish_app_job;
    #[test]
    fn older_reads_cannot_steal_the_latest_requested_tab() {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let mut releases = vec![];
        let mut ids = vec![];
        for _ in 0..2 {
            let id = app.workspace.editor.id();
            ids.push(id);
            let (tx, rx) = mpsc::channel();
            releases.push(tx);
            app.read_open(&ctx, "file".into(), move || {
                rx.recv().unwrap();
                let mut doc = Document::empty(id);
                let page = doc.page(0)?;
                Ok(Action::Add(doc, page, 0))
            });
        }
        releases[1].send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while app.active().view.id != ids[1] {
            app.receive(&ctx);
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        releases[0].send(()).unwrap();
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().view.id, ids[1]);
        assert!(app.workspace.open.iter().any(|b| b.view.id == ids[0]));
    }
}

#[cfg(test)]
mod remote_location_tests {
    use super::*;
    #[test]
    fn remote_navigation_suffixes_preserve_encoded_colons_in_filenames() {
        let uri = PathBuf::from("hedit-ssh://session/source%3A12.rs");
        assert_eq!(remote_location(&uri), (uri.clone(), None));
        assert_eq!(
            remote_location(&PathBuf::from(format!("{}:42:7", uri.display()))),
            (uri.clone(), Some((42, 7)))
        );
        assert_eq!(
            remote_location(&PathBuf::from(format!("{}:42", uri.display()))),
            (uri, Some((42, 1)))
        );
    }
}
