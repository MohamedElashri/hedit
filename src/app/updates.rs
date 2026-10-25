use super::*;
use hedit::update::{Client, Installation, Release};

#[derive(Default)]
pub(super) struct Updates {
    pub(super) open: bool,
    release: Option<Release>,
    installation: Option<Installation>,
    location_error: Option<String>,
    error: Option<String>,
    message: String,
    events: Option<Receiver<Event>>,
    installing: bool,
    installed: Option<String>,
}
type CheckResult = Result<(Release, Result<Installation, String>), String>;
enum Event {
    Progress(&'static str),
    Checked(Box<CheckResult>),
    Installed(Result<String, String>),
}
impl Updates {
    pub(super) fn installing(&self) -> bool {
        self.installing
    }
    pub(super) fn active(&self) -> bool {
        self.events.is_some()
    }
    fn check(&mut self, ctx: &egui::Context) {
        if self.active() || self.installed.is_some() {
            return;
        }
        self.error = None;
        self.release = None;
        self.installation = None;
        self.location_error = None;
        self.message = "Checking for updates…".into();
        let client = Client::default();
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = client
                .check()
                .map(|release| (release, Installation::detect()));
            let _ = tx.send(Event::Checked(Box::new(result)));
            ctx.request_repaint();
        });
    }
    fn install(&mut self, ctx: &egui::Context) {
        if self.active() || self.installed.is_some() {
            return;
        }
        let (Some(release), Some(installation)) = (self.release.clone(), self.installation.clone())
        else {
            return;
        };
        if !release
            .newer_than(env!("CARGO_PKG_VERSION"))
            .unwrap_or(false)
        {
            return;
        }
        self.error = None;
        self.installing = true;
        self.message = "Preparing download…".into();
        let client = Client::default();
        let (tx, rx) = mpsc::channel();
        self.events = Some(rx);
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = client
                .install(&release, &installation, |message| {
                    let _ = tx.send(Event::Progress(message));
                    ctx.request_repaint();
                })
                .map(|()| release.version.to_string());
            let _ = tx.send(Event::Installed(result));
            ctx.request_repaint();
        });
    }
    pub(super) fn poll(&mut self) {
        loop {
            let Some(receiver) = self.events.as_ref() else {
                return;
            };
            let event = match receiver.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("The update worker stopped unexpectedly. Check the installation before trying again.".into());
                    self.events = None;
                    self.installing = false;
                    return;
                }
            };
            match event {
                Event::Progress(message) => self.message = message.into(),
                Event::Checked(result) => {
                    self.events = None;
                    match *result {
                        Ok((release, installation)) => {
                            self.message = match release.newer_than(env!("CARGO_PKG_VERSION")) {
                                Ok(true) => format!("hedit {} is available.", release.version),
                                _ => "No newer stable release is available.".into(),
                            };
                            self.release = Some(release);
                            match installation {
                                Ok(location) => self.installation = Some(location),
                                Err(e) => self.location_error = Some(e),
                            }
                        }
                        Err(error) => {
                            self.message = "Update check failed.".into();
                            self.error = Some(error);
                        }
                    }
                    return;
                }
                Event::Installed(result) => {
                    self.events = None;
                    self.installing = false;
                    match result {
                        Ok(version) => {
                            self.message = format!("hedit {version} is installed.");
                            self.installed = Some(version);
                        }
                        Err(error) => {
                            self.message = "Update was not installed.".into();
                            self.error = Some(error);
                        }
                    }
                    return;
                }
            }
        }
    }
}
impl HeditApp {
    pub(super) fn open_updates(&mut self, ctx: &egui::Context) {
        self.updates.open = true;
        self.preferences_open = false;
        self.about_open = false;
        self.focus_editor = false;
        self.updates.check(ctx);
    }
    pub(super) fn updates_window(&mut self, ctx: &egui::Context) {
        if !self.updates.open {
            return;
        }
        let active = self.updates.active();
        let installed = self.updates.installed.is_some();
        let newer = self
            .updates
            .release
            .as_ref()
            .is_some_and(|r| r.newer_than(env!("CARGO_PKG_VERSION")).unwrap_or(false));
        let current = format!("Current version {}", env!("CARGO_PKG_VERSION"));
        let (title, subtitle) = if let Some(version) = &self.updates.installed {
            ("Update installed", format!("Version {version}"))
        } else if self.updates.installing {
            ("Installing update", current)
        } else if active {
            ("Checking for updates", current)
        } else if self.updates.error.is_some() {
            (
                if self.updates.release.is_some() {
                    "Update failed"
                } else {
                    "Update check failed"
                },
                current,
            )
        } else if newer {
            let version = &self.updates.release.as_ref().unwrap().version;
            (
                "Update available",
                format!("Version {version} · Current {}", env!("CARGO_PKG_VERSION")),
            )
        } else if self.updates.release.is_some() {
            ("Up to date", current)
        } else {
            ("Software updates", current)
        };
        let mut open = true;
        let mut close = false;
        let mut check = false;
        let mut install = false;
        let mut quit = false;
        egui::Window::new("Software updates")
            .id(egui::Id::new("updates"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .inner_margin(12.0)
                    .corner_radius(10),
            )
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                controls::info_header(ui, &self.theme, title, &subtitle, active);
                ui.add_space(12.0);
                if self.updates.installing {
                    ui.add(egui::Label::new(&self.updates.message).wrap());
                    ui.weak("You can keep editing while this finishes.");
                } else if installed {
                    ui.label("Reopen hedit to use the new version.");
                } else if newer && self.updates.installation.is_none() {
                    ui.weak("Manual installation required.");
                }
                // Keep long notes and diagnostic paths out of the default view.
                egui::ScrollArea::vertical()
                    .id_salt("update-details")
                    .max_height((ctx.screen_rect().height() - 310.0).clamp(60.0, 180.0))
                    .show(ui, |ui| {
                        if !installed && !active {
                            if let Some(release) = &self.updates.release {
                                if !release.notes.is_empty() {
                                    egui::CollapsingHeader::new("Release notes")
                                        .default_open(false)
                                        .show(ui, |ui| {
                                            ui.add(
                                                egui::Label::new(&release.notes)
                                                    .wrap()
                                                    .selectable(true),
                                            );
                                        });
                                }
                            }
                        }
                        if !installed
                            && (self.updates.error.is_some()
                                || newer
                                    && (self.updates.location_error.is_some()
                                        || self.updates.installation.is_some()))
                        {
                            egui::CollapsingHeader::new("Details")
                                .default_open(false)
                                .show(ui, |ui| {
                                    if let Some(error) = &self.updates.error {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(error)
                                                    .color(self.theme.error.color()),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    }
                                    if let Some(error) = &self.updates.location_error {
                                        ui.add(egui::Label::new(error).wrap().selectable(true));
                                    }
                                    if let Some(installation) = &self.updates.installation {
                                        ui.weak("Install location");
                                        ui.add(
                                            egui::Label::new(
                                                installation.location().display().to_string(),
                                            )
                                            .wrap()
                                            .selectable(true),
                                        );
                                    }
                                });
                        }
                    });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.hyperlink_to("All releases", hedit::update::RELEASES_URL);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if installed {
                            quit = controls::primary_button(
                                ui,
                                &self.theme,
                                "Quit hedit",
                                !self.busy && !self.closing,
                            )
                            .clicked();
                        } else if newer && !active {
                            if self.updates.installation.is_some() {
                                install = controls::primary_button(
                                    ui,
                                    &self.theme,
                                    "Install update",
                                    true,
                                )
                                .clicked();
                            } else if controls::primary_button(
                                ui,
                                &self.theme,
                                "Download update",
                                true,
                            )
                            .clicked()
                            {
                                ctx.open_url(egui::OpenUrl::new_tab(
                                    self.updates.release.as_ref().unwrap().url(),
                                ));
                            }
                        } else if !active {
                            check = controls::primary_button(ui, &self.theme, "Check again", true)
                                .clicked();
                        }
                        close = ui
                            .add_sized(
                                [64.0, 28.0],
                                egui::Button::new(if installed { "Later" } else { "Close" }),
                            )
                            .clicked();
                    });
                });
            });
        self.updates.open = open && !close;
        if !self.updates.open {
            self.focus_editor = true;
        }
        if check {
            self.updates.check(ctx);
        }
        if install {
            self.updates.install(ctx);
        }
        if quit {
            self.execute(ctx, Command::Quit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release() -> Release {
        Release::from_json(&serde_json::json!({
            "tag_name":"v9.4.0", "body":"Release notes.\n".repeat(200), "draft":false, "prerelease":false,
            "assets":[{"id":1,"name":format!("hedit-9.4.0-linux-{}.tar.gz",std::env::consts::ARCH),"size":200},
                {"id":2,"name":format!("hedit-9.4.0-linux-{}.tar.gz.sha256",std::env::consts::ARCH),"size":100}]
        }).to_string(), std::env::consts::ARCH).unwrap()
    }
    #[test]
    fn check_results_and_redrawing_never_start_an_install() {
        let mut updates = Updates::default();
        assert!(!updates.active());
        assert!(!updates.installing());
        let (tx, rx) = mpsc::channel();
        updates.events = Some(rx);
        tx.send(Event::Checked(Box::new(Ok((
            release(),
            Err("source build".into()),
        )))))
        .unwrap();
        updates.poll();
        assert!(!updates.active());
        assert!(!updates.installing());
        assert!(updates.installed.is_none());
        assert!(updates.release.is_some());
        assert!(updates.location_error.is_some());
    }
    #[test]
    fn quitting_waits_for_installation_worker_even_with_dialog_closed() {
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.closing = true;
        app.persisted = app.generation;
        app.updates.open = false;
        app.updates.installing = true;
        let (tx, rx) = mpsc::channel();
        app.updates.events = Some(rx);
        app.receive(&ctx);
        assert!(!app.exit_ready);
        tx.send(Event::Installed(Ok("9.4.0".into()))).unwrap();
        app.updates.poll();
        app.receive(&ctx);
        assert!(app.exit_ready);
        assert!(!app.updates.installing());
    }
    #[test]
    fn compact_update_dialog_keeps_actions_visible_and_does_not_check_on_render() {
        fn find(shape: &egui::Shape, text: &str) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Text(t) if t.galley.job.text == text => {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| find(s, text)),
                _ => None,
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.updates.open = true;
        app.updates.release = Some(release());
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 480.0));
        for preset in hedit::theme::builtins() {
            app.set_theme(&ctx, preset.clone());
            let mut output = None;
            for _ in 0..4 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        ..Default::default()
                    },
                    |ctx| app.updates_window(ctx),
                ));
            }
            let rect = output
                .unwrap()
                .shapes
                .iter()
                .find_map(|s| find(&s.shape, "Download update"))
                .unwrap();
            assert!(screen.contains_rect(rect));
            assert!(!app.updates.active());
            assert!(!app.updates.installing());
        }
    }
    #[test]
    #[ignore = "writes update dialog SVG previews to HEDIT_PREVIEW_DIR"]
    fn export_update_previews() {
        let output =
            PathBuf::from(std::env::var_os("HEDIT_PREVIEW_DIR").expect("set HEDIT_PREVIEW_DIR"));
        std::fs::create_dir_all(&output).unwrap();
        for (preset, name) in [(Theme::default(), "ink"), (Theme::paper(), "paper")] {
            let directory = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            app.set_theme(&ctx, preset.clone());
            app.updates.open = true;
            app.updates.release = Some(release());
            app.updates.message = "hedit 9.4.0 is available.".into();
            app.updates.location_error =
                Some("Source build: install a release package to enable updates in place.".into());
            for (state, size) in [
                ("available", [720.0, 480.0]),
                ("installed", [720.0, 480.0]),
                ("current", [720.0, 480.0]),
                ("error", [720.0, 480.0]),
                ("checking", [720.0, 480.0]),
                ("installing", [720.0, 480.0]),
                ("about", [720.0, 480.0]),
            ] {
                app.about_open = state == "about";
                app.updates.open = state != "about";
                let mut latest = release();
                if state == "current" {
                    latest.version = env!("CARGO_PKG_VERSION").parse().unwrap();
                }
                app.updates.release = if matches!(state, "error" | "checking") {
                    None
                } else {
                    Some(latest)
                };
                app.updates.error = (state == "error").then(|| {
                    "Could not connect. Check your internet connection and try again.".into()
                });
                let (_progress, events) = mpsc::channel();
                app.updates.events = matches!(state, "checking" | "installing").then_some(events);
                app.updates.installing = state == "installing";
                app.updates.message = if state == "installing" {
                    "Downloading update…".into()
                } else {
                    "Checking for updates…".into()
                };
                app.updates.installed = if state == "installed" {
                    Some("9.4.0".into())
                } else {
                    None
                };
                let mut last = None;
                for _ in 0..4 {
                    last = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(size[0], size[1]),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            app.chrome(ctx);
                            app.editor(ctx);
                            app.preferences(ctx);
                            app.updates_window(ctx);
                        },
                    ));
                }
                let svg = crate::preview::export(
                    &last.unwrap().shapes,
                    size,
                    app.theme.background.color(),
                );
                std::fs::write(output.join(format!("update-{name}-{state}.svg")), svg).unwrap();
            }
        }
    }
}
