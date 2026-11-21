//! Confined Lua proposals, host review, authorization rechecks, and atomic effects.
use super::{
    activity::{Invocation, InvocationOutcome},
    catalog,
    manifest::LuaPlugin,
    options,
};
use crate::{
    app::{clipboard_lines, HeditApp},
    plugin_runtime::{self, RunningPlugin},
};
use eframe::egui;
use hedit::{
    advanced::{self, Selection},
    editing,
};
use std::{
    fs::File,
    ops::Range,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

pub(in crate::app) struct LuaTransform {
    proposal: Proposal,
    phase: Phase,
}

impl LuaTransform {
    pub(in crate::app) fn needs_poll(&self) -> bool {
        !matches!(self.phase, Phase::Preview { .. } | Phase::Arguments)
    }
}

#[derive(Debug)]
struct Authority {
    file: File,
    storage: Option<plugin_runtime::sdk::Storage>,
    elapsed: std::time::Duration,
}
type ApprovalReceiver = Receiver<Result<Option<Authority>, String>>;

enum Phase {
    Arguments,
    Persisting(Receiver<Result<(), String>>),
    SdkRunning(plugin_runtime::RunningSdk),
    Checking(ApprovalReceiver),
    Running(RunningPlugin),
    Preview {
        diff: Diff,
        output: Output,
    },
    Committing {
        approval: ApprovalReceiver,
        output: Output,
    },
}

struct Output {
    replacements: Vec<String>,
    selections: Option<Vec<Selection>>,
    storage: Option<plugin_runtime::sdk::Storage>,
}

#[derive(Default)]
struct Diff {
    blocks: Vec<DiffBlock>,
    rows: usize,
}

struct DiffBlock {
    added: bool,
    text: String,
    starts: Vec<usize>,
}

impl Diff {
    fn push(&mut self, added: bool, text: String) {
        if text.is_empty() {
            return;
        }
        let mut starts = vec![0];
        starts.extend(
            text.match_indices('\n')
                .filter_map(|(at, _)| (at + 1 < text.len()).then_some(at + 1)),
        );
        self.rows += starts.len();
        self.blocks.push(DiffBlock {
            added,
            text,
            starts,
        });
    }

    fn is_empty(&self) -> bool {
        self.rows == 0
    }

    fn row(&self, mut index: usize) -> Option<(bool, String)> {
        for block in &self.blocks {
            if index < block.starts.len() {
                let end = block
                    .starts
                    .get(index + 1)
                    .copied()
                    .unwrap_or(block.text.len());
                let line = &block.text[block.starts[index]..end];
                let line = line
                    .strip_suffix("\r\n")
                    .or_else(|| line.strip_suffix('\n'))
                    .unwrap_or(line)
                    .replace('\r', "␍");
                return Some((
                    block.added,
                    format!("{} {line}", if block.added { '+' } else { '−' }),
                ));
            }
            index -= block.starts.len();
        }
        None
    }
}

struct Proposal {
    target: Target,
    ranges: Vec<Range<usize>>,
    plugin: LuaPlugin,
    invocation: Invocation,
    snapshot: Option<plugin_runtime::sdk::Snapshot>,
    storage: Option<plugin_runtime::sdk::Storage>,
    arguments: std::collections::BTreeMap<String, hedit::plugin_options::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    document: u64,
    revision: u64,
    group: u64,
    page_start: u64,
    language: &'static str,
    primary: Selection,
    extra: Vec<Selection>,
}

impl Target {
    pub(super) fn document(&self) -> u64 {
        self.document
    }
    pub(super) fn language(&self) -> &str {
        self.language
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn same_document(&self, other: &Self) -> bool {
        self.document == other.document
            && self.group == other.group
            && self.page_start == other.page_start
            && self.language == other.language
    }
}

#[derive(Debug)]
struct TransformFailure {
    message: String,
    outcome: InvocationOutcome,
}

impl TransformFailure {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            outcome: InvocationOutcome::Failed,
        }
    }
}

impl From<plugin_runtime::Error> for TransformFailure {
    fn from(error: plugin_runtime::Error) -> Self {
        Self {
            message: error.to_string(),
            outcome: InvocationOutcome::worker_failure(error),
        }
    }
}

impl From<plugin_runtime::sdk::Failure> for TransformFailure {
    fn from(error: plugin_runtime::sdk::Failure) -> Self {
        Self {
            message: error.to_string(),
            outcome: InvocationOutcome::worker_failure(error.kind),
        }
    }
}

impl HeditApp {
    pub(super) fn lua_target(&self) -> Option<Target> {
        let editor = self.active();
        let info = &editor.page.info;
        let primary = Selection {
            anchor: editor.view.anchor,
            head: editor.view.head,
        };
        if self.busy
            || self.closing
            || self.preferences_open
            || self.source_control.open
            || self.markdown_preview_only()
            || self.workspace.remote.is_some()
            || editor.follow
            || info.paged
            || editor.view.selections.len() >= plugin_runtime::MAX_SELECTIONS
        {
            return None;
        }
        Some(Target {
            document: info.id,
            revision: info.revision,
            group: self.panes.focused_id(),
            page_start: editor.page.start,
            language: editor.language().id(),
            primary,
            extra: editor.view.selections.clone(),
        })
    }

    pub(super) fn start_lua_transform(
        &mut self,
        ctx: &egui::Context,
        command: &str,
        plugin: LuaPlugin,
    ) {
        let proposal = self.lua_proposal(command, plugin);
        match proposal {
            Ok(proposal) if !proposal.plugin.arguments.is_empty() => {
                proposal.invocation.begin_wait();
                self.lua_transform = Some(LuaTransform {
                    proposal,
                    phase: Phase::Arguments,
                });
                ctx.request_repaint();
            }
            Ok(proposal) => match self.request_lua_approval(ctx, &proposal.plugin) {
                Ok(approval) => {
                    self.lua_transform = Some(LuaTransform {
                        proposal,
                        phase: Phase::Checking(approval),
                    });
                    self.notice = "Running extension command…".into();
                }
                Err(error) => {
                    self.extension_manager
                        .record(proposal.invocation, InvocationOutcome::Denied);
                    self.error = Some(error);
                }
            },
            Err(error) => {
                self.extension_manager
                    .record(Invocation::new(command), InvocationOutcome::Denied);
                self.error = Some(error);
            }
        }
    }

