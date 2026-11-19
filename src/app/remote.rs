//! SSH controls stay in the remote window; connection settings contain no secrets.
use super::*;
use hedit::remote::{explorer::Catalog, Config as RemoteConfig};
use hedit_remote::{client::SshTarget, ssh_config};
#[derive(Default)]
pub(super) struct RemoteUi {
    pub dialog: bool,
    selected_config: String,
    load_started: bool,
    query: String,
    catalog: Option<Catalog>,
    hosts: ssh_config::Hosts,
    loading: Option<mpsc::Receiver<Result<(Catalog, ssh_config::Hosts), String>>>,
    saving: Option<mpsc::Receiver<Result<Catalog, String>>>,
    host: String,
    user: String,
    port: String,
    root: String,
    helper: String,
    state: String,
    config: String,
    takeover: bool,
    editing: Option<RemoteConfig>,
    pub picker: Option<Picker>,
    pub path: String,
    pending: Option<mpsc::Receiver<Result<(), String>>>,
}
#[derive(Clone, Copy)]
pub(super) enum Picker {
    Open,
    Folder,
    Save(u64),
}
impl RemoteUi {
    fn edit(&mut self, config: Option<&RemoteConfig>, default_config: &str) {
        self.editing = config.cloned();
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
                host: self.host.trim().into(),
                user: (!self.user.trim().is_empty()).then(|| self.user.trim().into()),
                port,
                config: (!self.config.trim().is_empty()).then(|| self.config.trim().into()),
                helper: self.helper.trim().into(),
                state: self.state.trim().into(),
            },
            root: self.root.trim().into(),
            takeover: self.takeover,
        };
        let mut validate = Catalog::default();
        validate.remember(config.clone())?;
        Ok(config)
    }
}
impl HeditApp {
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
            self.load_remote_catalog(ctx, false);
        }
    }
    fn load_remote_catalog(&mut self, ctx: &egui::Context, refresh: bool) {
        if self.remote_ui.loading.is_some() || self.remote_ui.saving.is_some() {
            return;
        }
        self.remote_ui.load_started = true;
        let path = self
            .settings_files
            .dir
            .as_ref()
            .map(|p| p.join("ssh-workspaces.json"));
        let selected = self.remote_ui.selected_config.trim().to_owned();
        if !selected.is_empty() && !std::path::Path::new(&selected).is_absolute() {
            self.error = Some("Use an absolute local SSH config path.".into());
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.remote_ui.loading = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = (|| {
                let mut catalog = path
                    .as_deref()
                    .map(Catalog::load)
                    .transpose()?
                    .unwrap_or_default();
                if refresh {
                    let original = catalog.clone();
                    catalog.config = (!selected.is_empty()).then(|| selected.into());
                    if let Some(path) = &path {
                        catalog.save(path, &original)?;
                    }
                }
                let home = directories::BaseDirs::new()
                    .map(|d| d.home_dir().to_owned())
                    .ok_or("Cannot find your home directory.")?;
                let hosts = ssh_config::discover(catalog.config.as_deref(), &home);
                Ok((catalog, hosts))
            })();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
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
        let (tx, rx) = mpsc::channel();
        self.remote_ui.saving = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = catalog.save(&path, &expected).map(|()| catalog);
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    fn launch_remote(&mut self, ctx: &egui::Context, config: RemoteConfig) {
        if self.remote_ui.pending.is_some() {
            return;
        }
        self.remote_background(ctx, move || {
            config
                .target
                .command()
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
        ui.strong("SSH REMOTES");
        let ready = self.remote_ui.loading.is_none() && self.remote_ui.saving.is_none();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready && self.workspace.remote.is_none(),
                    egui::Button::new("+ Connect…"),
                )
                .clicked()
            {
                self.remote_ui
                    .edit(None, &self.remote_ui.selected_config.clone());
            }
            if ui
                .add_enabled(ready, egui::Button::new("Refresh"))
                .clicked()
            {
                self.load_remote_catalog(ui.ctx(), true);
            }
        });
        ui.label("SSH configuration");
        ui.add(
            egui::TextEdit::singleline(&mut self.remote_ui.selected_config)
                .desired_width(f32::INFINITY)
                .hint_text("~/.ssh/config (default)"),
        );
        if ui.button("Open SSH config…").clicked() {
            let path = if self.remote_ui.selected_config.trim().is_empty() {
                directories::BaseDirs::new().map(|d| d.home_dir().join(".ssh/config"))
            } else {
                Some(PathBuf::from(self.remote_ui.selected_config.trim()))
            };
            if let Some(path) = path {
                if !path.is_absolute() {
                    self.error = Some("Use an absolute local SSH config path.".into());
                } else if self.workspace.remote.is_none() {
                    self.open(ui.ctx(), Some(path));
                } else {
                    self.error =
                        Some("Open the local SSH config from a local editor window.".into());
                }
            }
        }
        ui.label(
            RichText::new(
                "Edit hosts in this file, then Refresh. Includes are discovered automatically.",
            )
            .small()
            .color(self.theme.muted.color()),
        );
        if self.remote_ui.loading.is_some() {
            ui.spinner();
            ui.label("Reading SSH configuration…");
        }
        if self.remote_ui.pending.is_some() {
            ui.spinner();
            ui.label("Connecting / updating SSH session…");
        }
        if self.remote_ui.saving.is_some() {
            ui.label("Saving workspace…");
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
                        ready && self.remote_ui.pending.is_none(),
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
        for host in &self.remote_ui.hosts.aliases {
            if !host.to_lowercase().contains(&query) {
                continue;
            }
            if ui
                .add_enabled(
                    ready && self.workspace.remote.is_none(),
                    egui::Button::new(format!("{host}  →")).frame(false),
                )
                .on_hover_text("Configure and open a workspace on this host")
                .clicked()
            {
                edit = Some(RemoteConfig {
                    target: SshTarget {
                        host: host.clone(),
                        user: None,
                        port: 0,
                        config: self
                            .remote_ui
                            .catalog
                            .as_ref()
                            .and_then(|c| c.config.clone()),
                        helper: String::new(),
                        state: String::new(),
                    },
                    root: String::new(),
                    takeover: false,
                });
            }
        }
        if self.remote_ui.hosts.aliases.is_empty() && self.remote_ui.loading.is_none() {
            ui.label("No SSH aliases found. Add a Host entry to your SSH config or connect to a hostname.");
        }
        if let Some(config) = edit {
            self.remote_ui.edit(Some(&config), "");
        }
        if let Some(config) = launch {
            self.launch_remote(ui.ctx(), config);
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
        if let Some(rx) = &self.remote_ui.pending {
            if let Ok(result) = rx.try_recv() {
                self.remote_ui.pending = None;
                if let Err(e) = result {
                    self.error = Some(e);
                }
                ctx.request_repaint();
            }
        }
        if let Some(rx) = &self.remote_ui.loading {
            if let Ok(result) = rx.try_recv() {
                self.remote_ui.loading = None;
                match result {
                    Ok((catalog, hosts)) => {
                        self.remote_ui.selected_config = catalog
                            .config
                            .as_ref()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.remote_ui.catalog = Some(catalog);
                        self.remote_ui.hosts = hosts;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if let Some(rx) = &self.remote_ui.saving {
            if let Ok(result) = rx.try_recv() {
                self.remote_ui.saving = None;
                match result {
                    Ok(catalog) => self.remote_ui.catalog = Some(catalog),
                    Err(error) => self.error = Some(error),
                }
            }
        }
        let remote = self.workspace.remote.clone();
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
                        ui.label("Direct SSH · pinned host keys · recovery stays on the host");
                        egui::Grid::new("ssh-fields").num_columns(2).show(ui, |ui| {
                            for (label, value) in [
                                ("Host / SSH alias", &mut self.remote_ui.host),
                                ("User (from SSH config)", &mut self.remote_ui.user),
                                ("Port (from SSH config)", &mut self.remote_ui.port),
                                ("Workspace (absolute)", &mut self.remote_ui.root),
                                ("Installed helper (absolute)", &mut self.remote_ui.helper),
                                ("Private state directory (absolute)", &mut self.remote_ui.state),
                                ("SSH config (blank uses default)", &mut self.remote_ui.config),
                            ] {
                                ui.label(label);
                                ui.add(egui::TextEdit::singleline(value).desired_width(280.0));
                                ui.end_row();
                            }
                        });
                        ui.checkbox(&mut self.remote_ui.takeover, "Take over an existing writer (fences its edits)");
                        ui.label("Install the matching hedit-remote helper and verify the host key with OpenSSH first. SSH keys remain managed by OpenSSH and your agent.");
                        ui.horizontal(|ui| {
                            let ready = self.remote_ui.pending.is_none() && self.remote_ui.saving.is_none();
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
                        if let Some(mut catalog) = self.remote_ui.catalog.clone() {
                            if let Some(original) = &self.remote_ui.editing {
                                catalog.workspaces.retain(|c| c != original);
                            }
                            match catalog.remember(config.clone()) {
                                Ok(()) => self.save_remote_catalog(ctx, catalog),
                                Err(error) => {
                                    self.error = Some(error);
                                    return;
                                }
                            }
                        }
                        if connect {
                            self.launch_remote(ctx, config);
                        }
                        self.remote_ui.dialog = false;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
        }
        if let Some(picker) = self.remote_ui.picker {
            let mut open = true;
            let mut submit = false;
            egui::Window::new(match picker {
                Picker::Open => "Open remote file",
                Picker::Folder => "Open remote folder",
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
                                Picker::Folder => self.job(ctx, move || {
                                    hedit::folder::read_directory(path).map(Action::Folder)
                                }),
                                Picker::Save(id) => self.save_to_path(ctx, id, Some(path)),
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
        }
        if self
            .workspace
            .remote
            .as_ref()
            .is_some_and(|r| r.status().2 > 0)
        {
            // Keep the backpressure deadline visible even if a receipt stalls.
            ctx.request_repaint_after(Duration::from_millis(100));
        }
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
        let (tx, rx) = mpsc::channel();
        self.remote_ui.pending = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(job());
            ctx.request_repaint();
        });
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
            },
        )
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
    #[ignore = "requires the disposable SSH fixture from make test-remote-live"]
    fn checkpoint_defers_remote_view_while_a_document_job_is_busy() {
        let fixture: serde_json::Value = serde_json::from_str(
            &std::env::var("HEDIT_REMOTE_TEST_CONFIG").expect("missing disposable SSH fixture"),
        )
        .unwrap();
        let config = hedit::remote::Config {
            target: hedit_remote::client::SshTarget {
                host: "lab".into(),
                user: None,
                port: fixture["port"].as_u64().unwrap() as u16,
                config: Some(fixture["config"].as_str().unwrap().into()),
                helper: fixture["helper"].as_str().unwrap().into(),
                state: fixture["state"].as_str().unwrap().into(),
            },
            root: fixture["root"].as_str().unwrap().into(),
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
