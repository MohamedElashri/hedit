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
        let remote = self.workspace.remote.clone();
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
                    let exe = hedit::windows::executable().map_err(|e| e.to_string())?;
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