    fn lua_proposal(&self, command: &str, plugin: LuaPlugin) -> Result<Proposal, String> {
        let started = std::time::Instant::now();
        let target = self
            .lua_target()
            .ok_or("Select text in a local editable buffer.")?;
        let snapshot = if plugin.sdk {
            let text = &self.active().page.text;
            if text.len() > plugin_runtime::MAX_SELECTION_BYTES {
                return Err("Document exceeds the 64 KiB editing SDK input limit.".into());
            }
            let selections = snapshot_selections(text, &self.selections());
            let mut snapshot = options::sdk_snapshot(
                target.language,
                text.clone(),
                selections,
                self.active().page.info.eol == "CRLF",
            )
            .map_err(|e| e.to_string())?;
            snapshot.revision = target.revision.to_string();
            Some(snapshot)
        } else {
            None
        };
        let ranges = if plugin.sdk {
            std::iter::once(0..self.active().page.text.len()).collect()
        } else {
            selected_ranges(&self.active().page.text, self.selections())?
        };
        let arguments = plugin
            .arguments
            .iter()
            .map(|(key, definition)| (key.clone(), definition.default.clone()))
            .collect();
        let invocation = if plugin.sdk {
            Invocation::document(command, plugin.storage)
        } else {
            Invocation::new(command)
        };
        invocation.trace.record(
            plugin_runtime::telemetry::Phase::Proposal,
            started.elapsed(),
        );
        Ok(Proposal {
            target,
            ranges,
            plugin,
            invocation,
            snapshot,
            storage: None,
            arguments,
        })
    }

    pub(super) fn propose_provider_edit(
        &mut self,
        ctx: &egui::Context,
        command: &str,
        plugin: LuaPlugin,
        document: &str,
        transaction: plugin_runtime::sdk::Transaction,
    ) -> Result<(), String> {
        if self.lua_transform.is_some() {
            return Err("An extension edit is already awaiting review.".into());
        }
        if !transaction.edits.is_empty()
            && !plugin
                .provider
                .as_ref()
                .is_some_and(|definition| definition.may_edit)
        {
            return Err("This provider has no document edit authority.".into());
        }
        let mut proposal = self.lua_proposal(command, plugin)?;
        let snapshot = proposal
            .snapshot
            .as_mut()
            .ok_or("Missing provider snapshot.")?;
        snapshot.document = document.into();
        let prepared = transaction
            .prepare(snapshot)
            .map_err(|error| error.to_string())?;
        let output = Output {
            selections: Some(byte_selections(&prepared.text, &prepared.selections)),
            replacements: vec![prepared.text],
            storage: None,
        };
        let phase = self
            .review_lua_output(ctx, &proposal, output)
            .map_err(|error| error.message)?;
        self.lua_transform = Some(LuaTransform { proposal, phase });
        Ok(())
    }

    fn request_lua_approval(
        &self,
        ctx: &egui::Context,
        plugin: &LuaPlugin,
    ) -> Result<ApprovalReceiver, String> {
        let config = self
            .settings_files
            .dir
            .clone()
            .ok_or("Extension approval requires a user configuration folder.")?;
        let plugin = plugin.clone();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        thread::Builder::new()
            .name("hedit-lua-approval".into())
            .spawn(move || {
                let started = std::time::Instant::now();
                let result = (|| {
                    let Some(file) = catalog::lua_authorization(&config, &plugin) else {
                        return Ok(None);
                    };
                    let storage = if plugin.storage {
                        Some(catalog::read_state(&config, &plugin)?)
                    } else {
                        None
                    };
                    Ok(Some(Authority {
                        file,
                        storage,
                        elapsed: started.elapsed(),
                    }))
                })();
                let _ = sender.send(result);
                ctx.request_repaint();
            })
            .map_err(|_| "The extension approval check could not start.")?;
        Ok(receiver)
    }

    pub(in crate::app) fn poll_lua_transform(&mut self, ctx: &egui::Context) {
        let Some(mut job) = self.lua_transform.take() else {
            return;
        };
        if !matches!(job.phase, Phase::Persisting(_))
            && self.lua_target().as_ref() != Some(&job.proposal.target)
        {
            match job.phase {
                Phase::Arguments => job
                    .proposal
                    .invocation
                    .end_wait(plugin_runtime::telemetry::Phase::Prompt),
                Phase::Preview { .. } => job
                    .proposal
                    .invocation
                    .end_wait(plugin_runtime::telemetry::Phase::Preview),
                _ => {}
            }
            self.extension_manager
                .record(job.proposal.invocation, InvocationOutcome::Cancelled);
            self.notice = "Extension command canceled because the editor context changed.".into();
            return;
        }
        match self.advance_lua_transform(ctx, &mut job) {
            Ok(None) => self.lua_transform = Some(job),
            Ok(Some(outcome)) => self
                .extension_manager
                .record(job.proposal.invocation, outcome),
            Err(error) => {
                self.extension_manager
                    .record(job.proposal.invocation, error.outcome);
                self.notice = "Extension command failed.".into();
                self.error = Some(error.message);
            }
        }
    }

