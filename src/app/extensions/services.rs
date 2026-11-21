//! Lazy, revision-bound providers. Lua is always in a rotated native worker.
mod policy;
use super::{
    activity::{Invocation, InvocationOutcome},
    catalog,
    manifest::{Contribution, ExtensionAction, LuaPlugin},
    options,
};
use crate::{
    app::HeditApp,
    plugin_runtime::{
        self,
        service::{self, schedule::State, Action, Kind, ResultSet, Trigger},
    },
};
use eframe::egui;
use std::{
    collections::BTreeSet,
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
const MAX_PROVIDERS: usize = 32;
const ACTION_LIFETIME_MS: u64 = 120_000;
struct Job {
    invocation: Invocation,
    worker: Option<plugin_runtime::RunningService>,
    checked: Option<std::sync::mpsc::Receiver<Option<std::fs::File>>>,
    output: Option<service::Output>,
    generation: u64,
    target: super::lua::Target,
    request: String,
    document: String,
    trigger: Trigger,
}
pub(in crate::app) struct Decoration {
    pub(in crate::app) start: usize,
    pub(in crate::app) finish: usize,
    pub(in crate::app) text: String,
    pub(in crate::app) severity: Option<service::Severity>,
}
struct Ready {
    decorations: Vec<Decoration>,
    output: service::Output,
    target: super::lua::Target,
    created: u64,
}
struct Entry {
    contribution: Contribution,
    plugin: LuaPlugin,
    definition: Arc<service::Definition>,
    state: State,
    scope: Option<super::lua::Target>,
    close: Option<(super::lua::Target, String)>,
    document: String,
    job: Option<Job>,
    ready: Option<Ready>,
    choice: Option<(String, hedit::plugin_options::Value)>,
    input: Option<hedit::plugin_options::Value>,
    error: Option<String>,
}
pub(in crate::app) struct Services {
    pub(in crate::app) open: bool,
    suspended: bool,
    controller: Option<policy::Controller>,
    sequence: Option<u64>,
    entries: Vec<Entry>,
    started: Instant,
    manual: Option<String>,
    stopped: BTreeSet<String>,
    quarantined: BTreeSet<(String, String)>,
    error: Option<String>,
    saved: Option<(u64, u64)>,
    origin: Option<(u64, u64, String)>,
}
impl Default for Services {
    fn default() -> Self {
        Self {
            open: false,
            suspended: false,
            controller: None,
            sequence: None,
            entries: vec![],
            started: Instant::now(),
            manual: None,
            stopped: Default::default(),
            quarantined: Default::default(),
            error: None,
            saved: None,
            origin: None,
        }
    }
}
fn token() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "Cannot create a provider handle.")?;
    Ok(hex::encode(bytes))
}
fn decorations(output: &service::Output, text: &str) -> Vec<Decoration> {
    let items: Vec<_> = match &output.result {
        ResultSet::Diagnostics(items) => items
            .iter()
            .map(|item| (&item.range, item.message.as_str(), Some(item.severity)))
            .collect(),
        ResultSet::Presentation(presentation) => presentation
            .annotations
            .iter()
            .map(|item| (&item.range, item.text.as_str(), None))
            .collect(),
        _ => return vec![],
    };
    let ranges: Vec<_> = items
        .iter()
        .map(|(range, _, _)| plugin_runtime::sdk::Selection {
            anchor: range.start,
            head: range.finish,
        })
        .collect();
    super::lua::byte_selections(text, &ranges)
        .into_iter()
        .zip(items)
        .map(|(selection, (_, text, severity))| Decoration {
            start: selection.anchor,
            finish: selection.head,
            text: text.into(),
            severity,
        })
        .collect()
}

