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
        let failed = self.updates.error.is_some();
        let (title, description) = if let Some(version) = &self.updates.installed {
            (
                "Update installed",
                format!("hedit {version} will be ready when you reopen the app."),
            )
        } else if self.updates.installing {
            ("Installing update…", self.updates.message.clone())
        } else if active {
            (
                "Checking for updates…",
                "Looking for the latest version of hedit.".into(),
            )
        } else if failed {
            (
                if self.updates.release.is_some() {
                    "Unable to install update"
                } else {
                    "Unable to check for updates"
                },
                "Please try again. If the problem continues, download the latest release.".into(),
            )
        } else if newer {
            let version = &self.updates.release.as_ref().unwrap().version;
            (
                "A new version is available",
                if self.updates.installation.is_some() {
                    format!("Update to hedit {version}.")
                } else {
                    format!("Download hedit {version} to update this installation.")
                },
            )
        } else if self.updates.release.is_some() {
            (
                "You’re up to date",
                "You’re using the latest version of hedit.".into(),
            )
        } else {
            (
                "Software Update",
                "Check for the latest version of hedit.".into(),
            )
        };
        let mut open = true;
        let mut close = false;
        let mut check = false;
        let mut install = false;
        let mut quit = false;
        egui::Window::new("Software Update")
            .id(egui::Id::new("updates"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .max_width((ctx.screen_rect().width() - 64.0).max(240.0))
            .frame(
                egui::Frame::window(&ctx.style())
                    .inner_margin(20.0)
                    .corner_radius(6),
            )
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Software Update")
                            .size(12.0)
                            .color(self.theme.muted.color()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if controls::close_button(ui, "Close · Esc").clicked() {
                            open = false;
                        }
                    });
                });
                ui.add_space(18.0);
                if active {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(title).size(19.0).strong());
                    });
                } else {
                    ui.label(RichText::new(title).size(19.0).strong());
                }
                ui.add_space(6.0);
                ui.add(egui::Label::new(&description).wrap());
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("Current version {}", env!("CARGO_PKG_VERSION")))
                        .size(11.0)
                        .color(self.theme.muted.color()),
                );
                if newer && !active && !installed {
                    ui.add_space(6.0);
                    ui.hyperlink_to(
                        "Release notes ↗",
                        self.updates.release.as_ref().unwrap().url(),
                    );
                }
                // Diagnostics are available only when an operation failed. Successful
                // checks never expose paths, installation internals, or raw release text.
                if failed && !active {
                    ui.add_space(8.0);
                    egui::CollapsingHeader::new("Error details")
                        .default_open(false)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("update-error")
                                .max_height((ctx.screen_rect().height() - 330.0).clamp(40.0, 120.0))
                                .show(ui, |ui| {
                                    if let Some(error) = &self.updates.error {
                                        ui.add(egui::Label::new(error).wrap().selectable(true));
                                    }
                                });
                            ui.hyperlink_to(
                                "Download latest release ↗",
                                hedit::update::RELEASES_URL,
                            );
                        });
                }
                ui.add_space(20.0);
                ui.separator();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
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
                                    if failed {
                                        "Try again"
                                    } else {
                                        "Install update"
                                    },
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
                        } else if !active && (failed || self.updates.release.is_none()) {
                            check = controls::primary_button(
                                ui,
                                &self.theme,
                                if failed {
                                    "Try again"
                                } else {
                                    "Check for updates"
                                },
                                true,
                            )
                            .clicked();
                        } else if !active {
                            close =
                                controls::primary_button(ui, &self.theme, "Done", true).clicked();
                            check = ui
                                .add(
                                    egui::Button::new("Check again")
                                        .frame(false)
                                        .min_size(egui::vec2(80.0, 28.0)),
                                )
                                .clicked();
                        }
                        if installed || active || newer || failed || self.updates.release.is_none()
                        {
                            close = ui
                                .add_sized(
                                    [64.0, 28.0],
                                    egui::Button::new(if installed || newer && !active {
                                        "Later"
                                    } else {
                                        "Close"
                                    }),
                                )
                                .clicked();
                        }
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
                {"id":2,"name":format!("hedit-9.4.0-linux-{}.tar.gz.sha256",std::env::consts::ARCH),"size":100},
                {"id":3,"name":"hedit-9.4.0-macos-universal.zip","size":200},
                {"id":4,"name":"hedit-9.4.0-macos-universal.zip.sha256","size":100}]
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
        for preset in hedit::theme::builtins() {
            app.set_theme(&ctx, preset.clone());
            for size in [egui::vec2(720.0, 480.0), egui::vec2(480.0, 360.0)] {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                for (state, button) in [
                    ("available", "Download update"),
                    ("current", "Done"),
                    ("failed", "Try again"),
                    ("checking", "Close"),
                    ("installing", "Close"),
                    ("installed", "Quit hedit"),
                ] {
                    let mut latest = release();
                    if state == "current" {
                        latest.version = env!("CARGO_PKG_VERSION").parse().unwrap();
                    }
                    let (_tx, rx) = mpsc::channel();
                    app.updates = Updates {
                        open: true,
                        release: (!matches!(state, "failed" | "checking")).then_some(latest),
                        installed: (state == "installed").then(|| "9.4.0".into()),
                        error: (state == "failed").then(|| "Sensitive diagnostic path".repeat(100)),
                        installing: state == "installing",
                        events: matches!(state, "checking" | "installing").then_some(rx),
                        message: "Downloading update…".into(),
                        ..Default::default()
                    };
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
                    let output = output.unwrap();
                    let rect = output
                        .shapes
                        .iter()
                        .find_map(|s| find(&s.shape, button))
                        .unwrap_or_else(|| panic!("missing {button}: {state}"));
                    assert!(
                        screen.contains_rect(rect),
                        "{}: {state} at {size:?}",
                        preset.name
                    );
                    assert!(!output
                        .shapes
                        .iter()
                        .any(|s| find(&s.shape, "Install location").is_some()));
                    assert!(!output.shapes.iter().any(|s| find(
                        &s.shape,
                        &"Sensitive diagnostic path".repeat(100)
                    )
                    .is_some()));
                    assert_eq!(
                        app.updates.active(),
                        matches!(state, "checking" | "installing")
                    );
                    assert_eq!(app.updates.installing(), state == "installing");
                }
            }
        }
    }

    #[test]
    fn redesign_expanded_update_errors_keep_retry_and_close_visible() {
        use super::super::tests::text_rect;
        for size in [egui::vec2(720.0, 480.0), egui::vec2(480.0, 360.0)] {
            let directory = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
            let ctx = egui::Context::default();
            ctx.style_mut(|style| style.animation_time = 0.0);
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            app.updates = Updates {
                open: true,
                error: Some("Could not read /a/very/long/path: connection failed. ".repeat(100)),
                ..Default::default()
            };
            let mut draw = |events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ctx| app.updates_window(ctx),
                )
            };
            for _ in 0..3 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            let header = output
                .shapes
                .iter()
                .find_map(|s| text_rect(&s.shape, "Error details"))
                .unwrap()
                .center();
            for pressed in [true, false] {
                draw(vec![
                    egui::Event::PointerMoved(header),
                    egui::Event::PointerButton {
                        pos: header,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ]);
            }
            for _ in 0..3 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            for label in ["Try again", "Close", "Download latest release ↗"] {
                let (shape, rect) = output
                    .shapes
                    .iter()
                    .find_map(|s| text_rect(&s.shape, label).map(|rect| (s, rect)))
                    .unwrap_or_else(|| panic!("missing {label} at {size:?}"));
                assert!(
                    screen.contains_rect(rect) && shape.clip_rect.contains_rect(rect),
                    "{label} was clipped at {size:?}: {rect:?}"
                );
            }
        }
    }

    #[test]
    fn redesign_update_close_buttons_preserve_installation_and_return_editor_focus() {
        use super::super::tests::text_rect;
        for (state, button) in [
            ("installing", "Close"),
            ("available", "Later"),
            ("current", "Done"),
            ("installed", "Later"),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let (workspace, _) = Workspace::restore(directory.path().into()).unwrap();
            let ctx = egui::Context::default();
            let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
            let (tx, rx) = mpsc::channel();
            let mut latest = release();
            if state == "current" {
                latest.version = env!("CARGO_PKG_VERSION").parse().unwrap();
            }
            app.focus_editor = false;
            app.updates = Updates {
                open: true,
                release: Some(latest),
                installing: state == "installing",
                events: (state == "installing").then_some(rx),
                installed: (state == "installed").then(|| "9.4.0".into()),
                message: "Downloading update…".into(),
                ..Default::default()
            };
            let mut draw = |events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(720.0, 480.0),
                        )),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ctx| app.updates_window(ctx),
                )
            };
            for _ in 0..3 {
                draw(vec![]);
            }
            let output = draw(vec![]);
            let at = output
                .shapes
                .iter()
                .find_map(|s| text_rect(&s.shape, button))
                .unwrap()
                .center();
            for pressed in [true, false] {
                draw(vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ]);
            }
            assert!(
                !app.updates.open,
                "{state}: {button} did not close the dialog"
            );
            assert!(app.focus_editor);
            assert!(
                !app.closing,
                "dismissing the dialog must not quit the editor"
            );
            assert_eq!(app.updates.active(), state == "installing");
            if state == "installing" {
                assert!(app.updates.installing());
                tx.send(Event::Progress("Installing update…")).unwrap();
                tx.send(Event::Installed(Ok("9.4.0".into()))).unwrap();
                app.updates.poll();
                assert_eq!(app.updates.installed.as_deref(), Some("9.4.0"));
                assert!(!app.updates.active());
                assert!(!app.updates.installing());
                assert!(!app.closing);
            }
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
                let last = last.unwrap();
                let svg = crate::preview::export(&last.shapes, size, app.theme.background.color());
                std::fs::write(output.join(format!("update-{name}-{state}.svg")), svg).unwrap();
                let ppm = super::super::rtl_tests::rasterize(
                    &ctx,
                    last,
                    egui::vec2(size[0], size[1]),
                    1.0,
                );
                std::fs::write(output.join(format!("update-{name}-{state}.ppm")), ppm).unwrap();
            }
        }
    }
}