    fn advance_lua_transform(
        &mut self,
        ctx: &egui::Context,
        job: &mut LuaTransform,
    ) -> Result<Option<InvocationOutcome>, TransformFailure> {
        let proposal = &mut job.proposal;
        match &mut job.phase {
            Phase::Persisting(result) => match result.try_recv() {
                Ok(Ok(())) => {
                    self.notice = "Private command storage updated.".into();
                    return Ok(Some(InvocationOutcome::Applied));
                }
                Ok(Err(_)) => {
                    return Err(TransformFailure::failed(
                        "Private command storage could not be updated.",
                    ))
                }
                Err(TryRecvError::Disconnected) => {
                    return Err(TransformFailure::failed("Private storage worker stopped."))
                }
                Err(TryRecvError::Empty) => {}
            },
            Phase::Arguments => {
                match show_arguments(ctx, &proposal.plugin, &mut proposal.arguments) {
                    Some(true) => {
                        proposal
                            .invocation
                            .end_wait(plugin_runtime::telemetry::Phase::Prompt);
                        job.phase = Phase::Checking(
                            self.request_lua_approval(ctx, &proposal.plugin)
                                .map_err(TransformFailure::failed)?,
                        )
                    }
                    Some(false) => {
                        proposal
                            .invocation
                            .end_wait(plugin_runtime::telemetry::Phase::Prompt);
                        return Ok(Some(InvocationOutcome::Discarded));
                    }
                    None => {}
                }
            }
            Phase::Checking(approval) => {
                if let Some(approval) = receive_approval(approval)? {
                    proposal.invocation.trace.record(
                        plugin_runtime::telemetry::Phase::Authorization,
                        approval.elapsed,
                    );
                    let wake = ctx.clone();
                    proposal.storage = approval.storage.clone();
                    job.phase = if let Some(snapshot) = &proposal.snapshot {
                        Phase::SdkRunning(plugin_runtime::start_sdk(
                            options::sdk_input(
                                &self.settings.extensions,
                                &proposal.plugin,
                                snapshot.clone(),
                                proposal.arguments.clone(),
                                approval.storage,
                            ),
                            approval.file,
                            move || wake.request_repaint(),
                        )?)
                    } else {
                        let selection = proposal
                            .ranges
                            .iter()
                            .map(|range| self.active().page.text[range.clone()].to_owned())
                            .collect();
                        let input = options::input(
                            &self.settings.extensions,
                            &proposal.plugin,
                            proposal.target.language,
                            selection,
                        );
                        Phase::Running(plugin_runtime::start(input, approval.file, move || {
                            wake.request_repaint()
                        })?)
                    };
                }
            }
            Phase::Running(worker) => {
                if let Some(replacements) = worker.try_result() {
                    let replacements = replacements?;
                    let output = Output {
                        replacements,
                        selections: None,
                        storage: None,
                    };
                    let crlf = self.active().page.info.eol == "CRLF";
                    if proposal.ranges.iter().zip(&output.replacements).all(
                        |(range, replacement)| {
                            self.active().page.text[range.clone()]
                                == clipboard_lines::line_endings(replacement, crlf)
                        },
                    ) {
                        self.notice = "The extension proposed no changes.".into();
                        return Ok(Some(InvocationOutcome::Applied));
                    }
                    job.phase = self.review_lua_output(ctx, proposal, output)?;
                }
            }
            Phase::SdkRunning(worker) => {
                if let Some(transaction) = worker.try_result() {
                    let snapshot = proposal
                        .snapshot
                        .as_ref()
                        .ok_or_else(|| TransformFailure::failed("Missing document snapshot."))?;
                    for sample in worker.timings().snapshot() {
                        proposal.invocation.trace.record(
                            sample.phase,
                            std::time::Duration::from_micros(sample.total_us),
                        );
                    }
                    let transaction = transaction?;
                    if transaction.storage.is_some() && !proposal.plugin.storage {
                        return Err(plugin_runtime::Error::Worker.into());
                    }
                    let prepared = transaction.prepare(snapshot)?;
                    let selections = byte_selections(&prepared.text, &prepared.selections);
                    if prepared.storage.is_none()
                        && prepared.text == self.active().page.text
                        && selections == self.selections()
                    {
                        self.notice = "The extension proposed no changes.".into();
                        return Ok(Some(InvocationOutcome::Applied));
                    }
                    job.phase = self.review_lua_output(
                        ctx,
                        proposal,
                        Output {
                            replacements: vec![prepared.text],
                            selections: Some(selections),
                            storage: prepared.storage,
                        },
                    )?;
                }
            }
            Phase::Preview { diff, output } => match show_preview(ctx, diff) {
                Some(true) => {
                    proposal
                        .invocation
                        .end_wait(plugin_runtime::telemetry::Phase::Preview);
                    job.phase = Phase::Committing {
                        output: Output {
                            replacements: std::mem::take(&mut output.replacements),
                            selections: output.selections.take(),
                            storage: output.storage.take(),
                        },
                        approval: self
                            .request_lua_approval(ctx, &proposal.plugin)
                            .map_err(TransformFailure::failed)?,
                    }
                }
                Some(false) => {
                    proposal
                        .invocation
                        .end_wait(plugin_runtime::telemetry::Phase::Preview);
                    self.notice = "Extension changes discarded.".into();
                    self.focus_editor = true;
                    return Ok(Some(InvocationOutcome::Discarded));
                }
                None => {}
            },
            Phase::Committing { approval, output } => {
                if let Some(authority) = receive_approval(approval)? {
                    proposal.invocation.trace.record(
                        plugin_runtime::telemetry::Phase::Authorization,
                        authority.elapsed,
                    );
                    let commit_started = std::time::Instant::now();
                    if authority.storage != proposal.storage {
                        return Err(TransformFailure::failed(
                            "Private command storage changed; run the command again.",
                        ));
                    }
                    if let Some(storage) = output.storage.take() {
                        if !proposal.plugin.storage {
                            return Err(plugin_runtime::Error::Worker.into());
                        }
                        let config = self.settings_files.dir.clone().ok_or_else(|| {
                            TransformFailure::failed("Missing extension configuration.")
                        })?;
                        let plugin = proposal.plugin.clone();
                        let trace = proposal.invocation.trace.clone();
                        let wake = ctx.clone();
                        let (sender, receiver) = mpsc::channel();
                        thread::Builder::new()
                            .name("hedit-plugin-storage".into())
                            .spawn(move || {
                                let _authority = authority;
                                let _ = sender.send(
                                    trace.measure(plugin_runtime::telemetry::Phase::Commit, || {
                                        catalog::write_state(&config, &plugin, &storage)
                                    }),
                                );
                                wake.request_repaint();
                            })
                            .map_err(|_| {
                                TransformFailure::failed("Private storage worker could not start.")
                            })?;
                        job.phase = Phase::Persisting(receiver);
                        return Ok(None);
                    }
                    let _authority = authority;
                    let result = if let Some(selections) = &output.selections {
                        let text = output
                            .replacements
                            .first()
                            .ok_or_else(|| TransformFailure::failed("Missing edit proposal."))?;
                        let cursor = selections.last().map_or(0, |s| s.head);
                        if text == &self.active().page.text {
                            self.set_selections(selections.clone());
                            true
                        } else if self.apply_edit(
                            editing::Edit {
                                text: text.clone(),
                                anchor: cursor,
                                head: cursor,
                            },
                            None,
                        ) {
                            self.set_selections(selections.clone());
                            self.snippet = None;
                            true
                        } else {
                            false
                        }
                    } else {
                        self.apply_lua_replacements(&proposal.ranges, &output.replacements)
                    };
                    if result {
                        if proposal.plugin.provider.is_some() {
                            self.plugin_services.note_origin(
                                self.active().page.info.id,
                                self.active().page.info.revision,
                                &proposal.plugin.plugin_id,
                            );
                        }
                        self.notice = "Extension command finished.".into();
                    }
                    proposal.invocation.trace.record(
                        plugin_runtime::telemetry::Phase::Commit,
                        commit_started.elapsed(),
                    );
                    return Ok(Some(if result {
                        InvocationOutcome::Applied
                    } else {
                        InvocationOutcome::Failed
                    }));
                }
            }
        }
        Ok(None)
    }

