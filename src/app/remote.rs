//! Connect to SSH hosts and workspaces, and control the active remote session.
use super::*;
use hedit::remote::{explorer::Catalog, Config as RemoteConfig};
use hedit_remote::{
    client::{Routing, SshTarget},
    ssh_config,
};
use std::sync::atomic::{AtomicBool, Ordering};
mod folder_picker;
mod host_import;
mod host_watch;
use folder_picker::FolderPicker;
use host_import::ImportHosts;
use host_watch::HostWatch;

#[derive(Default)]
pub(super) struct RemoteUi {
    dialog: bool,
    connect_picker: Option<ConnectPicker>,
    folder_picker: Option<FolderPicker>,
    import_hosts: Option<ImportHosts>,
    selected_config: String,
    load_started: bool,
    query: String,
    catalog: Option<Catalog>,
    hosts: ssh_config::Hosts,
    loading: Option<mpsc::Receiver<Result<DiscoveredHosts, String>>>,
    _host_watch: Option<HostWatch>,
    host_changes: Arc<AtomicBool>,
    refresh_hosts_at: Option<Instant>,
    saving: Option<mpsc::Receiver<Result<Catalog, String>>>,
    host: String,
    user: String,
    port: String,
    routing: Routing,
    root: String,
    helper: String,
    state: String,
    config: String,
    manual_helper: bool,
    config_dialog: bool,
    takeover: bool,
    editing: Option<RemoteConfig>,
    pub picker: Option<Picker>,
    pub path: String,
    pending: Option<mpsc::Receiver<Result<(), String>>>,
    connecting: Option<Connecting>,
    connection_status: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CatalogLoad {
    Read,
    ApplyConfig,
    SyncHosts,
}

struct DiscoveredHosts {
    catalog: Catalog,
    hosts: ssh_config::Hosts,
    watch: Option<HostWatch>,
    mode: CatalogLoad,
}

struct Connecting {
    events: mpsc::Receiver<ConnectEvent>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    implicit_home: bool,
    original_profile: Option<RemoteConfig>,
}
impl Drop for Connecting {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
enum ConnectEvent {
    Progress(String),
    Prepared(Result<RemoteConfig, String>),
}
struct ConnectPicker {
    query: String,
    selected: usize,
    focus: bool,
    error: Option<String>,
}

enum ConnectChoice<'a> {
    Host(&'a str),
    Workspace(&'a RemoteConfig),
}

impl ConnectChoice<'_> {
    fn label(&self) -> String {
        match self {
            Self::Host(host) => format!("Connect to {host} · Remote home"),
            Self::Workspace(config) => {
                let target = &config.target;
                let mut host = target.host.clone();
                if let Some(user) = &target.user {
                    host = format!("{user}@{host}");
                }
                if target.port != 0 {
                    host.push_str(&format!(":{}", target.port));
                }
                let root = if config.root.is_empty() {
                    "Remote home"
                } else {
                    &config.root
                };
                format!("Reopen {host} · {root}")
            }
        }
    }

    fn config(&self, ssh_config: Option<PathBuf>) -> RemoteConfig {
        match self {
            Self::Host(input) => {
                let (user, host) = input
                    .split_once('@')
                    .map_or((None, *input), |(user, host)| (Some(user.into()), host));
                RemoteConfig {
                    target: SshTarget {
                        routing: Default::default(),
                        host: host.into(),
                        user,
                        port: 0,
                        config: ssh_config,
                        helper: String::new(),
                        state: String::new(),
                    },
                    root: String::new(),
                    managed: true,
                    takeover: false,
                }
            }
            Self::Workspace(config) => (*config).clone(),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Picker {
    Open,
    Save(u64),
}
impl RemoteUi {
    pub(super) fn overlay_open(&self) -> bool {
        self.dialog
            || self.picker.is_some()
            || self.connect_picker.is_some()
            || self.folder_picker.is_some()
            || self.import_hosts.is_some()
    }

    pub(super) fn close_overlay(&mut self) {
        self.dialog = false;
        self.picker = None;
        self.connect_picker = None;
        self.folder_picker = None;
        self.import_hosts = None;
    }

    fn ready(&self) -> bool {
        self.loading.is_none()
            && self.saving.is_none()
            && self.connecting.is_none()
            && self.pending.is_none()
    }

    fn ssh_config(&self) -> Option<PathBuf> {
        self.catalog
            .as_ref()
            .and_then(|catalog| catalog.config.clone())
    }

    fn connect_choices<'a>(&'a self, query: &'a str) -> Vec<ConnectChoice<'a>> {
        let query = query.trim();
        let filter = query.to_ascii_lowercase();
        let mut choices = Vec::new();
        let exact_host = self
            .hosts
            .aliases
            .iter()
            .find(|host| host.eq_ignore_ascii_case(query));
        if let Some(host) = exact_host {
            choices.push(ConnectChoice::Host(host));
        } else if !query.is_empty() && !query.contains(['/', ' ']) {
            choices.push(ConnectChoice::Host(query));
        }
        choices.extend(
            self.hosts
                .aliases
                .iter()
                .filter(|host| {
                    !host.eq_ignore_ascii_case(query) && host.to_ascii_lowercase().contains(&filter)
                })
                .map(|host| ConnectChoice::Host(host)),
        );
        if let Some(catalog) = &self.catalog {
            choices.extend(catalog.workspaces.iter().rev().filter_map(|config| {
                let choice = ConnectChoice::Workspace(config);
                choice
                    .label()
                    .to_ascii_lowercase()
                    .contains(&filter)
                    .then_some(choice)
            }));
        }
        choices
    }

    fn updated_catalog(
        &self,
        config: RemoteConfig,
        original: Option<&RemoteConfig>,
        implicit_home: bool,
    ) -> Result<Option<Catalog>, String> {
        let Some(mut catalog) = self.catalog.clone() else {
            return Ok(None);
        };
        let original = original.filter(|original| catalog.workspaces.contains(original));
        if let Some(original) = original {
            catalog.workspaces.retain(|profile| profile != original);
        }
        if implicit_home && original.is_none() {
            catalog.remember_host_home(config)?;
        } else {
            catalog.remember(config)?;
        }
        Ok((self.catalog.as_ref() != Some(&catalog)).then_some(catalog))
    }

    fn edit(&mut self, config: Option<&RemoteConfig>, default_config: &str) {
        self.editing = config.cloned();
        self.manual_helper = config.is_some_and(|c| !c.managed);
        self.routing = config.map(|c| c.target.routing).unwrap_or_default();
        self.host = config.map(|c| c.target.host.clone()).unwrap_or_default();
        self.user = config
            .and_then(|c| c.target.user.clone())
            .unwrap_or_default();
        self.port = config
            .filter(|c| c.target.port != 0)
            .map(|c| c.target.port.to_string())
            .unwrap_or_default();
        self.root = config.map(|c| c.root.clone()).unwrap_or_default();
        self.helper = config.map(|c| c.target.helper.clone()).unwrap_or_default();
        self.state = config.map(|c| c.target.state.clone()).unwrap_or_default();
        self.config = config
            .and_then(|c| c.target.config.as_ref())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| default_config.into());
        self.takeover = false;
        self.dialog = true;
    }
    fn connection_config(&self) -> Result<RemoteConfig, String> {
        let port = if self.port.trim().is_empty() {
            0
        } else {
            let port = self
                .port
                .trim()
                .parse::<u16>()
                .map_err(|_| "Invalid SSH port.")?;
            if port == 0 {
                return Err("Invalid SSH port.".into());
            }
            port
        };
        let config = RemoteConfig {
            target: SshTarget {
                routing: self.routing,
                host: self.host.trim().into(),
                user: (!self.user.trim().is_empty()).then(|| self.user.trim().into()),
                port,
                config: (!self.config.trim().is_empty()).then(|| self.config.trim().into()),
                helper: self.helper.trim().into(),
                state: if !self.manual_helper
                    && self.editing.as_ref().is_some_and(|c| {
                        c.root != self.root.trim() && c.target.state == self.state.trim()
                    }) {
                    String::new()
                } else {
                    self.state.trim().into()
                },
            },
            root: self.root.trim().into(),
            managed: !self.manual_helper,
            takeover: self.takeover,
        };
        let mut validate = Catalog::default();
        validate.remember(config.clone())?;
        Ok(config)
    }
}
impl HeditApp {
    pub(super) fn open_remote_folder(&mut self, ctx: &egui::Context) {
        if self.remote_ui.folder_picker.is_some() {
            return;
        }
        if let Some(remote) = self.workspace.remote.clone() {
            self.close_transient(ctx);
            self.remote_ui.folder_picker = Some(FolderPicker::open(ctx, &remote));
            self.focus_editor = false;
            ctx.request_repaint();
        }
    }

    pub(super) fn show_ssh_picker(&mut self, ctx: &egui::Context) {
        if self.workspace.remote.is_some()
            || self.remote_ui.connecting.is_some()
            || self.remote_ui.pending.is_some()
        {
            self.show_remote_explorer(ctx);
            return;
        }
        self.ensure_remote_catalog(ctx);
        self.remote_ui.connect_picker = Some(ConnectPicker {
            query: String::new(),
            selected: 0,
            focus: true,
            error: None,
        });
        self.focus_editor = false;
        ctx.request_repaint();
    }

    fn ssh_picker(&mut self, ctx: &egui::Context) -> Option<RemoteConfig> {
        let mut picker = self.remote_ui.connect_picker.take()?;
        let input_id = egui::Id::new("ssh-connect-input");
        let focused = ctx.memory(|memory| memory.has_focus(input_id));
        let take = |key| focused && ctx.input_mut(|input| input.consume_key(Modifiers::NONE, key));
        let down = take(Key::ArrowDown);
        let up = take(Key::ArrowUp);
        let enter = take(Key::Enter);
        let ready = self.remote_ui.ready();
        let mut choice = None;
        let mut advanced = false;
        let shown = egui::Modal::new(egui::Id::new("ssh-connect-picker")).show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 40.0).clamp(0.0, 460.0));
            ui.heading("Connect via SSH");
            let input = ui.add(
                egui::TextEdit::singleline(&mut picker.query)
                    .id(input_id)
                    .hint_text("Hostname, SSH alias, user@host, or workspace path")
                    .desired_width(f32::INFINITY),
            );
            if picker.focus {
                input.request_focus();
                picker.focus = false;
            }
            if input.changed() {
                picker.selected = 0;
                picker.error = None;
            }
            let choices = self.remote_ui.connect_choices(&picker.query);
            let adjusted = if down {
                picker.selected.saturating_add(1)
            } else if up {
                picker.selected.saturating_sub(1)
            } else {
                picker.selected
            };
            picker.selected = adjusted.min(choices.len().saturating_sub(1));
            if self.remote_ui.loading.is_some() {
                ui.label("Reading SSH configuration…");
            } else if choices.is_empty() {
                ui.label("Enter a hostname, SSH alias, or user@host.");
            }
            egui::ScrollArea::vertical()
                .max_height(250.0_f32.min(ctx.screen_rect().height() * 0.45))
                .show(ui, |ui| {
                    for (index, option) in choices.iter().enumerate() {
                        let selected = index == picker.selected;
                        let row = ui.add_enabled(
                            ready,
                            egui::Button::new(option.label()).selected(selected).wrap(),
                        );
                        if selected && (up || down || input.changed()) {
                            row.scroll_to_me(Some(egui::Align::Center));
                        }
                        if row.clicked() || (ready && selected && enter) {
                            choice = Some(option.config(self.remote_ui.ssh_config()));
                        }
                    }
                });
            if let Some(error) = &picker.error {
                ui.colored_label(self.theme.warning.color(), error);
            }
            advanced = ui
                .add_enabled(ready, egui::Button::new("Connection settings…"))
                .clicked();
            if advanced {
                choice = choices
                    .get(picker.selected)
                    .map(|option| option.config(self.remote_ui.ssh_config()));
            }
        });
        if shown.should_close() {
            self.focus_editor = true;
            return None;
        }
        if advanced {
            let default_config = if choice.is_none() {
                self.remote_ui.selected_config.clone()
            } else {
                String::new()
            };
            self.remote_ui.edit(choice.as_ref(), &default_config);
            return None;
        }
        if let Some(config) = choice {
            match config.validate() {
                Ok(()) => {
                    self.focus_editor = true;
                    return Some(config);
                }
                Err(error) => {
                    picker.error = Some(format!(
                        "Check the SSH host and connection settings. {error}"
                    ));
                    picker.focus = true;
                    ctx.request_repaint();
                }
            }
        }
        self.remote_ui.connect_picker = Some(picker);
        None
    }