impl Services {
    pub(in crate::app) fn decorations(
        &self,
        document: u64,
        revision: u64,
    ) -> impl Iterator<Item = &Decoration> {
        self.entries
            .iter()
            .filter_map(move |entry| {
                entry.ready.as_ref().filter(|ready| {
                    ready.target.document() == document && ready.target.revision() == revision
                })
            })
            .flat_map(|ready| ready.decorations.iter())
    }
    pub(in crate::app) fn suspend(&mut self) {
        self.entries.clear();
        self.controller = None;
        self.sequence = None;
        self.suspended = true;
    }
    pub(in crate::app) fn invalidate(&mut self) {
        self.suspended = false;
        self.entries.clear();
        self.sequence = None;
        if let Some(controller) = &self.controller {
            controller.reload();
        }
    }
    pub(in crate::app) fn request(&mut self, id: &str) {
        self.manual = Some(id.to_owned());
        self.open = true;
    }
    pub(in crate::app) fn note_save(&mut self, document: u64, revision: u64) {
        self.saved = Some((document, revision));
    }
    pub(in crate::app) fn note_origin(&mut self, document: u64, revision: u64, owner: &str) {
        self.origin = Some((document, revision, owner.into()));
    }
    pub(in crate::app) fn summary(&self) -> Option<usize> {
        (!self.entries.is_empty() || self.error.is_some()).then(|| {
            self.entries
                .iter()
                .filter_map(|e| e.ready.as_ref())
                .map(|r| match &r.output.result {
                    ResultSet::Diagnostics(items) => items.len(),
                    _ => 0,
                })
                .sum()
        })
    }
    pub(in crate::app) fn next_wake(&self) -> Option<Duration> {
        let now = self.started.elapsed().as_millis() as u64;
        self.entries
            .iter()
            .flat_map(|entry| {
                [
                    if entry.job.is_some() {
                        Some(now.saturating_add(50))
                    } else {
                        entry.state.next_due(now)
                    },
                    entry
                        .ready
                        .as_ref()
                        .map(|ready| ready.created.saturating_add(ACTION_LIFETIME_MS)),
                ]
            })
            .flatten()
            .min()
            .map(|due| Duration::from_millis(due.saturating_sub(now)))
    }
    fn install(&mut self, contributions: Vec<Contribution>) {
        self.entries.clear();
        let mut contributions: Vec<_> = contributions
            .into_iter()
            .filter(
                |c| matches!(&c.action,ExtensionAction::Lua(plugin) if plugin.provider.is_some()),
            )
            .collect();
        contributions.sort_by(|a, b| {
            let priority = |c: &Contribution| match &c.action {
                ExtensionAction::Lua(p) => p.provider.as_ref().unwrap().priority,
                _ => 0,
            };
            priority(b).cmp(&priority(a)).then(a.id.cmp(&b.id))
        });
        if contributions.len() > MAX_PROVIDERS {
            self.error=Some("Only 32 approved providers can be active in a window. Disable unused providers to free capacity.".into());
        }
        for contribution in contributions.into_iter().take(MAX_PROVIDERS) {
            let ExtensionAction::Lua(plugin) = &contribution.action else {
                continue;
            };
            let plugin = plugin.clone();
            let definition = plugin.provider.as_ref().unwrap().clone();
            let mut state = State::default();
            state.stopped = self.stopped.contains(&contribution.id);
            state.quarantined = plugin
                .approval_id()
                .is_some_and(|token| self.quarantined.contains(&(contribution.id.clone(), token)));
            self.entries.push(Entry {
                contribution,
                plugin,
                definition,
                state,
                scope: None,
                close: None,
                document: String::new(),
                job: None,
                ready: None,
                choice: None,
                input: None,
                error: None,
            });
        }
    }
    fn poll(&mut self, app: &mut HeditApp, ctx: &egui::Context) {
        if self.suspended {
            return;
        }
        let Some(config) = app.settings_files.dir.clone() else {
            self.entries.clear();
            self.controller = None;
            return;
        };
        if self.controller.is_none()
            && app
                .extensions
                .iter()
                .any(|c| matches!(&c.action,ExtensionAction::Lua(p) if p.provider.is_some()))
        {
            match policy::Controller::start(config.clone(), ctx.clone()) {
                Ok(controller) => self.controller = Some(controller),
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(controller) = &self.controller {
            let sequence = controller.sequence.load(Ordering::Acquire);
            if self.sequence.is_some_and(|known| known != sequence) {
                self.entries.clear();
                self.sequence = None;
            }
            let mut latest = None;
            for _ in 0..4 {
                match controller.loaded.try_recv() {
                    Ok(loaded) => {
                        if loaded.sequence == sequence {
                            latest = Some(loaded);
                        }
                    }
                    Err(_) => break,
                }
            }
            if let Some(loaded) = latest {
                self.sequence = Some(loaded.sequence);
                match loaded.catalog {
                    Ok(catalog) => {
                        self.error = None;
                        app.extensions = catalog.contributions.clone();
                        self.install(catalog.contributions);
                    }
                    Err(error) => {
                        self.entries.clear();
                        self.error = Some(error);
                    }
                }
            }
        }
        let now = self.started.elapsed().as_millis() as u64;
        if self.entries.is_empty() {
            self.show(app, ctx, now);
            return;
        }
        let target = app.lua_target();
        let saved = self.saved.take();
        let selection = app.active().view.anchor != app.active().view.head
            || app
                .active()
                .view
                .selections
                .iter()
                .any(|s| s.anchor != s.head);
        let info = &app.workspace.open[app.workspace.active].page.info;
        let language = app.active().language();
        let filename = info.path.as_ref().map(|_| info.name.as_str());
        let manual = self.manual.take();
        for entry in &mut self.entries {
            let eligible = entry
                .contribution
                .when
                .matches(language, filename, selection)
                && target.is_some();
            let current = target.as_ref().filter(|_| eligible);
            let scope_changed = match (&entry.scope, current) {
                (Some(old), Some(current)) => !old.same_document(current),
                (None, None) => false,
                _ => true,
            };
            if scope_changed {
                entry.close = entry
                    .scope
                    .take()
                    .filter(|_| entry.definition.events.contains(&Trigger::Close))
                    .map(|target| (target, entry.document.clone()));
                if let Some(job) = entry.job.take() {
                    app.extension_manager
                        .record(job.invocation, InvocationOutcome::Cancelled);
                }
                entry.ready = None;
                entry.choice = None;
                entry.input = None;
                entry.state.change_scope(now);
                entry.scope = current.cloned();
                entry.document = match token() {
                    Ok(token) => token,
                    Err(error) => {
                        entry.error = Some(error);
                        entry.state.stop(now);
                        String::new()
                    }
                };
                if entry.close.is_some() {
                    entry.state.queue(Trigger::Close, now, 0);
                } else if current.is_some() && entry.definition.events.contains(&Trigger::Open) {
                    entry
                        .state
                        .queue(Trigger::Open, now, entry.definition.debounce_ms);
                }
            } else if entry.close.is_none() {
                if let (Some(old), Some(current)) = (&entry.scope, current) {
                    if old.revision() != current.revision() {
                        entry.ready = None;
                        if let Some(job) = entry.job.take() {
                            app.extension_manager
                                .record(job.invocation, InvocationOutcome::Cancelled);
                        }
                        entry.state.invalidate_request(now);
                        entry.choice = None;
                        let own = self.origin.as_ref().is_some_and(|(id, rev, owner)| {
                            *id == current.document()
                                && *rev == current.revision()
                                && owner == &entry.plugin.plugin_id
                        });
                        if !own && entry.definition.events.contains(&Trigger::Change) {
                            entry
                                .state
                                .queue(Trigger::Change, now, entry.definition.debounce_ms);
                        }
                    } else if old != current
                        && entry.definition.events.contains(&Trigger::Selection)
                    {
                        entry
                            .state
                            .queue(Trigger::Selection, now, entry.definition.debounce_ms);
                    }
                    if matches!(
                        entry.definition.kind,
                        Kind::Completions | Kind::Hover | Kind::CodeActions | Kind::Formatting
                    ) && old != current
                    {
                        entry.ready = None;
                    }
                    entry.scope = Some(current.clone());
                }
            }
            if current.is_none() || entry.close.is_some() {
                continue;
            }
            let own = self.origin.as_ref().is_some_and(|(id, rev, owner)| {
                *id == current.unwrap().document()
                    && *rev == current.unwrap().revision()
                    && owner == &entry.plugin.plugin_id
            });
            if !scope_changed
                && !own
                && saved == Some((current.unwrap().document(), current.unwrap().revision()))
                && entry.definition.events.contains(&Trigger::Save)
            {
                entry
                    .state
                    .queue(Trigger::Save, now, entry.definition.debounce_ms);
            }
            if manual.as_deref() == Some(entry.contribution.id.as_str())
                && entry.definition.events.contains(&Trigger::Manual)
            {
                entry.state.queue(Trigger::Manual, now, 0);
            }
            if entry.state.superseded() {
                if let Some(job) = entry.job.take() {
                    app.extension_manager
                        .record(job.invocation, InvocationOutcome::Cancelled);
                }
                entry.state.cancel(now);
            }
            entry.state.expire_idle(now);
            if entry
                .ready
                .as_ref()
                .is_some_and(|ready| now.saturating_sub(ready.created) >= ACTION_LIFETIME_MS)
            {
                entry.ready = None;
                entry.input = None;
            }
        }
        if let Some(id) = manual {
            if !self.entries.iter().any(|entry| entry.contribution.id == id) {
                self.error =
                    Some("This provider is unapproved, unavailable, or still loading.".into());
            }
        }
        let mut effect = None;
        for entry in &mut self.entries {
            let Some(job) = entry.job.as_mut() else {
                continue;
            };
            let result = if let Some(worker) = job.worker.as_mut() {
                worker.try_result()
            } else {
                None
            };
            if result.is_some() {
                if let Some(worker) = &job.worker {
                    for sample in worker.timings().snapshot() {
                        job.invocation
                            .trace
                            .record(sample.phase, Duration::from_micros(sample.total_us));
                    }
                }
            }
            let result = match result {
                Some(Ok(output)) => {
                    job.worker = None;
                    let plugin = entry.plugin.clone();
                    let config = config.clone();
                    let wake = ctx.clone();
                    let (sender, receiver) = std::sync::mpsc::channel();
                    match std::thread::Builder::new()
                        .name("hedit-provider-publish".into())
                        .spawn(move || {
                            let _ = sender.send(catalog::lua_authorization(&config, &plugin));
                            wake.request_repaint();
                        }) {
                        Ok(_) => {
                            job.checked = Some(receiver);
                            job.output = Some(output);
                            None
                        }
                        Err(_) => Some(Err(plugin_runtime::Error::Startup.into())),
                    }
                }
                result => result,
            };
            let (result, _authority) = if let Some(result) = result {
                (Some(result), None)
            } else if let Some(receiver) = &job.checked {
                match receiver.try_recv() {
                    Ok(Some(authority)) => (Some(Ok(job.output.take().unwrap())), Some(authority)),
                    Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        (Some(Err(plugin_runtime::Error::Cancelled.into())), None)
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => (None, None),
                }
            } else {
                (None, None)
            };
            if let Some(result) = result {
                let job = entry.job.take().unwrap();
                let valid = job.trigger == Trigger::Close
                    || target.as_ref().is_some_and(|current| {
                        job.target.same_document(current)
                            && job.target.revision() == current.revision()
                            && (!matches!(
                                entry.definition.kind,
                                Kind::Completions
                                    | Kind::Hover
                                    | Kind::CodeActions
                                    | Kind::Formatting
                            ) || &job.target == current)
                    });
                let mut outcome = match &result {
                    Err(error) => InvocationOutcome::worker_failure(error.kind),
                    _ => InvocationOutcome::Cancelled,
                };
                match result {
                    Ok(output)
                        if valid
                            && output.request == job.request
                            && output.document == job.document =>
                    {
                        if entry
                            .state
                            .finish(job.generation, now, true, Some(output.cache.clone()))
                        {
                            outcome = InvocationOutcome::Published;
                            entry.error = None;
                            if job.trigger == Trigger::Close {
                                entry.close = None;
                                entry.state.cache = Default::default();
                                if entry.scope.is_some()
                                    && entry.definition.events.contains(&Trigger::Open)
                                {
                                    entry.state.queue(
                                        Trigger::Open,
                                        now,
                                        entry.definition.debounce_ms,
                                    );
                                }
                                app.extension_manager.record(job.invocation, outcome);
                                continue;
                            }
                            if let ResultSet::Formatting(transaction) = &output.result {
                                if job.trigger == Trigger::Manual {
                                    effect = Some((
                                        entry.contribution.id.clone(),
                                        entry.plugin.clone(),
                                        output.document.clone(),
                                        transaction.clone(),
                                    ));
                                }
                            }
                            entry.ready = Some(Ready {
                                decorations: decorations(&output, &app.active().page.text),
                                output,
                                target: job.target,
                                created: now,
                            });
                        }
                    }
                    Ok(_) => {
                        entry.state.cancel(now);
                    }
                    Err(error) => {
                        let cancelled = error.kind == plugin_runtime::Error::Cancelled;
                        if job.trigger == Trigger::Close {
                            entry.close = None;
                        }
                        if cancelled {
                            entry.state.cancel(now);
                        } else {
                            entry.state.finish(job.generation, now, false, None);
                            entry.error = Some(error.to_string());
                        }
                        if entry.state.quarantined {
                            if let Some(token) = entry.plugin.approval_id() {
                                self.quarantined
                                    .insert((entry.contribution.id.clone(), token));
                            }
                        }
                    }
                }
                app.extension_manager.record(job.invocation, outcome);
            }
        }
        let mut busy: BTreeSet<_> = self
            .entries
            .iter()
            .filter(|e| e.job.is_some())
            .map(|e| e.plugin.plugin_id.clone())
            .collect();
        for entry in &mut self.entries {
            if busy.len() >= service::MAX_ACTIVE {
                break;
            }
            if busy.contains(&entry.plugin.plugin_id)
                || (entry.scope.is_none() && entry.close.is_none())
            {
                continue;
            }
            if !plugin_runtime::service_available() {
                entry.state.defer(now, 100);
                continue;
            }
            let Some(pending) = entry.state.begin(now) else {
                continue;
            };
            let closing = pending.trigger == Trigger::Close;
            let Some(current) = entry
                .close
                .as_ref()
                .filter(|_| closing)
                .map(|close| &close.0)
                .or(target.as_ref())
            else {
                entry.state.cancel(now);
                continue;
            };
            let document = entry
                .close
                .as_ref()
                .filter(|_| closing)
                .map_or(&entry.document, |close| &close.1)
                .clone();
            let text = if closing { "" } else { &app.active().page.text };
            if text.len() > plugin_runtime::MAX_SELECTION_BYTES {
                entry.state.cancel(now);
                entry.error = Some("Document exceeds the provider snapshot limit.".into());
                continue;
            }
            let mut snapshot = match options::sdk_snapshot(
                current.language(),
                text.to_owned(),
                if closing {
                    vec![plugin_runtime::sdk::Selection { anchor: 0, head: 0 }]
                } else {
                    super::lua::snapshot_selections(text, &app.selections())
                },
                !closing && app.active().page.info.eol == "CRLF",
            ) {
                Ok(s) => s,
                Err(error) => {
                    entry.state.cancel(now);
                    entry.error = Some(error.to_string());
                    continue;
                }
            };
            snapshot.document = document.clone();
            snapshot.revision = current.revision().to_string();
            let request_id = match token() {
                Ok(id) => id,
                Err(error) => {
                    entry.state.cancel(now);
                    entry.error = Some(error);
                    continue;
                }
            };
            let choice = if pending.trigger == Trigger::Interaction {
                entry.choice.clone()
            } else {
                None
            };
            let input = service::Input {
                protocol_version: service::VERSION,
                request: service::Request {
                    id: request_id.clone(),
                    kind: entry.definition.kind,
                    trigger: pending.trigger,
                    position: snapshot.selections[0].head,
                    action: choice.as_ref().map(|c| c.0.clone()),
                    choice: choice.map(|c| c.1),
                },
                package: options::sdk_input(
                    &app.settings.extensions,
                    &entry.plugin,
                    snapshot,
                    Default::default(),
                    None,
                ),
                cache: if closing {
                    Default::default()
                } else {
                    entry.state.cache.clone()
                },
                may_edit: !closing && entry.definition.may_edit,
            };
            let plugin = entry.plugin.clone();
            let config = config.clone();
            let wake = ctx.clone();
            match plugin_runtime::start_service_checked(
                input,
                move |cancelled| {
                    if cancelled.load(Ordering::Acquire) {
                        return Err(plugin_runtime::Error::Cancelled);
                    }
                    catalog::lua_authorization(&config, &plugin)
                        .ok_or(plugin_runtime::Error::Cancelled)
                },
                move || wake.request_repaint(),
            ) {
                Ok(worker) => {
                    entry.choice = None;
                    entry.job = Some(Job {
                        invocation: Invocation::provider(&entry.contribution.id),
                        worker: Some(worker),
                        checked: None,
                        output: None,
                        generation: pending.generation,
                        target: current.clone(),
                        request: request_id,
                        document,
                        trigger: pending.trigger,
                    });
                    busy.insert(entry.plugin.plugin_id.clone());
                }
                Err(error) => {
                    entry.state.cancel(now);
                    if error == plugin_runtime::Error::ServiceBusy {
                        entry.state.queue(pending.trigger, now, 150);
                        entry.state.defer(now, 100);
                    } else {
                        entry.error = Some(error.to_string());
                    }
                }
            }
        }
        if let Some((id, plugin, document, transaction)) = effect {
            self.open = false;
            if let Err(error) = app.propose_provider_edit(ctx, &id, plugin, &document, transaction)
            {
                app.error = Some(error);
            }
        }
        self.show(app, ctx, now);
        if let Some(delay) = self.next_wake() {
            ctx.request_repaint_after(delay.max(Duration::from_millis(10)));
        }
    }

    fn show(&mut self, app: &mut HeditApp, ctx: &egui::Context, now: u64) {
        if !self.open {
            return;
        }
        let mut open = true;
        let mut chosen = None;
        egui::Window::new("Extension results")
            .id(egui::Id::new("plugin-results"))
            .open(&mut open)
            .default_width(480.0)
            .show(ctx, |ui| {
                if let Some(error) = &self.error {
                    ui.label(error);
                }
                egui::ScrollArea::vertical()
                    .max_height(520.0)
                    .show(ui, |ui| {
                        for entry in &mut self.entries {
                            ui.push_id(&entry.contribution.id, |ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(&entry.contribution.title);
                                    if entry.state.stopped || entry.state.quarantined {
                                        if ui.button("Resume").clicked() {
                                            self.stopped.remove(&entry.contribution.id);
                                            if let Some(token) = entry.plugin.approval_id() {
                                                self.quarantined.remove(&(
                                                    entry.contribution.id.clone(),
                                                    token,
                                                ));
                                            }
                                            entry.state.resume();
                                            entry.scope = None;
                                            entry.error = None;
                                        }
                                    } else if ui.button("Stop").clicked() {
                                        if let Some(job) = entry.job.take() {
                                            app.extension_manager.record(
                                                job.invocation,
                                                InvocationOutcome::Cancelled,
                                            );
                                        }
                                        entry.close = None;
                                        entry.ready = None;
                                        entry.choice = None;
                                        entry.input = None;
                                        entry.state.stop(now);
                                        self.stopped.insert(entry.contribution.id.clone());
                                    }
                                });
                                ui.weak(&entry.plugin.plugin_id);
                                if entry.state.quarantined {
                                    ui.label("Stopped after repeated failures. Resume to retry.");
                                }
                                if let Some(error) = &entry.error {
                                    ui.label(error);
                                }
                                if entry.job.is_some() {
                                    ui.spinner();
                                }
                                let Some(ready) = &entry.ready else {
                                    return;
                                };
                                let mut button =
                                    |ui: &mut egui::Ui, title: &str, action: Action| {
                                        if ui.button(title).clicked() {
                                            chosen = Some((
                                                entry.contribution.id.clone(),
                                                ready.output.request.clone(),
                                                action,
                                            ));
                                        }
                                    };
                                match &ready.output.result {
                                    ResultSet::Closed => {}
                                    ResultSet::Diagnostics(items) => {
                                        for item in items {
                                            ui.label(format!(
                                                "{:?}: {}",
                                                item.severity, item.message
                                            ));
                                            for action in &item.actions {
                                                button(ui, &action.title, action.action.clone());
                                            }
                                        }
                                    }
                                    ResultSet::Completions(items) => {
                                        for item in items {
                                            let transaction = plugin_runtime::sdk::Transaction {
                                                document: ready.output.document.clone(),
                                                revision: ready.output.revision.clone(),
                                                edits: vec![plugin_runtime::sdk::Edit {
                                                    start: item.range.start,
                                                    finish: item.range.finish,
                                                    text: item.insert.clone(),
                                                }],
                                                selections: None,
                                                storage: None,
                                            };
                                            button(ui, &item.label, Action::Edit(transaction));
                                            if !item.detail.is_empty() {
                                                ui.weak(&item.detail);
                                            }
                                        }
                                    }
                                    ResultSet::Hover(item) => {
                                        ui.label(&item.text);
                                    }
                                    ResultSet::Symbols(items) => {
                                        for item in items {
                                            button(
                                                ui,
                                                &item.name,
                                                Action::Select(plugin_runtime::sdk::Selection {
                                                    anchor: item.range.start,
                                                    head: item.range.finish,
                                                }),
                                            );
                                            ui.weak(&item.detail);
                                        }
                                    }
                                    ResultSet::Formatting(transaction) => button(
                                        ui,
                                        "Review formatting",
                                        Action::Edit(transaction.clone()),
                                    ),
                                    ResultSet::CodeActions(items) => {
                                        for item in items {
                                            button(ui, &item.title, item.action.clone());
                                        }
                                    }
                                    ResultSet::Presentation(presentation) => {
                                        if let Some(status) = &presentation.status {
                                            ui.label(status);
                                        }
                                        if let Some(output) = &presentation.output {
                                            ui.monospace(output);
                                        }
                                        for item in &presentation.annotations {
                                            button(
                                                ui,
                                                &item.text,
                                                Action::Select(plugin_runtime::sdk::Selection {
                                                    anchor: item.range.start,
                                                    head: item.range.finish,
                                                }),
                                            );
                                        }
                                        if let Some(list) = &presentation.quick_list {
                                            ui.label(&list.title);
                                            for item in &list.items {
                                                button(ui, &item.title, item.action.clone());
                                            }
                                        }
                                        if let Some(prompt) = &presentation.input {
                                            ui.label(&prompt.title);
                                            let value = entry
                                                .input
                                                .get_or_insert_with(|| prompt.default.clone());
                                            if prompt.choices.is_empty() {
                                                match value {
                                                    hedit::plugin_options::Value::Text(text) => {
                                                        ui.add(
                                                            egui::TextEdit::singleline(text)
                                                                .char_limit(256),
                                                        );
                                                    }
                                                    hedit::plugin_options::Value::Integer(
                                                        number,
                                                    ) => {
                                                        ui.add(
                                                            egui::DragValue::new(number)
                                                                .range(i32::MIN..=i32::MAX),
                                                        );
                                                    }
                                                    hedit::plugin_options::Value::Boolean(
                                                        value,
                                                    ) => {
                                                        ui.checkbox(value, "Enabled");
                                                    }
                                                }
                                            } else {
                                                for option in &prompt.choices {
                                                    ui.selectable_value(
                                                        value,
                                                        option.clone(),
                                                        format!("{option:?}"),
                                                    );
                                                }
                                            }
                                            if ui.button("Submit").clicked() {
                                                chosen = Some((
                                                    entry.contribution.id.clone(),
                                                    ready.output.request.clone(),
                                                    Action::Callback {
                                                        name: prompt.name.clone(),
                                                        value: value.clone(),
                                                    },
                                                ));
                                            }
                                        }
                                    }
                                }
                                ui.separator();
                            });
                        }
                    });
            });
        self.open = open;
        if let Some((id, request, action)) = chosen {
            let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.contribution.id == id)
            else {
                return;
            };
            let Some(ready) = &entry.ready else {
                return;
            };
            if ready.output.request != request
                || now.saturating_sub(ready.created) >= ACTION_LIFETIME_MS
                || app.lua_target().as_ref() != Some(&ready.target)
            {
                entry.ready = None;
                return;
            }
            match action {
                Action::Callback { name, value } => {
                    entry.choice = Some((name, value));
                    entry.state.queue(Trigger::Interaction, now, 0);
                    entry.ready = None;
                    entry.input = None;
                }
                Action::Edit(transaction) => {
                    let plugin = entry.plugin.clone();
                    let document = ready.output.document.clone();
                    entry.ready = None;
                    self.open = false;
                    if let Err(error) =
                        app.propose_provider_edit(ctx, &id, plugin, &document, transaction)
                    {
                        app.error = Some(error);
                    }
                }
                Action::Select(selection) => {
                    let transaction = plugin_runtime::sdk::Transaction {
                        document: ready.output.document.clone(),
                        revision: ready.output.revision.clone(),
                        edits: vec![],
                        selections: Some(vec![selection]),
                        storage: None,
                    };
                    let plugin = entry.plugin.clone();
                    let document = ready.output.document.clone();
                    entry.ready = None;
                    self.open = false;
                    if let Err(error) =
                        app.propose_provider_edit(ctx, &id, plugin, &document, transaction)
                    {
                        app.error = Some(error);
                    }
                }
            }
        }
    }
}
impl HeditApp {
    pub(in crate::app) fn poll_plugin_services(&mut self, ctx: &egui::Context) {
        let mut services = std::mem::take(&mut self.plugin_services);
        services.poll(self, ctx);
        self.plugin_services = services;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{extensions::tests::app_with, Command};
    fn provider_app(
        config: &std::path::Path,
        text: &str,
    ) -> (tempfile::TempDir, egui::Context, HeditApp) {
        let directory = config.join("extensions");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("document-assist.json");
        let mut manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../examples/extensions/document-assist.json"
        ))
        .unwrap();
        for command in manifest["commands"].as_array_mut().unwrap() {
            command["provider"]["events"] = serde_json::json!(["manual"]);
        }
        let source = manifest.to_string();
        std::fs::write(&path, &source).unwrap();
        catalog::set_lua_approved(
            config,
            &path,
            "document-assist",
            "document-assist.diagnostics",
            &super::super::manifest::digest(&source),
            true,
        )
        .unwrap();
        app_with(text, Some(config))
    }
    fn finish(app: &mut HeditApp, ctx: &egui::Context) {
        let until = Instant::now() + Duration::from_secs(3);
        while app.lua_transform.is_some() {
            assert!(Instant::now() < until);
            app.poll_lua_transform(ctx);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn provider_quick_fixes_recheck_authority_reject_stale_revisions_and_undo_once() {
        for revoke in [false, true] {
            let config = tempfile::tempdir().unwrap();
            let (_session, ctx, mut app) = provider_app(config.path(), "café  \r\n東京");
            let ExtensionAction::Lua(plugin) = app.extensions[0].action.clone() else {
                panic!()
            };
            let transaction = plugin_runtime::sdk::Transaction {
                document: "a".repeat(32),
                revision: app.active().page.info.revision.to_string(),
                edits: vec![plugin_runtime::sdk::Edit {
                    start: 5,
                    finish: 7,
                    text: String::new(),
                }],
                selections: None,
                storage: None,
            };
            let mut stale = transaction.clone();
            stale.revision = "18446744073709551615".into();
            assert!(app
                .propose_provider_edit(
                    &ctx,
                    "document-assist.diagnostics",
                    plugin.clone(),
                    &"a".repeat(32),
                    stale
                )
                .is_err());
            assert!(app.lua_transform.is_none());
            app.settings.extensions.preview_edits = false;
            if revoke {
                catalog::set_lua_approved(
                    config.path(),
                    &config.path().join("extensions/document-assist.json"),
                    "document-assist",
                    "document-assist.diagnostics",
                    &super::super::manifest::digest(
                        std::fs::read(config.path().join("extensions/document-assist.json"))
                            .unwrap(),
                    ),
                    false,
                )
                .unwrap();
            }
            app.propose_provider_edit(
                &ctx,
                "document-assist.diagnostics",
                plugin,
                &"a".repeat(32),
                transaction,
            )
            .unwrap();
            finish(&mut app, &ctx);
            if revoke {
                assert_eq!(app.active().page.text, "café  \r\n東京");
            } else {
                assert_eq!(app.active().page.text, "café\r\n東京");
                app.execute(&ctx, Command::Undo);
                crate::app::tests::finish_app_job(&mut app, &ctx);
                assert_eq!(app.active().page.text, "café  \r\n東京");
            }
        }
    }
    #[test]
    fn invalidation_drops_cache_results_and_pending_work_without_touching_text() {
        let config = tempfile::tempdir().unwrap();
        let (_session, _ctx, app) = provider_app(config.path(), "private fixture");
        let mut services = Services::default();
        services.install(app.extensions.clone());
        assert_eq!(services.entries.len(), 1);
        services.entries[0].state.cache = plugin_runtime::sdk::Storage(
            [("counter".into(), hedit::plugin_options::Value::Integer(1))].into(),
        );
        services.entries[0].state.queue(Trigger::Manual, 0, 150);
        services.invalidate();
        assert!(services.entries.is_empty());
        assert!(services.next_wake().is_none());
        assert_eq!(app.active().page.text, "private fixture");
    }
}
