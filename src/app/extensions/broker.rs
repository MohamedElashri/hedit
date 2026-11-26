//! Installed SDK 3 commands use a scoped host, never the legacy document worker.
pub(super) mod commit;
mod context;
mod deferred;
pub(super) use context::Context;
mod limit;
use super::{
    activity::{Invocation, InvocationOutcome},
    authority::Receipt,
    catalog, lua,
    manifest::LuaPlugin,
    options, presentation,
    services::policy::Monitor,
};
use crate::{
    app::HeditApp,
    plugin_runtime::{self, sdk, service},
};
pub(in crate::app) use commit::Refresh;
pub(in crate::app) use deferred::Deferred;
use eframe::egui;
use hedit_broker::{
    declaration::Capability,
    host::{self, Event, Host},
    plan::Plan,
    state::State,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, OnceLock,
    },
    time::{Duration, Instant},
};
static CHECKS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static NETWORK: OnceLock<Arc<hedit_broker::network::Network>> = OnceLock::new();

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Extension context or resource approval changed.",
    )
}

struct Finished {
    transaction: Option<sdk::Transaction>,
    result: Option<Box<service::ResultSet>>,
    cache: sdk::Storage,
}
enum Phase {
    Arguments,
    Running {
        host: Host,
        review: Option<Arc<Plan>>,
        file: usize,
        before: usize,
        after: usize,
    },
    Checking {
        finished: Finished,
        authority: Receiver<Option<Receipt>>,
    },
    Ready {
        result: Box<service::ResultSet>,
        cache: sdk::Storage,
        created: Instant,
        page: usize,
        input: Option<hedit::plugin_options::Value>,
    },
}
pub(in crate::app) struct BrokerCommand {
    context: Arc<Context>,
    phase: Phase,
    invocation: Option<Invocation>,
    arguments: BTreeMap<String, hedit::plugin_options::Value>,
}
impl Drop for BrokerCommand {
    fn drop(&mut self) {
        self.context.close();
    }
}
impl BrokerCommand {
    pub(super) fn idle(&self) -> bool {
        matches!(self.phase, Phase::Ready { .. })
    }
    fn launch(
        &mut self,
        app: &HeditApp,
        ctx: &egui::Context,
        cache: sdk::Storage,
        request: Option<service::Request>,
    ) -> Result<(), String> {
        let host = self
            .context
            .start(app, ctx, cache, request, self.arguments.clone())
            .map_err(|e| e.to_string())?;
        self.phase = Phase::Running {
            host,
            review: None,
            file: 0,
            before: 0,
            after: 0,
        };
        Ok(())
    }
}
impl HeditApp {
    pub(super) fn start_broker_command(
        &mut self,
        ctx: &egui::Context,
        command: &str,
        plugin: LuaPlugin,
    ) {
        let result = (|| {
            if self
                .broker_command
                .as_ref()
                .is_some_and(|command| !command.idle())
                || self.lua_transform.is_some()
            {
                return Err("An extension command is already active.".into());
            }
            self.broker_command.take();
            let config = self
                .settings_files
                .dir
                .clone()
                .ok_or("Extension commands need a user configuration folder.")?;
            let definition = plugin
                .broker
                .as_ref()
                .ok_or("Missing broker declaration.")?;
            let (target, snapshot) = if definition.has(Capability::DocumentRead) {
                let target = self
                    .lua_target()
                    .ok_or("Select a local, editable document.")?;
                if self.active().page.text.len() > plugin_runtime::MAX_SELECTION_BYTES {
                    return Err("Document exceeds the 64 KiB snapshot limit.".into());
                }
                let mut snapshot = options::sdk_snapshot(
                    target.language(),
                    self.active().page.text.clone(),
                    lua::snapshot_selections(&self.active().page.text, &self.selections()),
                    self.active().page.info.eol == "CRLF",
                )
                .map_err(|e| e.to_string())?;
                snapshot.revision = target.revision().to_string();
                (Some(target), snapshot)
            } else {
                (
                    None,
                    options::sdk_snapshot(
                        "plain_text",
                        String::new(),
                        vec![sdk::Selection { anchor: 0, head: 0 }],
                        false,
                    )
                    .map_err(|e| e.to_string())?,
                )
            };
            let arguments = plugin
                .arguments
                .iter()
                .map(|(key, value)| (key.clone(), value.default.clone()))
                .collect();
            let monitor = self.plugin_services.policy_monitor(&config, ctx)?;
            let package = options::sdk_input(
                &self.settings.extensions,
                &plugin,
                snapshot,
                BTreeMap::new(),
                None,
            );
            let mut command = BrokerCommand {
                context: Context::new(
                    config,
                    command.into(),
                    plugin,
                    package,
                    target,
                    self.workspace.editor.clone(),
                    monitor,
                ),
                phase: Phase::Arguments,
                invocation: Some(Invocation::broker(command)),
                arguments,
            };
            if command.context.plugin.arguments.is_empty() {
                command.launch(self, ctx, Default::default(), None)?;
            }
            self.broker_command = Some(command);
            ctx.request_repaint();
            Ok::<_, String>(())
        })();
        if let Err(error) = result {
            self.error = Some(error);
        }
    }
    pub(in crate::app) fn poll_broker_command(&mut self, ctx: &egui::Context) {
        let Some(mut command) = self.broker_command.take() else {
            return;
        };
        let context = command.context.clone();
        if !context.retained()
            || (!context.commit.committing.load(Ordering::Acquire)
                && context
                    .target
                    .as_ref()
                    .is_some_and(|target| self.lua_target().as_ref() != Some(target)))
        {
            let outcome = if context.commit.applied.load(Ordering::Acquire) {
                InvocationOutcome::Applied
            } else {
                InvocationOutcome::Cancelled
            };
            self.finish_broker_activity(&mut command, outcome);
            return;
        }
        let mut stop = false;
        let result = (|| -> Result<(), String> {
            match &mut command.phase {
                Phase::Arguments => {
                    match lua::show_arguments(ctx, &context.plugin, &mut command.arguments) {
                        Some(true) => command.launch(self, ctx, Default::default(), None)?,
                        Some(false) => stop = true,
                        None => {}
                    }
                }
                Phase::Running {
                    host,
                    review,
                    file,
                    before,
                    after,
                } => {
                    if let Some(event) = host.try_event() {
                        match event {
                            Event::Review(plan) => {
                                *review = Some(plan);
                                ctx.request_repaint_after(Duration::from_secs(120));
                            }
                            Event::Failed(code) => {
                                let applied = context.commit.applied.load(Ordering::Acquire);
                                self.finish_broker_activity(
                                    &mut command,
                                    if applied {
                                        InvocationOutcome::Applied
                                    } else {
                                        InvocationOutcome::Failed
                                    },
                                );
                                return Err(format!(
                                    "{}: {}.",
                                    if context.commit.incomplete.load(Ordering::Acquire) {
                                        "Workspace edit interrupted; review Workspace recovery"
                                    } else if applied {
                                        "Workspace edit applied; the extension then stopped"
                                    } else {
                                        "Extension command stopped"
                                    },
                                    failure(code)
                                ));
                            }
                            Event::Finished {
                                transaction,
                                result,
                                cache,
                            } => {
                                let receiver = context.result_authority(ctx)?;
                                command.phase = Phase::Checking {
                                    finished: Finished {
                                        transaction,
                                        result,
                                        cache,
                                    },
                                    authority: receiver,
                                };
                                return Ok(());
                            }
                        }
                    }
                    let mut open = true;
                    let mut decision = None;
                    egui::Window::new("Extension command").id(egui::Id::unique("scoped-extension-command")).open(&mut open).default_width(650.0).show(ctx, |ui| {
                        ui.heading(&context.command);
                        if let Some(plan) = review {
                            ui.label("Review every affected file before applying this workspace edit.");
                            if let Some(directory) = context.directory.get() {
                                let path = display_path(&directory.to_string_lossy());
                                ui.add(egui::Label::new(format!("Selected directory: {path}")).truncate()).on_hover_text(path);
                            }
                            *file = (*file).min(plan.files().len() - 1);
                            egui::ComboBox::from_id_salt("workspace-preview-file").width(560.0).selected_text(display_path(plan.files()[*file].relative())).show_ui(ui, |ui| {
                                for (index, proposed) in plan.files().iter().enumerate() { if ui.selectable_value(file, index, display_path(proposed.relative())).clicked() { *before = 0; *after = 0; } }
                            });
                            let (text, pages) = plan.files()[*file].before_page(*before);
                            ui.label("Before"); presentation::indexed_page(ui, text, before, pages);
                            let (text, pages) = plan.files()[*file].after_page(*after);
                            ui.separator(); ui.label("After"); presentation::indexed_page(ui, text, after, pages);
                            ui.horizontal(|ui| {
                                if ui.add_enabled(plan.live() && !self.busy, egui::Button::new("Apply workspace edit")).clicked() { decision = Some(true); }
                                if ui.button("Discard").clicked() { decision = Some(false); }
                            });
                        } else { ui.horizontal(|ui| { ui.spinner(); ui.label("Running in the scoped extension host…"); }); }
                        if ui.button("Stop command").clicked() { stop = true; }
                    });
                    if !open {
                        stop = true;
                    }
                    if let (Some(accept), Some(plan)) = (decision, review.as_ref()) {
                        context.commit.committing.store(accept, Ordering::Release);
                        host.decide(host::Decision {
                            plan: plan.id(),
                            accept,
                        })
                        .map_err(|_| "This workspace preview expired.")?;
                        *review = None;
                    }
                }
                Phase::Checking {
                    finished,
                    authority,
                } => {
                    let receipt = match authority.try_recv() {
                        Ok(Some(receipt)) => receipt
                            .take()
                            .ok_or("Result authorization expired. Run the command again.")?,
                        Ok(None) => {
                            return Err("Extension approval changed before result delivery.".into())
                        }
                        Err(mpsc::TryRecvError::Empty) => return Ok(()),
                        Err(_) => return Err("Result authorization stopped.".into()),
                    };
                    if !context.current() {
                        return Err("Extension context changed before result delivery.".into());
                    }
                    if let Some(transaction) = finished.transaction.take() {
                        self.propose_broker_edit(ctx, &context, transaction)?;
                        stop = true;
                    } else if let Some(result) = finished.result.take() {
                        command.phase = Phase::Ready {
                            result,
                            cache: std::mem::take(&mut finished.cache),
                            created: Instant::now(),
                            page: 0,
                            input: None,
                        };
                        self.finish_broker_activity(
                            &mut command,
                            if context.commit.applied.load(Ordering::Acquire) {
                                InvocationOutcome::Applied
                            } else {
                                InvocationOutcome::Published
                            },
                        );
                        ctx.request_repaint_after(Duration::from_secs(120));
                    } else {
                        stop = true;
                        self.finish_broker_activity(
                            &mut command,
                            if context.commit.applied.load(Ordering::Acquire) {
                                InvocationOutcome::Applied
                            } else {
                                InvocationOutcome::Published
                            },
                        );
                    }
                    drop(receipt);
                }
                Phase::Ready {
                    result,
                    cache,
                    created,
                    page,
                    input,
                } => {
                    if created.elapsed() >= Duration::from_secs(120) {
                        stop = true;
                        return Ok(());
                    }
                    let mut open = true;
                    let mut action = None;
                    egui::Window::new("Extension command result")
                        .id(egui::Id::unique("scoped-extension-result"))
                        .open(&mut open)
                        .default_width(480.0)
                        .show(ctx, |ui| {
                            ui.heading(&context.command);
                            action = presentation::render(
                                ui,
                                result,
                                &context.package.snapshot.document,
                                &context.package.snapshot.revision,
                                page,
                                input,
                            );
                        });
                    if !open {
                        stop = true;
                    }
                    if let Some(action) = action {
                        match action {
                            service::Action::Callback { name, value } => {
                                let request = service::Request {
                                    id: hedit_broker::Handle::fresh()
                                        .map_err(|_| "Cannot create an interaction handle.")?
                                        .to_string(),
                                    kind: service::Kind::Presentation,
                                    trigger: service::Trigger::Interaction,
                                    position: context.package.snapshot.selections[0].head,
                                    action: Some(name),
                                    choice: Some(value),
                                };
                                let cache = std::mem::take(cache);
                                command.invocation = Some(Invocation::broker(&context.command));
                                command.launch(self, ctx, cache, Some(request))?;
                            }
                            action => {
                                let transaction = match action {
                                    service::Action::Edit(transaction) => transaction,
                                    service::Action::Select(selection) => sdk::Transaction {
                                        document: context.package.snapshot.document.clone(),
                                        revision: context.package.snapshot.revision.clone(),
                                        edits: vec![],
                                        selections: Some(vec![selection]),
                                        storage: None,
                                    },
                                    _ => unreachable!(),
                                };
                                self.propose_broker_edit(ctx, &context, transaction)?;
                                stop = true;
                            }
                        }
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(error);
            self.finish_broker_activity(&mut command, InvocationOutcome::Failed);
            return;
        }
        if stop {
            self.finish_broker_activity(&mut command, InvocationOutcome::Cancelled);
        } else {
            self.broker_command = Some(command);
        }
    }
    pub(in crate::app::extensions) fn propose_broker_edit(
        &mut self,
        ctx: &egui::Context,
        context: &Context,
        transaction: sdk::Transaction,
    ) -> Result<(), String> {
        if context.target.is_none()
            || self.lua_target().as_ref() != context.target.as_ref()
            || self.active().page.text != context.package.snapshot.text
        {
            return Err("The originating document changed. Run the command again.".into());
        }
        self.propose_provider_edit(
            ctx,
            &context.command,
            context.approved_plugin(),
            &context.package.snapshot.document,
            transaction,
        )
    }
    fn finish_broker_activity(&mut self, command: &mut BrokerCommand, outcome: InvocationOutcome) {
        if let Some(invocation) = command.invocation.take() {
            self.extension_manager.record(invocation, outcome);
        }
    }
    pub(in crate::app) fn refresh_broker_buffers(&mut self, refresh: Refresh) {
        let mut updated = false;
        let active = self.active().page.info.id;
        let mut origin = None;
        for page in refresh.pages {
            if let Some(buffer) = self.workspace.open.iter_mut().find(|buffer| {
                buffer.page.info.id == page.info.id
                    && buffer.page.info.revision <= page.info.revision
            }) {
                if buffer.page.info.revision != page.info.revision {
                    buffer.cursor_undo.clear();
                    buffer.cursor_redo.clear();
                    buffer.changes.clear();
                    buffer.pending_selection = None;
                    buffer.view.folds.clear();
                    let count = page.text.chars().count();
                    buffer.view.anchor = buffer.view.anchor.min(count);
                    buffer.view.head = buffer.view.head.min(count);
                    for selection in &mut buffer.view.selections {
                        selection.anchor = selection.anchor.min(count);
                        selection.head = selection.head.min(count);
                    }
                }
                if !refresh.conflicts && page.info.id == active {
                    origin = refresh
                        .origin
                        .as_ref()
                        .map(|owner| (page.info.id, page.info.revision, owner.clone()));
                }
                buffer.view.page_start = page.start;
                buffer.page = page;
                buffer.refresh_name();
                buffer.restore_cursor = true;
                buffer.external = false;
                updated = true;
            }
        }
        if updated {
            self.changed();
        }
        if let Some((document, revision, owner)) = origin {
            self.plugin_services.note_origin(document, revision, &owner);
        }
        if refresh.conflicts {
            self.error = Some("Workspace commit encountered a changed file. Original buffer text was kept unsaved; review recovery before continuing.".into());
        } else if refresh.incomplete {
            self.error = Some("Workspace edit stopped before completion. Some files may have changed. Review Workspace recovery in Extensions.".into());
        } else if refresh.applied {
            self.notice = format!("Workspace edit applied to {} files.", refresh.files);
        } else if refresh.changed != 0 {
            self.notice = format!(
                "Updated {} open buffers after workspace editing.",
                refresh.changed
            );
        }
    }
}
pub(super) fn failure(code: plugin_runtime::broker::FailureCode) -> &'static str {
    use plugin_runtime::broker::FailureCode::*;
    match code {
        Denied => "approval denied",
        Changed => "resource changed",
        Limit => "resource limit reached",
        Cancelled => "cancelled",
        Unavailable => "capability unavailable",
        Failed => "host operation failed",
    }
}
pub(super) fn display_path(path: &str) -> String {
    let mut visible = String::with_capacity(path.len());
    for ch in path.chars() {
        if ch.is_control()
            || matches!(ch, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
        {
            visible.extend(ch.escape_default());
        } else {
            visible.push(ch);
        }
    }
    visible
}

#[cfg(test)]
mod tests;
