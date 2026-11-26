//! Revision-scoped Find and selection matches. Large scans use one cancellable
//! reader per editor group; navigation never acquires the document mutation lane.
use super::*;
use eframe::egui::text::Galley;
use hedit::search::{Options, MATCH_LIMIT};
use std::collections::VecDeque;

type Ranges = Arc<Vec<(usize, usize)>>;
const INLINE_BYTES: usize = 64 * 1024;

#[derive(Clone, PartialEq, Eq)]
struct DocumentKey {
    view: u64,
    revision: u64,
    start: u64,
    path: Option<String>,
    bytes: usize,
}
#[derive(Clone, PartialEq, Eq)]
struct Key {
    doc: DocumentKey,
    query: String,
    options: Options,
    scope: std::ops::Range<usize>,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Navigation {
    anchor: usize,
    head: usize,
    previous: bool,
    skip_zero: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hit {
    range: (usize, usize),
    ordinal: usize,
}
struct Found {
    ranges: Ranges,
    count: usize,
    navigation: Option<(Navigation, Option<Hit>)>,
}
impl Found {
    fn select(&self, nav: Navigation) -> Option<Option<Hit>> {
        if self.count <= MATCH_LIMIT {
            return Some(choose(&self.ranges, nav));
        }
        self.navigation
            .filter(|(n, _)| *n == nav)
            .map(|(_, hit)| hit)
    }
    fn status(&self, hit: Option<Hit>) -> String {
        let mut status = match hit {
            Some(hit) => format!("{} of {}", hit.ordinal + 1, self.count),
            None if self.count == 0 => "No matches".into(),
            None => format!("{} matches", self.count),
        };
        if self.count > MATCH_LIMIT {
            status.push_str(" · first 20,000 highlighted");
        }
        status
    }
}
fn choose(ranges: &[(usize, usize)], nav: Navigation) -> Option<Hit> {
    let index = if nav.previous {
        ranges
            .iter()
            .rposition(|&(a, _)| a < nav.anchor)
            .or_else(|| ranges.len().checked_sub(1))
    } else {
        ranges
            .iter()
            .position(|&(a, b)| a >= nav.head && !(nav.skip_zero && a == b && a == nav.head))
            .or((!ranges.is_empty()).then_some(0))
    }?;
    Some(Hit {
        range: ranges[index],
        ordinal: index,
    })
}
/// Count every match while retaining bounded highlight ranges and one requested
/// navigation answer. UTF-8 offsets are converted monotonically, in one pass.
fn scan(
    text: &str,
    key: &Key,
    nav: Option<Navigation>,
    cancel: &AtomicBool,
) -> Option<Result<Found, String>> {
    let compute = || -> Result<Found, String> {
        text.get(key.scope.clone())
            .ok_or("Invalid search selection.")?;
        let regex = key.options.compile(&key.query)?;
        let mut ranges = Vec::new();
        let (mut byte, mut chars, mut count) = (0, 0, 0);
        let (mut first, mut last, mut selected) = (None, None, None);
        let mut to_char = |target: usize| {
            chars += text[byte..target].chars().count();
            byte = target;
            chars
        };
        for matched in hedit::search::scoped_matches(&regex, text, key.scope.clone()) {
            if cancel.load(Ordering::Relaxed) {
                return Err("Search cancelled.".into());
            }
            let a = to_char(matched.start());
            let b = to_char(matched.end());
            let hit = Hit {
                range: (a, b),
                ordinal: count,
            };
            first.get_or_insert(hit);
            last = Some(hit);
            if let Some(nav) = nav {
                if nav.previous {
                    if a < nav.anchor {
                        selected = Some(hit);
                    }
                } else if selected.is_none()
                    && a >= nav.head
                    && !(nav.skip_zero && a == b && a == nav.head)
                {
                    selected = Some(hit);
                }
            }
            count += 1;
            if ranges.len() < MATCH_LIMIT {
                ranges.push((a, b));
            }
        }
        if let Some(nav) = nav {
            selected = selected.or(if nav.previous { last } else { first });
        }
        Ok(Found {
            ranges: Arc::new(ranges),
            count,
            navigation: nav.map(|n| (n, selected)),
        })
    };
    let result = compute();
    (!cancel.load(Ordering::Relaxed)).then_some(result)
}
struct Reply {
    key: Key,
    selection: bool,
    result: Option<Result<Arc<Found>, String>>,
}
struct Task {
    key: Key,
    selection: bool,
    navigation: Option<Navigation>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Reply>,
}
#[derive(Default)]
struct Cache {
    find: Option<Reply>,
    selection: Option<Reply>,
    snapshot: Option<(DocumentKey, Arc<str>)>,
    task: Option<Task>,
}
impl Cache {
    fn cancel(&self) {
        if let Some(task) = &self.task {
            task.cancel.store(true, Ordering::Relaxed);
        }
    }
    fn get(
        &mut self,
        ctx: &egui::Context,
        key: Key,
        text: &str,
        selection: bool,
        nav: Option<Navigation>,
    ) -> Option<Result<Arc<Found>, String>> {
        if let Some(task) = &self.task {
            if task.key != key
                || task.selection != selection
                || nav.is_some() && task.navigation != nav
            {
                self.cancel();
            }
        }
        if let Some(task) = &self.task {
            match task.rx.try_recv() {
                Ok(reply) => {
                    if !task.cancel.load(Ordering::Relaxed) && reply.result.is_some() {
                        if reply.selection {
                            self.selection = Some(reply);
                        } else {
                            self.find = Some(reply);
                        }
                    }
                    self.task = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.task = None,
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        let cached = if selection {
            &self.selection
        } else {
            &self.find
        };
        if let Some(reply) = cached.as_ref().filter(|r| r.key == key) {
            let result = reply.result.as_ref().unwrap();
            if nav.is_none_or(|nav| {
                result
                    .as_ref()
                    .map_or(true, |found| found.select(nav).is_some())
            }) {
                return Some(result.clone());
            }
        }
        if self.task.is_some() {
            return None;
        }
        if text.len() <= INLINE_BYTES && key.query.len() <= 256 {
            let result = scan(text, &key, nav, &AtomicBool::new(false))
                .unwrap()
                .map(Arc::new);
            let reply = Reply {
                key,
                selection,
                result: Some(result.clone()),
            };
            if selection {
                self.selection = Some(reply);
            } else {
                self.find = Some(reply);
            }
            return Some(result);
        }
        if self
            .snapshot
            .as_ref()
            .is_none_or(|(doc, _)| *doc != key.doc)
        {
            self.snapshot = Some((key.doc.clone(), Arc::from(text)));
        }
        let text = self.snapshot.as_ref().unwrap().1.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let (tx, rx) = mpsc::channel();
        let work_key = key.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = scan(&text, &work_key, nav, &flag).map(|result| result.map(Arc::new));
            let _ = tx.send(Reply {
                key: work_key,
                selection,
                result,
            });
            ctx.request_repaint();
        });
        self.task = Some(Task {
            key,
            selection,
            navigation: nav,
            cancel,
            rx,
        });
        None
    }
}
impl Drop for Cache {
    fn drop(&mut self) {
        self.cancel();
    }
}
struct Pending {
    group: u64,
    key: Key,
    pane: usize,
    selection: (usize, usize),
    typing: bool,
    navigation: Navigation,
    next: VecDeque<bool>,
}
#[derive(Default)]
pub(super) struct LiveFind {
    pub origin: Option<(u64, usize)>,
    pub origin_group: u64,
    caches: [Cache; panes::MAX_GROUPS],
    pending: Option<Pending>,
    status_key: Option<Key>,
    pub error: Option<String>,
}

impl HeditApp {
    fn live_key(&self, query: &str, options: Options) -> Key {
        let b = self.active();
        Key {
            doc: DocumentKey {
                view: b.view.id,
                revision: b.page.info.revision,
                start: b.page.start,
                path: b.page.info.path.clone(),
                bytes: b.page.text.len(),
            },
            query: query.into(),
            options,
            scope: 0..b.page.text.len(),
        }
    }
    fn find_key(&self) -> Key {
        let mut key = self.live_key(&self.query, self.quality.options.clone());
        if let Some((_, start, end)) = self
            .quality
            .scope
            .filter(|(id, _, _)| self.quality.selection_only && *id == key.doc.view)
        {
            let page = &self.active().page;
            key.scope = start
                .saturating_sub(page.start as usize)
                .min(page.text.len())
                ..end.saturating_sub(page.start as usize).min(page.text.len());
        }
        key
    }
    fn find_set(
        &mut self,
        ctx: &egui::Context,
        pane: usize,
        nav: Option<Navigation>,
    ) -> Option<Result<Arc<Found>, String>> {
        let key = self.find_key();
        let text = &self.workspace.open[self.workspace.active].page.text;
        self.live_find.caches[pane].get(ctx, key, text, false, nav)
    }
    #[cfg(test)]
    pub(super) fn find_highlights(&mut self, ctx: &egui::Context) -> Option<Ranges> {
        self.find_highlights_in(ctx, self.panes.focused)
    }
    pub(super) fn find_highlights_in(
        &mut self,
        ctx: &egui::Context,
        pane: usize,
    ) -> Option<Ranges> {
        if !self.find_open || self.query.is_empty() {
            return None;
        }
        self.find_set(ctx, pane, None)?
            .ok()
            .map(|found| found.ranges.clone())
    }
    #[cfg(test)]
    pub(super) fn selection_highlights(&mut self, ctx: &egui::Context) -> Option<Ranges> {
        self.selection_highlights_in(ctx, self.panes.focused)
    }
    pub(super) fn selection_highlights_in(
        &mut self,
        ctx: &egui::Context,
        pane: usize,
    ) -> Option<Ranges> {
        if !self.settings.match_selection || self.find_open || self.live_find.pending.is_some() {
            return None;
        }
        let b = self.active();
        let (a, z) = (
            b.view.anchor.min(b.view.head),
            b.view.anchor.max(b.view.head),
        );
        if a == z || z - a > 100 || !b.view.selections.is_empty() {
            return None;
        }
        let text = &self.workspace.open[self.workspace.active].page.text;
        let index = self.projections[pane].source_index(text);
        let b = self.active();
        let word = &b.page.text[index.byte(&b.page.text, a)..index.byte(&b.page.text, z)];
        if !word.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return None;
        }
        let key = self.live_key(
            word,
            Options {
                regex: false,
                case_sensitive: true,
                whole_word: true,
            },
        );
        let text = &self.workspace.open[self.workspace.active].page.text;
        let found = self.live_find.caches[pane]
            .get(ctx, key, text, true, None)?
            .ok()?;
        (found.count > 1).then(|| found.ranges.clone())
    }
    fn schedule_find(&mut self, typing: bool, previous: bool) {
        let key = self.find_key();
        let b = self.active();
        let selection = (b.view.anchor, b.view.head);
        let pane = self.panes.focused;
        if !typing {
            if let Some(pending) = &mut self.live_find.pending {
                if !pending.typing
                    && pending.key == key
                    && pending.pane == pane
                    && pending.group == self.panes.focused_id()
                    && pending.selection == selection
                {
                    if pending.next.len() < 256 {
                        pending.next.push_back(previous);
                    }
                    return;
                }
            }
        }
        let b = self.active();
        let origin = self
            .live_find
            .origin
            .filter(|(id, _)| {
                *id == b.view.id && self.live_find.origin_group == self.panes.focused_id()
            })
            .map_or(b.view.anchor.min(b.view.head), |(_, at)| at);
        let skip_zero = b.view.anchor == b.view.head
            && self
                .quality
                .last_find
                .as_ref()
                .is_some_and(|last| last.matches(b, &self.query, &self.quality.options));
        self.live_find.pending = Some(Pending {
            group: self.panes.focused_id(),
            key,
            pane,
            selection,
            typing,
            navigation: Navigation {
                anchor: if typing { origin } else { b.view.anchor },
                head: if typing { origin } else { b.view.head },
                previous,
                skip_zero: !typing && skip_zero,
            },
            next: VecDeque::new(),
        });
    }
    pub(super) fn find_as_you_type(&mut self, ctx: &egui::Context) {
        if self.active().page.info.paged {
            return;
        }
        let b = self.active();
        if self.live_find.origin.is_none_or(|(id, _)| id != b.view.id)
            || self.live_find.origin_group != self.panes.focused_id()
        {
            self.live_find.origin = Some((b.view.id, b.view.anchor.min(b.view.head)));
            self.live_find.origin_group = self.panes.focused_id();
        }
        self.schedule_find(true, false);
        self.poll_live_find(ctx);
    }
    pub(super) fn navigate_find(&mut self, ctx: &egui::Context, previous: bool) {
        if self.query.is_empty() || self.busy || self.closing {
            return;
        }
        self.schedule_find(false, previous);
        self.poll_live_find(ctx);
    }
    /// Runs before worker results are consumed, including when a keystroke and
    /// completion arrive in the same frame. Query input gets a fresh request later.
    pub(super) fn note_find_input(&mut self, ctx: &egui::Context) {
        if self.live_find.pending.is_none() {
            return;
        }
        let find_focus = self.find_input_focused(ctx);
        let interrupt = ctx.input(|input| {
            input.events.iter().any(|event| match event {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    !(*key == egui::Key::F3
                        && !modifiers.ctrl
                        && !modifiers.alt
                        && !modifiers.command
                        || find_focus && *key == egui::Key::Enter)
                }
                egui::Event::Text(_)
                | egui::Event::Paste(_)
                | egui::Event::Cut
                | egui::Event::Ime(_)
                | egui::Event::PointerButton { pressed: true, .. }
                | egui::Event::MouseWheel { .. }
                | egui::Event::Touch { .. } => true,
                _ => false,
            })
        });
        if interrupt {
            if let Some(pending) = self.live_find.pending.take() {
                self.live_find.caches[pending.pane].cancel();
            }
        }
    }
    pub(super) fn poll_live_find(&mut self, ctx: &egui::Context) {
        if self.active().page.info.paged {
            self.live_find.pending = None;
            return;
        }
        if self.query.is_empty() {
            self.live_find.pending = None;
            self.live_find.error = None;
            self.quality.match_status.clear();
            return;
        }
        if self.busy || self.closing {
            return;
        }
        let key = self.find_key();
        if self.live_find.pending.as_ref().is_some_and(|p| {
            p.key != key
                || p.group != self.panes.focused_id()
                || p.pane != self.panes.focused
                || p.selection != (self.active().view.anchor, self.active().view.head)
                || p.typing && !self.find_open
        }) {
            self.live_find.pending = None;
        }
        if !self.find_open && self.live_find.pending.is_none() {
            return;
        }
        let nav = self.live_find.pending.as_ref().map(|p| p.navigation);
        let Some(result) = self.find_set(ctx, self.panes.focused, nav) else {
            self.quality.match_status = "Searching…".into();
            self.live_find.error = None;
            return;
        };
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                self.live_find.pending = None;
                self.quality.match_status = "Invalid pattern".into();
                self.live_find.error = Some(error);
                return;
            }
        };
        self.live_find.error = None;
        let Some(mut pending) = self.live_find.pending.take() else {
            if self.live_find.status_key.as_ref() != Some(&key)
                || self.quality.match_status == "Searching…"
            {
                self.quality.match_status = found.status(None);
                self.live_find.status_key = Some(key);
            }
            return;
        };
        let hit = found.select(pending.navigation).unwrap();
        if !pending.typing {
            self.navigation_mark();
            self.reveal_markdown_source(ctx);
        }
        let b = &mut self.workspace.open[self.workspace.active];
        if let Some(hit) = hit {
            (b.view.anchor, b.view.head) = hit.range;
            self.quality.last_find =
                Some(quality::LastFind::at(b, &self.query, &self.quality.options));
        } else if pending.typing {
            b.view.anchor = pending.navigation.head;
            b.view.head = pending.navigation.head;
            self.quality.last_find = None;
        }
        if hit.is_some() || pending.typing {
            b.view.selections.clear();
            b.reveal_cursor = true;
            b.restore_cursor = true;
        }
        self.quality.match_status = found.status(hit);
        if hit.is_none() && !pending.typing {
            self.notice = "No matches found.".into();
        }
        self.live_find.status_key = Some(key);
        if !pending.typing {
            self.navigation_mark();
            self.focus_editor = !self.find_input_focused(ctx);
        }
        self.changed();
        if let Some(previous) = pending.next.pop_front() {
            let b = self.active();
            pending.selection = (b.view.anchor, b.view.head);
            pending.navigation = Navigation {
                anchor: b.view.anchor,
                head: b.view.head,
                previous,
                skip_zero: b.view.anchor == b.view.head,
            };
            self.live_find.pending = Some(pending);
            ctx.request_repaint();
        }
    }
}
pub(super) struct View<'a> {
    pub galley: &'a Galley,
    pub rows: &'a row_index::Rows,
    pub projection: &'a hedit::advanced::Projection,
}
/// Map and paint only ranges overlapping the viewport, preserving RTL spans,
/// folded regions, multiline matches and zero-width matches at EOF.
pub(super) fn paint_ranges(
    painter: &egui::Painter,
    view: View<'_>,
    bidi: &super::bidi_view::BidiRows,
    origin: egui::Pos2,
    clip: egui::Rect,
    ranges: &[(usize, usize)],
    (fill, stroke): (egui::Color32, egui::Stroke),
) {
    let View {
        galley,
        rows,
        projection,
    } = view;
    let visible = rows.visible(galley, origin, clip);
    let Some(first_row) = visible.clone().next() else {
        return;
    };
    let last_row = visible.clone().last().unwrap();
    let source_start = projection.source(rows.row(first_row).start);
    let source_end = projection.source(rows.end(last_row));
    let first = ranges.partition_point(|&(a, b)| b < source_start || b == source_start && a != b);
    let shown: Vec<_> = ranges[first..]
        .iter()
        .take_while(|(a, _)| *a <= source_end)
        .filter(|&&(a, _)| projection.source(projection.display(a)) == a)
        .map(|&(a, b)| (projection.display(a), projection.display(b)))
        .collect();
    for r in visible {
        let row = &galley.rows[r];
        let row_start = rows.row(r).start;
        let row_end = rows.end(r);
        let top = origin.y + row.rect().top();
        let bottom = origin.y + row.rect().bottom();
        for &(a, b) in &shown {
            let zero_at_end = a == b && a == row_end && r + 1 == galley.rows.len();
            if (a >= row_end && !zero_at_end) || b < row_start || b == row_start && a != b {
                continue;
            }
            for (x0, x1) in super::bidi_view::spans(
                row,
                bidi.row(r),
                a.max(row_start) - row_start,
                b.min(row_end) - row_start,
            ) {
                painter.rect(
                    egui::Rect::from_min_max(
                        egui::pos2(origin.x + x0, top),
                        egui::pos2(origin.x + x1.max(x0 + 2.0), bottom),
                    ),
                    2.0,
                    fill,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
        }
    }
}

#[cfg(test)]
mod performance {
    use super::*;
    use crate::app::tests::frame;
    #[test]
    #[ignore = "manual CPU profile of current-file Find scans and warm highlight rendering"]
    fn profile_live_find() {
        let source: String = (0..40_000)
            .map(|i| format!("    let value_{i} = compute_{}(needle);\n", i % 997))
            .collect();
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
        workspace.open[0].page.text = source.clone();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.auto_complete = false;
        app.settings.draw_white_space = true;
        app.find_open = true;
        app.query = "needle".into();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        for (name, cursor) in [("start", 0), ("end", source.chars().count())] {
            app.workspace.open[0].view.anchor = cursor;
            app.workspace.open[0].view.head = cursor;
            app.workspace.open[0].reveal_cursor = true;
            frame(&ctx, &mut app, vec![]);
            let start = Instant::now();
            for _ in 0..30 {
                frame(&ctx, &mut app, vec![]);
            }
            eprintln!(
                "Find + whitespace at {name} in {} bytes: {:.2} ms/warm CPU frame",
                source.len(),
                start.elapsed().as_secs_f64() * 1000.0 / 30.0
            );
        }
        let start = Instant::now();
        for i in 0..10 {
            app.query = format!("needle{i}");
            app.find_as_you_type(&ctx);
        }
        eprintln!(
            "New literal Find queries in {} bytes: {:.2} ms/UI call",
            source.len(),
            start.elapsed().as_secs_f64() * 1000.0 / 10.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{frame, key as key_event};
    fn app(text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        workspace.open[0].page.text = text.into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.focus_editor = true;
        frame(&ctx, &mut app, vec![]);
        (dir, ctx, app)
    }
    fn wait(app: &mut HeditApp, ctx: &egui::Context) {
        let start = Instant::now();
        while app.live_find.pending.is_some() {
            app.poll_live_find(ctx);
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::yield_now();
        }
    }
    fn blocked(
        app: &mut HeditApp,
        typing: bool,
    ) -> (Sender<Reply>, Key, Navigation, Arc<AtomicBool>) {
        app.schedule_find(typing, false);
        let pending = app.live_find.pending.as_ref().unwrap();
        let (key, nav) = (pending.key.clone(), pending.navigation);
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        app.live_find.caches[0].task = Some(Task {
            key: key.clone(),
            selection: false,
            navigation: Some(nav),
            cancel: flag.clone(),
            rx,
        });
        (tx, key, nav, flag)
    }
    fn reply(text: &str, key: Key, nav: Navigation) -> Reply {
        let result = scan(text, &key, Some(nav), &AtomicBool::new(false)).map(|r| r.map(Arc::new));
        Reply {
            key,
            selection: false,
            result,
        }
    }
    fn painted(
        source: &str,
        folds: &[hedit::advanced::Fold],
        query: &str,
        width: f32,
        scale: f32,
        clip: egui::Rect,
    ) -> (
        Vec<egui::Rect>,
        Arc<Galley>,
        bidi_view::BidiRows,
        hedit::advanced::Projection,
        Ranges,
    ) {
        let (_dir, _, app) = app(source);
        let found = scan(
            source,
            &app.live_key(
                query,
                Options {
                    regex: true,
                    ..Default::default()
                },
            ),
            None,
            &AtomicBool::new(false),
        )
        .unwrap()
        .unwrap();
        let projection = hedit::advanced::Projection::new(source, 4, folds);
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(scale);
        let fill = egui::Color32::from_rgb(251, 1, 7);
        let mut layout = None;
        let output = crate::app::tests::run_pass(&ctx, egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show_in_context(ctx, |ui| {
                let galley = ui.fonts_mut(|f| {
                    f.layout(
                        projection.text.clone(),
                        FontId::monospace(14.0),
                        egui::Color32::WHITE,
                        width,
                    )
                });
                let (galley, bidi) =
                    ui.fonts_mut(|f| bidi_view::apply(f, galley, bidi_view::Base::Auto));
                let rows = row_index::Cache::default().get(&galley);
                paint_ranges(
                    &ui.painter().with_clip_rect(clip),
                    View {
                        galley: &galley,
                        rows: &rows,
                        projection: &projection,
                    },
                    &bidi,
                    egui::Pos2::ZERO,
                    clip,
                    &found.ranges,
                    (fill, egui::Stroke::NONE),
                );
                layout = Some((galley, bidi));
            });
        });
        let rects = output
            .shapes
            .into_iter()
            .filter_map(|s| match s.shape {
                egui::Shape::Rect(r) if r.fill == fill => Some(r.rect),
                _ => None,
            })
            .collect();
        let (galley, bidi) = layout.unwrap();
        (rects, galley, bidi, projection, found.ranges)
    }
    #[test]
    fn viewport_highlights_match_full_geometry_with_wrap_folds_tabs_rtl_and_scaling() {
        let source = "head\tneedle\r\nשלום needle\n{\n    hidden needle\n}\nlast needle\n";
        let start = source[..source.find("    hidden").unwrap()].chars().count();
        let end = start + "    hidden needle\n".chars().count();
        for folds in [vec![], vec![hedit::advanced::Fold { start, end }]] {
            for scale in [1.0, 1.25, 1.5] {
                for width in [100.0, f32::INFINITY] {
                    for y in [0.0, 20.0, 55.0, 95.0] {
                        let clip =
                            egui::Rect::from_min_size(egui::pos2(0.0, y), egui::vec2(500.0, 45.0));
                        let (actual, galley, bidi, projection, ranges) = painted(
                            source,
                            &folds,
                            "needle|needle\\r?\\nשלום",
                            width,
                            scale,
                            clip,
                        );
                        let shown: Vec<_> = ranges
                            .iter()
                            .filter(|&&(a, _)| projection.source(projection.display(a)) == a)
                            .map(|&(a, b)| (projection.display(a), projection.display(b)))
                            .collect();
                        let mut expected = vec![];
                        let mut row_start = 0;
                        for (r, row) in galley.rows.iter().enumerate() {
                            let row_end = row_start + row.char_count_including_newline().0;
                            if row.rect().bottom() >= clip.top()
                                && row.rect().top() <= clip.bottom()
                            {
                                for &(a, b) in &shown {
                                    if a >= row_end || b <= row_start {
                                        continue;
                                    }
                                    for (x0, x1) in bidi_view::spans(
                                        row,
                                        bidi.row(r),
                                        a.max(row_start) - row_start,
                                        b.min(row_end) - row_start,
                                    ) {
                                        expected.push(egui::Rect::from_min_max(
                                            egui::pos2(x0, row.rect().top()),
                                            egui::pos2(x1.max(x0 + 2.0), row.rect().bottom()),
                                        ));
                                    }
                                }
                            }
                            row_start = row_end;
                        }
                        assert_eq!(actual, expected, "scale={scale} width={width} y={y}");
                    }
                }
            }
        }
    }
    #[test]
    fn zero_width_highlights_include_empty_lines_and_eof() {
        let clip = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 200.0));
        for source in ["", "α\n\n", "שלום\nlast"] {
            let (rects, _, _, _, ranges) = painted(source, &[], "$", f32::INFINITY, 1.25, clip);
            assert_eq!(rects.len(), ranges.len());
            assert!(rects.iter().all(|r| (r.width() - 2.0).abs() < 0.01));
        }
    }
    #[test]
    fn paged_find_status_stays_with_the_whole_file_search() {
        let (_dir, ctx, mut app) = app("hit");
        app.workspace.open[0].page.info.paged = true;
        app.query = "hit".into();
        app.find_open = true;
        app.quality.match_status = "24 matches".into();
        app.poll_live_find(&ctx);
        assert_eq!(app.quality.match_status, "24 matches");
        assert!(app.live_find.pending.is_none());
    }

    #[test]
    fn scans_match_unicode_regex_scope_and_zero_width_reference_ranges() {
        let text = "α🦀 hit\r\nשלום hit\nlast hit";
        let (_dir, _ctx, app) = app(text);
        for (query, regex, whole) in [
            ("hit", false, false),
            ("^|$", true, false),
            ("🦀|שלום", true, false),
            ("hi", false, true),
            ("hit.*", true, false),
        ] {
            for scope in [0..text.len(), "α🦀 ".len()..text.len() - 4] {
                let mut key = app.live_key(
                    query,
                    Options {
                        regex,
                        whole_word: whole,
                        case_sensitive: true,
                    },
                );
                key.scope = scope.clone();
                let found = scan(text, &key, None, &AtomicBool::new(false))
                    .unwrap()
                    .unwrap();
                let reference = hedit::search::matches(text, query, &key.options, scope).unwrap();
                assert_eq!(found.count, reference.len());
                assert_eq!(
                    *found.ranges,
                    reference
                        .iter()
                        .map(|r| (
                            text[..r.start].chars().count(),
                            text[..r.end].chars().count()
                        ))
                        .collect::<Vec<_>>()
                );
            }
        }
    }
    #[test]
    fn navigation_beyond_the_highlight_limit_counts_every_match_and_wraps() {
        let source = "hit ".repeat(30_000);
        let (_dir, ctx, mut app) = app(&source);
        app.query = "hit".into();
        app.workspace.open[0].view.anchor = 100_000;
        app.workspace.open[0].view.head = 100_000;
        app.navigate_find(&ctx, false);
        assert!(!app.busy);
        wait(&mut app, &ctx);
        assert_eq!(
            (app.active().view.anchor, app.active().view.head),
            (100_000, 100_003)
        );
        assert_eq!(
            app.quality.match_status,
            "25001 of 30000 · first 20,000 highlighted"
        );
        app.navigate_find(&ctx, true);
        wait(&mut app, &ctx);
        assert_eq!(app.active().view.anchor, 99_996);
        app.workspace.open[0].view.anchor = 0;
        app.workspace.open[0].view.head = 0;
        app.navigate_find(&ctx, true);
        wait(&mut app, &ctx);
        assert_eq!(app.active().view.anchor, 119_996);
        app.navigate_find(&ctx, false);
        wait(&mut app, &ctx);
        assert_eq!(app.active().view.anchor, 0);
        assert_eq!(
            app.live_find.caches[0]
                .find
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .ranges
                .len(),
            MATCH_LIMIT
        );
    }
    #[test]
    fn an_async_result_ready_in_the_typing_frame_cannot_redirect_a_keystroke() {
        let source = format!("{}needle", "padding\n".repeat(10_000));
        let (_dir, ctx, mut app) = app(&source);
        app.query = "needle".into();
        let (tx, key, nav, flag) = blocked(&mut app, false);
        tx.send(reply(&source, key, nav)).unwrap();
        frame(&ctx, &mut app, vec![egui::Event::Text("kept".into())]);
        assert!(flag.load(Ordering::Relaxed));
        assert_eq!(app.active().page.text, format!("kept{source}"));
        assert_eq!(app.active().view.head, 4);
        assert!(!app.busy);
    }
    #[test]
    fn escape_query_scope_revision_and_tab_changes_reject_late_navigation() {
        for change in [
            "escape", "query", "scope", "revision", "tab", "options", "rename",
        ] {
            let source = format!("{}needle", "line\n".repeat(14_000));
            let (_dir, ctx, mut app) = app(&source);
            app.query = "needle".into();
            app.find_open = true;
            let (tx, key, nav, _) = blocked(&mut app, true);
            tx.send(reply(&source, key, nav)).unwrap();
            match change {
                "escape" => {
                    frame(
                        &ctx,
                        &mut app,
                        vec![key_event(egui::Key::Escape, Modifiers::NONE)],
                    );
                }
                "query" => {
                    app.query = "absent".into();
                    app.poll_live_find(&ctx);
                }
                "scope" => {
                    app.quality.selection_only = true;
                    app.quality.scope = Some((app.active().view.id, 0, 4));
                    app.poll_live_find(&ctx);
                }
                "revision" => {
                    app.workspace.open[0].page.text.insert_str(0, "new");
                    app.workspace.edit(0, "").unwrap();
                    app.poll_live_find(&ctx);
                }
                "options" => {
                    app.quality.options.case_sensitive = false;
                    app.poll_live_find(&ctx);
                }
                "rename" => {
                    app.workspace.open[0].page.info.path = Some("renamed.rs".into());
                    app.poll_live_find(&ctx);
                }
                "tab" => {
                    app.workspace.new_buffer().unwrap();
                    app.poll_live_find(&ctx);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                app.active().view.head,
                0,
                "late cursor movement after {change}"
            );
            assert!(app.live_find.pending.is_none());
        }
    }
    #[test]
    fn repeated_next_keys_during_a_scan_are_preserved_and_warm_navigation_reuses_results() {
        let source = format!("{}hit hit hit", "padding ".repeat(9000));
        let (_dir, ctx, mut app) = app(&source);
        app.query = "hit".into();
        let (tx, key, nav, _) = blocked(&mut app, false);
        app.navigate_find(&ctx, false);
        app.navigate_find(&ctx, false);
        tx.send(reply(&source, key, nav)).unwrap();
        wait(&mut app, &ctx);
        assert_eq!(app.active().view.anchor, source.len() - 3);
        let cached = app.live_find.caches[0]
            .find
            .as_ref()
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        app.navigate_find(&ctx, false);
        assert_eq!(app.active().view.anchor, source.len() - 11);
        assert!(app.live_find.caches[0].task.is_none());
        assert!(Arc::ptr_eq(
            &cached,
            app.live_find.caches[0]
                .find
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
        ));
    }
    #[test]
    fn all_editor_groups_finish_independently_without_cancelling_each_other() {
        let source = format!("{}hit", "line\n".repeat(14_000));
        let (_dir, ctx, mut app) = app(&source);
        app.query = "hit".into();
        app.find_open = true;
        for pane in 0..panes::MAX_GROUPS {
            assert!(app.find_highlights_in(&ctx, pane).is_none());
        }
        let start = Instant::now();
        let mut ready = [false; panes::MAX_GROUPS];
        while ready.contains(&false) {
            for (pane, ready) in ready.iter_mut().enumerate() {
                *ready = app.find_highlights_in(&ctx, pane).is_some();
            }
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::yield_now();
        }
        assert!(app
            .live_find
            .caches
            .iter()
            .all(|cache| cache.task.is_none()));
    }
    #[test]
    fn a_ready_obsolete_query_cannot_evict_the_cached_query_we_returned_to() {
        let source = format!("alpha beta {}", "padding ".repeat(9000));
        let (_dir, ctx, mut app) = app(&source);
        app.query = "alpha".into();
        app.find_open = true;
        app.find_as_you_type(&ctx);
        wait(&mut app, &ctx);
        let cached = app.live_find.caches[0]
            .find
            .as_ref()
            .unwrap()
            .result
            .as_ref()
            .unwrap()
            .as_ref()
            .unwrap()
            .clone();
        let key = app.live_key("beta", Options::default());
        let nav = Navigation {
            anchor: 0,
            head: 0,
            previous: false,
            skip_zero: false,
        };
        let (tx, rx) = mpsc::channel();
        let flag = Arc::new(AtomicBool::new(false));
        tx.send(reply(&source, key.clone(), nav)).unwrap();
        app.live_find.caches[0].task = Some(Task {
            key,
            selection: false,
            navigation: Some(nav),
            cancel: flag.clone(),
            rx,
        });
        app.find_as_you_type(&ctx);
        assert!(flag.load(Ordering::Relaxed));
        assert!(
            app.live_find.caches[0].task.is_none(),
            "returning to the cached query must not start another scan"
        );
        assert!(Arc::ptr_eq(
            &cached,
            app.live_find.caches[0]
                .find
                .as_ref()
                .unwrap()
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
        ));
        assert_eq!((app.active().view.anchor, app.active().view.head), (0, 5));
    }
    #[test]
    fn query_changes_reuse_a_snapshot_and_cancel_without_spawning_a_second_reader() {
        let source = "word ".repeat(20_000);
        let (_dir, ctx, mut app) = app(&source);
        app.query = "word".into();
        app.find_open = true;
        let (tx, key, nav, flag) = blocked(&mut app, true);
        for i in 0..200 {
            app.query = format!("new{i}");
            app.find_as_you_type(&ctx);
        }
        assert!(flag.load(Ordering::Relaxed));
        assert!(
            app.live_find.caches[0].snapshot.is_none(),
            "a blocked reader must not queue copies of each query's text"
        );
        tx.send(Reply {
            key,
            selection: false,
            result: None,
        })
        .unwrap();
        wait(&mut app, &ctx);
        let snapshot = app.live_find.caches[0].snapshot.as_ref().unwrap().1.clone();
        app.query = "word".into();
        app.find_as_you_type(&ctx);
        wait(&mut app, &ctx);
        assert!(Arc::ptr_eq(
            &snapshot,
            &app.live_find.caches[0].snapshot.as_ref().unwrap().1
        ));
        assert_eq!(app.quality.match_status, "1 of 20000");
        let _ = nav;
    }
    #[test]
    fn invalid_patterns_and_cancelled_scans_never_move_the_cursor() {
        let (_dir, ctx, mut app) = app("α hit");
        app.query = "(".repeat(300);
        app.quality.options.regex = true;
        app.find_open = true;
        app.find_as_you_type(&ctx);
        assert!(app.live_find.caches[0].task.is_some());
        wait(&mut app, &ctx);
        assert_eq!(app.quality.match_status, "Invalid pattern");
        assert!(app.live_find.error.is_some());
        assert_eq!(app.active().view.head, 0);
        let key = app.find_key();
        assert!(scan(&app.active().page.text, &key, None, &AtomicBool::new(true)).is_none());
    }
}