    fn review_lua_output(
        &self,
        ctx: &egui::Context,
        proposal: &Proposal,
        output: Output,
    ) -> Result<Phase, TransformFailure> {
        if self.settings.extensions.preview_edits {
            let mut diff = edit_diff(self.active(), &proposal.ranges, &output.replacements);
            if output.storage.is_some() {
                diff.push(true, "Update private command storage".into());
            }
            if diff.is_empty() {
                diff.push(true, "Update editor selections".into());
            }
            ctx.request_repaint();
            proposal.invocation.begin_wait();
            Ok(Phase::Preview { diff, output })
        } else {
            Ok(Phase::Committing {
                output,
                approval: self
                    .request_lua_approval(ctx, &proposal.plugin)
                    .map_err(TransformFailure::failed)?,
            })
        }
    }

    fn apply_lua_replacements(&mut self, ranges: &[Range<usize>], replacements: &[String]) -> bool {
        let editor = self.active();
        let (text, selections) = build_edit(
            &editor.page.text,
            ranges,
            replacements,
            editor.page.info.eol == "CRLF",
        );
        let cursor = selections.last().map_or(0, |selection| selection.head);
        let edit = editing::Edit {
            text,
            anchor: cursor,
            head: cursor,
        };
        if self.apply_edit(edit, None) {
            if selections.len() > 1 {
                self.set_selections(selections);
            }
            self.snippet = None;
            self.notice = "Extension command finished.".into();
            return true;
        }
        false
    }
}

pub(super) fn snapshot_selections(
    text: &str,
    selections: &[Selection],
) -> Vec<plugin_runtime::sdk::Selection> {
    let mut positions: Vec<_> = selections.iter().flat_map(|s| [s.anchor, s.head]).collect();
    positions.sort_unstable();
    positions.dedup();
    let mut chars = text.char_indices().peekable();
    let mut count = 0;
    let mut mapped = std::collections::BTreeMap::new();
    for position in positions {
        while count < position && chars.peek().is_some() {
            chars.next();
            count += 1;
        }
        mapped.insert(position, chars.peek().map_or(text.len(), |(byte, _)| *byte));
    }
    selections
        .iter()
        .map(|s| {
            let snap = |at: usize| {
                if at > 0 && text.as_bytes().get(at - 1..=at) == Some(b"\r\n") {
                    at - 1
                } else {
                    at
                }
            };
            plugin_runtime::sdk::Selection {
                anchor: snap(mapped[&s.anchor]),
                head: snap(mapped[&s.head]),
            }
        })
        .collect()
}

pub(super) fn byte_selections(
    text: &str,
    selections: &[plugin_runtime::sdk::Selection],
) -> Vec<Selection> {
    let mut positions: Vec<_> = selections.iter().flat_map(|s| [s.anchor, s.head]).collect();
    positions.sort_unstable();
    positions.dedup();
    let mut indices = text.char_indices().peekable();
    let mut characters = 0;
    let mut mapped = std::collections::BTreeMap::new();
    for byte in positions {
        while indices.peek().is_some_and(|(at, _)| *at < byte) {
            indices.next();
            characters += 1;
        }
        mapped.insert(byte, characters);
    }
    selections
        .iter()
        .map(|s| Selection {
            anchor: mapped[&s.anchor],
            head: mapped[&s.head],
        })
        .collect()
}

fn show_arguments(
    ctx: &egui::Context,
    plugin: &LuaPlugin,
    values: &mut std::collections::BTreeMap<String, hedit::plugin_options::Value>,
) -> Option<bool> {
    use hedit::plugin_options::Value;
    let response = egui::Modal::new(egui::Id::new("extension-command-inputs")).show(ctx, |ui| {
        ui.heading("Extension command inputs");
        ui.set_max_width((ctx.screen_rect().width() - 48.0).clamp(180.0, 480.0));
        egui::ScrollArea::vertical()
            .max_height((ctx.screen_rect().height() - 180.0).max(80.0))
            .show(ui, |ui| {
                for (key, definition) in plugin.arguments.iter() {
                    ui.push_id(key, |ui| {
                        let value = values.get_mut(key).unwrap();
                        if definition.choices.is_empty() {
                            options::setting_control(ui, &definition.label, value);
                        } else {
                            let label = |v: &Value| match v {
                                Value::Boolean(v) => v.to_string(),
                                Value::Integer(v) => v.to_string(),
                                Value::Text(v) => v.clone(),
                            };
                            ui.label(&definition.label);
                            egui::ComboBox::from_id_salt(key)
                                .selected_text(label(value))
                                .show_ui(ui, |ui| {
                                    for choice in &definition.choices {
                                        ui.selectable_value(value, choice.clone(), label(choice));
                                    }
                                });
                        }
                    });
                }
            });
        let valid = values.values().all(Value::valid);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                Some(false)
            } else if ui
                .add_enabled(valid, egui::Button::new("Run command"))
                .clicked()
            {
                Some(true)
            } else {
                None
            }
        })
        .inner
    });
    if response.should_close() {
        Some(false)
    } else {
        response.inner
    }
}

fn build_edit(
    text: &str,
    ranges: &[Range<usize>],
    replacements: &[String],
    crlf: bool,
) -> (String, Vec<Selection>) {
    let mut edited = String::with_capacity(text.len());
    let mut selections = Vec::with_capacity(ranges.len());
    let mut previous = 0;
    let mut cursor = 0;
    for (range, replacement) in ranges.iter().zip(replacements) {
        let before = &text[previous..range.start];
        let replacement = clipboard_lines::line_endings(replacement, crlf);
        edited.push_str(before);
        edited.push_str(&replacement);
        cursor += before.chars().count() + replacement.chars().count();
        selections.push(Selection::caret(cursor));
        previous = range.end;
    }
    edited.push_str(&text[previous..]);
    (edited, selections)
}

