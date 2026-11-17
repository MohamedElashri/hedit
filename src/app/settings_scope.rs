//! Scope selection shares the existing project draft; provenance is on demand.
use super::*;
use hedit::projects::{Defaults, FormattingSource, ResolvedFormatting};
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Scope {
    #[default]
    User,
    Project,
}
#[derive(Clone, PartialEq, Eq)]
struct ResolutionKey {
    path: PathBuf,
    language: String,
    user: Defaults,
    project: Defaults,
    epoch: u64,
}
type Resolution = Result<ResolvedFormatting, String>;
#[derive(Default)]
struct Inspector {
    key: Option<ResolutionKey>,
    result: Option<Resolution>,
    rx: Option<Receiver<(ResolutionKey, Resolution)>>,
}
#[derive(Default)]
pub(super) struct State {
    pub scope: Scope,
    ready: bool,
    origin: Option<PathBuf>,
    epoch: u64,
    inspector: Inspector,
}
impl State {
    pub(super) fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }
}
impl Inspector {
    fn poll(&mut self, ctx: &egui::Context, key: ResolutionKey) {
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key.clone());
            self.result = None;
        }
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok((received, result)) => {
                    self.rx = None;
                    if self.key.as_ref() == Some(&received) {
                        self.result = Some(result);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.result = Some(Err(
                        "The settings inspector stopped. Refresh to retry.".into()
                    ));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.rx.is_none() && self.result.is_none() {
            let (tx, rx) = mpsc::channel();
            let wake = ctx.clone();
            match thread::Builder::new()
                .name("hedit-settings-inspector".into())
                .spawn(move || {
                    let result = hedit::projects::resolved_formatting(
                        &key.path,
                        &key.language,
                        &key.user,
                        Some(&key.project),
                    );
                    let _ = tx.send((key, result));
                    wake.request_repaint();
                }) {
                Ok(_) => self.rx = Some(rx),
                Err(error) => self.result = Some(Err(format!("Cannot inspect settings: {error}"))),
            }
        }
    }
}
impl HeditApp {
    pub(super) fn settings_key_command(command: Command) -> bool {
        matches!(
            command,
            Command::Save | Command::Close | Command::Quit | Command::Palette | Command::Settings
        )
    }
    pub(super) fn save_scope_settings(&mut self) {
        if self.busy || self.closing {
            return;
        }
        if self.settings_scope.scope == Scope::Project {
            if let Err(error) = self.save_project_settings_draft() {
                self.quality.project_editor.error = Some(error);
            }
        } else {
            self.flush_settings();
        }
    }
    pub(super) fn settings_shortcuts(&mut self, ctx: &egui::Context) {
        if self.busy || self.closing {
            return;
        }
        if self.handle_chords(ctx) {
            return;
        }
        let bindings = self.configured_bindings().to_vec();
        for binding in bindings
            .into_iter()
            .filter(|b| !b.editor_only && Self::settings_key_command(b.command))
        {
            if let Ok(keys) = chords::parse(&binding.shortcut) {
                if let [key] = keys.as_slice() {
                    if tab_switch::take_exact(ctx, key.modifiers, key.logical_key) {
                        self.execute(ctx, binding.command);
                        return;
                    }
                }
            }
        }
        for (modifiers, key, command) in [
            (Modifiers::COMMAND, Key::S, Command::Save),
            (Modifiers::COMMAND, Key::W, Command::Close),
            (Modifiers::COMMAND, Key::Q, Command::Quit),
            (Modifiers::COMMAND, Key::Comma, Command::Settings),
            (
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::P,
                Command::Palette,
            ),
        ] {
            if !self.overridden(command) && tab_switch::take_exact(ctx, modifiers, key) {
                self.execute(ctx, command);
                return;
            }
        }
    }
    pub(super) fn prepare_project_scope(&mut self) {
        if !self.settings_scope.ready || self.settings_scope.origin != self.quality.project_path {
            self.open_project_settings();
            self.quality.settings_open = false;
            self.settings_scope.ready = true;
            self.settings_scope.origin = self.quality.project_path.clone();
        }
        self.settings_scope.scope = Scope::Project;
    }
    pub(super) fn settings_scope_selector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.settings_scope.scope == Scope::User, "User")
                .clicked()
            {
                self.settings_scope.scope = Scope::User;
            }
            if ui
                .selectable_label(self.settings_scope.scope == Scope::Project, "Project")
                .clicked()
            {
                self.prepare_project_scope();
            }
        });
    }
    pub(super) fn project_scope_contents(&mut self, ui: &mut egui::Ui) {
        if self.settings_scope.origin != self.quality.project_path
            && !self.quality.project_editor.dirty
        {
            self.prepare_project_scope();
        }
        ui.weak("Project formatting defaults and language overrides. Changes apply when you choose Save settings.");
        self.effective_settings_ui(ui);
        if self.quality.project_path.is_none() {
            ui.horizontal_wrapped(|ui| {
                ui.weak("This workspace session · Save a project file to keep these settings.");
                if ui
                    .add_enabled(
                        !self.quality.project_editor.dirty
                            && self.workspace.remote.is_none()
                            && self.command_enabled(Command::SaveProject),
                        egui::Button::new("Save project…"),
                    )
                    .clicked()
                {
                    self.execute(ui.ctx(), Command::SaveProject);
                }
            });
        }
        if self.project_settings_contents(ui, true) {
            self.preferences_open = false;
            self.focus_editor = true;
        }
    }
    fn effective_key(&self) -> ResolutionKey {
        let b = self.active();
        let language = b.language().id();
        let path = b
            .page
            .info
            .path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.workspace.folders.first().map_or_else(
                    || PathBuf::from("untitled"),
                    |f| PathBuf::from(&f.path).join("untitled"),
                )
            });
        let mut user = Defaults {
            formatting: self.settings.format.to_formatting(),
            ..Default::default()
        };
        if let Some(value) = self.settings.syntax_specific.get(language) {
            user.languages
                .insert(language.into(), value.to_formatting());
        }
        let mut project = Defaults {
            formatting: self.quality.project_settings.formatting.clone(),
            ..Default::default()
        };
        if let Some(value) = self.quality.project_settings.languages.get(language) {
            project.languages.insert(language.into(), value.clone());
        }
        ResolutionKey {
            path,
            language: language.into(),
            user,
            project,
            epoch: self.settings_scope.epoch,
        }
    }
    pub(super) fn effective_settings_ui(&mut self, ui: &mut egui::Ui) {
        if self.workspace.remote.is_some() {
            ui.weak("Formatting sources are available for local files.");
            return;
        }
        let mut source = None;
        egui::CollapsingHeader::new("Resolved settings for the active file").id_salt("effective-settings").default_open(false).show(ui,|ui| {
            if ui.small_button("Refresh sources").clicked() {self.settings_scope.invalidate();}
            let key=self.effective_key();let path=key.path.clone();
            self.settings_scope.inspector.poll(ui.ctx(),key);
            ui.add(egui::Label::new(path.display().to_string()).truncate()).on_hover_text(path.display().to_string());
            let Some(result)=&self.settings_scope.inspector.result else {ui.spinner();return;};
            let resolved=match result {Ok(value)=>value,Err(error)=>{ui.colored_label(self.theme.warning.color(),error);return;}};
            let f=&resolved.formatting;
            let rows=[
                ("Indentation","indent_style",f.indent_style.clone()),
                ("Tab width","indent_size",f.indent_size.map(|n|n.to_string())),
                ("Line ending on save","end_of_line",f.end_of_line.clone()),
                ("Encoding on save","charset",f.charset.clone()),
                ("Final newline on save","insert_final_newline",f.insert_final_newline.map(|v|v.to_string())),
                ("Trim spaces on save","trim_trailing_whitespace",f.trim_trailing_whitespace.map(|v|v.to_string())),
            ];
            egui::ScrollArea::vertical().id_salt("effective-settings-rows").max_height((ui.available_height()*0.4).max(60.0)).show(ui,|ui| {
                for (label,key,value) in rows {
                    preferences::row(ui,label,|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(value.as_deref().unwrap_or("Keep file’s value"));
                            match resolved.sources.get(key) {
                                Some(FormattingSource::EditorConfig {path,property,unset}) => {
                                    let response=ui.small_button(if *unset {".editorconfig · unset"} else {".editorconfig"}).on_hover_text(format!("{property} · {}",path.display()));
                                    response.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Button,true,format!("Open source for {label}")));
                                    if response.clicked() {source=Some(path.clone());}
                                },
                                Some(FormattingSource::User)=>{ui.weak("User defaults");},
                                Some(FormattingSource::UserLanguage)=>{ui.weak("User · language");},
                                Some(FormattingSource::Project)=>{ui.weak("Project defaults");},
                                Some(FormattingSource::ProjectLanguage)=>{ui.weak("Project · language");},
                                None=>{ui.weak("File / default");},
                            }
                        });
                    });
                }
            });
            ui.weak("Editor indentation can reflect file detection or per-file choices. Project drafts apply on Save; .editorconfig has final precedence.");
        });
        if let Some(path) = source {
            self.preferences_open = false;
            self.open(ui.ctx(), Some(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{frame, text_rect};
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        settings_store::TEST_CONFIG_DIR.with(|p| *p.borrow_mut() = Some(dir.path().join("config")));
        std::fs::write(dir.path().join(".editorconfig"), "root=true\n").unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        app.set_folder(hedit::folder::read_directory(dir.path().to_owned()).unwrap());
        let path = dir.path().join("project.hedit-project");
        let value = serde_json::json!({"folders":[dir.path()],"custom":{"keep":true},"settings":{"formatting":{"indent_size":2,"vendor":"keep"},"languages":{"rust":{"indent_size":4}},"plugin":{"keep":true}}});
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        app.quality.project_settings = serde_json::from_value(value["settings"].clone()).unwrap();
        app.quality.project_path = Some(path);
        (dir, ctx, app)
    }
    #[test]
    fn scope_switches_preserve_project_drafts_and_do_not_change_user_settings() {
        let (_dir, ctx, mut app) = app();
        let user = app.settings.clone();
        app.open_preferences(PreferencesPage::Formatting);
        app.prepare_project_scope();
        app.quality.project_editor.draft.formatting.indent_size = Some(6);
        app.sync_project_settings_gui();
        let draft = app.quality.settings_json.clone();
        assert!(app.quality.project_editor.dirty);
        app.settings_scope.scope = Scope::User;
        frame(&ctx, &mut app, vec![]);
        app.prepare_project_scope();
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.quality.settings_json, draft);
        assert!(app.quality.project_editor.dirty);
        assert_eq!(app.settings, user);
        assert!(app.settings_scope.inspector.rx.is_none());
        app.save_project_settings_draft().unwrap();
        let value: serde_json::Value =
            hedit::projects::read_json(app.quality.project_path.as_ref().unwrap(), 1024 * 1024)
                .unwrap();
        assert_eq!(value["settings"]["formatting"]["indent_size"], 6);
        assert_eq!(value["settings"]["formatting"]["vendor"], "keep");
        assert_eq!(value["settings"]["plugin"]["keep"], true);
        assert_eq!(value["custom"]["keep"], true);
        assert_eq!(app.settings, user);
        settings_store::TEST_CONFIG_DIR.with(|p| *p.borrow_mut() = None);
    }
    #[test]
    fn project_command_uses_the_shared_window_and_keeps_save_visible() {
        let (_dir, ctx, mut app) = app();
        app.settings.ui_font_size = 20.0;
        app.execute(&ctx, Command::ProjectSettings);
        assert!(app.preferences_open && !app.quality.settings_open);
        assert!(app.settings_scope.scope == Scope::Project);
        for _ in 0..3 {
            crate::app::tests::frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        }
        let out = crate::app::tests::frame_size(&ctx, &mut app, vec![], egui::vec2(720.0, 480.0));
        assert!(out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "Save settings")
                .is_some_and(|r| s.clip_rect.contains_rect(r))));
        settings_store::TEST_CONFIG_DIR.with(|p| *p.borrow_mut() = None);
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use crate::app::tests::{finish_app_job, frame, key};
    fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.terminal_onboarding_complete = true;
        app.set_folder(hedit::folder::read_directory(dir.path().to_owned()).unwrap());
        std::fs::write(dir.path().join(".editorconfig"), "root=true\n").unwrap();
        let project = dir.path().join("settings.hedit-project");
        std::fs::write(
            &project,
            r#"{"folders":[],"settings":{"formatting":{"indent_size":2}}}"#,
        )
        .unwrap();
        app.quality.project_path = Some(project);
        app.quality.project_settings.formatting.indent_size = Some(2);
        (dir, ctx, app)
    }
    #[test]
    fn save_and_close_shortcuts_target_the_settings_window_and_preserve_the_document() {
        let (_dir, ctx, mut app) = fixture();
        app.workspace.open[0].page.text = "Keep the unsaved document".into();
        app.workspace.edit(0, "").unwrap();
        let id = app.active().view.id;
        app.execute(&ctx, Command::ProjectSettings);
        app.quality.project_editor.draft.formatting.indent_size = Some(6);
        app.sync_project_settings_gui();
        frame(&ctx, &mut app, vec![key(Key::S, Modifiers::COMMAND)]);
        assert!(!app.busy && app.preferences_open);
        assert!(app.active().page.info.dirty);
        assert_eq!(app.active().page.text, "Keep the unsaved document");
        let value: serde_json::Value =
            hedit::projects::read_json(app.quality.project_path.as_ref().unwrap(), 1024 * 1024)
                .unwrap();
        assert_eq!(value["settings"]["formatting"]["indent_size"], 6);
        frame(&ctx, &mut app, vec![key(Key::W, Modifiers::COMMAND)]);
        assert!(!app.preferences_open && app.close_flow.is_none());
        assert_eq!(app.workspace.open.len(), 1);
        assert_eq!(app.active().view.id, id);
    }
    #[test]
    fn project_json_keeps_direct_ime_commits_in_its_text_field() {
        let (_dir, ctx, mut app) = fixture();
        app.execute(&ctx, Command::ProjectSettings);
        app.switch_project_settings_view(project_settings::View::Json);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("project-settings-json")));
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![key(Key::A, Modifiers::COMMAND)]);
        let mut events = vec![egui::Event::Text("{\"custom\":\"".into())];
        events.extend(
            "مرحبا"
                .chars()
                .map(|c| egui::Event::Ime(egui::ImeEvent::Commit(c.to_string()))),
        );
        events.push(egui::Event::Text("\"}".into()));
        frame(&ctx, &mut app, events);
        assert!(app.quality.settings_json.contains("مرحبا"));
        assert!(app.active().page.text.is_empty());
    }
    #[test]
    fn concurrent_project_changes_are_rejected_without_losing_the_draft_or_editor_edits() {
        let (_dir, ctx, mut app) = fixture();
        app.execute(&ctx, Command::ProjectSettings);
        app.quality.project_editor.draft.formatting.indent_size = Some(6);
        app.sync_project_settings_gui();
        let draft = app.quality.settings_json.clone();
        let project = app.quality.project_path.clone().unwrap();
        let external =
            r#"{"folders":[],"settings":{"formatting":{"indent_size":8},"external":true}}"#;
        std::fs::write(&project, external).unwrap();
        assert!(app
            .save_project_settings_draft()
            .unwrap_err()
            .contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(&project).unwrap(), external);
        assert_eq!(app.quality.settings_json, draft);
        app.open_project_settings();
        app.quality.settings_open = false;
        let mut document = Document::open(app.workspace.editor.id(), &project).unwrap();
        let page = document.page(0).unwrap();
        app.workspace.add(document, page).unwrap();
        let index = app.workspace.active;
        let previous = app.active().page.text.clone();
        app.workspace.open[index].page.text.push(' ');
        app.workspace.edit(index, &previous).unwrap();
        assert!(app
            .save_project_settings_draft()
            .unwrap_err()
            .contains("project file"));
        assert_eq!(std::fs::read_to_string(project).unwrap(), external);
        assert!(app.active().page.info.dirty);
    }
    #[test]
    fn project_changes_do_not_retarget_a_pending_draft() {
        let (dir, ctx, mut app) = fixture();
        app.execute(&ctx, Command::ProjectSettings);
        app.quality.project_editor.draft.formatting.indent_size = Some(6);
        app.sync_project_settings_gui();
        let original = app.quality.project_path.clone().unwrap();
        let other = dir.path().join("other.hedit-project");
        std::fs::copy(&original, &other).unwrap();
        let before = std::fs::read(&other).unwrap();
        app.quality.project_path = Some(other.clone());
        assert!(app
            .save_project_settings_draft()
            .unwrap_err()
            .contains("active project changed"));
        assert_eq!(std::fs::read(other).unwrap(), before);
        assert!(app.quality.project_editor.dirty);
    }
    #[test]
    fn inspector_discards_old_contexts_and_refreshes_external_editorconfig_changes() {
        let (dir, ctx, mut app) = fixture();
        let config = dir.path().join(".editorconfig");
        std::fs::write(
            &config,
            "root=true\n[alpha.rs]\nindent_size=3\n[beta.rs]\nindent_size=7\n",
        )
        .unwrap();
        for name in ["alpha.rs", "beta.rs"] {
            std::fs::write(dir.path().join(name), "fn item() {}\n").unwrap();
        }
        app.open(&ctx, Some(dir.path().join("alpha.rs")));
        finish_app_job(&mut app, &ctx);
        let first = app.effective_key();
        app.settings_scope.inspector.poll(&ctx, first);
        app.open(&ctx, Some(dir.path().join("beta.rs")));
        finish_app_job(&mut app, &ctx);
        for expected in [7, 5] {
            if expected == 5 {
                std::fs::write(&config, "root=true\n[beta.rs]\nindent_size=5\n").unwrap();
                app.settings_scope.invalidate();
            }
            let key = app.effective_key();
            let until = Instant::now() + Duration::from_secs(10);
            loop {
                app.settings_scope.inspector.poll(&ctx, key.clone());
                if app.settings_scope.inspector.result.is_some() {
                    break;
                }
                assert!(Instant::now() < until);
                thread::sleep(Duration::from_millis(5));
            }
            let result = app
                .settings_scope
                .inspector
                .result
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap();
            assert_eq!(result.formatting.indent_size, Some(expected));
            assert!(app.settings_scope.inspector.key.as_ref() == Some(&key));
        }
    }
}
