//! SSH controls stay in the remote window; connection settings contain no secrets.
use super::*;
#[derive(Default)]
pub(super) struct RemoteUi {
    pub dialog: bool,
    host: String,
    user: String,
    port: String,
    root: String,
    helper: String,
    state: String,
    config: String,
    takeover: bool,
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
impl HeditApp {
    pub(super) fn remote_new(&mut self, ctx: &egui::Context) {
        if let Some(remote) = self.workspace.remote.clone() {
            let id = self.workspace.editor.id();
            self.job(ctx, move || {
                let mut doc = remote.scratch(id)?;
                let page = doc.page(0)?;
                Ok(Action::Add(doc, page, 0))
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
        let remote = self.workspace.remote.clone();
        egui::TopBottomPanel::top("ssh-workspace").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if let Some(remote) = &remote {
                    let (connected, trusted, pending, status) = remote.status();
                    ui.strong(format!(
                        "SSH · {} · {}",
                        remote.config.target.host, remote.config.root
                    ));
                    ui.label(format!("{status} · {pending} pending"));
                    if !connected
                        && ui
                            .add_enabled(
                                self.remote_ui.pending.is_none(),
                                egui::Button::new("Reconnect"),
                            )
                            .clicked()
                    {
                        let remote = remote.clone();
                        self.remote_background(ctx, move || remote.reconnect());
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
                            let remote = remote.clone();
                            self.remote_background(ctx, move || remote.trust(false));
                        } else {
                            self.remote_ui.dialog = true;
                        }
                    }
                    if ui.button("New remote file").clicked() {
                        self.remote_new(ctx);
                    }
                    if ui.button("Open remote path…").clicked() {
                        self.remote_ui.picker = Some(Picker::Open);
                        self.remote_ui.path.clear();
                    }
                } else if ui.button("Connect via SSH…").clicked() {
                    self.remote_ui.dialog = true;
                }
            });
        });
        if self.remote_ui.dialog {
            let mut open = true;
            let mut connect = false;
            let mut trust = false;
            egui::Window::new(if remote.is_some(){"Trust SSH workspace"}else{"Connect via SSH"}).open(&mut open).collapsible(false).resizable(false).show(ctx,|ui|{
                if remote.is_some(){ui.set_max_width(520.0);ui.label("Git, builds, and terminals execute code with your SSH account’s permissions. Repository configuration, hooks, filters, and build scripts may run programs. Trust only a workspace and host you control.");trust=ui.button("Trust this connection for execution").clicked();}
                else {ui.label("Direct SSH · pinned host keys · recovery stays on the host");egui::Grid::new("ssh-fields").num_columns(2).show(ui,|ui|{
                    for (label,value) in [("Host / SSH alias",&mut self.remote_ui.host),("User (optional)",&mut self.remote_ui.user),("Port (default 22)",&mut self.remote_ui.port),("Workspace (absolute)",&mut self.remote_ui.root),("Installed helper (absolute)",&mut self.remote_ui.helper),("Private state directory (absolute)",&mut self.remote_ui.state),("Trusted SSH config (optional)",&mut self.remote_ui.config)]{ui.label(label);ui.text_edit_singleline(value);ui.end_row();}
                });ui.checkbox(&mut self.remote_ui.takeover,"Take over an existing writer (fences its edits)");ui.label("Install the matching hedit-remote helper and verify the host key with OpenSSH first. SSH keys remain managed by OpenSSH and your agent.");connect=ui.button("Open remote window").clicked();}
            });
            self.remote_ui.dialog = open;
            if trust {
                if let Some(remote) = remote.clone() {
                    self.remote_background(ctx, move || remote.trust(true));
                }
                self.remote_ui.dialog = false;
            }
            if connect {
                let result = (|| {
                    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                    let port = if self.remote_ui.port.is_empty() {
                        "22"
                    } else {
                        &self.remote_ui.port
                    };
                    port.parse::<u16>().map_err(|_| "Invalid SSH port.")?;
                    let mut command = std::process::Command::new(exe);
                    command.args([
                        "--ssh",
                        &self.remote_ui.host,
                        "--ssh-root",
                        &self.remote_ui.root,
                        "--ssh-helper",
                        &self.remote_ui.helper,
                        "--ssh-state",
                        &self.remote_ui.state,
                        "--ssh-port",
                        port,
                    ]);
                    if !self.remote_ui.user.is_empty() {
                        command.args(["--ssh-user", &self.remote_ui.user]);
                    }
                    if !self.remote_ui.config.is_empty() {
                        command.args(["--ssh-config", &self.remote_ui.config]);
                    }
                    if self.remote_ui.takeover {
                        command.arg("--ssh-takeover");
                    }
                    command
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::piped());
                    command.spawn().map_err(|e| e.to_string())
                })();
                match result {
                    Ok(child) => self.remote_background(ctx, move || wait_for_window(child)),
                    Err(error) => self.error = Some(error),
                }
                self.remote_ui.dialog = false;
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
        if self.workspace.remote.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn remote_background(
        &mut self,
        ctx: &egui::Context,
        job: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
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
