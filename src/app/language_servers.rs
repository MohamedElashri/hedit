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
    language: syntax::Language,
    formatting_options: (u8, bool),
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
    Save(String),
    Detect(String),
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
    pub(super) installed: Vec<Installed>,
    statuses: BTreeMap<(String, PathBuf), Status>,
    pending: HashMap<u64, Intent>,
    next: u64,
    snapshots: Vec<Snapshot>,
    observed: Option<(Vec<Snapshot>, Instant)>,
    diagnostics: BTreeMap<PathBuf, Vec<Diagnostics>>,
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
    initialization: String,
    environment: String,
    markers: String,
    override_arguments: bool,
    new: bool,
    registration: Registration,
}
impl Configuration {
    fn new(id: String, registration: Registration, new: bool) -> Self {
        Self {
            languages: registration.languages.join(", "),
            executable: registration
                .external
                .as_ref()
                .map(|l| l.executable.display().to_string())
                .unwrap_or_default(),
            arguments: serde_json::to_string_pretty(
                &registration
                    .arguments
                    .clone()
                    .or_else(|| registration.external.as_ref().map(|l| l.arguments.clone()))
                    .or_else(|| registration.managed.as_ref().map(|r| r.arguments.clone()))
                    .unwrap_or_else(|| manager::default_arguments(&id)),
            )
            .unwrap(),
            settings: serde_json::to_string_pretty(&registration.configuration).unwrap(),
            initialization: registration
                .initialization_options
                .as_ref()
                .map(|v| serde_json::to_string_pretty(v).unwrap())
                .unwrap_or_default(),
            environment: serde_json::to_string_pretty(&registration.environment).unwrap(),
            markers: registration.root_markers.join(", "),
            override_arguments: registration.arguments.is_some(),
            id,
            new,
            registration,
        }
    }
    fn registration(&self) -> Result<Registration, String> {
        manager::Manager::validate_id(&self.id)?;
        let mut registration = self.registration.clone();
        registration.languages = self
            .languages
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        registration.root_markers = self
            .markers
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        registration.configuration =
            serde_json::from_str(&self.settings).map_err(|e| format!("Server settings: {e}"))?;
        registration.initialization_options = if self.initialization.trim().is_empty() {
            None
        } else {
            Some(
                serde_json::from_str(&self.initialization)
                    .map_err(|e| format!("Initialization options: {e}"))?,
            )
        };
        registration.environment =
            serde_json::from_str(&self.environment).map_err(|e| format!("Environment: {e}"))?;
        let external = !self.executable.trim().is_empty();
        let arguments: Vec<String> = if self.override_arguments || external {
            serde_json::from_str(&self.arguments).map_err(|e| format!("Arguments: {e}"))?
        } else {
            vec![]
        };
        registration.arguments = self.override_arguments.then_some(arguments.clone());
        registration.external = if self.executable.trim().is_empty() {
            None
        } else {
            Some(manager::Launch {
                executable: self.executable.trim().into(),
                arguments,
            })
        };
        manager::validate(&registration)?;
        Ok(registration)
    }
}
impl LanguageServers {
    fn send(&mut self, operation: Operation, intent: Intent) -> Result<(), String> {
        let service = self
            .service
            .as_ref()
            .ok_or("Language-server manager is not available.")?;
        let changes_servers = matches!(
            operation,
            Operation::Install { .. }
                | Operation::Remove { .. }
                | Operation::Configure { .. }
                | Operation::Restart { .. }
                | Operation::Stop { .. }
        );
        self.next += 1;
        service.submit(self.next, operation)?;
        if changes_servers {
            // Replies already in flight belong to the previous configuration.
            // Invalidate them when the change is queued, before the next poll.
            self.pending
                .retain(|_, intent| !matches!(intent, Intent::Feature(_)));
            self.last_completion = None;
        }
        self.pending.insert(self.next, intent);
        Ok(())
    }
    fn managing(&self) -> bool {
        self.pending.values().any(|i| {
            matches!(
                i,
                Intent::Manage | Intent::List | Intent::Save(_) | Intent::Detect(_)
            )
        })
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
        self.languages.diagnostics.retain(|path, sources| {
            sources.retain(|diagnostics| {
                self.workspace.open.iter().any(|b| {
                    b.page
                        .info
                        .path
                        .as_ref()
                        .is_some_and(|p| Path::new(p) == path)
                        && b.page.info.revision == diagnostics.revision
                })
            });
            !sources.is_empty()
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
                if status.state == "Failed" {
                    self.languages.snapshots.clear();
                }
                if status.state != "Running" {
                    self.languages.diagnostics.retain(|_, sources| {
                        sources.retain(|d| d.server != status.id || d.root != status.root);
                        !sources.is_empty()
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
                    let sources = self.languages.diagnostics.entry(path).or_default();
                    sources.retain(|d| d.server != server || d.root != root);
                    if !items.is_empty() {
                        sources.push(Diagnostics {
                            server,
                            root,
                            revision,
                            encoding,
                            items,
                        });
                    }
                    sources.sort_by(|a, b| (&a.server, &a.root).cmp(&(&b.server, &b.root)));
                    self.languages
                        .diagnostics
                        .retain(|_, sources| !sources.is_empty());
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
                        Some(Intent::Detect(id)) => {
                            if let Some(editor) = &mut self.languages.editor {
                                if editor.id == id {
                                    editor.executable = serde_json::from_value::<PathBuf>(value)
                                        .map(|p| p.display().to_string())
                                        .unwrap_or_default();
                                }
                            }
                        }
                        Some(Intent::Manage | Intent::Save(_)) => {
                            if let Some(Intent::Save(id)) = intent {
                                if self.languages.editor.as_ref().is_some_and(|e| e.id == id) {
                                    self.languages.editor = None;
                                }
                            }
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
                    || b.language() != guard.language
                    || (guard.feature == Feature::Formatting
                        && (
                            b.view.tab_size.unwrap_or(4),
                            b.view.indent.as_deref() != Some("tabs"),
                        ) != guard.formatting_options)
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
            || self.languages.managing()
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
                && e.registration.features.enabled(feature)
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
            language: b.language(),
            formatting_options: (
                b.view.tab_size.unwrap_or(4),
                b.view.indent.as_deref() != Some("tabs"),
            ),
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
            tab_size: guard.formatting_options.0,
            spaces: guard.formatting_options.1,
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
                                .is_some_and(|p| text::same_path(Path::new(p), &path))
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
        if self.languages.progress.is_empty() && self.languages.managing() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Applying server changes…");
            });
        }
        if let Some(error) = &self.languages.error {
            ui.colored_label(self.theme.warning.color(), error);
        }
        if self.languages.editor.is_some() {
            self.language_configuration_editor(ui);
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.weak(format!(
                "{} configured servers",
                self.languages.installed.len()
            ));
            ui.weak("Choose Configure for launch, features and advanced settings.");
        });
        let mut entries: Vec<_> = manager::catalog()
            .into_iter()
            .map(|c| {
                let managed = !c.repository.is_empty();
                (c.id, c.languages, c.requirements, managed)
            })
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
                            .unwrap_or_else(|| manager::preset(&id));
                        self.languages.editor =
                            Some(Configuration::new(id.clone(), registration, false));
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
                        let key = root.to_string_lossy().into_owned();
                        let mut choice = registration.workspaces.get(&key).copied();
                        let previous = choice;
                        egui::ComboBox::from_id_salt(("server-workspace", &id))
                            .selected_text(match choice {
                                None => "Workspace: use default",
                                Some(true) => "Workspace: enabled",
                                Some(false) => "Workspace: disabled",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut choice, None, "Use default");
                                ui.selectable_value(&mut choice, Some(true), "Enabled");
                                ui.selectable_value(&mut choice, Some(false), "Disabled");
                            });
                        if choice != previous {
                            if let Some(enabled) = choice {
                                registration.workspaces.insert(key, enabled);
                            } else {
                                registration.workspaces.remove(&key);
                            }
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
            self.languages.editor = Some(Configuration::new(
                String::new(),
                Registration {
                    initialization_options: Some(json!({})),
                    ..Registration::default()
                },
                true,
            ));
        }
    }
    fn language_configuration_editor(&mut self, ui: &mut egui::Ui) {
        let Some(mut editor) = self.languages.editor.take() else {
            return;
        };
        let mut keep = true;
        let mut save = false;
        let busy = self.languages.managing();
        ui.group(|ui| {
            ui.strong(format!("Configure {}", if editor.id.is_empty() { "external server" } else { &editor.id }));
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal(|ui| {
                    save |= ui.button("Save configuration").clicked();
                    if ui.button("Cancel editing").clicked() { keep = false; }
                });
                if editor.new { ui.label("Server ID"); ui.text_edit_singleline(&mut editor.id); }
                ui.checkbox(&mut editor.registration.enabled, "Enabled by default");
                if let Some(path) = self.active().page.info.path.as_ref().map(PathBuf::from) {
                    let root = self.language_root(&path);
                    let key = root.to_string_lossy().into_owned();
                    let mut choice = editor.registration.workspaces.get(&key).copied();
                    egui::ComboBox::from_id_salt("editing-server-workspace")
                        .selected_text(match choice { None => "Workspace: use default", Some(true) => "Workspace: enabled", Some(false) => "Workspace: disabled" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut choice, None, "Use default");
                            ui.selectable_value(&mut choice, Some(true), "Enabled");
                            ui.selectable_value(&mut choice, Some(false), "Disabled");
                        });
                    if let Some(enabled) = choice { editor.registration.workspaces.insert(key, enabled); } else { editor.registration.workspaces.remove(&key); }
                }
                ui.label("Language IDs (comma separated)");
                ui.text_edit_singleline(&mut editor.languages);
                ui.label("Executable path (empty uses managed installation)");
                ui.text_edit_singleline(&mut editor.executable);
                if manager::catalog().iter().any(|e| e.id == editor.id) && ui.button("Detect installed on PATH").clicked() {
                    self.languages.error = None;
                    if let Err(e) = self.languages.send(Operation::Detect { id: editor.id.clone() }, Intent::Detect(editor.id.clone())) { self.languages.error = Some(e); }
                }
                if cfg!(windows) { ui.small("For npm servers on Windows, choose node.exe and put the server script followed by --stdio in Arguments."); }
                ui.horizontal(|ui| {
                    ui.label("Priority"); ui.add(egui::DragValue::new(&mut editor.registration.priority));
                });
                ui.small("Higher priority wins for each supported action. Equal priorities use server ID order. Diagnostics are combined.");
                ui.label("Features");
                let features = &mut editor.registration.features;
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut features.completion, "Completion"); ui.checkbox(&mut features.hover, "Hover");
                    ui.checkbox(&mut features.definition, "Definition"); ui.checkbox(&mut features.references, "References");
                    ui.checkbox(&mut features.formatting, "Formatting"); ui.checkbox(&mut features.diagnostics, "Diagnostics");
                });
                if editor.executable.trim().is_empty() { ui.checkbox(&mut editor.override_arguments, "Override default launch arguments"); }
                if ui.small_button("Use preset arguments").clicked() {
                    editor.arguments = serde_json::to_string_pretty(&editor.registration.managed.as_ref().map(|r| r.arguments.clone()).unwrap_or_else(|| manager::default_arguments(&editor.id))).unwrap();
                    editor.override_arguments = false;
                }
                ui.add_enabled_ui(editor.override_arguments || !editor.executable.trim().is_empty(), |ui| {
                    ui.label("Arguments (JSON array, for example [\"--stdio\"])");
                    ui.add(egui::TextEdit::multiline(&mut editor.arguments).desired_width(f32::INFINITY).desired_rows(2).code_editor());
                });
                egui::CollapsingHeader::new("Advanced launch and project settings").id_salt(("advanced-lsp", &editor.id)).show(ui, |ui| {
                ui.label("Server settings (JSON object)");
                ui.add(egui::TextEdit::multiline(&mut editor.settings).desired_width(f32::INFINITY).desired_rows(4).code_editor());
                    ui.label("Initialization options (JSON object)");
                    ui.small("{} sends empty options; blank uses Server settings for compatibility.");
                    ui.add(egui::TextEdit::multiline(&mut editor.initialization).desired_width(f32::INFINITY).desired_rows(4).code_editor());
                    ui.label("Environment overrides (JSON object of strings)");
                    ui.add(egui::TextEdit::multiline(&mut editor.environment).desired_width(f32::INFINITY).desired_rows(3).code_editor());
                    ui.label("Project root markers (comma separated)");
                    ui.text_edit_singleline(&mut editor.markers);
                    ui.small("Files or directories relative to a project root. Empty uses language defaults. Search stays inside the workspace.");
                });
                ui.horizontal(|ui| {
                    save |= ui.button("Save configuration").clicked();
                    if ui.button("Cancel editing").clicked() { keep = false; }
                });
            });
        });
        if keep && save {
            match editor.registration() {
                Ok(registration) => {
                    self.languages.error = None;
                    if let Err(e) = self.languages.send(
                        Operation::Configure {
                            id: editor.id.clone(),
                            registration: Box::new(registration),
                        },
                        Intent::Save(editor.id.clone()),
                    ) {
                        self.languages.error = Some(e);
                    }
                }
                Err(e) => self.languages.error = Some(e),
            }
        }
        if keep {
            self.languages.editor = Some(editor);
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
                    for (path, sources) in &self.languages.diagnostics {
                        for (diagnostics, item) in sources
                            .iter()
                            .flat_map(|d| d.items.iter().map(move |item| (d, item)))
                        {
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
                                        "{severity} · {} · {}:{} · {}",
                                        diagnostics.server,
                                        path.file_name().unwrap_or_default().to_string_lossy(),
                                        u64::from(range.start.line) + 1,
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
        let root = self.language_root(&path);
        let relevant: Vec<_> = self
            .languages
            .installed
            .iter()
            .filter(|entry| {
                entry.registration.enabled_at(&root)
                    && entry
                        .registration
                        .languages
                        .iter()
                        .any(|l| l == b.language().id())
                    && (entry.registration.external.is_some()
                        || entry.registration.managed.is_some())
            })
            .filter_map(|entry| {
                self.languages
                    .statuses
                    .values()
                    .filter(|s| s.id == entry.id && path.starts_with(&s.root))
                    .max_by_key(|s| s.root.components().count())
            })
            .collect();
        let running = relevant.iter().filter(|s| s.state == "Running").count();
        let label = if running > 1 {
            format!("LSP: {running} running")
        } else if let Some(status) = relevant
            .iter()
            .find(|s| s.state == "Running")
            .or_else(|| relevant.first())
        {
            format!("LSP: {}", status.state)
        } else {
            "LSP: Set up…".into()
        };
        let detail = if relevant.is_empty() {
            "Install or configure a language server for this file".into()
        } else {
            relevant
                .iter()
                .map(|s| {
                    format!(
                        "{} · {} · {}\n{}",
                        s.id,
                        s.state,
                        s.root.display(),
                        s.detail
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
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
                && !a.languages.diagnostics.is_empty()
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
    fn formatting_responses_require_the_original_syntax_and_format_options() {
        for change in 0..4 {
            let (_dir, ctx, mut app) = app();
            assert!(app.request_language_feature(&ctx, Feature::Formatting));
            let ticket = app.languages.next;
            let path = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
            let revision = app.active().page.info.revision;
            match change {
                0 => app.set_syntax(Some(syntax::Language::Python)),
                1 => app.set_browser_value("tab_size", json!(8)).unwrap(),
                2 => app
                    .set_browser_value("translate_tabs_to_spaces", json!(false))
                    .unwrap(),
                _ => app.set_browser_value("font_size", json!(20.0)).unwrap(),
            }
            assert_eq!(app.active().page.info.revision, revision);
            app.language_event(&ctx, Event::Response {
                ticket,
                result: Ok(Reply {
                    path,
                    revision,
                    encoding: Encoding::Utf8,
                    feature: Feature::Formatting,
                    value: json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"FORMATTED"}]),
                }),
            });
            assert_eq!(
                app.active().page.text,
                if change == 3 { "FORMATTEDfo" } else { "fo" },
                "format context change {change}"
            );
        }
    }
    #[test]
    fn managing_servers_discards_earlier_feature_responses() {
        let (_dir, ctx, mut app) = app();
        for operation in [
            Operation::Stop {
                id: "fixture".into(),
            },
            Operation::Restart {
                id: "fixture".into(),
            },
            Operation::Configure {
                id: "fixture".into(),
                registration: Box::new(app.languages.installed[0].registration.clone()),
            },
            // Unknown managed IDs fail before performing any network access.
            Operation::Install {
                id: "fixture".into(),
            },
            Operation::Remove {
                id: "fixture".into(),
            },
        ] {
            assert!(app.request_language_feature(&ctx, Feature::Formatting));
            let ticket = app.languages.next;
            let path = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
            let revision = app.active().page.info.revision;
            app.languages.send(operation, Intent::Manage).unwrap();
            app.language_event(&ctx, Event::Response {
                ticket,
                result: Ok(Reply {
                    path,
                    revision,
                    encoding: Encoding::Utf8,
                    feature: Feature::Formatting,
                    value: json!([{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"newText":"STALE"}]),
                }),
            });
            assert_eq!(
                app.active().page.text,
                "fo",
                "a response survived server management"
            );
            assert!(
                !app.request_language_feature(&ctx, Feature::Formatting),
                "requests must wait for refreshed configuration"
            );
            pump(&ctx, &mut app, |a| a.languages.pending.is_empty());
        }
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
    fn malformed_diagnostic_line_numbers_cannot_crash_or_move_the_editor() {
        let (_dir, ctx, mut app) = app();
        let path = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
        app.language_event(&ctx, Event::Diagnostics {
            server: "fixture".into(),
            root: app.language_root(&path),
            path,
            revision: app.active().page.info.revision,
            encoding: Encoding::Utf16,
            items: vec![json!({
                "range":{"start":{"line":u32::MAX,"character":0},"end":{"line":u32::MAX,"character":0}},
                "severity":2,"message":"unexpected line"
            })],
        });
        app.languages.problems = true;
        let draw = |app: &mut HeditApp, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 820.0),
                    )),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| app.language_panels(ctx),
            )
        };
        draw(&mut app, vec![]);
        let output = draw(&mut app, vec![]);
        let label = "Warning · fixture · main.rs:4294967296 · unexpected line";
        let position = output
            .shapes
            .iter()
            .find_map(|shape| super::super::tests::text_rect(&shape.shape, label))
            .expect("diagnostic row is visible")
            .center();
        let head = app.active().view.head;
        for pressed in [true, false] {
            draw(
                &mut app,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.active().view.head, head);
        assert_eq!(app.active().page.text, "fo");
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
            .flatten()
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
                language: b.language(),
                formatting_options: (
                    b.view.tab_size.unwrap_or(4),
                    b.view.indent.as_deref() != Some("tabs"),
                ),
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
    fn navigation_uses_unsaved_open_target_text_for_utf8_columns() {
        let (dir, ctx, mut app) = app();
        app.open(&ctx, Some(dir.path().join("target.rs")));
        finish_app_job(&mut app, &ctx);
        app.batch(hedit::advanced::Batch {
            text: "éabc target\n".into(),
            selections: vec![hedit::advanced::Selection::caret(0)],
        });
        let source = app
            .workspace
            .open
            .iter()
            .position(|b| b.view.name == "main.rs")
            .unwrap();
        app.select_tab(source);
        assert!(app.request_language_feature(&ctx, Feature::Definition));
        pump(&ctx, &mut app, |a| {
            !a.languages
                .pending
                .values()
                .any(|i| matches!(i, Intent::Feature(_)))
        });
        finish_app_job(&mut app, &ctx);
        assert!(app.active().view.name.ends_with("target.rs"));
        assert_eq!(app.active().page.text, "éabc target\n");
        assert_eq!(app.active().view.head, 4);
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
    fn cpp_status_button_uses_the_selected_cpp_server() {
        let (dir, ctx, mut app) = app();
        let path = dir.path().join("main.cpp");
        std::fs::write(&path, "int main() {}\n").unwrap();
        app.open(&ctx, Some(path));
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().language().id(), "cpp");
        let path = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
        let root = app.language_root(&path);
        let launch = app.languages.installed[0].registration.external.clone();
        app.languages.statuses.insert(
            ("clangd".into(), root.clone()),
            Status {
                id: "clangd".into(),
                root,
                state: "Starting".into(),
                detail: String::new(),
                logs: vec![],
            },
        );
        for (registered, enabled, expected) in [
            (false, false, "LSP: Set up…"),
            (true, true, "LSP: Starting"),
            (true, false, "LSP: Set up…"),
        ] {
            app.languages.installed.retain(|entry| entry.id != "clangd");
            if registered {
                app.languages.installed.push(Installed {
                    id: "clangd".into(),
                    registration: Registration {
                        languages: vec!["cpp".into()],
                        enabled,
                        external: launch.clone(),
                        ..Registration::default()
                    },
                });
            }
            let mut shown = false;
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| app.language_status_button(ui));
                });
                shown = output
                    .shapes
                    .iter()
                    .any(|shape| super::super::tests::text_rect(&shape.shape, expected).is_some());
            }
            assert!(shown, "missing C++ status: {expected}");
        }
    }
    #[test]
    fn server_configuration_save_is_visible_without_scrolling_at_minimum_size() {
        let (_dir, ctx, mut app) = app();
        app.open_preferences(PreferencesPage::LanguageServers);
        app.languages.editor = Some(Configuration::new(
            "ty".into(),
            manager::preset("ty"),
            false,
        ));
        for _ in 0..3 {
            frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        }
        let output = frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        assert!(output
            .shapes
            .iter()
            .any(
                |shape| super::super::tests::text_rect(&shape.shape, "Save configuration")
                    .is_some_and(|rect| shape.clip_rect.contains(rect.center()))
            ));
    }
    #[test]
    fn unused_managed_arguments_do_not_block_restoring_defaults() {
        let mut editor = Configuration::new("ty".into(), manager::preset("ty"), false);
        editor.arguments = "unfinished JSON".into();
        assert!(editor.registration().unwrap().arguments.is_none());
        editor.override_arguments = true;
        assert!(editor.registration().is_err());
        editor.override_arguments = false;
        editor.executable = std::env::current_exe().unwrap().display().to_string();
        assert!(
            editor.registration().is_err(),
            "external commands still validate their active arguments"
        );
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
    #[test]
    fn diagnostics_from_companion_servers_are_replaced_and_cleared_independently() {
        let (_dir, ctx, mut app) = app();
        app.languages.diagnostics.clear();
        let path = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
        let root = app.language_root(&path);
        let revision = app.active().page.info.revision;
        for (server, message) in [("ty", "typing"), ("ruff", "lint"), ("ty", "updated typing")] {
            app.language_event(
                &ctx,
                Event::Diagnostics {
                    server: server.into(),
                    root: root.clone(),
                    path: path.clone(),
                    revision,
                    encoding: Encoding::Utf16,
                    items: vec![json!({"message":message})],
                },
            );
        }
        let sources = &app.languages.diagnostics[&path];
        assert_eq!(sources.len(), 2);
        assert_eq!(
            sources.iter().find(|d| d.server == "ty").unwrap().items[0]["message"],
            "updated typing"
        );
        app.language_event(
            &ctx,
            Event::Diagnostics {
                server: "ruff".into(),
                root: root.clone(),
                path: path.clone(),
                revision,
                encoding: Encoding::Utf16,
                items: vec![],
            },
        );
        assert_eq!(app.languages.diagnostics[&path].len(), 1);
        assert_eq!(app.languages.diagnostics[&path][0].server, "ty");
        app.language_event(
            &ctx,
            Event::Status(Status {
                id: "ruff".into(),
                root: root.clone(),
                state: "Stopped".into(),
                detail: String::new(),
                logs: vec![],
            }),
        );
        assert_eq!(app.languages.diagnostics[&path].len(), 1);
        app.language_event(
            &ctx,
            Event::Status(Status {
                id: "ty".into(),
                root,
                state: "Stopped".into(),
                detail: String::new(),
                logs: vec![],
            }),
        );
        assert!(!app.languages.diagnostics.contains_key(&path));
    }
    #[test]
    fn configuration_form_roundtrips_advanced_fields_and_keeps_failed_saves() {
        let (_dir, ctx, mut app) = app();
        let registration = app.languages.installed[0].registration.clone();
        let mut editor = Configuration::new("fixture".into(), registration, false);
        editor.override_arguments = true;
        editor.arguments = r#"["--custom"]"#.into();
        editor.environment = r#"{"EXAMPLE":"value"}"#.into();
        editor.initialization = r#"{"init":true}"#.into();
        editor.settings = r#"{"runtime":true}"#.into();
        editor.markers = "project.json, .git".into();
        editor.registration.priority = 42;
        editor.registration.features.formatting = false;
        let updated = editor.registration().unwrap();
        assert_eq!(updated.arguments.as_ref().unwrap(), &["--custom"]);
        assert_eq!(updated.environment["EXAMPLE"], "value");
        assert_eq!(updated.root_markers, ["project.json", ".git"]);
        assert_eq!(updated.initialization_options, Some(json!({"init":true})));
        assert_eq!(updated.configuration, json!({"runtime":true}));
        assert!(!updated.features.formatting);
        assert_eq!(updated.priority, 42);
        editor.environment = "[]".into();
        assert!(editor.registration().is_err());
        editor.environment = r#"{"EXAMPLE":"value"}"#.into();
        app.languages.editor = Some(editor);
        // Simulate an asynchronous storage/lock failure, preserving the draft.
        app.languages
            .pending
            .insert(500, Intent::Save("fixture".into()));
        app.language_event(
            &ctx,
            Event::Result {
                ticket: 500,
                result: Err("installation is locked".into()),
            },
        );
        assert!(app.languages.editor.is_some());
        assert_eq!(
            app.languages.editor.as_ref().unwrap().settings,
            r#"{"runtime":true}"#
        );
        assert_eq!(
            app.languages.error.as_deref(),
            Some("installation is locked")
        );
        // A late PATH detection must not replace another selected server's path.
        app.languages
            .pending
            .insert(501, Intent::Detect("ty".into()));
        let before = app.languages.editor.as_ref().unwrap().executable.clone();
        app.language_event(
            &ctx,
            Event::Result {
                ticket: 501,
                result: Ok(json!("/old/server")),
            },
        );
        assert_eq!(app.languages.editor.as_ref().unwrap().executable, before);
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
            ("configuration", egui::vec2(1280.0, 820.0)),
        ] {
            if name == "configuration" {
                app.languages.editor = Some(Configuration::new(
                    "ty".into(),
                    manager::preset("ty"),
                    false,
                ));
            }
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