fn edit_diff(
    buffer: &hedit::workspace::Buffer,
    ranges: &[Range<usize>],
    replacements: &[String],
) -> Diff {
    let crlf = buffer.page.info.eol == "CRLF";
    let mut diff = Diff::default();
    for (range, replacement) in ranges.iter().zip(replacements) {
        let original = &buffer.page.text[range.clone()];
        let replacement = clipboard_lines::line_endings(replacement, crlf);
        if original != replacement {
            diff.push(false, original.to_owned());
            diff.push(true, replacement);
        }
    }
    diff
}

fn show_preview(ctx: &egui::Context, diff: &Diff) -> Option<bool> {
    let modal = egui::Modal::new(egui::Id::new("lua-edit-preview")).show(ctx, |ui| {
        ui.set_width((ctx.screen_rect().width() - 64.0).clamp(224.0, 740.0));
        ui.heading("Extension edit preview");
        let height = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both()
            .max_height((ctx.screen_rect().height() * 0.52).max(70.0))
            .show_rows(ui, height, diff.rows, |ui, lines| {
                for row in lines {
                    let Some((added, text)) = diff.row(row) else {
                        continue;
                    };
                    let color = if added {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).monospace().color(color))
                            .extend(),
                    );
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Discard").clicked() {
                Some(false)
            } else if ui.button("Apply changes").clicked() {
                Some(true)
            } else {
                None
            }
        })
        .inner
    });
    if modal.should_close() {
        Some(false)
    } else {
        modal.inner
    }
}

// The returned lock serializes delivery or application with a revocation.
fn receive_approval(approval: &ApprovalReceiver) -> Result<Option<Authority>, TransformFailure> {
    match approval.try_recv() {
        Ok(Ok(Some(guard))) => Ok(Some(guard)),
        Ok(Err(_)) => Err(TransformFailure::failed(
            "Private command storage could not be read. Revoke approval to clear it.",
        )),
        Ok(Ok(None)) => Err(TransformFailure {
            message: "Extension approval changed. Review it in Settings → Extensions.".into(),
            outcome: InvocationOutcome::Denied,
        }),
        Err(TryRecvError::Empty) => Ok(None),
        Err(TryRecvError::Disconnected) => Err(TransformFailure::failed(
            "The extension approval check stopped.",
        )),
    }
}

fn selected_ranges(text: &str, selections: Vec<Selection>) -> Result<Vec<Range<usize>>, String> {
    let mut selections: Vec<_> = selections
        .into_iter()
        .filter(|selection| selection.start() != selection.end())
        .collect();
    if selections.len() > plugin_runtime::MAX_SELECTIONS {
        return Err("A Lua extension accepts at most 16 selections.".into());
    }
    if selections.is_empty() {
        return Err("Select text to run this extension command.".into());
    }
    selections.sort_by_key(|selection| (selection.start(), selection.end()));
    if selections
        .windows(2)
        .any(|pair| pair[0].end() > pair[1].start())
    {
        return Err("Lua selections must not overlap.".into());
    }
    let ranges: Vec<_> = advanced::byte_ranges(text, &selections)
        .map(|range| validate_selection_range(text, range))
        .collect::<Result<_, _>>()?;
    if ranges.iter().map(Range::len).sum::<usize>() > plugin_runtime::MAX_SELECTION_BYTES {
        return Err("Selected text exceeds the Lua input limit.".into());
    }
    Ok(ranges)
}