    pub(super) fn remote_sidebar_button(&mut self, ui: &mut egui::Ui) {
        if ui
            .add(
                egui::Button::new("Remote Explorer")
                    .selected(self.sidebar == workbench_navigation::Sidebar::Remote)
                    .min_size(egui::vec2(ui.available_width(), 26.0)),
            )
            .clicked()
        {
            if self.sidebar == workbench_navigation::Sidebar::Remote {
                self.sidebar = workbench_navigation::Sidebar::Files;
            } else {
                self.show_remote_explorer(ui.ctx());
            }
        }
    }
    pub(super) fn show_remote_explorer(&mut self, ctx: &egui::Context) {
        self.settings.show_sidebar = true;
        self.distraction_free = false;
        self.sidebar = workbench_navigation::Sidebar::Remote;
        self.ensure_remote_catalog(ctx);
    }
    pub(super) fn ensure_remote_catalog(&mut self, ctx: &egui::Context) {
        if self.remote_ui.catalog.is_none() && !self.remote_ui.load_started {
            self.load_remote_catalog(ctx, CatalogLoad::Read);
        }
    }
    fn load_remote_catalog(&mut self, ctx: &egui::Context, mode: CatalogLoad) {
        if !self.remote_ui.ready() {
            return;
        }
        self.remote_ui.load_started = true;
        let path = self
            .settings_files
            .dir
            .as_ref()
            .map(|p| p.join("ssh-workspaces.json"));
        let selected = self.remote_ui.selected_config.trim().to_owned();
        if mode == CatalogLoad::ApplyConfig
            && !selected.is_empty()
            && !std::path::Path::new(&selected).is_absolute()
        {
            self.error = Some("Use an absolute local SSH config path.".into());
            return;
        }
        let previous = self.remote_ui.catalog.clone().unwrap_or_default();
        let host_changes = self.remote_ui.host_changes.clone();
        let repaint = ctx.clone();
        self.remote_ui.loading = Some(remote_worker(ctx, move || {
            let mut catalog = if mode == CatalogLoad::SyncHosts {
                previous
            } else if let Some(path) = &path {
                Catalog::load(path)?
            } else {
                previous
            };
            if mode == CatalogLoad::ApplyConfig {
                let original = catalog.clone();
                catalog.config = (!selected.is_empty()).then(|| selected.into());
                if catalog != original {
                    if let Some(path) = &path {
                        catalog.save(path, &original)?;
                    }
                }
            }
            let home = directories::BaseDirs::new()
                .map(|d| d.home_dir().to_owned())
                .ok_or("Cannot find your home directory.")?;
            let mut watch = HostWatch::new(repaint, host_changes);
            let mut hosts =
                ssh_config::discover_with(catalog.config.as_deref(), &home, |path, scope| {
                    if let Some(watch) = &mut watch {
                        watch.observe(path, scope);
                    }
                });
            if watch.as_ref().is_none_or(HostWatch::incomplete) {
                hosts.warnings.push(
                    "Automatic SSH updates could not watch every config path. Use Refresh to check for changes."
                        .into(),
                );
            }
            Ok(DiscoveredHosts {
                catalog,
                hosts,
                watch,
                mode,
            })
        }));
    }

