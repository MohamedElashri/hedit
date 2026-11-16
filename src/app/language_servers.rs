//! Native management UI and revision-checked language-server results.
use super::*;
use hedit::language_server::{
    self as lsp,
    manager::{self, Installed, Registration},
    text::{self, Encoding, TextEdit},
    Document, Event, Feature, Operation, Reply, Service, Status,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    id: u64,
    revision: u64,
    path: PathBuf,
    root: PathBuf,
    language: String,
    dirty: bool,
}
struct Guard {
    view: u64,
    pane: usize,
    head: usize,
    revision: u64,
    anchor: usize,
    selections: Vec<hedit::advanced::Selection>,
    root: PathBuf,
    feature: Feature,
}
enum Intent {
    List,
    Manage,
    Sync,
    Feature(Guard),
}
struct Diagnostics {
    server: String,
    root: PathBuf,
    revision: u64,
    encoding: Encoding,
    items: Vec<Value>,
}
#[derive(Default)]
pub(super) struct LanguageServers {
    pub(super) service: Option<Service>,
    installed: Vec<Installed>,
    statuses: BTreeMap<(String, PathBuf), Status>,
    pending: HashMap<u64, Intent>,
    next: u64,
    snapshots: Vec<Snapshot>,
    observed: Option<(Vec<Snapshot>, Instant)>,
    diagnostics: BTreeMap<PathBuf, Diagnostics>,
    pub(super) problems: bool,
    pub(super) info: Option<String>,
    filter: String,
    progress: String,
    error: Option<String>,
    editor: Option<Configuration>,
    listed: bool,
    blocked: bool,
    pub(super) directory: Option<PathBuf>,
    last_completion: Option<(u64, u64, usize)>,
}
struct Configuration {
    id: String,
    languages: String,
    executable: String,
    arguments: String,
    settings: String,
    registration: Registration,
}
impl LanguageServers {
    fn send(&mut self, operation: Operation, intent: Intent) -> Result<(), String> {
        let service = self
            .service
            .as_ref()
            .ok_or("Language-server manager is not available.")?;
        self.next += 1;
        service.submit(self.next, operation)?;
        self.pending.insert(self.next, intent);
        Ok(())
    }
    fn managing(&self) -> bool {
        self.pending
            .values()
            .any(|i| matches!(i, Intent::Manage | Intent::List))
    }
    pub(super) fn completion_edit(
        text: &str,
        choice: &Completion,
    ) -> Result<(String, usize), String> {
        let primary = choice.edits.first().ok_or("Completion has no edit.")?;
        let start = choice.encoding.byte(text, primary.range.start)?;
        let mut cursor = text[..start].chars().count() + primary.new_text.chars().count();
        for edit in choice.edits.iter().skip(1) {
            let a = choice.encoding.byte(text, edit.range.start)?;
            let b = choice.encoding.byte(text, edit.range.end)?;
            if b <= start {
                cursor = cursor.saturating_sub(text[a..b].chars().count())
                    + edit.new_text.chars().count();
            }
        }
        Ok((text::apply(text, &choice.edits, choice.encoding)?, cursor))
    }
}
#[derive(Clone, Debug)]
pub(super) struct Completion {
    pub label: String,
    pub detail: String,
    pub edits: Vec<TextEdit>,
    pub encoding: Encoding,
}
impl HeditApp {
    fn ensure_language_servers(&mut self, ctx: &egui::Context) {
        if self.languages.service.is_some() || self.closing || self.languages.blocked {
            return;
        }
        let wake_ctx = ctx.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || wake_ctx.request_repaint());
        let service = if let Some(remote) = &self.workspace.remote {
            let (connected, trusted, _, _) = remote.status();
            if !connected || !trusted {
                return;
            }
            remote.language_servers(wake)
        } else {
            #[cfg(test)]
            let directory = self.languages.directory.clone();
            #[cfg(not(test))]
            let directory = self.languages.directory.clone().or_else(|| {
                directories::ProjectDirs::from("", "hedit", "hedit")
                    .map(|d| d.data_local_dir().join("language-servers"))
            });
            let Some(directory) = directory else { return };
            Service::start(directory, wake)
        };
        self.languages.service = Some(service);
        self.languages.listed = false;
        self.languages.snapshots.clear();
        self.languages.pending.clear();
        self.languages.statuses.clear();
        if let Err(e) = self.languages.send(Operation::List, Intent::List) {
            self.languages.error = Some(e);
        }
    }
    fn language_root(&self, path: &Path) -> PathBuf {
        self.workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .filter(|r| path.starts_with(r))
            .max_by_key(|r| r.components().count())
            .or_else(|| path.parent().map(PathBuf::from))
            .unwrap_or_default()
    }
    fn language_snapshots(&self) -> Vec<Snapshot> {
        // JSON can expand control bytes sixfold. Keep SSH snapshots within
        // the helper's 32 MiB transfer cap even for heavily escaped text.
        let mut remaining = if self.workspace.remote.is_some() {
            4 * 1024 * 1024
        } else {
            32 * 1024 * 1024
        };
        // The focused document is retained when the workspace exceeds limits.
        std::iter::once(self.active())
            .chain(
                self.workspace
                    .open
                    .iter()
                    .filter(|b| b.view.id != self.active().view.id),
            )
            .filter(|b| {
                !b.page.info.paged
                    && !b.follow
                    && b.page.start == 0
                    && b.page.text.len() <= lsp::MAX_DOCUMENT
            })
            .filter_map(|b| {
                let path = PathBuf::from(b.page.info.path.as_ref()?);
                if b.page.text.len() > remaining {
                    return None;
                }
                remaining -= b.page.text.len();
                Some(Snapshot {
                    id: b.view.id,
                    revision: b.page.info.revision,
                    root: self.language_root(&path),
                    path,
                    language: match b.language().id() {
                        "c++" => "cpp",
                        l => l,
                    }
                    .into(),
                    dirty: b.page.info.dirty,
                })
            })
            .take(32)
            .collect()
    }
    fn sync_language_documents(&mut self, force: bool) -> Result<(), String> {
        if !self.languages.listed || self.languages.installed.is_empty() {
            return Ok(());
        }
        let snapshots = self.language_snapshots();
        if snapshots == self.languages.snapshots {
            return Ok(());
        }
        let changed = self
            .languages
            .observed
            .as_ref()
            .is_none_or(|(key, _)| *key != snapshots);
        if changed {
            self.languages.observed = Some((snapshots.clone(), Instant::now()));
        }
        if !force
            && (self
                .languages
                .pending
                .values()
                .any(|i| matches!(i, Intent::Sync))
                || self
                    .languages
                    .observed
                    .as_ref()
                    .is_some_and(|(_, at)| at.elapsed() < Duration::from_millis(150)))
        {
            return Ok(());
        }
        let documents = snapshots
            .iter()
            .filter_map(|key| {
                self.workspace
                    .open
                    .iter()
                    .find(|b| b.view.id == key.id)
                    .map(|b| Document {
                        path: key.path.clone(),
                        root: key.root.clone(),
                        language: key.language.clone(),
                        revision: key.revision,
                        text: b.page.text.clone(),
                        dirty: key.dirty,
                    })
            })
            .collect();
        self.languages
            .send(Operation::Sync { documents }, Intent::Sync)?;
        self.languages.snapshots = snapshots;
        Ok(())
    }
    pub(super) fn poll_language_servers(&mut self, ctx: &egui::Context) {
        // Escape in this input batch wins over a response already waiting in
        // the worker mailbox, before any editor mutation can be applied.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.cancel_language_requests();
        }
        if let Some(remote) = &self.workspace.remote {
            let (connected, trusted, _, _) = remote.status();
            if !connected || !trusted {
                self.languages.service = None;
                self.languages.blocked = false;
                self.languages.pending.clear();
                self.languages.statuses.clear();
                self.languages.diagnostics.clear();
                return;
            }
        }
        self.ensure_language_servers(ctx);
        let events = self
            .languages
            .service
            .as_ref()
            .map(|s| s.poll())
            .unwrap_or_default();
        for event in events {
            self.language_event(ctx, event);
        }
        self.languages.diagnostics.retain(|path, diagnostics| {
            self.workspace.open.iter().any(|b| {
                b.page
                    .info
                    .path
                    .as_ref()
                    .is_some_and(|p| Path::new(p) == path)
                    && b.page.info.revision == diagnostics.revision
            })
        });
        let _ = self.sync_language_documents(false);
        if !self.languages.installed.is_empty()
            && self.languages.snapshots != self.language_snapshots()
        {
            ctx.request_repaint_after(Duration::from_millis(160));
        }
    }
    fn language_event(&mut self, ctx: &egui::Context, event: Event) {
        match event {
            Event::Disconnected(e) => {
                self.languages.service = None;
                self.languages.blocked = true;
                self.languages.pending.clear();
                self.languages.error = Some(e);
                self.languages.listed = false;
                self.languages.statuses.clear();
                self.languages.diagnostics.clear();
            }
            Event::Progress { id, message } => self.languages.progress = format!("{id}: {message}"),
            Event::Status(status) => {
                if status.state != "Running" {
                    self.languages.diagnostics.retain(|_, diagnostics| {
                        diagnostics.server != status.id || diagnostics.root != status.root
                    });
                }
                self.languages
                    .statuses
                    .insert((status.id.clone(), status.root.clone()), status);
                while self.languages.statuses.len() > 128 {
                    let obsolete = self
                        .languages
                        .statuses
                        .iter()
                        .find(|(_, s)| s.state != "Running" && s.state != "Starting")
                        .map(|(key, _)| key.clone());
                    if let Some(key) = obsolete {
                        self.languages.statuses.remove(&key);
                    } else {
                        self.languages.statuses.pop_first();
                    }
                }
            }
            Event::Diagnostics {
                server,
                root,
                path,
                revision,
                encoding,
                items,
            } => {
                if self.workspace.open.iter().any(|b| {
                    b.page
                        .info
                        .path
                        .as_ref()
                        .is_some_and(|p| Path::new(p) == path)
                        && b.page.info.revision == revision
                }) {
                    self.languages.diagnostics.insert(
                        path,
                        Diagnostics {
                            server,
                            root,
                            revision,
                            encoding,
                            items,
                        },
                    );
                }
            }
            Event::Result { ticket, result } => {
                let intent = self.languages.pending.remove(&ticket);
                match result {
                    Ok(value) => match intent {
                        Some(Intent::List) => {
                            match serde_json::from_value::<Vec<Installed>>(
                                value["installed"].clone(),
                            ) {
                                Ok(installed) => {
                                    self.languages.installed = installed;
                                    self.languages.listed = true;
                                    // A CLI or another window may have changed
                                    // the available servers since our last sync.
                                    self.languages.snapshots.clear();
                                    self.languages.last_completion = None;
                                }
                                Err(e) => self.languages.error = Some(e.to_string()),
                            }
                        }
                        Some(Intent::Manage) => {
                            self.languages.progress.clear();
                            self.languages.snapshots.clear();
                            self.languages.last_completion = None;
                            if let Err(e) = self.languages.send(Operation::List, Intent::List) {
                                self.languages.error = Some(e);
                            }
                        }
                        _ => (),
                    },
                    Err(e) => {
                        self.languages.progress.clear();
                        if !matches!(intent, Some(Intent::Sync)) {
                            self.languages.error = Some(e);
                        }
                    }
                }
            }
            Event::Response { ticket, result } => {
                let Some(Intent::Feature(guard)) = self.languages.pending.remove(&ticket) else {
                    return;
                };
                let b = self.active();
                if b.view.id != guard.view
                    || b.page.info.revision != guard.revision
                    || b.view.head != guard.head
                    || b.view.anchor != guard.anchor
                    || b.view.selections != guard.selections
                    || b.page
                        .info
                        .path
                        .as_ref()
                        .is_none_or(|p| self.language_root(&PathBuf::from(p)) != guard.root)
                    || self.panes.focused != guard.pane
                    || self.closing
                    || self.busy
                {
                    return;
                }
                match result {
                    Ok(reply) => {
                        if let Err(e) = self.apply_language_reply(ctx, reply) {
                            self.notice = e;
                        }
                    }
                    Err(e) => {
                        if matches!(guard.feature, Feature::Definition | Feature::References)
                            && self.workspace.remote.is_none()
                        {
                            self.look_up_indexed(ctx, guard.feature == Feature::References);
                        } else if guard.feature != Feature::Completion {
                            self.notice = e;
                        } else if self.auto.popup.is_none() {
                            self.auto.dismiss();
                        }
                    }
                }
            }
        }
    }
    pub(super) fn request_language_feature(
        &mut self,
        ctx: &egui::Context,
        feature: Feature,
    ) -> bool {
        self.ensure_language_servers(ctx);
        if self.busy
            || self.closing
            || self.active().page.info.paged
            || self.active().follow
            || self.languages.service.is_none()
        {
            return false;
        }
        let Some(path) = self.active().page.info.path.as_ref().map(PathBuf::from) else {
            return false;
        };
        let language = self.active().language().id();
        let root = self.language_root(&path);
        if !self.languages.installed.iter().any(|e| {
            e.registration.enabled_at(&root)
                && e.registration
                    .languages
                    .iter()
                    .any(|l| l == language || (l == "cpp" && language == "c++"))
        }) {
            return false;
        }
        if self
            .languages
            .pending
            .values()
            .filter(|i| matches!(i, Intent::Feature(_)))
            .count()
            >= 4
        {
            return false;
        }
        if self.sync_language_documents(true).is_err() {
            return false;
        }
        let b = self.active();
        let guard = Guard {
            view: b.view.id,
            pane: self.panes.focused,
            head: b.view.head,
            revision: b.page.info.revision,
            anchor: b.view.anchor,
            selections: b.view.selections.clone(),
            root: self.language_root(&path),
            feature,
        };
        let operation = Operation::Request {
            path,
            revision: guard.revision,
            scalar: guard.head,
            feature,
            tab_size: b.view.tab_size.unwrap_or(4),
            spaces: b.view.indent.as_deref() != Some("tabs"),
        };
        self.languages
            .send(operation, Intent::Feature(guard))
            .is_ok()
    }
    pub(super) fn request_language_completion(&mut self, ctx: &egui::Context) {
        let b = self.active();
        let key = (b.view.id, b.page.info.revision, b.view.head);
        if self.languages.last_completion == Some(key) {
            return;
        }
        if self.request_language_feature(ctx, Feature::Completion) {
            self.languages.last_completion = Some(key);
        }
    }
    fn apply_language_reply(&mut self, ctx: &egui::Context, reply: Reply) -> Result<(), String> {
        if self.active().page.info.path.as_ref().map(PathBuf::from) != Some(reply.path)
            || self.active().page.info.revision != reply.revision
        {
            return Ok(());
        }
        match reply.feature {
            Feature::Completion => self.publish_language_completions(reply.value, reply.encoding),
            Feature::Hover => {
                fn collect(value: &Value, out: &mut String) {
                    match value {
                        Value::String(s) => {
                            out.push_str(s);
                            out.push('\n');
                        }
                        Value::Array(a) => {
                            for v in a.iter().take(32) {
                                collect(v, out)
                            }
                        }
                        Value::Object(o) => {
                            if let Some(v) = o.get("value") {
                                collect(v, out)
                            }
                        }
                        _ => (),
                    }
                }
                let mut content = String::new();
                collect(&reply.value["contents"], &mut content);
                if content.trim().is_empty() {
                    self.notice = "No language information at this position.".into();
                } else {
                    self.close_transient(ctx);
                    self.languages.info = Some(content.chars().take(32_768).collect());
                }
            }
            Feature::Formatting => {
                let edits: Vec<TextEdit> = serde_json::from_value(if reply.value.is_null() {
                    json!([])
                } else {
                    reply.value
                })
                .map_err(|e| e.to_string())?;
                let updated = text::apply(&self.active().page.text, &edits, reply.encoding)?;
                if updated != self.active().page.text {
                    let cursor = self.active().view.head.min(updated.chars().count());
                    self.batch(hedit::advanced::Batch {
                        text: updated,
                        selections: vec![hedit::advanced::Selection::caret(cursor)],
                    });
                }
            }
            Feature::Definition | Feature::References => {
                let values = match reply.value {
                    Value::Array(v) => v,
                    Value::Object(_) => vec![reply.value],
                    _ => vec![],
                };
                let mut locations = vec![];
                for value in values.into_iter().take(2000) {
                    if value["heditInvalidPosition"] == true {
                        continue;
                    }
                    let Some(uri) = value["uri"]
                        .as_str()
                        .or_else(|| value["targetUri"].as_str())
                    else {
                        continue;
                    };
                    let path = if uri.starts_with("hedit-ssh:") {
                        let path = PathBuf::from(uri);
                        let Some(remote) = &self.workspace.remote else {
                            continue;
                        };
                        if remote.relative(&path).is_err() {
                            continue;
                        }
                        path
                    } else {
                        let Ok(path) = text::path(uri) else { continue };
                        if self.workspace.remote.is_some() {
                            continue;
                        }
                        path
                    };
                    let range = value
                        .get("targetSelectionRange")
                        .or_else(|| value.get("range"));
                    let Some(range) = range else { continue };
                    let Ok(range) = serde_json::from_value::<text::Range>(range.clone()) else {
                        continue;
                    };
                    // Opening unknown files accepts UTF-16 columns. Servers
                    // negotiating UTF-8/32 need a bounded worker-side conversion.
                    let column = value["heditColumn16"]
                        .as_u64()
                        .map(|n| n as usize)
                        .unwrap_or(range.start.character as usize + 1);
                    let (view, scalar_column, line_text) = self
                        .workspace
                        .open
                        .iter()
                        .find(|b| {
                            b.page
                                .info
                                .path
                                .as_ref()
                                .is_some_and(|p| Path::new(p) == path)
                        })
                        .and_then(|b| {
                            let byte = reply.encoding.byte(&b.page.text, range.start).ok()?;
                            let line_start = b.page.text[..byte].rfind('\n').map_or(0, |n| n + 1);
                            Some((
                                Some(b.view.id),
                                b.page.text[line_start..byte].chars().count() + 1,
                                b.page.text[line_start..]
                                    .lines()
                                    .next()
                                    .unwrap_or("")
                                    .chars()
                                    .take(160)
                                    .collect(),
                            ))
                        })
                        .unwrap_or((None, column, String::new()));
                    locations.push(definitions::Location {
                        view,
                        file: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        path: Some(path),
                        column16: column,
                        place: hedit::intelligence::Place {
                            line: range.start.line as usize + 1,
                            column: scalar_column,
                            text: line_text,
                        },
                    });
                }
                self.show_locations(
                    ctx,
                    reply.feature == Feature::References,
                    "language-server result".into(),
                    locations,
                );
            }
        }
        Ok(())
    }
    fn manage_language_server(&mut self, operation: Operation) {
        self.languages.error = None;
        self.languages.progress = "Working…".into();
        if let Err(e) = self.languages.send(operation, Intent::Manage) {
            self.languages.progress.clear();
            self.languages.error = Some(e);
        }
    }
    pub(super) fn language_preferences(&mut self, ui: &mut egui::Ui) {
        let target = self
            .workspace
            .remote
            .as_ref()
            .map(|r| format!("SSH host: {}", r.config.target.host))
            .unwrap_or_else(|| "This computer".into());
        ui.label(RichText::new(target).strong());
        if self.workspace.remote.is_some() && self.languages.service.is_none() {
            ui.weak("Allow execution in the SSH bar to manage language servers on the host.");
            return;
        }
        ui.weak("Install servers privately for hedit, or configure an existing executable. Installs require curl. External executables remain on disk when removed.");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.languages.filter)
                    .hint_text("Filter servers or languages")
                    .desired_width(220.0),
            );
            if ui
                .add_enabled(!self.languages.managing(), egui::Button::new("Refresh"))
                .clicked()
            {
                self.languages.error = None;
                self.languages.blocked = false;
                self.ensure_language_servers(ui.ctx());
                if !self.languages.managing() {
                    let _ = self.languages.send(Operation::List, Intent::List);
                }
            }
        });
        if !self.languages.progress.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(&self.languages.progress);
                if ui.small_button("Cancel").clicked() {
                    self.languages.service = None;
                    self.languages.pending.clear();
                    self.languages.progress.clear();
                }
            });
        }
        if let Some(error) = &self.languages.error {
            ui.colored_label(self.theme.warning.color(), error);
        }
        let mut entries: Vec<_> = manager::catalog()
            .into_iter()
            .map(|c| (c.id, c.languages, c.requirements, true))
            .collect();
        for entry in &self.languages.installed {
            if !entries.iter().any(|(id, _, _, _)| id == &entry.id) {
                entries.push((
                    entry.id.clone(),
                    entry.registration.languages.clone(),
                    String::new(),
                    false,
                ));
            }
        }
        let filter = self.languages.filter.to_lowercase();
        for (id, languages, requirements, managed) in entries {
            if !format!("{id} {}", languages.join(" "))
                .to_lowercase()
                .contains(&filter)
            {
                continue;
            }
            ui.separator();
            let installed = self
                .languages
                .installed
                .iter()
                .find(|e| e.id == id)
                .cloned();
            ui.horizontal_wrapped(|ui| {
                ui.strong(&id);
                ui.weak(languages.join(", "));
                if let Some(entry) = &installed {
                    ui.label(if entry.registration.external.is_some() {
                        "External".into()
                    } else {
                        entry
                            .registration
                            .managed
                            .as_ref()
                            .map(|r| format!("Managed · {}", r.version))
                            .unwrap_or_else(|| "Configured".into())
                    });
                } else {
                    ui.weak("Available");
                }
            });
            if !requirements.is_empty() {
                ui.small(requirements);
            }
            let statuses: Vec<_> = self
                .languages
                .statuses
                .values()
                .filter(|s| s.id == id)
                .cloned()
                .collect();
            for status in statuses {
                ui.horizontal_wrapped(|ui| {
                    ui.label(&status.state);
                    ui.weak(status.root.display().to_string());
                });
                if !status.detail.is_empty() {
                    ui.colored_label(self.theme.warning.color(), status.detail);
                }
                if !status.logs.is_empty() {
                    egui::CollapsingHeader::new("Server log")
                        .id_salt((&id, &status.root))
                        .show(ui, |ui| {
                            for log in &status.logs {
                                ui.monospace(log);
                            }
                        });
                }
            }
            let busy = self.languages.managing();
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if managed
                        && ui
                            .button(
                                if installed
                                    .as_ref()
                                    .is_some_and(|e| e.registration.managed.is_some())
                                {
                                    "Update / Repair"
                                } else {
                                    "Install"
                                },
                            )
                            .clicked()
                    {
                        self.manage_language_server(Operation::Install { id: id.clone() });
                    }
                    if ui.button("Configure").clicked() {
                        let registration = installed
                            .as_ref()
                            .map(|e| e.registration.clone())
                            .unwrap_or_else(|| Registration {
                                languages: languages.clone(),
                                ..Registration::default()
                            });
                        self.languages.editor = Some(Configuration {
                            id: id.clone(),
                            languages: registration.languages.join(", "),
                            executable: registration
                                .external
                                .as_ref()
                                .map(|l| l.executable.display().to_string())
                                .unwrap_or_default(),
                            arguments: serde_json::to_string_pretty(
                                &registration
                                    .external
                                    .as_ref()
                                    .map(|l| l.arguments.clone())
                                    .unwrap_or_default(),
                            )
                            .unwrap(),
                            settings: serde_json::to_string_pretty(&registration.configuration)
                                .unwrap(),
                            registration,
                        });
                    }
                    if installed.is_some() {
                        if ui.button("Restart").clicked() {
                            self.manage_language_server(Operation::Restart { id: id.clone() });
                        }
                        if ui.button("Stop").clicked() {
                            self.manage_language_server(Operation::Stop { id: id.clone() });
                        }
                        if ui.button("Remove").clicked() {
                            self.manage_language_server(Operation::Remove { id: id.clone() });
                        }
                    }
                });
                if let Some(entry) = installed {
                    let root = self
                        .active()
                        .page
                        .info
                        .path
                        .as_ref()
                        .map(PathBuf::from)
                        .map(|p| self.language_root(&p));
                    let mut registration = entry.registration;
                    let mut enabled = registration.enabled;
                    if ui.checkbox(&mut enabled, "Enabled by default").changed() {
                        registration.enabled = enabled;
                        self.manage_language_server(Operation::Configure {
                            id: id.clone(),
                            registration: Box::new(registration.clone()),
                        });
                    }
                    if let Some(root) = root {
                        let mut enabled = registration.enabled_at(&root);
                        if ui
                            .checkbox(&mut enabled, "Enabled in this workspace")
                            .changed()
                        {
                            registration
                                .workspaces
                                .insert(root.to_string_lossy().into_owned(), enabled);
                            self.manage_language_server(Operation::Configure {
                                id: id.clone(),
                                registration: Box::new(registration),
                            });
                        }
                    }
                }
            });
        }
        ui.separator();
        if ui
            .add_enabled(
                !self.languages.managing(),
                egui::Button::new("Add external server…"),
            )
            .clicked()
        {
            self.languages.editor = Some(Configuration {
                id: String::new(),
                languages: String::new(),
                executable: String::new(),
                arguments: "[]".into(),
                settings: "{}".into(),
                registration: Registration::default(),
            });
        }
        if let Some(mut editor) = self.languages.editor.take() {
            ui.separator();
            ui.strong("Server configuration");
            ui.label("Server ID");
            ui.text_edit_singleline(&mut editor.id);
            ui.label("Language IDs (comma separated)");
            ui.text_edit_singleline(&mut editor.languages);
            ui.label("Absolute executable path (empty uses managed installation)");
            ui.text_edit_singleline(&mut editor.executable);
            ui.label("Arguments (JSON array, for example [\"--stdio\"])");
            ui.add(
                egui::TextEdit::multiline(&mut editor.arguments)
                    .desired_rows(2)
                    .code_editor(),
            );
            ui.label("Server settings (JSON object)");
            ui.add(
                egui::TextEdit::multiline(&mut editor.settings)
                    .desired_rows(4)
                    .code_editor(),
            );
            let mut keep = true;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !self.languages.managing(),
                        egui::Button::new("Save configuration"),
                    )
                    .clicked()
                {
                    let result = (|| -> Result<Registration, String> {
                        manager::Manager::validate_id(&editor.id)?;
                        let mut registration = editor.registration.clone();
                        registration.languages = editor
                            .languages
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned)
                            .collect();
                        registration.configuration =
                            serde_json::from_str(&editor.settings).map_err(|e| e.to_string())?;
                        registration.external = if editor.executable.trim().is_empty() {
                            None
                        } else {
                            Some(manager::Launch {
                                executable: PathBuf::from(editor.executable.trim()),
                                arguments: serde_json::from_str(&editor.arguments)
                                    .map_err(|e| e.to_string())?,
                            })
                        };
                        Ok(registration)
                    })();
                    match result {
                        Ok(registration) => {
                            self.manage_language_server(Operation::Configure {
                                id: editor.id.clone(),
                                registration: Box::new(registration),
                            });
                            keep = false
                        }
                        Err(e) => self.languages.error = Some(e),
                    }
                }
                if ui.button("Cancel editing").clicked() {
                    keep = false;
                }
            });
            if keep {
                self.languages.editor = Some(editor);
            }
        }
    }
    pub(super) fn language_panels(&mut self, ctx: &egui::Context) {
        if let Some(content) = &self.languages.info {
            let mut open = true;
            egui::Window::new("Language information")
                .open(&mut open)
                .default_width(550.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(400.0)
                        .show(ui, |ui| {
                            ui.add(egui::Label::new(content).selectable(true).wrap());
                        });
                });
            if !open {
                self.languages.info = None;
                self.focus_editor = true;
            }
        }
        if !self.languages.problems {
            return;
        }
        let mut jump = None;
        egui::TopBottomPanel::bottom("language-problems")
            .resizable(true)
            .default_height(150.0)
            .max_height(300.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Language server problems");
                    if controls::close_button(ui, "Close problems").clicked() {
                        self.languages.problems = false;
                    }
                });
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.languages.diagnostics.is_empty() {
                        ui.weak("No diagnostics for open files.");
                    }
                    for (path, diagnostics) in &self.languages.diagnostics {
                        for item in &diagnostics.items {
                            let Ok(range) =
                                serde_json::from_value::<text::Range>(item["range"].clone())
                            else {
                                continue;
                            };
                            let message = item["message"].as_str().unwrap_or("");
                            let severity = match item["severity"].as_u64() {
                                Some(1) => "Error",
                                Some(2) => "Warning",
                                _ => "Info",
                            };
                            if ui
                                .selectable_label(
                                    false,
                                    format!(
                                        "{severity} · {}:{} · {}",
                                        path.file_name().unwrap_or_default().to_string_lossy(),
                                        range.start.line + 1,
                                        message.chars().take(500).collect::<String>()
                                    ),
                                )
                                .clicked()
                            {
                                jump = Some((path.clone(), diagnostics.encoding, range.start));
                            }
                        }
                    }
                });
            });
        if let Some((path, encoding, position)) = jump {
            if let Some(index) = self.workspace.open.iter().position(|b| {
                b.page
                    .info
                    .path
                    .as_ref()
                    .is_some_and(|p| Path::new(p) == path)
            }) {
                let text = &self.workspace.open[index].page.text;
                if let Ok(byte) = encoding.byte(text, position) {
                    let scalar = text[..byte].chars().count();
                    self.select_tab(index);
                    self.set_selections(vec![hedit::advanced::Selection::caret(scalar)]);
                }
            }
        }
    }
}