fn validate_selection_range(text: &str, range: Range<usize>) -> Result<Range<usize>, String> {
    let Range { start, end } = range;
    if start == end {
        return Err("Select text to run this extension command.".into());
    }
    if end - start > plugin_runtime::MAX_SELECTION_BYTES {
        return Err("The selection is too large for a Lua extension.".into());
    }
    for at in [start, end] {
        if at != 0 && text.as_bytes().get(at - 1..=at) == Some(b"\r\n") {
            return Err("Select the whole line break before running this command.".into());
        }
    }
    Ok(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::app::{extensions::tests::app_with, tests::finish_app_job, Command};
    use eframe::egui;
    use std::path::Path;

    const ID: &str = "markdown-quote";
    const COMMAND: &str = "markdown-quote.quote";
    const MANIFEST: &str = include_str!("../../../examples/extensions/markdown-quote.json");

    fn set_approval(config: &Path, digest: &str, approved: bool) {
        catalog::set_lua_approved(
            config,
            &config.join("extensions/markdown-quote.json"),
            ID,
            COMMAND,
            digest,
            approved,
        )
        .unwrap();
    }

    fn approved(config: &Path, text: &str) -> (tempfile::TempDir, egui::Context, HeditApp, String) {
        let directory = config.join("extensions");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("markdown-quote.json"), MANIFEST).unwrap();
        let catalog = catalog::read(config);
        assert!(catalog.contributions.is_empty());
        let digest = catalog.view.files[0]
            .details
            .as_ref()
            .unwrap()
            .lua
            .as_ref()
            .unwrap()
            .digest
            .clone();
        set_approval(config, &digest, true);
        let (session, ctx, mut app) = app_with(text, Some(config));
        app.settings.extensions.preview_edits = false;
        assert_eq!(app.extensions.len(), 1);
        (session, ctx, app, digest)
    }

    fn fake_result(app: &mut HeditApp, text: &str) {
        let plugin = match &app.extensions[0].action {
            super::super::ExtensionAction::Lua(plugin) => plugin.clone(),
            _ => panic!("expected a Lua command"),
        };
        let proposal = app.lua_proposal(COMMAND, plugin).unwrap();
        app.lua_transform = Some(LuaTransform {
            proposal,
            phase: Phase::Running(RunningPlugin::from_test_result(Ok(vec![text.into()]))),
        });
    }

    fn finish_transform(app: &mut HeditApp, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.lua_transform.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "Lua proposal did not finish"
            );
            app.poll_lua_transform(ctx);
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn sdk_app(config: &Path, text: &str) -> (tempfile::TempDir, egui::Context, HeditApp) {
        let directory = config.join("extensions");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("sdk-markdown.json");
        let manifest = include_str!("../../../examples/extensions/sdk-markdown.json");
        std::fs::write(&path, manifest).unwrap();
        catalog::set_lua_approved(
            config,
            &path,
            "sdk-markdown",
            "sdk-markdown.prefix",
            &super::super::manifest::digest(manifest),
            true,
        )
        .unwrap();
        let (session, ctx, mut app) = app_with(text, Some(config));
        app.settings.extensions.preview_edits = false;
        (session, ctx, app)
    }
    fn sdk_result(
        app: &mut HeditApp,
        edits: Vec<plugin_runtime::sdk::Edit>,
        selections: Option<Vec<plugin_runtime::sdk::Selection>>,
        forged: bool,
    ) {
        let super::super::ExtensionAction::Lua(plugin) = &app.extensions[0].action else {
            unreachable!()
        };
        let proposal = app
            .lua_proposal("sdk-markdown.prefix", plugin.clone())
            .unwrap();
        let snapshot = proposal.snapshot.as_ref().unwrap();
        let transaction = plugin_runtime::sdk::Transaction {
            document: if forged {
                "f".repeat(32)
            } else {
                snapshot.document.clone()
            },
            revision: snapshot.revision.clone(),
            edits,
            selections,
            storage: None,
        };
        app.lua_transform = Some(LuaTransform {
            proposal,
            phase: Phase::SdkRunning(plugin_runtime::RunningSdk::from_test_result(Ok(
                transaction,
            ))),
        });
    }
    #[test]
    fn sdk_storage_only_transactions_persist_without_mutating_document_or_revision() {
        for conflicting in [false, true] {
            let config = tempfile::tempdir().unwrap();
            let (_session, ctx, mut app) = sdk_app(config.path(), "private fixture");
            let path = config.path().join("extensions/sdk-markdown.json");
            let mut manifest: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            manifest["permissions"]
                .as_array_mut()
                .unwrap()
                .push("storage.private".into());
            manifest["commands"][0]["private_storage"] = true.into();
            std::fs::write(&path, manifest.to_string()).unwrap();
            let catalog = catalog::set_lua_approved(
                config.path(),
                &path,
                "sdk-markdown",
                "sdk-markdown.prefix",
                &super::super::manifest::digest(manifest.to_string()),
                true,
            )
            .unwrap();
            app.extensions = catalog.contributions;
            let super::super::ExtensionAction::Lua(plugin) = &app.extensions[0].action else {
                unreachable!()
            };
            let plugin = plugin.clone();
            let revision = app.active().page.info.revision;
            let mut proposal = app
                .lua_proposal("sdk-markdown.prefix", plugin.clone())
                .unwrap();
            proposal.storage = Some(catalog::read_state(config.path(), &plugin).unwrap());
            let snapshot = proposal.snapshot.as_ref().unwrap();
            let values = plugin_runtime::sdk::Storage(
                [("count".into(), hedit::plugin_options::Value::Integer(1))].into(),
            );
            let transaction = plugin_runtime::sdk::Transaction {
                document: snapshot.document.clone(),
                revision: snapshot.revision.clone(),
                edits: vec![],
                selections: None,
                storage: Some(values.clone()),
            };
            app.lua_transform = Some(LuaTransform {
                proposal,
                phase: Phase::SdkRunning(plugin_runtime::RunningSdk::from_test_result(Ok(
                    transaction,
                ))),
            });
            let concurrent = plugin_runtime::sdk::Storage(
                [("count".into(), hedit::plugin_options::Value::Integer(99))].into(),
            );
            if conflicting {
                let _authority = catalog::lua_authorization(config.path(), &plugin).unwrap();
                catalog::write_state(config.path(), &plugin, &concurrent).unwrap();
            }
            finish_transform(&mut app, &ctx);
            assert_eq!(app.active().page.text, "private fixture");
            assert_eq!(app.active().page.info.revision, revision);
            assert_eq!(
                catalog::read_state(config.path(), &plugin).unwrap(),
                if conflicting { concurrent } else { values }
            );
            if conflicting {
                assert!(app.error.as_ref().unwrap().contains("storage changed"));
            }
        }
    }

    #[test]
    fn sdk_document_transactions_apply_multiple_unicode_edits_and_undo_once() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app) = sdk_app(config.path(), "λ\n東京");
        app.edit_command(editing::Operation::ToCrlf);
        let original = app.active().page.text.clone();
        assert!(app.extension_available_by_id("sdk-markdown.prefix"));
        sdk_result(
            &mut app,
            vec![
                plugin_runtime::sdk::Edit {
                    start: 0,
                    finish: 2,
                    text: "α".into(),
                },
                plugin_runtime::sdk::Edit {
                    start: 4,
                    finish: 10,
                    text: "β".into(),
                },
            ],
            Some(vec![plugin_runtime::sdk::Selection { anchor: 4, head: 6 }]),
            false,
        );
        finish_transform(&mut app, &ctx);
        assert_eq!(app.active().page.text, "α\r\nβ");
        assert_eq!(app.selections(), [Selection { anchor: 3, head: 4 }]);
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, original);
    }
    #[test]
    fn sdk_forged_transaction_does_not_edit_or_create_an_undo_entry() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app) = sdk_app(config.path(), "private fixture");
        let revision = app.active().page.info.revision;
        sdk_result(
            &mut app,
            vec![plugin_runtime::sdk::Edit {
                start: 0,
                finish: 0,
                text: "bad".into(),
            }],
            None,
            true,
        );
        finish_transform(&mut app, &ctx);
        assert_eq!(app.active().page.text, "private fixture");
        assert_eq!(app.active().page.info.revision, revision);
        assert!(app.error.is_some());
    }
    #[test]
    fn sdk_selection_only_changes_preserve_document_revision() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app) = sdk_app(config.path(), "λ");
        let revision = app.active().page.info.revision;
        sdk_result(
            &mut app,
            vec![],
            Some(vec![plugin_runtime::sdk::Selection { anchor: 0, head: 2 }]),
            false,
        );
        finish_transform(&mut app, &ctx);
        assert_eq!(app.selections(), [Selection { anchor: 0, head: 1 }]);
        assert_eq!(app.active().page.info.revision, revision);
    }
    #[test]
    fn sdk_collects_arguments_before_start_and_cancellation_preserves_text() {
        use crate::app::tests::{click_label, frame};
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app) = sdk_app(config.path(), "private fixture");
        assert!(app.run_extension_by_id(&ctx, "sdk-markdown.prefix"));
        frame(&ctx, &mut app, vec![]);
        assert!(matches!(
            app.lua_transform.as_ref().unwrap().phase,
            Phase::Arguments
        ));
        click_label(&ctx, &mut app, "Cancel");
        assert!(app.lua_transform.is_none());
        assert_eq!(app.active().page.text, "private fixture");
    }

    #[test]
    fn approval_completion_wakes_an_idle_ui_with_the_authority_ready() {
        let config = tempfile::tempdir().unwrap();
        let (_session, _ctx, app, _) = approved(config.path(), "selected");
        let super::super::ExtensionAction::Lua(plugin) = &app.extensions[0].action else {
            panic!("expected a Lua command");
        };
        let ctx = egui::Context::default();
        for _ in 0..4 {
            let _ = ctx.run(egui::RawInput::default(), |_| {});
        }
        let (sender, receiver) = mpsc::channel();
        ctx.set_request_repaint_callback(move |info| {
            let _ = sender.send(info.delay);
        });
        let approval = app.request_lua_approval(&ctx, plugin).unwrap();
        assert_eq!(
            receiver
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap(),
            std::time::Duration::ZERO
        );
        assert!(receive_approval(&approval).unwrap().is_some());
    }

    #[test]
    fn approval_denial_and_worker_limits_preserve_the_document() {
        let (sender, receiver) = mpsc::channel();
        sender.send(Ok(None)).unwrap();
        assert_eq!(
            receive_approval(&receiver).unwrap_err().outcome,
            InvocationOutcome::Denied
        );
        drop(sender);
        assert_eq!(
            receive_approval(&receiver).unwrap_err().outcome,
            InvocationOutcome::Failed
        );

        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "private text");
        app.set_selections(vec![Selection { anchor: 0, head: 7 }]);
        fake_result(&mut app, "unused");
        app.lua_transform.as_mut().unwrap().phase = Phase::Running(
            RunningPlugin::from_test_result(Err(plugin_runtime::Error::Instructions)),
        );
        app.poll_lua_transform(&ctx);
        assert!(app.lua_transform.is_none());
        assert_eq!(app.active().page.text, "private text");
        assert_eq!(
            app.error.as_deref(),
            Some("The Lua plugin reached its instruction limit.")
        );
    }

    #[test]
    fn preview_discards_or_applies_bounded_multi_selection_edits_atomically() {
        use crate::app::tests::{click_label, frame};

        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "α one β two");
        app.settings.extensions.preview_edits = true;
        let original = app.active().page.text.clone();
        app.set_selections(vec![
            Selection { anchor: 0, head: 1 },
            Selection { anchor: 6, head: 7 },
        ]);
        let plugin = match &app.extensions[0].action {
            super::super::ExtensionAction::Lua(plugin) => plugin.clone(),
            _ => unreachable!(),
        };
        let inject = |app: &mut HeditApp| {
            let proposal = app.lua_proposal(COMMAND, plugin.clone()).unwrap();
            app.lua_transform = Some(LuaTransform {
                proposal,
                phase: Phase::Running(RunningPlugin::from_test_result(Ok(vec![
                    "λ".into(),
                    "東京".into(),
                ]))),
            });
        };

        inject(&mut app);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().page.text, original);
        click_label(&ctx, &mut app, "Discard");
        assert_eq!(app.active().page.text, original);
        assert!(app.lua_transform.is_none());

        inject(&mut app);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "Apply changes");
        finish_transform(&mut app, &ctx);
        assert_eq!(app.active().page.text, "λ one 東京 two");
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, original);
    }

    #[test]
    fn approved_lua_result_replaces_a_single_unicode_selection_and_undoes_once() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "head\nλ\nβ\ntail\n");
        app.edit_command(editing::Operation::ToCrlf);
        let original = app.active().page.text.clone();
        assert!(!app.extension_available_by_id(COMMAND));
        app.set_selections(vec![Selection {
            anchor: 10,
            head: 6,
        }]);
        assert_eq!(
            app.extension_available_by_id(COMMAND),
            plugin_runtime::SUPPORTED
        );
        fake_result(&mut app, "> λ\n> β");
        finish_transform(&mut app, &ctx);
        assert_eq!(app.active().page.text, "head\r\n> λ\r\n> β\r\ntail\r\n");
        assert_eq!(app.active().page.info.eol, "CRLF");
        assert_eq!(app.selections(), [Selection::caret(14)]);
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, original);
    }

    #[test]
    fn changing_revision_or_selection_discards_a_late_lua_result() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "abc def");
        let selection = Selection { anchor: 0, head: 3 };
        app.set_selections(vec![selection]);
        fake_result(&mut app, "replacement");
        app.set_selections(vec![Selection { anchor: 4, head: 7 }]);
        app.poll_lua_transform(&ctx);
        assert_eq!(app.active().page.text, "abc def");
        assert!(app.lua_transform.is_none());
        assert!(app.notice.contains("context changed"));

        app.set_selections(vec![selection]);
        fake_result(&mut app, "replacement");
        assert!(app.apply_edit(
            editing::Edit {
                text: "abc def!".into(),
                anchor: selection.anchor,
                head: selection.head,
            },
            None
        ));
        app.poll_lua_transform(&ctx);
        assert_eq!(app.active().page.text, "abc def!");
        assert!(app.lua_transform.is_none());
    }

    #[test]
    fn revoke_and_reapprove_discards_an_in_flight_lua_proposal() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, digest) = approved(config.path(), "selected");
        app.set_selections(vec![Selection { anchor: 0, head: 8 }]);
        fake_result(&mut app, "replacement");
        set_approval(config.path(), &digest, false);
        set_approval(config.path(), &digest, true);
        finish_transform(&mut app, &ctx);
        assert_eq!(app.active().page.text, "selected");
        assert!(app.error.as_deref().unwrap().contains("approval changed"));
    }

    #[test]
    fn pending_denied_or_disconnected_authorization_never_starts_a_worker() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "private text");
        app.set_selections(vec![Selection { anchor: 0, head: 7 }]);
        for disconnected in [false, true] {
            fake_result(&mut app, "must not be applied");
            let (sender, receiver) = mpsc::channel();
            app.lua_transform.as_mut().unwrap().phase = Phase::Checking(receiver);
            app.poll_lua_transform(&ctx);
            assert!(matches!(
                app.lua_transform.as_ref().unwrap().phase,
                Phase::Checking(_)
            ));
            if !disconnected {
                sender.send(Ok(None)).unwrap();
            }
            drop(sender);
            app.poll_lua_transform(&ctx);
            assert!(app.lua_transform.is_none());
            assert_eq!(app.active().page.text, "private text");
            assert!(app.error.is_some());
        }
    }

    #[test]
    fn preview_cannot_apply_after_revocation_disable_removal_or_source_change() {
        use crate::app::tests::{click_label, frame};

        for change in ["revoke", "reapprove", "disable", "remove", "source"] {
            let config = tempfile::tempdir().unwrap();
            let (_session, ctx, mut app, digest) = approved(config.path(), "selected");
            app.settings.extensions.preview_edits = true;
            let selection = Selection { anchor: 0, head: 8 };
            app.set_selections(vec![selection]);
            fake_result(&mut app, "untrusted replacement");
            frame(&ctx, &mut app, vec![]);
            frame(&ctx, &mut app, vec![]);
            assert!(matches!(
                app.lua_transform.as_ref().unwrap().phase,
                Phase::Preview { .. }
            ));
            match change {
                "revoke" => set_approval(config.path(), &digest, false),
                "reapprove" => {
                    set_approval(config.path(), &digest, false);
                    set_approval(config.path(), &digest, true);
                }
                "disable" => {
                    catalog::set_enabled(config.path(), ID, false).unwrap();
                }
                "remove" => {
                    std::fs::remove_file(config.path().join("extensions/markdown-quote.json"))
                        .unwrap()
                }
                "source" => std::fs::write(
                    config.path().join("extensions/markdown-quote.json"),
                    MANIFEST.replace("quote.lua", "changed.lua"),
                )
                .unwrap(),
                _ => unreachable!(),
            }
            click_label(&ctx, &mut app, "Apply changes");
            finish_transform(&mut app, &ctx);
            assert_eq!(app.active().page.text, "selected", "{change}");
            assert_eq!(app.selections(), [selection], "{change}");
            assert!(
                app.error.as_deref().unwrap().contains("approval changed"),
                "{change}"
            );
        }
    }

    #[test]
    fn moving_to_another_pane_cancels_a_result_even_for_the_same_document() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "selected");
        app.set_selections(vec![Selection { anchor: 0, head: 8 }]);
        let document = app.active().page.info.id;
        fake_result(&mut app, "untrusted replacement");
        let group = app.panes.focused_id();
        app.split(1);
        app.execute(&ctx, Command::FocusGroup2);
        assert_ne!(app.panes.focused_id(), group);
        assert_eq!(app.active().page.info.id, document);
        app.poll_lua_transform(&ctx);
        assert!(app.lua_transform.is_none());
        assert_eq!(app.active().page.text, "selected");
        assert!(app.notice.contains("context changed"));
    }

    #[test]
    fn selection_mapping_orders_unicode_ranges_and_rejects_overlaps() {
        let text = "aλ🦀\r\n東京";
        assert_eq!(
            selected_ranges(
                text,
                vec![
                    Selection { anchor: 7, head: 5 },
                    Selection { anchor: 3, head: 1 }
                ]
            )
            .unwrap(),
            vec![1..7, 9..15]
        );
        assert!(selected_ranges(
            text,
            vec![
                Selection { anchor: 1, head: 3 },
                Selection { anchor: 2, head: 4 }
            ]
        )
        .is_err());
        assert!(selected_ranges(text, vec![Selection::caret(0)]).is_err());
    }

    #[test]
    fn adjacent_selections_enforce_the_combined_lua_input_limit() {
        let limit = plugin_runtime::MAX_SELECTION_BYTES;
        let text = "x".repeat(limit + 1);
        let ranges = |end| {
            vec![
                Selection {
                    anchor: 0,
                    head: limit / 2,
                },
                Selection {
                    anchor: limit / 2,
                    head: end,
                },
            ]
        };
        assert_eq!(
            selected_ranges(&text, ranges(limit)).unwrap(),
            vec![0..limit / 2, limit / 2..limit]
        );
        assert!(selected_ranges(&text, ranges(limit + 1)).is_err());
    }

    #[test]
    fn selection_limits_count_utf8_bytes_and_preserve_crlf_pairs() {
        let text = "λ\r\n";
        let selected = |anchor, head| {
            selected_ranges(text, vec![Selection { anchor, head }])
                .map(|mut ranges| ranges.remove(0))
        };
        assert_eq!(selected(1, 0).unwrap(), 0..2);
        assert_eq!(selected(1, 3).unwrap(), 2..4);
        assert!(selected(0, 2).is_err());
        assert!(selected(2, 3).is_err());
        assert!(selected(3, 4).is_err());

        let text = "λ".repeat(plugin_runtime::MAX_SELECTION_BYTES / 2 + 1);
        assert!(selected_ranges(
            &text,
            vec![Selection {
                anchor: 0,
                head: plugin_runtime::MAX_SELECTION_BYTES / 2,
            }]
        )
        .is_ok());
        assert!(selected_ranges(
            &text,
            vec![Selection {
                anchor: 0,
                head: plugin_runtime::MAX_SELECTION_BYTES / 2 + 1,
            }]
        )
        .is_err());
    }
    #[test]
    fn compact_preview_rows_match_unicode_and_crlf_without_per_line_strings() {
        let mut diff = Diff::default();
        diff.push(false, "café\r\n東京\r\n\nend\r".into());
        diff.push(true, "→ 東京\n".into());
        assert_eq!(diff.rows, 5);
        let expected = [
            (false, "− café"),
            (false, "− 東京"),
            (false, "− "),
            (false, "− end␍"),
            (true, "+ → 東京"),
        ];
        for (row, (added, text)) in expected.into_iter().enumerate() {
            assert_eq!(diff.row(row).unwrap(), (added, text.into()));
        }
        assert!(diff.row(5).is_none());
        let mut large = Diff::default();
        large.push(true, "\n".repeat(plugin_runtime::sdk::MAX_OUTPUT_BYTES));
        assert_eq!(large.rows, plugin_runtime::sdk::MAX_OUTPUT_BYTES);
        assert_eq!(large.blocks.len(), 1);
        assert_eq!(large.row(large.rows - 1).unwrap(), (true, "+ ".into()));
    }
    include!("lua/performance.rs");
}