    fn sync_remote_hosts(&mut self, ctx: &egui::Context, now: Instant) {
        if self.remote_ui.host_changes.swap(false, Ordering::AcqRel) {
            self.remote_ui
                .refresh_hosts_at
                .get_or_insert(now + Duration::from_millis(150));
        }
        if let Some(at) = self.remote_ui.refresh_hosts_at {
            if now < at {
                ctx.request_repaint_after(at - now);
            } else if self.remote_ui.ready() {
                self.remote_ui.refresh_hosts_at = None;
                self.load_remote_catalog(ctx, CatalogLoad::SyncHosts);
            }
        }
    }
    fn save_remote_catalog(&mut self, ctx: &egui::Context, catalog: Catalog) {
        if self.remote_ui.saving.is_some() {
            return;
        }
        let Some(path) = self
            .settings_files
            .dir
            .as_ref()
            .map(|p| p.join("ssh-workspaces.json"))
        else {
            self.remote_ui.catalog = Some(catalog);
            return;
        };
        let expected = self.remote_ui.catalog.clone().unwrap_or_default();
        self.remote_ui.saving = Some(remote_worker(ctx, move || {
            catalog.save(&path, &expected).map(|()| catalog)
        }));
    }
    fn launch_remote(
        &mut self,
        ctx: &egui::Context,
        config: RemoteConfig,
        original_profile: Option<RemoteConfig>,
    ) {
        if self.remote_ui.pending.is_some() || self.remote_ui.connecting.is_some() {
            return;
        }
        if config.managed {
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            self.remote_ui.connecting = Some(Connecting {
                events: rx,
                cancel: cancel.clone(),
                implicit_home: config.root.is_empty() && config.target.state.is_empty(),
                original_profile,
            });
            self.remote_ui.connection_status = "Connecting to SSH host…".into();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let result = hedit::remote::bootstrap::prepare(config, &cancel, |message| {
                    let _ = tx.send(ConnectEvent::Progress(message.into()));
                    ctx.request_repaint();
                });
                let _ = tx.send(ConnectEvent::Prepared(result));
                ctx.request_repaint();
            });
            return;
        }
        self.remote_background(ctx, move || {
            config
                .target
                .validate_settings(false)
                .map_err(|e| format!("Invalid SSH settings: {e}"))?;
            let mut command = std::process::Command::new(
                hedit::windows::executable().map_err(|e| e.to_string())?,
            );
            command.args([
                "--ssh",
                &config.target.host,
                "--ssh-root",
                &config.root,
                "--ssh-helper",
                &config.target.helper,
                "--ssh-state",
                &config.target.state,
            ]);
            if config.target.port != 0 {
                command.args(["--ssh-port", &config.target.port.to_string()]);
            }
            if let Some(user) = &config.target.user {
                command.args(["--ssh-user", user]);
            }
            if let Some(path) = &config.target.config {
                command.arg("--ssh-config").arg(path);
            }
            if config.target.routing.jump {
                command.arg("--ssh-use-config-jump");
            }
            if config.target.routing.forwards {
                command.arg("--ssh-forward-config-ports");
            }
            if config.takeover {
                command.arg("--ssh-takeover");
            }
            let child = command
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| e.to_string())?;
            wait_for_window(child)
        });
    }
    pub(super) fn remote_explorer(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("remote-explorer-panel")
            .show(ui, |ui| self.remote_explorer_contents(ui));
    }
    fn remote_explorer_contents(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        let ready = self.remote_ui.ready();
        let header = ui.strong("SSH REMOTES");
        header.context_menu(|ui| {
            if ui.button("SSH configuration…").clicked() {
                self.remote_ui.config_dialog = true;
                ui.close_menu();
            }
            if ui
                .add_enabled(ready, egui::Button::new("Refresh hosts"))
                .clicked()
            {
                self.load_remote_catalog(ui.ctx(), CatalogLoad::Read);
                ui.close_menu();
            }
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready && self.workspace.remote.is_none(),
                    egui::Button::new("+ Connect…"),
                )
                .clicked()
            {
                self.show_ssh_picker(ui.ctx());
            }
            if ui
                .add_enabled(ready, egui::Button::new("Refresh"))
                .clicked()
            {
                self.load_remote_catalog(ui.ctx(), CatalogLoad::Read);
            }
        });
        if ui
            .add_enabled(
                ready && self.remote_ui.catalog.is_some(),
                egui::Button::new("Import SSH hosts…"),
            )
            .clicked()
        {
            self.close_transient(ui.ctx());
            if let Some(catalog) = &self.remote_ui.catalog {
                self.remote_ui.import_hosts = Some(ImportHosts::new(
                    &catalog.imports,
                    &self.remote_ui.hosts.aliases,
                ));
            }
            self.focus_editor = false;
            ui.ctx().request_repaint();
        }
        ui.weak("Click a host to connect · Right-click for options");
        if self.remote_ui.loading.is_some() {
            ui.spinner();
            ui.label("Reading SSH configuration…");
        }
        if self.remote_ui.pending.is_some() {
            ui.spinner();
            ui.label("Connecting / updating SSH session…");
        }
        if let Some(task) = &self.remote_ui.connecting {
            ui.spinner();
            ui.label(&self.remote_ui.connection_status);
            if ui.button("Cancel connection").clicked() {
                task.cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        if self.remote_ui.saving.is_some() {
            ui.label("Saving SSH settings…");
        }
        if let Some(remote) = self.workspace.remote.clone() {
            ui.separator();
            let (connected, trusted, pending, status) = remote.status();
            ui.strong(format!("SSH · {}", remote.config.target.host));
            ui.label(&remote.config.root);
            ui.label(if connected {
                "Connected"
            } else {
                "Disconnected"
            });
            ui.label(format!(
                "{pending} pending edits · {}",
                if trusted {
                    "Execution trusted"
                } else {
                    "Execution restricted"
                }
            ));
            ui.label(status);
            let forwarded = remote.forwarded_ports();
            if !forwarded.is_empty() {
                let ports = forwarded
                    .iter()
                    .map(u16::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                ui.label(format!("Forwarded localhost ports · {ports}"));
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        self.remote_ui.pending.is_none() && !connected,
                        egui::Button::new("Reconnect"),
                    )
                    .clicked()
                {
                    let remote = remote.clone();
                    self.remote_background(ui.ctx(), move || remote.reconnect());
                }
                if ui
                    .add_enabled(
                        self.remote_ui.pending.is_none(),
                        egui::Button::new("Disconnect"),
                    )
                    .clicked()
                {
                    let remote = remote.clone();
                    self.remote_background(ui.ctx(), move || remote.disconnect());
                }
            });
            if connected
                && ui
                    .button(if trusted {
                        "Revoke execution trust"
                    } else {
                        "Trust workspace…"
                    })
                    .clicked()
            {
                if trusted {
                    self.remote_background(ui.ctx(), move || remote.trust(false));
                } else {
                    self.remote_ui.dialog = true;
                }
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut self.remote_ui.query)
                .hint_text("Filter hosts and workspaces")
                .desired_width(f32::INFINITY),
        );
        let query = self.remote_ui.query.to_lowercase();
        let mut edit = None;
        let mut launch = None;
        let mut remove = None;
        for warning in &self.remote_ui.hosts.warnings {
            ui.colored_label(self.theme.warning.color(), warning);
        }
        if let Some(catalog) = &self.remote_ui.catalog {
            for (index, config) in catalog.workspaces.iter().enumerate() {
                let title = format!("{} · {}", config.target.host, config.root);
                if !title.to_lowercase().contains(&query) {
                    continue;
                }
                ui.push_id(index, |ui| {
                    let row = ui.add_enabled(
                        ready
                            && self.remote_ui.pending.is_none()
                            && self.remote_ui.connecting.is_none(),
                        egui::Button::new(&title).wrap(),
                    );
                    if row.clicked() {
                        launch = Some(config.clone());
                    }
                    row.context_menu(|ui| {
                        if ui
                            .add_enabled(
                                self.workspace.remote.is_none(),
                                egui::Button::new("Edit workspace…"),
                            )
                            .clicked()
                        {
                            edit = Some(config.clone());
                            ui.close_menu();
                        }
                        if ui.button("Remove saved workspace").clicked() {
                            remove = Some(index);
                            ui.close_menu();
                        }
                    });
                });
            }
        }
        let ssh_config = self.remote_ui.ssh_config();
        let mut has_imports = false;
        for host in &self.remote_ui.hosts.aliases {
            if self
                .remote_ui
                .catalog
                .as_ref()
                .is_some_and(|catalog| !catalog.imports.includes(host))
            {
                continue;
            }
            has_imports = true;
            if !host.to_lowercase().contains(&query) {
                continue;
            }
            let config = || ConnectChoice::Host(host).config(ssh_config.clone());
            let row = ui
                .add_enabled(
                    ready && self.workspace.remote.is_none() && self.remote_ui.connecting.is_none(),
                    egui::Button::new(format!("{host}  →")).frame(false),
                )
                .on_hover_text(
                    "Open the remote home directory. Right-click for connection settings.",
                );
            if row.clicked() {
                launch = Some(config());
            }
            row.context_menu(|ui| {
                if ui.button("Connect to remote home").clicked() {
                    launch = Some(config());
                    ui.close_menu();
                }
                if ui.button("Connection settings…").clicked() {
                    edit = Some(config());
                    ui.close_menu();
                }
                if ui.button("SSH configuration…").clicked() {
                    self.remote_ui.config_dialog = true;
                    ui.close_menu();
                }
            });
        }
        if self.remote_ui.hosts.aliases.is_empty() && self.remote_ui.loading.is_none() {
            ui.label("No SSH aliases found. Add a Host entry to your SSH config or connect to a hostname.");
        } else if !has_imports && self.remote_ui.loading.is_none() {
            ui.label("No imported SSH hosts.");
        }
        if let Some(config) = edit {
            self.remote_ui.edit(Some(&config), "");
        }
        if let Some(config) = launch {
            self.launch_remote(ui.ctx(), config, None);
        }
        if let Some(index) = remove {
            if let Some(mut catalog) = self.remote_ui.catalog.clone() {
                catalog.workspaces.remove(index);
                self.save_remote_catalog(ui.ctx(), catalog);
            }
        }
    }
    pub(super) fn remote_new(&mut self, ctx: &egui::Context) {
        if let Some(remote) = self.workspace.remote.clone() {
            let id = self.workspace.editor.id();
            self.job(ctx, move || {
                let mut doc = remote.scratch(id)?;
                let page = doc.page(0)?;
                Ok(Action::Add(Box::new(doc), page, 0))
            });
        }
    }
    pub(super) fn remote_controls(&mut self, ctx: &egui::Context) {
        loop {
            let event = self
                .remote_ui
                .connecting
                .as_ref()
                .map(|task| task.events.try_recv());
            match event {
                Some(Ok(ConnectEvent::Progress(message))) => {
                    self.remote_ui.connection_status = message
                }
                Some(Ok(ConnectEvent::Prepared(result))) => {
                    let connecting = self.remote_ui.connecting.take();
                    let cancelled = connecting
                        .as_ref()
                        .is_some_and(|t| t.cancel.load(std::sync::atomic::Ordering::Relaxed));
                    if cancelled {
                        self.notice = "SSH connection cancelled.".into();
                        break;
                    }
                    match result {
                        Ok(config) => {
                            let original = connecting
                                .as_ref()
                                .and_then(|task| task.original_profile.as_ref());
                            let implicit_home =
                                connecting.as_ref().is_some_and(|task| task.implicit_home);
                            match self.remote_ui.updated_catalog(
                                config.clone(),
                                original,
                                implicit_home,
                            ) {
                                Ok(Some(catalog)) => self.save_remote_catalog(ctx, catalog),
                                Ok(None) => (),
                                Err(error) => self.error = Some(error),
                            }
                            let mut resolved = config;
                            resolved.managed = false;
                            self.launch_remote(ctx, resolved, None);
                        }
                        Err(error) => {
                            self.error =
                                Some(ssh_setup_error(&self.remote_ui.connection_status, &error));
                        }
                    }
                    break;
                }
                Some(Err(mpsc::TryRecvError::Disconnected)) => {
                    self.remote_ui.connecting = None;
                    self.error =
                        Some("SSH setup worker stopped unexpectedly. Retry the connection.".into());
                    break;
                }
                _ => break,
            }
        }
        if self.remote_ui.config_dialog {
            let mut open = true;
            let mut refresh = false;
            let mut edit = false;
            let ready = self.remote_ui.ready();
            egui::Window::new("SSH configuration")
                .open(&mut open)
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("SSH config (blank uses ~/.ssh/config and system config)");
                    ui.text_edit_singleline(&mut self.remote_ui.selected_config);
                    ui.horizontal(|ui| {
                        edit = ui.button("Open SSH config…").clicked();
                        refresh = ui
                            .add_enabled(ready, egui::Button::new("Refresh hosts"))
                            .clicked();
                    });
                });
            self.remote_ui.config_dialog = open;
            if refresh {
                self.load_remote_catalog(ctx, CatalogLoad::ApplyConfig);
            }
            if edit {
                let path = if self.remote_ui.selected_config.trim().is_empty() {
                    directories::BaseDirs::new().map(|d| d.home_dir().join(".ssh/config"))
                } else {
                    Some(PathBuf::from(self.remote_ui.selected_config.trim()))
                };
                if let Some(path) = path {
                    if !path.is_absolute() {
                        self.error = Some("Use an absolute local SSH config path.".into());
                    } else if self.workspace.remote.is_some() {
                        self.error =
                            Some("Open the local SSH config from a local editor window.".into());
                    } else {
                        self.open(ctx, Some(path));
                        self.remote_ui.config_dialog = false;
                    }
                }
            }
        }
        if let Some(result) = remote_worker_result(&mut self.remote_ui.pending, "SSH operation") {
            if let Err(error) = result {
                self.error = Some(error);
            }
            ctx.request_repaint();
        }
        if let Some(result) =
            remote_worker_result(&mut self.remote_ui.loading, "SSH configuration load")
        {
            match result {
                Ok(DiscoveredHosts {
                    catalog,
                    hosts,
                    watch,
                    mode,
                }) => {
                    if mode != CatalogLoad::SyncHosts {
                        self.remote_ui.selected_config = catalog
                            .config
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                    }
                    self.remote_ui.catalog = Some(catalog);
                    self.remote_ui.hosts = hosts;
                    self.remote_ui._host_watch = watch;
                }
                Err(error) => self.error = Some(error),
            }
        }
        if let Some(result) = remote_worker_result(&mut self.remote_ui.saving, "SSH workspace save")
        {
            match result {
                Ok(catalog) => self.remote_ui.catalog = Some(catalog),
                Err(error) => self.error = Some(error),
            }
        }
        self.sync_remote_hosts(ctx, Instant::now());
        if let Some(mut import) = self.remote_ui.import_hosts.take() {
            let (cancel, imports) =
                import.show(ctx, &self.remote_ui.hosts.aliases, self.remote_ui.ready());
            if let Some(imports) = imports {
                if let Some(mut catalog) = self.remote_ui.catalog.clone() {
                    match catalog.set_imports(imports) {
                        Ok(()) if Some(&catalog) != self.remote_ui.catalog.as_ref() => {
                            self.save_remote_catalog(ctx, catalog);
                        }
                        Ok(()) => {}
                        Err(error) => {
                            self.error = Some(error);
                            self.remote_ui.import_hosts = Some(import);
                        }
                    }
                }
            } else if !cancel {
                self.remote_ui.import_hosts = Some(import);
            }
        }
        let remote = self.workspace.remote.clone();
        if let Some(config) = self.ssh_picker(ctx) {
            self.show_remote_explorer(ctx);
            self.launch_remote(ctx, config, None);
        }
        if self.remote_ui.dialog {
            let mut open = true;
            let mut connect = false;
            let mut save = false;
            let mut trust = false;
            egui::Window::new(if remote.is_some() { "Trust SSH workspace" } else { "Connect via SSH" })
                .open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
                    ui.set_max_width(520.0);
                    if remote.is_some() {
                        ui.label("Git, builds, and terminals execute code with your SSH account’s permissions. Repository configuration, hooks, filters, and build scripts may run programs. Trust only a workspace and host you control.");
                        trust = ui.add_enabled(self.remote_ui.pending.is_none(), egui::Button::new("Trust this connection for execution")).clicked();
                    } else {
                        ui.label("Pinned SSH host keys · recovery stays on the host");
                        egui::Grid::new("ssh-fields").num_columns(2).show(ui, |ui| {
                            for (label, value) in [
                                ("Host / SSH alias", &mut self.remote_ui.host),
                                ("User (from SSH config)", &mut self.remote_ui.user),
                                ("Port (from SSH config)", &mut self.remote_ui.port),
                                ("Workspace (blank opens remote home)", &mut self.remote_ui.root),
                                ("Recovery directory (automatic if blank)", &mut self.remote_ui.state),
                                ("SSH config (blank uses default)", &mut self.remote_ui.config),
                            ] {
                                ui.label(label);
                                ui.add(egui::TextEdit::singleline(value).desired_width(280.0));
                                ui.end_row();
                            }
                        });
                        ui.add_enabled(cfg!(unix), egui::Checkbox::new(&mut self.remote_ui.routing.jump, "Use SSH config jump hosts"));
                        ui.add_enabled(cfg!(unix), egui::Checkbox::new(&mut self.remote_ui.routing.forwards, "Forward SSH config ports to localhost"));
                        ui.checkbox(&mut self.remote_ui.manual_helper,"Use a manually installed helper");
                        if self.remote_ui.manual_helper {
                            ui.label("Installed helper (absolute)");ui.text_edit_singleline(&mut self.remote_ui.helper);
                        } else {ui.label("The matching helper is installed automatically for your remote user.");}
                        ui.checkbox(&mut self.remote_ui.takeover, "Take over an existing writer (fences its edits)");
                        ui.label("SSH keys remain managed by OpenSSH and your agent.");
                        ui.horizontal(|ui| {
                            let ready = self.remote_ui.pending.is_none() && self.remote_ui.saving.is_none() && self.remote_ui.connecting.is_none();
                            connect = ui.add_enabled(ready, egui::Button::new("Open remote window")).clicked();
                            save = ui.add_enabled(ready && self.remote_ui.catalog.is_some(), egui::Button::new("Save workspace")).clicked();
                        });
                    }
                });
            self.remote_ui.dialog = open;
            if trust {
                if let Some(remote) = remote.clone() {
                    self.remote_background(ctx, move || remote.trust(true));
                }
                self.remote_ui.dialog = false;
            }
            if connect || save {
                match self.remote_ui.connection_config() {
                    Ok(config) => {
                        if connect && config.managed {
                            let original = self.remote_ui.editing.take();
                            self.launch_remote(ctx, config, original);
                            self.remote_ui.dialog = false;
                            return;
                        }
                        match self.remote_ui.updated_catalog(
                            config.clone(),
                            self.remote_ui.editing.as_ref(),
                            false,
                        ) {
                            Ok(Some(catalog)) => self.save_remote_catalog(ctx, catalog),
                            Ok(None) => (),
                            Err(error) => {
                                self.error = Some(error);
                                return;
                            }
                        }
                        if connect {
                            self.launch_remote(ctx, config, None);
                        }
                        self.remote_ui.dialog = false;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if let Some(browser) = self.remote_ui.folder_picker.as_mut() {
            let outcome = remote
                .as_ref()
                .map_or(folder_picker::Outcome::Closed, |remote| {
                    browser.show(ctx, remote)
                });
            match outcome {
                folder_picker::Outcome::Browsing => (),
                folder_picker::Outcome::Closed => {
                    self.remote_ui.folder_picker = None;
                    self.focus_editor = true;
                }
                folder_picker::Outcome::Opened(listing) => {
                    self.remote_ui.folder_picker = None;
                    self.sidebar = workbench_navigation::Sidebar::Files;
                    self.set_folder(listing);
                    self.focus_editor = true;
                }
            }
        }
        if let Some(picker) = self.remote_ui.picker {
            let mut open = true;
            let mut submit = false;
            egui::Window::new(match picker {
                Picker::Open => "Open remote file",
                Picker::Save(_) => "Save on SSH host",
            })
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| {
                ui.label("Path relative to the SSH workspace (Save As creates a new file):");
                let response = ui.text_edit_singleline(&mut self.remote_ui.path);
                submit = ui.button("Continue").clicked()
                    || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
            if !open {
                self.remote_ui.picker = None;
            }
            if submit {
                if let Some(remote) = remote {
                    match remote.path(&self.remote_ui.path) {
                        Ok(path) => {
                            self.remote_ui.picker = None;
                            match picker {
                                Picker::Open => self.open(ctx, Some(path)),
                                Picker::Save(id) => self.save_to_path(ctx, id, Some(path)),
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
        }
        schedule_backpressure_repaint(
            ctx,
            self.workspace
                .remote
                .as_deref()
                .and_then(hedit::remote::Connection::backpressure_repaint_after),
        );
    }
    /// Host identity and execution permissions remain visible in the menu bar,
    /// including distraction-free mode and when the status bar is hidden.
    pub(super) fn remote_menu(&mut self, ui: &mut egui::Ui) {
        let Some(remote) = self.workspace.remote.clone() else {
            return;
        };
        let (connected, trusted, pending, status) = remote.status();
        let state = if !connected {
            "disconnected".to_owned()
        } else if pending > 0 {
            format!("{pending} pending")
        } else if trusted {
            "trusted".into()
        } else {
            "restricted".into()
        };
        let color = if !connected || pending > 0 {
            self.theme.warning
        } else {
            self.theme.muted
        };
        ui.menu_button(
            RichText::new(format!("SSH · {} · {state}", remote.config.target.host))
                .size(11.0)
                .color(color.color()),
            |ui| {
                ui.strong(format!("SSH · {}", remote.config.target.host));
                ui.label(&remote.config.root);
                ui.label(&status);
                ui.label(if trusted {
                    "Execution trusted for this connection"
                } else {
                    "Execution restricted"
                });
                ui.separator();
                if !connected
                    && ui
                        .add_enabled(
                            self.remote_ui.pending.is_none(),
                            egui::Button::new("Reconnect"),
                        )
                        .clicked()
                {
                    let connection = remote.clone();
                    self.remote_background(ui.ctx(), move || connection.reconnect());
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        self.remote_ui.pending.is_none(),
                        egui::Button::new("Disconnect"),
                    )
                    .clicked()
                {
                    let connection = remote.clone();
                    self.remote_background(ui.ctx(), move || connection.disconnect());
                    ui.close_menu();
                }
                if ui.button("Remote Explorer").clicked() {
                    self.show_remote_explorer(ui.ctx());
                    ui.close_menu();
                }
                if connected
                    && ui
                        .add_enabled(
                            self.remote_ui.pending.is_none(),
                            egui::Button::new(if trusted {
                                "Revoke execution trust"
                            } else {
                                "Trust for Git, builds and terminal…"
                            }),
                        )
                        .clicked()
                {
                    if trusted {
                        let connection = remote.clone();
                        self.remote_background(ui.ctx(), move || connection.trust(false));
                    } else {
                        self.remote_ui.dialog = true;
                    }
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        connected && !self.busy,
                        egui::Button::new("New remote file"),
                    )
                    .clicked()
                {
                    self.remote_new(ui.ctx());
                    ui.close_menu();
                }
                if ui
                    .add_enabled(
                        connected && !self.busy,
                        egui::Button::new("Open remote path…"),
                    )
                    .clicked()
                {
                    self.remote_ui.picker = Some(Picker::Open);
                    self.remote_ui.path.clear();
                    ui.close_menu();
                }
            },
        )
        .response
        .on_hover_text(format!("{status}\nRecovery stays on the SSH host."));
    }
    fn remote_background(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        if self.remote_ui.pending.is_some() {
            return;
        }
        self.remote_ui.pending = Some(remote_worker(ctx, job));
    }
}

fn remote_worker<T: Send + 'static>(
    ctx: &egui::Context,
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> mpsc::Receiver<Result<T, String>> {
    struct RepaintOnExit(egui::Context);
    impl Drop for RepaintOnExit {
        fn drop(&mut self) {
            self.0.request_repaint();
        }
    }
    let (tx, rx) = mpsc::channel();
    let repaint = RepaintOnExit(ctx.clone());
    let _ = thread::Builder::new().spawn(move || {
        // Drop the sender before waking the UI, including on unwind.
        let _repaint = repaint;
        let sender = tx;
        let _ = sender.send(job());
    });
    rx
}

fn remote_worker_result<T>(
    pending: &mut Option<mpsc::Receiver<Result<T, String>>>,
    operation: &str,
) -> Option<Result<T, String>> {
    let result = match pending.as_ref()?.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return None,
        Err(mpsc::TryRecvError::Disconnected) => Err(format!("{operation} stopped. Try again.")),
    };
    *pending = None;
    Some(result)
}

fn ssh_setup_error(progress: &str, error: &str) -> String {
    let action = if error.contains("Host key verification failed")
        || error.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
    {
        " Verify the host key in OpenSSH, then retry."
    } else if error.contains("Permission denied (publickey") {
        " Check the SSH user and load your key into the SSH agent, then retry."
    } else {
        ""
    };
    let stage = progress.trim_end_matches('…').to_lowercase();
    format!("SSH setup failed while {stage}.{} {error}", action)
}

fn schedule_backpressure_repaint(ctx: &egui::Context, delay: Option<Duration>) {
    if let Some(delay) = delay {
        ctx.request_repaint_after(delay);
    }
}

fn wait_for_window(mut child: std::process::Child) -> Result<(), String> {
    use std::io::Read;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        if start.elapsed() > Duration::from_secs(40) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(
                "SSH window startup timed out. Check the host and helper configuration.".into(),
            );
        }
        thread::sleep(Duration::from_millis(50));
    };
    if status.success() {
        return Ok(());
    }
    let mut bytes = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        let _ = stderr.take(8192).read_to_end(&mut bytes);
    }
    Err(format!(
        "Cannot open SSH window: {}",
        String::from_utf8_lossy(&bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.remote_ui.catalog = Some(Catalog::default());
        app.remote_ui.hosts.aliases = vec!["devbox".into(), "cluster".into()];
        (dir, ctx, app)
    }
    fn frame(
        ctx: &egui::Context,
        app: &mut HeditApp,
        size: egui::Vec2,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |ctx| {
                app.remote_controls(ctx);
                app.shortcuts(ctx);
                app.chrome(ctx);
                app.editor(ctx);
                app.command_palette(ctx);
            },
        )
    }

    fn picker_frame(
        ctx: &egui::Context,
        app: &mut HeditApp,
        events: Vec<egui::Event>,
    ) -> Option<RemoteConfig> {
        let mut choice = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(720.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| choice = app.ssh_picker(ctx),
        );
        choice
    }

    #[test]
    fn palette_connect_command_opens_picker_and_escape_closes_it() {
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(720.0, 480.0);
        app.execute(&ctx, Command::Palette);
        frame(&ctx, &mut app, size, vec![]);
        frame(
            &ctx,
            &mut app,
            size,
            vec![
                egui::Event::Text("Connect via SSH".into()),
                crate::app::tests::key(Key::Enter, Modifiers::NONE),
            ],
        );
        assert!(!app.palette);
        assert!(app.remote_ui.overlay_open());
        assert!(app.remote_ui.connect_picker.is_some());
        frame(&ctx, &mut app, size, vec![]);
        frame(
            &ctx,
            &mut app,
            size,
            vec![crate::app::tests::key(Key::Escape, Modifiers::NONE)],
        );
        assert!(!app.remote_ui.overlay_open());
        assert!(app.remote_ui.connecting.is_none());
    }

    #[test]
    fn ssh_picker_distinguishes_host_home_from_saved_workspace() {
        let (dir, ctx, mut app) = app();
        let key = |key| crate::app::tests::key(key, Modifiers::NONE);
        let ssh_config = dir.path().join("custom-config");
        let mut workspace = ConnectChoice::Host("devbox").config(Some(ssh_config.clone()));
        workspace.root = "/projects/demo repo".into();
        workspace.target.state = "/private/saved-recovery".into();
        let catalog = app.remote_ui.catalog.as_mut().unwrap();
        catalog.config = Some(ssh_config.clone());
        catalog.remember(workspace.clone()).unwrap();

        for (query, use_arrow, saved) in [
            ("DEVBOX", false, false),
            ("devbox", true, true),
            ("demo repo", false, true),
        ] {
            app.show_ssh_picker(&ctx);
            picker_frame(&ctx, &mut app, vec![]);
            picker_frame(&ctx, &mut app, vec![egui::Event::Text(query.into())]);
            if use_arrow {
                picker_frame(&ctx, &mut app, vec![key(Key::ArrowDown)]);
            }
            let choice = picker_frame(&ctx, &mut app, vec![key(Key::Enter)]).unwrap();
            if saved {
                assert_eq!(choice, workspace);
            } else {
                assert_eq!(choice.target.host, "devbox");
                assert_eq!(choice.target.config.as_ref(), Some(&ssh_config));
                assert!(choice.managed && choice.root.is_empty() && choice.target.state.is_empty());
            }
        }
    }

    #[test]
    fn ssh_picker_validates_direct_user_and_host_without_launching() {
        let (_dir, ctx, mut app) = app();
        let key = |key| crate::app::tests::key(key, Modifiers::NONE);
        app.show_ssh_picker(&ctx);
        picker_frame(&ctx, &mut app, vec![]);
        picker_frame(&ctx, &mut app, vec![egui::Event::Text("-invalid".into())]);
        assert!(picker_frame(&ctx, &mut app, vec![key(Key::Enter)]).is_none());
        assert!(app
            .remote_ui
            .connect_picker
            .as_ref()
            .unwrap()
            .error
            .is_some());

        app.show_ssh_picker(&ctx);
        picker_frame(&ctx, &mut app, vec![]);
        picker_frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("editor@remote.example".into())],
        );
        let choice = picker_frame(&ctx, &mut app, vec![key(Key::Enter)]).unwrap();
        assert_eq!(choice.target.host, "remote.example");
        assert_eq!(choice.target.user.as_deref(), Some("editor"));
        assert_eq!(choice.target.port, 0);
        assert!(choice.target.config.is_none());
        assert!(choice.root.is_empty());
    }

    #[test]
    fn ssh_picker_defaults_to_the_typed_address_ahead_of_a_partial_alias() {
        let (_dir, ctx, mut app) = app();
        app.remote_ui.hosts.aliases = vec!["dev.example.net-staging".into()];
        let key = |key| crate::app::tests::key(key, Modifiers::NONE);
        for alternate in [false, true] {
            app.show_ssh_picker(&ctx);
            picker_frame(&ctx, &mut app, vec![]);
            picker_frame(
                &ctx,
                &mut app,
                vec![egui::Event::Text("dev.example.net".into())],
            );
            if alternate {
                picker_frame(&ctx, &mut app, vec![key(Key::ArrowDown)]);
            }
            let choice = picker_frame(&ctx, &mut app, vec![key(Key::Enter)]).unwrap();
            assert_eq!(
                choice.target.host,
                if alternate {
                    "dev.example.net-staging"
                } else {
                    "dev.example.net"
                }
            );
        }
    }

    #[test]
    fn managed_setup_replaces_an_edited_profile_only_on_success() {
        let (_dir, ctx, mut app) = app();
        app.settings_files.dir = None;
        let mut original = ConnectChoice::Host("devbox").config(None);
        original.root = "/original".into();
        original.target.state = "/private/original".into();
        app.remote_ui
            .catalog
            .as_mut()
            .unwrap()
            .remember(original.clone())
            .unwrap();
        let mut resolved = original.clone();
        resolved.root = "/renamed".into();
        resolved.target.state = "/private/renamed".into();

        // Keep the fixture from launching an actual SSH window after setup.
        let (_keep_pending, pending) = mpsc::channel();
        app.remote_ui.pending = Some(pending);
        for result in [
            Err("SSH setup failed in test.".into()),
            Ok(resolved.clone()),
        ] {
            let success = result.is_ok();
            let (tx, events) = mpsc::channel();
            tx.send(ConnectEvent::Prepared(result)).ok().unwrap();
            app.remote_ui.connecting = Some(Connecting {
                events,
                cancel: Arc::new(AtomicBool::new(false)),
                implicit_home: false,
                original_profile: Some(original.clone()),
            });
            app.remote_controls(&ctx);
            let expected = if success { &resolved } else { &original };
            assert_eq!(
                app.remote_ui
                    .catalog
                    .as_ref()
                    .unwrap()
                    .workspaces
                    .as_slice(),
                std::slice::from_ref(expected)
            );
        }
    }

    #[test]
    fn stopped_ssh_ui_workers_release_controls_and_keep_saved_profiles() {
        let (_dir, ctx, mut app) = app();
        fn abandoned<T>() -> mpsc::Receiver<T> {
            let (tx, rx) = mpsc::channel();
            drop(tx);
            rx
        }
        let original = app.remote_ui.catalog.clone().unwrap();
        app.remote_ui.pending = Some(abandoned());
        app.remote_ui.loading = Some(abandoned());
        app.remote_ui.saving = Some(abandoned());
        assert!(!app.remote_ui.ready());

        app.remote_controls(&ctx);
        assert!(app.remote_ui.ready());
        assert!(app.remote_ui.catalog.as_ref() == Some(&original));
        assert_eq!(
            app.error.as_deref(),
            Some("SSH workspace save stopped. Try again.")
        );
    }

    #[test]
    fn setup_failure_explains_host_key_and_agent_errors() {
        let stage = "Connecting to SSH host…";
        let host_key = ssh_setup_error(stage, "Host key verification failed.");
        assert!(host_key.contains("Verify the host key in OpenSSH"));
        let auth = ssh_setup_error(stage, "editor@devbox: Permission denied (publickey).");
        assert!(auth.contains("load your key into the SSH agent"));
        assert!(auth.contains("editor@devbox"));
        let other = ssh_setup_error("Installing helper…", "Unsafe directory permissions.");
        assert_eq!(
            other,
            "SSH setup failed while installing helper. Unsafe directory permissions."
        );
    }

    #[test]
    fn remote_explorer_is_available_from_the_sidebar_and_filters_hosts() {
        use crate::app::tests::text_rect;
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(720.0, 480.0);
        for _ in 0..3 {
            frame(&ctx, &mut app, size, vec![]);
        }
        let output = frame(&ctx, &mut app, size, vec![]);
        let rect = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "Remote Explorer"))
            .unwrap();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                size,
                vec![
                    egui::Event::PointerMoved(rect.center()),
                    egui::Event::PointerButton {
                        pos: rect.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert_eq!(app.sidebar, workbench_navigation::Sidebar::Remote);
        app.remote_ui.query = "dev".into();
        for _ in 0..3 {
            frame(&ctx, &mut app, size, vec![]);
        }
        let output = frame(&ctx, &mut app, size, vec![]);
        assert!(output.shapes.iter().any(
            |s| text_rect(&s.shape, "devbox  →").is_some_and(|r| s.clip_rect.contains_rect(r))
        ));
        assert!(!output
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "cluster  →").is_some()));
    }

    #[test]
    fn remote_explorer_can_import_selected_hosts_or_sync_all() {
        use crate::app::tests::text_rect;
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(880.0, 650.0);
        app.show_remote_explorer(&ctx);
        let click = |app: &mut HeditApp, label: &str| {
            frame(&ctx, app, size, vec![]);
            let output = frame(&ctx, app, size, vec![]);
            let position = output
                .shapes
                .iter()
                .find_map(|shape| text_rect(&shape.shape, label))
                .unwrap_or_else(|| panic!("missing control {label}"))
                .center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    app,
                    size,
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
        };
        click(&mut app, "Import SSH hosts…");
        click(&mut app, "Clear selection");
        click(&mut app, "devbox");
        click(&mut app, "Import selected");
        let imports = &app.remote_ui.catalog.as_ref().unwrap().imports;
        assert!(imports.includes("devbox"));
        assert!(!imports.includes("cluster"));
        let output = frame(&ctx, &mut app, size, vec![]);
        assert!(output
            .shapes
            .iter()
            .any(|shape| text_rect(&shape.shape, "devbox  →").is_some()));
        assert!(!output
            .shapes
            .iter()
            .any(|shape| text_rect(&shape.shape, "cluster  →").is_some()));
        assert!(!app.remote_ui.connect_choices("cluster").is_empty());

        click(&mut app, "Import SSH hosts…");
        click(&mut app, "Sync all hosts");
        assert!(app
            .remote_ui
            .catalog
            .as_ref()
            .unwrap()
            .imports
            .includes("future"));
    }

    #[test]
    fn ssh_imports_follow_config_replacements_and_new_include_files() {
        use hedit::remote::explorer::HostImports;
        use std::io::Write;

        let (dir, ctx, mut app) = app();
        let config = dir.path().join("ssh-config");
        let includes = dir.path().join("included");
        let pattern = includes.join("*.conf").to_string_lossy().replace('\\', "/");
        let configured = |hosts: &str| format!("Include \"{pattern}\"\n{hosts}");
        std::fs::write(&config, configured("Host devbox\n  User first\n")).unwrap();
        app.remote_ui.selected_config = config.to_string_lossy().into_owned();
        app.load_remote_catalog(&ctx, CatalogLoad::ApplyConfig);
        let wait = |app: &mut HeditApp, aliases: &[&str]| {
            let start = Instant::now();
            loop {
                app.remote_controls(&ctx);
                if app.remote_ui.loading.is_none() && app.remote_ui.hosts.aliases == aliases {
                    break;
                }
                assert!(
                    start.elapsed() < Duration::from_secs(15),
                    "expected {aliases:?}, found {:?}: {:?}",
                    app.remote_ui.hosts.aliases,
                    app.error,
                );
                thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&mut app, &["devbox"]);
        app.remote_ui
            .catalog
            .as_mut()
            .unwrap()
            .set_imports(HostImports::Selected(["devbox".into()].into()))
            .unwrap();

        app.remote_ui.selected_config = "unapplied config path".into();
        let mut replacement = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        replacement
            .write_all(configured("Host renamed\n  User second\n").as_bytes())
            .unwrap();
        replacement.persist(&config).unwrap();
        wait(&mut app, &["renamed"]);
        let imports = &app.remote_ui.catalog.as_ref().unwrap().imports;
        assert!(!imports.includes("renamed"));
        assert_eq!(app.remote_ui.selected_config, "unapplied config path");

        std::fs::create_dir(&includes).unwrap();
        let restored = includes.join("devbox.conf");
        std::fs::write(&restored, "Host DevBox\n  User current\n  Port 2222\n").unwrap();
        wait(&mut app, &["DevBox", "renamed"]);
        assert!(app
            .remote_ui
            .catalog
            .as_ref()
            .unwrap()
            .imports
            .includes("DevBox"));
        let target = ConnectChoice::Host("DevBox")
            .config(app.remote_ui.ssh_config())
            .target;
        assert_eq!(target.config.as_ref(), Some(&config));
        assert_eq!(target.user, None);
        assert_eq!(target.port, 0);

        app.remote_ui
            .catalog
            .as_mut()
            .unwrap()
            .set_imports(HostImports::All)
            .unwrap();
        std::fs::remove_file(restored).unwrap();
        std::fs::write(includes.join("new.conf"), "Host new-alias\n").unwrap();
        wait(&mut app, &["new-alias", "renamed"]);
        assert!(app
            .remote_ui
            .catalog
            .as_ref()
            .unwrap()
            .imports
            .includes("new-alias"));
    }

    #[test]
    fn activity_bar_switches_between_remote_and_files_without_changing_documents() {
        use hedit::icons::Icon;
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(720.0, 480.0);
        app.workspace.open[0].page.text = "Keep this draft".into();
        app.workspace.edit(0, "").unwrap();
        for icon in [Icon::Remote, Icon::Files, Icon::Remote] {
            for _ in 0..3 {
                frame(&ctx, &mut app, size, vec![]);
            }
            let output = frame(&ctx, &mut app, size, vec![]);
            let rect = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Mesh(mesh) if icon.matches_texture(&ctx, mesh.texture_id) => {
                        Some(mesh.calc_bounds())
                    }
                    _ => None,
                })
                .expect("visible activity icon");
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut app,
                    size,
                    vec![
                        egui::Event::PointerMoved(rect.center()),
                        egui::Event::PointerButton {
                            pos: rect.center(),
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(
                app.sidebar,
                if icon == Icon::Remote {
                    workbench_navigation::Sidebar::Remote
                } else {
                    workbench_navigation::Sidebar::Files
                }
            );
            assert_eq!(app.active().page.text, "Keep this draft");
        }
    }
    #[test]
    fn ssh_form_inherits_config_and_escape_closes_it() {
        let (dir, ctx, mut app) = app();
        let ssh_config = dir.path().join("config");
        app.remote_ui.edit(None, &ssh_config.to_string_lossy());
        app.remote_ui.host = "devbox".into();
        app.remote_ui.root = "/project".into();
        app.remote_ui.helper = "/opt/hedit-remote".into();
        app.remote_ui.state = "/private/recovery".into();
        let config = app.remote_ui.connection_config().unwrap();
        assert_eq!(config.target.port, 0);
        assert!(config.target.user.is_none());
        assert_eq!(config.target.config.unwrap(), ssh_config);
        frame(&ctx, &mut app, egui::vec2(720.0, 480.0), vec![]);
        frame(
            &ctx,
            &mut app,
            egui::vec2(720.0, 480.0),
            vec![crate::app::tests::key(Key::Escape, Modifiers::NONE)],
        );
        assert!(!app.remote_ui.dialog);
        app.remote_ui.port = "0".into();
        assert!(app.remote_ui.connection_config().is_err());
    }
    #[test]
    fn new_connections_need_only_a_host_and_manual_paths_are_explicit() {
        let (_dir, _ctx, mut app) = app();
        app.remote_ui.edit(None, "");
        app.remote_ui.host = "devbox".into();
        let config = app.remote_ui.connection_config().unwrap();
        assert!(config.managed && config.root.is_empty() && config.target.helper.is_empty());
        app.remote_ui.manual_helper = true;
        assert!(app.remote_ui.connection_config().is_err());
    }
    #[test]
    fn enabling_automatic_helpers_retains_existing_recovery_and_new_roots_get_new_storage() {
        let (_dir, _ctx, mut app) = app();
        let original = RemoteConfig {
            target: SshTarget {
                routing: Default::default(),
                host: "devbox".into(),
                user: None,
                port: 0,
                config: None,
                helper: "/old/helper".into(),
                state: "/private/existing-drafts".into(),
            },
            root: "/work/project".into(),
            managed: false,
            takeover: false,
        };
        app.remote_ui.edit(Some(&original), "");
        app.remote_ui.manual_helper = false;
        let config = app.remote_ui.connection_config().unwrap();
        assert!(config.managed);
        assert_eq!(config.target.state, original.target.state);
        app.remote_ui.root = "/work/other".into();
        assert!(app
            .remote_ui
            .connection_config()
            .unwrap()
            .target
            .state
            .is_empty());
        app.remote_ui.state = "/private/other-recovery".into();
        assert_eq!(
            app.remote_ui.connection_config().unwrap().target.state,
            "/private/other-recovery"
        );
    }
    #[test]
    fn host_connection_settings_are_on_the_context_menu_without_starting_ssh() {
        use crate::app::tests::text_rect;
        let (_dir, ctx, mut app) = app();
        let size = egui::vec2(720.0, 480.0);
        app.show_remote_explorer(&ctx);
        for _ in 0..4 {
            frame(&ctx, &mut app, size, vec![]);
        }
        let output = frame(&ctx, &mut app, size, vec![]);
        let row = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "devbox  →"))
            .unwrap();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                size,
                vec![
                    egui::Event::PointerMoved(row.center()),
                    egui::Event::PointerButton {
                        pos: row.center(),
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        let output = frame(&ctx, &mut app, size, vec![]);
        let settings = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "Connection settings…"))
            .unwrap();
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut app,
                size,
                vec![
                    egui::Event::PointerMoved(settings.center()),
                    egui::Event::PointerButton {
                        pos: settings.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(app.remote_ui.dialog);
        assert_eq!(app.remote_ui.host, "devbox");
        assert!(!app.remote_ui.manual_helper);
        assert!(app.remote_ui.connecting.is_none());
    }
    #[test]
    #[ignore = "writes Remote Explorer PNG previews to HEDIT_PREVIEW_DIR"]
    fn export_remote_explorer() {
        let path =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&path).unwrap();
        for (name, size) in [
            ("small", egui::vec2(720.0, 480.0)),
            ("large", egui::vec2(1280.0, 820.0)),
        ] {
            let (_dir, ctx, mut app) = app();
            app.show_remote_explorer(&ctx);
            for dialog in [false, true] {
                if dialog {
                    app.remote_ui.edit(None, "");
                    app.remote_ui.host = "devbox".into();
                }
                for _ in 0..4 {
                    frame(&ctx, &mut app, size, vec![]);
                }
                let output = frame(&ctx, &mut app, size, vec![]);
                let svg = crate::preview::export(
                    &output.shapes,
                    [size.x, size.y],
                    app.theme.background.color(),
                );
                let mut fonts = resvg::usvg::fontdb::Database::new();
                fonts.load_system_fonts();
                let options = resvg::usvg::Options {
                    fontdb: Arc::new(fonts),
                    ..Default::default()
                };
                let tree = resvg::usvg::Tree::from_data(svg.as_bytes(), &options).unwrap();
                let mut pixels =
                    resvg::tiny_skia::Pixmap::new(size.x as u32, size.y as u32).unwrap();
                resvg::render(
                    &tree,
                    resvg::tiny_skia::Transform::identity(),
                    &mut pixels.as_mut(),
                );
                pixels
                    .save_png(path.join(format!(
                        "remote-{name}-{}.png",
                        if dialog { "dialog" } else { "explorer" }
                    )))
                    .unwrap();
            }
        }
    }

    #[test]
    fn ssh_receipt_ui_schedules_a_deadline_but_no_offline_poll() {
        let ctx = egui::Context::default();
        let frame = |delay| {
            ctx.run(
                egui::RawInput {
                    predicted_dt: 0.0,
                    ..Default::default()
                },
                |ctx| schedule_backpressure_repaint(ctx, delay),
            )
            .viewport_output[&egui::ViewportId::ROOT]
                .repaint_delay
        };
        for _ in 0..2 {
            frame(None);
        }
        let deadline = Duration::from_millis(1250);
        assert_eq!(frame(Some(deadline)), deadline);
        assert_eq!(frame(None), Duration::MAX);
    }

    #[test]
    #[ignore = "requires the disposable SSH fixture from make test-remote-live"]
    fn checkpoint_defers_remote_view_while_a_document_job_is_busy() {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::env::var("HEDIT_REMOTE_TEST_CONFIG").expect("missing disposable SSH fixture"),
        )
        .unwrap();
        let config = hedit::remote::Config {
            target: hedit_remote::client::SshTarget {
                routing: Default::default(),
                host: "lab".into(),
                user: None,
                port: fixture["port"].as_u64().unwrap() as u16,
                config: Some(fixture["config"].as_str().unwrap().into()),
                helper: fixture["helper"].as_str().unwrap().into(),
                state: fixture["state"].as_str().unwrap().into(),
            },
            root: fixture["root"].as_str().unwrap().into(),
            managed: false,
            takeover: false,
        };
        let directory = tempfile::tempdir().unwrap();
        let (workspace, errors) =
            Workspace::connect_remote(directory.path().join("client"), config).unwrap();
        assert!(errors.is_empty());
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        let document = app.workspace.editor.get(app.active().view.id).unwrap();
        let (ready, started) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _document = document.lock().unwrap();
            ready.send(()).unwrap();
            // A bounded hold makes the unfixed UI block observable without
            // leaving a deadlocked test worker behind after failure.
            let _ = wait.recv_timeout(Duration::from_secs(2));
        });
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        app.busy = true;
        let before = Instant::now();
        app.checkpoint(&ctx);
        let elapsed = before.elapsed();
        let _ = release.send(());
        worker.join().unwrap();
        assert!(
            elapsed < Duration::from_millis(500),
            "remote checkpoint blocked the UI for {elapsed:?} while a document job held its lock"
        );
        assert!(!app.checkpoint_pending);
        app.busy = false;
        app.checkpoint(&ctx);
        let before = Instant::now();
        while app.checkpoint_pending {
            assert!(before.elapsed() < Duration::from_secs(10));
            app.receive(&ctx);
            thread::sleep(Duration::from_millis(5));
        }
        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(!directory.path().join("client").exists());
    }
}
