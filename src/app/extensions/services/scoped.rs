//! SDK 3 provider transport and lifecycle. Scope and approval remain host-owned.
use super::*;
use crate::app::extensions::{authority::Receipt, broker, lua};
use hedit_broker::{
    declaration::Capability,
    host::{Event, Host},
    plan::Plan,
};
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Default)]
pub(super) struct Scoped {
    initialized: bool,
    run: Option<Run>,
    ready: Option<Ready>,
    native_services: Arc<hedit_broker::lsp::Registry>,
    lifetime: Option<hedit::lifetime::Lease>,
}
struct Preview {
    plan: Arc<Plan>,
    file: usize,
    before: usize,
    after: usize,
}
struct Run {
    context: Option<Arc<broker::Context>>,
    host: Option<Host>,
    checking: Option<Receiver<Option<Receipt>>>,
    output: Option<Box<ResultSet>>,
    cache: plugin_runtime::sdk::Storage,
    preview: Option<Preview>,
    generation: u64,
    trigger: Trigger,
    work: u64,
    invocation: Option<Invocation>,
}
impl Drop for Run {
    fn drop(&mut self) {
        if let Some(context) = &self.context {
            context.close();
        }
    }
}
struct Ready {
    context: Arc<broker::Context>,
    result: Box<ResultSet>,
    decorations: Vec<Decoration>,
    target: Option<lua::Target>,
    request: u128,
    created: u64,
}
impl Drop for Ready {
    fn drop(&mut self) {
        self.context.close();
    }
}
pub(super) struct Tick<'a> {
    pub config: &'a std::path::Path,
    pub monitor: &'a policy::Monitor,
    pub target: Option<&'a lua::Target>,
    pub manual: Option<&'a str>,
    pub saved: Option<(u64, u64)>,
    pub origin: Option<&'a (u64, u64, String)>,
    pub now: u64,
    pub allow_start: bool,
}
impl Scoped {
    pub(super) fn reset(&mut self) {
        self.initialized = false;
    }
    pub(super) fn running(&self) -> bool {
        self.run.is_some()
    }
    pub(super) fn diagnostic_count(&self) -> usize {
        self.ready
            .as_ref()
            .map_or(0, |ready| match ready.result.as_ref() {
                ResultSet::Diagnostics(items) => items.len(),
                _ => 0,
            })
    }
    pub(super) fn reviewing(&self) -> bool {
        self.run.as_ref().is_some_and(|run| run.preview.is_some())
    }
    pub(super) fn expires(&self) -> Option<u64> {
        self.ready
            .as_ref()
            .map(|ready| ready.created.saturating_add(ACTION_LIFETIME_MS))
    }
    pub(super) fn source(&self, document: Option<(u64, u64)>) -> Option<u128> {
        self.ready
            .as_ref()
            .filter(|ready| {
                ready
                    .target
                    .as_ref()
                    .is_some_and(|t| document == Some((t.document(), t.revision())))
            })
            .map(|ready| ready.request)
    }
    pub(super) fn decorations(&self, document: Option<(u64, u64)>) -> &[Decoration] {
        self.ready
            .as_ref()
            .filter(|ready| {
                ready
                    .target
                    .as_ref()
                    .is_some_and(|t| document == Some((t.document(), t.revision())))
            })
            .map_or(&[], |ready| &ready.decorations)
    }
    pub(super) fn stop(&mut self, app: &mut HeditApp) {
        self.native_services.stop();
        self.native_services = Default::default();
        self.stop_run(app);
    }
    fn stop_run(&mut self, app: &mut HeditApp) {
        if let Some(mut run) = self.run.take() {
            if let Some(invocation) = run.invocation.take() {
                app.extension_manager.record(
                    invocation,
                    if run.context.as_ref().is_some_and(|c| c.applied()) {
                        InvocationOutcome::Applied
                    } else {
                        InvocationOutcome::Cancelled
                    },
                );
            }
        }
        self.ready = None;
    }
    fn poll(
        &mut self,
        entry: &mut Entry,
        app: &mut HeditApp,
        ctx: &egui::Context,
        tick: &Tick<'_>,
    ) {
        let now = tick.now;
        let reads = entry
            .plugin
            .broker
            .as_ref()
            .unwrap()
            .has(Capability::DocumentRead);
        let target = tick.target.filter(|_| reads);
        let lifetime = target.and_then(|target| {
            app.workspace
                .editor
                .document_lifetime(target.document())
                .ok()
        });
        let info = &app.active().page.info;
        let selection = app.selections().iter().any(|s| s.anchor != s.head);
        let eligible = entry.contribution.when.matches(
            app.active().language(),
            info.path.as_ref().map(|_| info.name.as_str()),
            selection,
        ) && (!reads || lifetime.is_some());
        if self
            .run
            .as_ref()
            .and_then(|run| run.context.as_ref())
            .is_some_and(|context| context.committing())
        {
            self.poll_run(entry, app, ctx, now);
            return;
        }
        let same_lifetime = match (&self.lifetime, &lifetime) {
            (Some(old), Some(new)) => old.same(new),
            (None, None) => true,
            _ => false,
        };
        let changed = !self.initialized
            || !same_lifetime
            || match (&entry.scope, target.filter(|_| eligible)) {
                (Some(old), Some(new)) => !old.same_document(new),
                (None, None) => false,
                _ => true,
            };
        if changed {
            self.stop(app);
            entry.close = entry
                .scope
                .take()
                .filter(|_| entry.definition.events.contains(&Trigger::Close))
                .map(|old| (old, entry.document.clone()));
            entry.state.change_scope(now);
            entry.scope = target.filter(|_| eligible).cloned();
            entry.document = match token() {
                Ok(id) => id,
                Err(error) => {
                    entry.error = Some(error);
                    entry.state.stop(now);
                    return;
                }
            };
            self.lifetime = lifetime;
            self.initialized = true;
            if entry.close.is_some() {
                entry.state.queue(Trigger::Close, now, 0);
            } else if eligible && entry.definition.events.contains(&Trigger::Open) {
                entry
                    .state
                    .queue(Trigger::Open, now, entry.definition.debounce_ms);
            }
        } else if entry.close.is_none() {
            if let (Some(old), Some(current)) = (&entry.scope, target) {
                if old.revision() != current.revision() {
                    self.stop_run(app);
                    entry.state.invalidate_request(now);
                    entry.choice = None;
                    if !own(tick, current, &entry.plugin.plugin_id)
                        && entry.definition.events.contains(&Trigger::Change)
                    {
                        entry
                            .state
                            .queue(Trigger::Change, now, entry.definition.debounce_ms);
                    }
                } else if old != current {
                    if sensitive(entry.definition.kind) {
                        self.stop_run(app);
                        entry.state.invalidate_request(now);
                    }
                    if entry.definition.events.contains(&Trigger::Selection) {
                        entry
                            .state
                            .queue(Trigger::Selection, now, entry.definition.debounce_ms);
                    }
                }
                entry.scope = Some(current.clone());
            }
        }
        if !eligible && entry.close.is_none() {
            self.stop(app);
            entry.state.invalidate_request(now);
            return;
        }
        if entry.close.is_none() {
            if target.is_some_and(|t| {
                tick.saved == Some((t.document(), t.revision()))
                    && !own(tick, t, &entry.plugin.plugin_id)
            }) && entry.definition.events.contains(&Trigger::Save)
            {
                entry
                    .state
                    .queue(Trigger::Save, now, entry.definition.debounce_ms);
            }
            if tick.manual == Some(entry.contribution.id.as_str())
                && entry.definition.events.contains(&Trigger::Manual)
            {
                entry.state.queue(Trigger::Manual, now, 0);
            }
        }
        if entry.state.superseded() {
            self.stop_run(app);
            entry.state.cancel(now);
        }
        entry.state.expire_idle(now);
        if self.ready.as_ref().is_some_and(|r| {
            !r.context.current() || now.saturating_sub(r.created) >= ACTION_LIFETIME_MS
        }) {
            self.ready = None;
            entry.input = None;
        }
        self.poll_run(entry, app, ctx, now);
        if self.run.is_none() && tick.allow_start {
            if let Some(pending) = entry.state.begin(now) {
                match self.start(entry, app, ctx, tick, pending) {
                    Ok(run) => {
                        self.ready = None;
                        entry.choice = None;
                        self.run = Some(run);
                    }
                    Err(StartFailure::Busy) => {
                        entry.state.cancel(now);
                        entry.state.queue(pending.trigger, now, 0);
                        entry.state.defer(now, 100);
                    }
                    Err(StartFailure::Failed(error)) => {
                        entry.state.finish(pending.generation, now, false, None);
                        entry.error = Some(error);
                    }
                }
            }
        }
    }
    fn start(
        &self,
        entry: &Entry,
        app: &HeditApp,
        ctx: &egui::Context,
        tick: &Tick<'_>,
        pending: service::schedule::Pending,
    ) -> Result<Run, StartFailure> {
        let reads = entry
            .plugin
            .broker
            .as_ref()
            .unwrap()
            .has(Capability::DocumentRead);
        let closing = pending.trigger == Trigger::Close;
        let target = if closing {
            entry.close.as_ref().map(|close| &close.0)
        } else {
            entry.scope.as_ref()
        };
        let text = if reads && !closing {
            app.active().page.text.clone()
        } else {
            String::new()
        };
        if text.len() > plugin_runtime::MAX_SELECTION_BYTES {
            return Err("Document exceeds the provider snapshot limit.".into());
        }
        let mut snapshot = options::sdk_snapshot(
            if reads {
                target
                    .ok_or("Provider document is unavailable.")?
                    .language()
            } else {
                "plain_text"
            },
            text.clone(),
            if reads && !closing {
                lua::snapshot_selections(&text, &app.selections())
            } else {
                vec![plugin_runtime::sdk::Selection { anchor: 0, head: 0 }]
            },
            reads && !closing && app.active().page.info.eol == "CRLF",
        )
        .map_err(|e| e.to_string())?;
        snapshot.document = if closing {
            entry.close.as_ref().unwrap().1.clone()
        } else {
            entry.document.clone()
        };
        snapshot.revision = if reads {
            target.unwrap().revision().to_string()
        } else {
            "0".into()
        };
        let choice = (pending.trigger == Trigger::Interaction)
            .then(|| entry.choice.clone())
            .flatten();
        let request = service::Request {
            id: token()?,
            kind: entry.definition.kind,
            trigger: pending.trigger,
            position: snapshot.selections[0].head,
            action: choice.as_ref().map(|c| c.0.clone()),
            choice: choice.map(|c| c.1),
        };
        let context = broker::Context::new(
            tick.config.to_owned(),
            entry.contribution.id.clone(),
            entry.plugin.clone(),
            options::sdk_input(
                &app.settings.extensions,
                &entry.plugin,
                snapshot,
                Default::default(),
                None,
            ),
            if closing { None } else { target.cloned() },
            app.workspace.editor.clone(),
            tick.monitor.clone(),
        );
        context.share_services(self.native_services.clone())?;
        let host = context.start(
            app,
            ctx,
            if closing {
                Default::default()
            } else {
                entry.state.cache.clone()
            },
            Some(request),
            Default::default(),
        )?;
        Ok(Run {
            context: Some(context),
            host: Some(host),
            checking: None,
            output: None,
            cache: Default::default(),
            preview: None,
            generation: pending.generation,
            trigger: pending.trigger,
            work: 0,
            invocation: Some(Invocation::scoped_provider(&entry.contribution.id)),
        })
    }
    fn poll_run(&mut self, entry: &mut Entry, app: &mut HeditApp, ctx: &egui::Context, now: u64) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        let context = run.context.as_ref().unwrap().clone();
        let result = (|| -> Result<bool, String> {
            if !context.current() {
                return Err("Provider scope or approval changed.".into());
            }
            if let Some(host) = &run.host {
                if let Some(event) = host.try_event() {
                    match event {
                        Event::Review(plan) => {
                            run.preview = Some(Preview {
                                plan,
                                file: 0,
                                before: 0,
                                after: 0,
                            });
                            entry.state.pause_work(now);
                        }
                        Event::Failed(code) => {
                            return Err(format!("Provider stopped: {}.", broker::failure(code)))
                        }
                        Event::Finished {
                            transaction,
                            result,
                            cache,
                        } => {
                            if transaction.is_some() {
                                return Err(
                                    "A provider returned an ordinary-command transaction.".into()
                                );
                            }
                            run.work =
                                host.work_duration().as_millis().min(u64::MAX as u128) as u64;
                            run.output = result;
                            run.cache = cache;
                            run.checking = Some(context.result_authority(ctx)?);
                            run.host = None;
                            run.preview = None;
                        }
                    }
                }
            }
            let Some(checking) = &run.checking else {
                return Ok(false);
            };
            let receipt = match checking.try_recv() {
                Ok(Some(receipt)) => receipt
                    .take()
                    .ok_or("Provider publication authorization expired.")?,
                Err(TryRecvError::Empty) => return Ok(false),
                _ => return Err("Provider approval changed before publication.".into()),
            };
            if !context.current() {
                return Err("Provider context changed before publication.".into());
            }
            let result = run.output.take().ok_or("Provider result is missing.")?;
            if !entry.state.finish_with_work(
                run.generation,
                now,
                run.work,
                true,
                Some(std::mem::take(&mut run.cache)),
            ) {
                return Err("Provider request was superseded.".into());
            }
            entry.error = None;
            if run.trigger == Trigger::Close {
                entry.close = None;
                entry.state.cache = Default::default();
                if entry.scope.is_some() && entry.definition.events.contains(&Trigger::Open) {
                    entry
                        .state
                        .queue(Trigger::Open, now, entry.definition.debounce_ms);
                }
            } else {
                if let ResultSet::Formatting(transaction) = result.as_ref() {
                    if run.trigger == Trigger::Manual {
                        app.propose_broker_edit(ctx, &context, transaction.clone())?;
                    }
                }
                let snapshot = context.snapshot();
                let paint = decorations_result(&result, &snapshot.text);
                let request =
                    u128::from_str_radix(&token()?, 16).map_err(|_| "Invalid provider handle.")?;
                self.ready = Some(Ready {
                    context: run.context.take().unwrap(),
                    result,
                    decorations: paint,
                    target: entry.scope.clone(),
                    request,
                    created: now,
                });
                entry.input = None;
                entry.output_page = 0;
            }
            drop(receipt);
            Ok(true)
        })();
        match result {
            Ok(false) => self.run = Some(run),
            result => {
                let applied = context.applied();
                let outcome = if applied {
                    InvocationOutcome::Applied
                } else if result.is_ok() {
                    InvocationOutcome::Published
                } else {
                    InvocationOutcome::Failed
                };
                if let Some(invocation) = run.invocation.take() {
                    app.extension_manager.record(invocation, outcome);
                }
                if let Err(error) = result {
                    let work = run.host.as_ref().map_or(run.work, |host| {
                        host.work_duration().as_millis().min(u64::MAX as u128) as u64
                    });
                    entry
                        .state
                        .finish_with_work(run.generation, now, work, applied, None);
                    entry.error = Some(if applied {
                        format!("Workspace edit applied; {error}")
                    } else {
                        error
                    });
                    entry.close = None;
                }
            }
        }
    }
}
fn sensitive(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Completions | Kind::Hover | Kind::CodeActions | Kind::Formatting
    )
}
fn own(tick: &Tick<'_>, target: &lua::Target, plugin: &str) -> bool {
    tick.origin.is_some_and(|(id, rev, owner)| {
        *id == target.document() && *rev == target.revision() && owner == plugin
    })
}
pub(super) fn poll(entry: &mut Entry, app: &mut HeditApp, ctx: &egui::Context, tick: &Tick<'_>) {
    let mut data = entry.scoped.take().unwrap();
    data.poll(entry, app, ctx, tick);
    entry.scoped = Some(data);
}
pub(super) fn show(
    entry: &mut Entry,
    app: &mut HeditApp,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    now: u64,
) {
    let mut data = entry.scoped.take().unwrap();
    if let Some(run) = &mut data.run {
        if let Some(preview) = &mut run.preview {
            let context = run.context.as_ref().unwrap();
            if let Some(root) = context.directory() {
                let path = broker::display_path(&root.to_string_lossy());
                ui.add(egui::Label::new(format!("Selected directory: {path}")).truncate())
                    .on_hover_text(path);
            }
            ui.label("Review every affected file before applying this workspace edit.");
            egui::ComboBox::from_id_salt("provider-preview-file")
                .selected_text(broker::display_path(
                    preview.plan.files()[preview.file].relative(),
                ))
                .show_ui(ui, |ui| {
                    for (index, file) in preview.plan.files().iter().enumerate() {
                        if ui
                            .selectable_value(
                                &mut preview.file,
                                index,
                                broker::display_path(file.relative()),
                            )
                            .clicked()
                        {
                            preview.before = 0;
                            preview.after = 0;
                        }
                    }
                });
            let file = &preview.plan.files()[preview.file];
            ui.label("Before");
            let (text, count) = file.before_page(preview.before);
            super::super::presentation::indexed_page(ui, text, &mut preview.before, count);
            ui.label("After");
            let (text, count) = file.after_page(preview.after);
            super::super::presentation::indexed_page(ui, text, &mut preview.after, count);
            let mut decision = None;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        preview.plan.live() && !app.busy,
                        egui::Button::new("Apply workspace edit"),
                    )
                    .clicked()
                {
                    decision = Some(true);
                }
                if ui.button("Discard workspace edit").clicked() {
                    decision = Some(false);
                }
            });
            if let Some(accept) = decision {
                match context.decide(run.host.as_ref().unwrap(), preview.plan.id(), accept) {
                    Ok(()) => {
                        run.preview = None;
                        entry.state.resume_work(now);
                    }
                    Err(error) => entry.error = Some(error),
                }
            }
        }
    }
    let mut action = None;
    if let Some(ready) = &data.ready {
        if ready.context.current() {
            let snapshot = ready.context.snapshot();
            action = super::super::presentation::render(
                ui,
                &ready.result,
                &snapshot.document,
                &snapshot.revision,
                &mut entry.output_page,
                &mut entry.input,
            );
        }
    }
    if let Some(action) = action {
        let ready = data.ready.take().unwrap();
        match action {
            Action::Callback { name, value } => {
                entry.choice = Some((name, value));
                entry.state.queue(Trigger::Interaction, now, 0);
                entry.input = None;
            }
            action => {
                let snapshot = ready.context.snapshot();
                let transaction = match action {
                    Action::Edit(transaction) => transaction,
                    Action::Select(selection) => plugin_runtime::sdk::Transaction {
                        document: snapshot.document.clone(),
                        revision: snapshot.revision.clone(),
                        edits: vec![],
                        selections: Some(vec![selection]),
                        storage: None,
                    },
                    _ => unreachable!(),
                };
                if let Err(error) = app.propose_broker_edit(ctx, &ready.context, transaction) {
                    app.error = Some(error);
                }
            }
        }
    }
    entry.scoped = Some(data);
}

enum StartFailure {
    Busy,
    Failed(String),
}
impl From<String> for StartFailure {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}
impl From<&str> for StartFailure {
    fn from(error: &str) -> Self {
        Self::Failed(error.into())
    }
}
impl From<std::io::Error> for StartFailure {
    fn from(error: std::io::Error) -> Self {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Self::Busy
        } else {
            Self::Failed("The scoped provider host could not start.".into())
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests;