impl HeditApp {
    pub(super) fn restart_language_servers(&mut self) {
        let ids: Vec<_> = self
            .languages
            .installed
            .iter()
            .map(|i| i.id.clone())
            .collect();
        for id in ids {
            if let Err(e) = self
                .languages
                .send(Operation::Restart { id }, Intent::Manage)
            {
                self.languages.error = Some(e);
                break;
            }
        }
    }
}

impl HeditApp {
    pub(super) fn language_completion_pending(&self) -> bool {
        self.languages
            .pending
            .values()
            .any(|i| matches!(i,Intent::Feature(guard) if guard.feature==Feature::Completion))
    }
    pub(super) fn reset_language_completion(&mut self) {
        self.languages.last_completion = None;
    }
    pub(super) fn cancel_language_requests(&mut self) {
        self.languages
            .pending
            .retain(|_, intent| !matches!(intent, Intent::Feature(_)));
        self.languages.last_completion = None;
    }
}

impl HeditApp {
    pub(super) fn language_status_button(&mut self, ui: &mut egui::Ui) {
        let b = self.active();
        let Some(path) = b.page.info.path.as_ref().map(PathBuf::from) else {
            return;
        };
        if b.page.info.paged || b.follow {
            return;
        }
        let known = manager::catalog()
            .iter()
            .any(|e| e.languages.iter().any(|l| l == b.language().id()))
            || self.languages.installed.iter().any(|e| {
                e.registration
                    .languages
                    .iter()
                    .any(|l| l == b.language().id())
            });
        if !known {
            return;
        }
        let status = self
            .languages
            .statuses
            .values()
            .filter(|s| path.starts_with(&s.root))
            .max_by_key(|s| s.root.components().count());
        let (label, detail) = status
            .map(|s| {
                (
                    format!("LSP: {}", s.state),
                    format!("{} · {}\n{}", s.id, s.root.display(), s.detail),
                )
            })
            .unwrap_or_else(|| {
                (
                    "LSP: Set up…".into(),
                    "Install or configure a language server for this file".into(),
                )
            });
        if ui.small_button(label).on_hover_text(detail).clicked() {
            self.open_preferences(PreferencesPage::LanguageServers);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{finish_app_job, frame, frame_size};
    use super::*;
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let (workspace, _) = Workspace::restore(root.join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let path = root.join("main.rs");
        std::fs::write(&path, "fo").unwrap();
        std::fs::write(root.join("target.rs"), "😀 target\n").unwrap();
        app.open(&ctx, Some(path));
        finish_app_job(&mut app, &ctx);
        app.set_selections(vec![hedit::advanced::Selection::caret(2)]);
        let python = std::process::Command::new(if cfg!(windows) { "python" } else { "python3" })
            .args(["-c", "import sys; print(sys.executable)"])
            .output()
            .unwrap();
        assert!(python.status.success());
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("crates/hedit-core/tests/support/fake_language_server.py");
        let manager = manager::Manager::new(root.join("servers"));
        manager
            .configure(
                "fixture",
                Registration {
                    languages: vec!["rust".into()],
                    external: Some(manager::Launch {
                        executable: String::from_utf8(python.stdout).unwrap().trim().into(),
                        arguments: vec![
                            script.display().to_string(),
                            "--encoding".into(),
                            "utf-8".into(),
                        ],
                    }),
                    ..Registration::default()
                },
            )
            .unwrap();
        app.languages.directory = Some(manager.root().to_owned());
        pump(&ctx, &mut app, |a| {
            a.languages.statuses.values().any(|s| s.state == "Running")
                && a.languages.pending.is_empty()
        });
        frame(&ctx, &mut app, vec![]);
        (dir, ctx, app)
    }
    fn pump(ctx: &egui::Context, app: &mut HeditApp, done: impl Fn(&HeditApp) -> bool) {
        let start = Instant::now();
        loop {
            app.poll_language_servers(ctx);
            if done(app) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "LSP UI timeout: {:?}",
                app.languages.error
            );
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn language_formatting_uses_normal_undo_and_recovery() {
        let (_dir, ctx, mut app) = app();
        assert!(app.request_language_feature(&ctx, Feature::Formatting));
        pump(&ctx, &mut app, |a| {
            !a.languages
                .pending
                .values()
                .any(|i| matches!(i, Intent::Feature(_)))
        });
        assert_eq!(app.active().page.text, "// formatted\nfo");
        assert!(app.active().page.info.dirty);
        app.history(&ctx, false);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "fo");
    }
    #[test]
    fn stopping_one_server_keeps_diagnostics_from_other_sessions() {
        let (_dir, ctx, mut app) = app();
        let path = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
        let root = app.language_root(&path);
        app.language_event(
            &ctx,
            Event::Diagnostics {
                server: "fixture".into(),
                root: root.clone(),
                path: path.clone(),
                revision: app.active().page.info.revision,
                encoding: Encoding::Utf16,
                items: vec![json!({"message":"keep this diagnostic"})],
            },
        );
        for (id, affected_root) in [
            ("another-server", root.clone()),
            ("fixture", root.join("other")),
        ] {
            app.language_event(
                &ctx,
                Event::Status(Status {
                    id: id.into(),
                    root: affected_root,
                    state: "Failed".into(),
                    detail: "another session stopped".into(),
                    logs: vec![],
                }),
            );
            assert!(
                app.languages.diagnostics.contains_key(&path),
                "unrelated status cleared diagnostics"
            );
        }
        app.language_event(
            &ctx,
            Event::Status(Status {
                id: "fixture".into(),
                root,
                state: "Stopped".into(),
                detail: String::new(),
                logs: vec![],
            }),
        );
        assert!(!app.languages.diagnostics.contains_key(&path));
    }
    #[test]
    fn language_hover_and_problems_are_available_from_commands() {
        let (_dir, ctx, mut app) = app();
        app.execute(&ctx, Command::LanguageHover);
        pump(&ctx, &mut app, |a| a.languages.info.is_some());
        assert_eq!(
            app.languages.info.as_deref().unwrap().trim(),
            "Fixture hover information"
        );
        app.open_completion(&ctx);
        assert!(
            !app.auto.visible(),
            "completion must not edit behind the language information window"
        );
        assert_eq!(app.active().page.text, "fo");
        app.execute(&ctx, Command::LanguageProblems);
        assert!(app.languages.problems);
        assert!(app
            .languages
            .diagnostics
            .values()
            .flat_map(|diagnostics| &diagnostics.items)
            .any(|d| d["message"] == "fixture diagnostic"));
        app.escape(&ctx); // No key event: remains open.
    }
    #[test]
    fn language_results_are_discarded_after_cursor_revision_or_pane_changes() {
        let (_dir, ctx, mut app) = app();
        for change in 0..5 {
            let b = app.active();
            let guard = Guard {
                view: b.view.id,
                revision: b.page.info.revision,
                head: b.view.head,
                pane: app.panes.focused,
                anchor: b.view.anchor,
                selections: b.view.selections.clone(),
                root: app.language_root(&PathBuf::from(b.page.info.path.as_ref().unwrap())),
                feature: Feature::Formatting,
            };
            let revision = guard.revision;
            let path = PathBuf::from(b.page.info.path.as_ref().unwrap());
            let ticket = 100 + change;
            app.languages.pending.insert(ticket, Intent::Feature(guard));
            match change {
                0 => app.workspace.open[app.workspace.active].view.head = 0,
                1 => app.workspace.open[app.workspace.active].page.info.revision += 1,
                2 => app.panes.focused = 1,
                3 => app.workspace.open[app.workspace.active].view.anchor = 1,
                _ => app.busy = true,
            }
            app.language_event(&ctx,Event::Response{ticket,result:Ok(Reply{path,revision,encoding:Encoding::Utf8,feature:Feature::Formatting,value:json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"BAD"}])})});
            assert_eq!(app.active().page.text, "fo");
        }
    }
    #[test]
    fn language_completion_applies_server_range_and_is_one_undo_step() {
        let (_dir, ctx, mut app) = app();
        app.open_completion(&ctx);
        pump(&ctx, &mut app, |a| {
            !a.languages
                .pending
                .values()
                .any(|i| matches!(i, Intent::Feature(_)))
        });
        let popup = app.auto.popup.as_ref().unwrap();
        let index = popup
            .choices
            .iter()
            .position(|c| matches!(c, auto_complete::Choice::Language(_)))
            .expect("LSP choice");
        app.commit_choice(index);
        assert_eq!(app.active().page.text, "format_value");
        assert_eq!(app.active().view.head, 12);
        app.history(&ctx, false);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "fo");
    }
    #[test]
    fn navigation_converts_utf8_server_columns_before_opening_unicode_target() {
        let (_dir, ctx, mut app) = app();
        assert!(app.request_language_feature(&ctx, Feature::Definition));
        pump(&ctx, &mut app, |a| {
            !a.languages
                .pending
                .values()
                .any(|i| matches!(i, Intent::Feature(_)))
        });
        finish_app_job(&mut app, &ctx);
        assert!(app.active().view.name.ends_with("target.rs"));
        assert_eq!(app.active().view.head, 2);
    }
    #[test]
    fn settings_commands_open_manager_and_info_closes_with_escape() {
        let (_dir, ctx, mut app) = app();
        app.execute(&ctx, Command::LanguageServers);
        assert_eq!(app.preferences_page, PreferencesPage::LanguageServers);
        assert!(app.preferences_open);
        app.languages.info = Some("info".into());
        frame(
            &ctx,
            &mut app,
            vec![super::super::tests::key(Key::Escape, Modifiers::NONE)],
        );
        assert!(app.languages.info.is_none());
    }
    #[test]
    fn manager_renders_at_minimum_window_size_and_keeps_external_configuration() {
        let (_dir, ctx, mut app) = app();
        app.execute(&ctx, Command::LanguageServers);
        for _ in 0..3 {
            let _ = frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        }
        assert!(app
            .languages
            .installed
            .iter()
            .any(|s| s.id == "fixture" && s.registration.external.is_some()));
    }
    #[test]
    fn navigation_falls_back_to_source_indexing_when_the_server_lacks_the_capability() {
        let (_dir, ctx, mut app) = app();
        let mut registration = app
            .languages
            .installed
            .iter()
            .find(|i| i.id == "fixture")
            .unwrap()
            .registration
            .clone();
        registration
            .external
            .as_mut()
            .unwrap()
            .arguments
            .push("--unsupported-definitions".into());
        app.manage_language_server(Operation::Configure {
            id: "fixture".into(),
            registration: Box::new(registration),
        });
        pump(&ctx, &mut app, |a| a.languages.pending.is_empty());
        let source = "fn chosen() {}\nchosen();\n";
        let caret = source.find("chosen();").unwrap() + 2;
        app.batch(hedit::advanced::Batch {
            text: source.into(),
            selections: vec![hedit::advanced::Selection::caret(caret)],
        });
        app.look_up(&ctx, false);
        pump(&ctx, &mut app, |a| {
            !a.languages
                .pending
                .values()
                .any(|i| matches!(i, Intent::Feature(_)))
        });
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().view.head, 3);
        assert_eq!(app.active().page.text, source);
    }
    #[test]
    fn escape_cancels_pending_editor_results_and_keeps_manager_requests() {
        let (_dir, ctx, mut app) = app();
        assert!(app.request_language_feature(&ctx, Feature::Formatting));
        app.languages.send(Operation::List, Intent::List).unwrap();
        frame(
            &ctx,
            &mut app,
            vec![super::super::tests::key(Key::Escape, Modifiers::NONE)],
        );
        assert!(!app
            .languages
            .pending
            .values()
            .any(|i| matches!(i, Intent::Feature(_))));
        pump(&ctx, &mut app, |a| a.languages.pending.is_empty());
        assert_eq!(app.active().page.text, "fo");
    }
    #[test]
    fn refreshing_reconciles_a_registration_changed_by_another_window() {
        let (dir, ctx, mut app) = app();
        let mut registration = app
            .languages
            .installed
            .iter()
            .find(|i| i.id == "fixture")
            .unwrap()
            .registration
            .clone();
        registration.languages = vec!["python".into()];
        app.manage_language_server(Operation::Configure {
            id: "fixture".into(),
            registration: Box::new(registration.clone()),
        });
        pump(&ctx, &mut app, |a| {
            a.languages.pending.is_empty() && !a.languages.snapshots.is_empty()
        });
        assert!(app
            .languages
            .statuses
            .values()
            .all(|s| s.state != "Running"));
        registration.languages = vec!["rust".into()];
        manager::Manager::new(dir.path().canonicalize().unwrap().join("servers"))
            .configure("fixture", registration)
            .unwrap();
        app.languages.send(Operation::List, Intent::List).unwrap();
        pump(&ctx, &mut app, |a| {
            a.languages.pending.is_empty()
                && a.languages.statuses.values().any(|s| s.state == "Running")
        });
    }
    #[test]
    fn paged_documents_and_unsaved_notes_do_not_enter_lsp_snapshots() {
        let (_dir, _ctx, mut app) = app();
        assert_eq!(app.language_snapshots().len(), 1);
        app.workspace.open[app.workspace.active].page.info.paged = true;
        assert!(app.language_snapshots().is_empty());
        app.workspace.open[app.workspace.active].page.info.paged = false;
        app.workspace.open[app.workspace.active].page.info.path = None;
        assert!(app.language_snapshots().is_empty());
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    #[test]
    #[ignore = "manual native language-server settings export to HEDIT_PREVIEW_DIR"]
    fn export_language_server_settings() {
        let output =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&output).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.open_preferences(PreferencesPage::LanguageServers);
        for (name, size) in [
            ("wide", egui::vec2(1280.0, 820.0)),
            ("small", egui::vec2(720.0, 480.0)),
        ] {
            for _ in 0..3 {
                super::super::tests::frame_size(&ctx, &mut app, vec![], size);
            }
            let frame = super::super::tests::frame_size(&ctx, &mut app, vec![], size);
            std::fs::write(
                output.join(format!("language-servers-{name}.svg")),
                crate::preview::export(
                    &frame.shapes,
                    [size.x, size.y],
                    app.theme.background.color(),
                ),
            )
            .unwrap();
            std::fs::write(
                output.join(format!("language-servers-{name}.ppm")),
                super::super::rtl_tests::rasterize(&ctx, frame, size, 1.0),
            )
            .unwrap();
        }
    }
}
