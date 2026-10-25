//! Two views of one project-settings draft. Nothing applies until Save.
use super::*;
use hedit::projects::{self, Defaults, Formatting};
use serde_json::{Map, Value};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Gui,
    Json,
}

#[derive(Default)]
pub(super) struct Editor {
    pub view: View,
    pub draft: Defaults,
    pub scope: Option<String>,
    pub error: Option<String>,
    pub dirty: bool,
    pub saved: bool,
}

fn parse(text: &str) -> Result<Defaults, String> {
    if text.len() > 1024 * 1024 {
        return Err("Project settings exceed 1 MiB.".into());
    }
    let draft: Defaults = serde_json::from_str(text).map_err(|e| e.to_string())?;
    draft.formatting.validate()?;
    for (language, formatting) in &draft.languages {
        formatting
            .validate()
            .map_err(|e| format!("{language}: {e}"))?;
    }
    Ok(draft)
}

/// Update supported fields while retaining extra JSON keys and custom syntax IDs.
fn merge_formatting(value: &mut Value, formatting: &Formatting) -> Result<(), String> {
    if !value.is_object() {
        *value = Value::Object(Map::new());
    }
    let fields = serde_json::to_value(formatting).map_err(|e| e.to_string())?;
    value
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    Ok(())
}

impl Editor {
    fn sync_json(&self, text: &mut String) -> Result<(), String> {
        let mut value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let root = value
            .as_object_mut()
            .ok_or("Settings must be a JSON object.")?;
        merge_formatting(
            root.entry("formatting")
                .or_insert_with(|| Value::Object(Map::new())),
            &self.draft.formatting,
        )?;
        let languages = root
            .entry("languages")
            .or_insert_with(|| Value::Object(Map::new()));
        let languages = languages
            .as_object_mut()
            .ok_or("Languages must be a JSON object.")?;
        languages.retain(|language, _| self.draft.languages.contains_key(language));
        for (language, formatting) in &self.draft.languages {
            merge_formatting(
                languages
                    .entry(language.clone())
                    .or_insert_with(|| Value::Object(Map::new())),
                formatting,
            )?;
        }
        *text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn gui(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        let mut add = None;
        let mut remove = false;
        ui.horizontal_wrapped(|ui| {
            ui.label("Scope");
            egui::ComboBox::from_id_salt("project-settings-scope")
                .selected_text(
                    self.scope
                        .as_deref()
                        .map_or("Project defaults", language_name),
                )
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.scope, None, "Project defaults");
                    for language in self.draft.languages.keys() {
                        ui.selectable_value(
                            &mut self.scope,
                            Some(language.clone()),
                            language_name(language),
                        );
                    }
                });
            egui::ComboBox::from_id_salt("project-settings-add-language")
                .selected_text("Add language…")
                .height(200.0)
                .show_ui(ui, |ui| {
                    for language in syntax::Language::ALL {
                        if !self.draft.languages.contains_key(language.id())
                            && ui.selectable_label(false, language.label()).clicked()
                        {
                            add = Some(language.id().to_owned());
                        }
                    }
                });
            if self.scope.is_some() {
                remove = ui.small_button("Remove override").clicked();
            }
        });
        if let Some(language) = add {
            self.draft
                .languages
                .insert(language.clone(), Formatting::default());
            self.scope = Some(language);
            changed = true;
        }
        if remove {
            if let Some(language) = self.scope.take() {
                self.draft.languages.remove(&language);
            }
            changed = true;
        }
        ui.add_space(6.0);
        ui.add(
            egui::Label::new(
                RichText::new(if self.scope.is_some() {
                    "Inherited values use project defaults. .editorconfig takes precedence."
                } else {
                    "Inherited values use editor settings. .editorconfig takes precedence."
                })
                .small()
                .weak(),
            )
            .wrap(),
        );
        ui.add_space(12.0);
        let formatting = match self.scope.as_deref() {
            Some(language) => self.draft.languages.get_mut(language).unwrap(),
            None => &mut self.draft.formatting,
        };
        changed |= formatting_fields(ui, formatting);
        changed
    }
}

fn language_name(id: &str) -> &str {
    syntax::Language::from_id(id).map_or(id, |l| l.label())
}

fn string_field(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<String>,
    choices: &[(&str, &str)],
) -> bool {
    let mut changed = false;
    preferences::row(ui, label, |ui| {
        let selected = value.as_deref().map_or("Inherit", |current| {
            choices
                .iter()
                .find(|(v, _)| *v == current)
                .map_or(current, |(_, label)| *label)
        });
        egui::ComboBox::from_id_salt(label)
            .width(220.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(value, None, "Inherit").changed();
                for (v, label) in choices {
                    changed |= ui
                        .selectable_value(value, Some((*v).into()), *label)
                        .changed();
                }
            });
    });
    changed
}

fn bool_field(ui: &mut egui::Ui, label: &str, value: &mut Option<bool>) -> bool {
    let mut changed = false;
    preferences::row(ui, label, |ui| {
        egui::ComboBox::from_id_salt(label)
            .width(220.0)
            .selected_text(match value {
                None => "Inherit",
                Some(true) => "Enabled",
                Some(false) => "Disabled",
            })
            .show_ui(ui, |ui| {
                for (choice, label) in [
                    (None, "Inherit"),
                    (Some(true), "Enabled"),
                    (Some(false), "Disabled"),
                ] {
                    changed |= ui.selectable_value(value, choice, label).changed();
                }
            });
    });
    changed
}

fn formatting_fields(ui: &mut egui::Ui, f: &mut Formatting) -> bool {
    let mut changed = string_field(
        ui,
        "Indentation",
        &mut f.indent_style,
        &[("spaces", "Spaces"), ("tabs", "Tabs")],
    );
    ui.add_space(6.0);
    preferences::row(ui, "Tab width", |ui| {
        egui::ComboBox::from_id_salt("project-tab-width")
            .width(220.0)
            .selected_text(f.indent_size.map_or("Inherit".into(), |n| n.to_string()))
            .show_ui(ui, |ui| {
                changed |= ui
                    .selectable_value(&mut f.indent_size, None, "Inherit")
                    .changed();
                for n in 1..=8 {
                    changed |= ui
                        .selectable_value(&mut f.indent_size, Some(n), n.to_string())
                        .changed();
                }
            });
    });
    ui.add_space(6.0);
    changed |= string_field(
        ui,
        "Line endings",
        &mut f.end_of_line,
        &[("lf", "LF (Unix / macOS)"), ("crlf", "CRLF (Windows)")],
    );
    ui.add_space(6.0);
    changed |= string_field(
        ui,
        "Encoding on save",
        &mut f.charset,
        &[
            ("utf-8", "UTF-8"),
            ("utf-8-bom", "UTF-8 with BOM"),
            ("UTF-16LE", "UTF-16 LE"),
            ("UTF-16BE", "UTF-16 BE"),
            ("windows-1252", "Windows-1252"),
            ("ISO-8859-1", "Latin-1"),
            ("Shift_JIS", "Shift JIS"),
            ("GBK", "GBK"),
        ],
    );
    ui.add_space(6.0);
    changed |= bool_field(ui, "Final newline on save", &mut f.insert_final_newline);
    ui.add_space(6.0);
    changed |= bool_field(ui, "Trim trailing spaces", &mut f.trim_trailing_whitespace);
    changed
}

impl HeditApp {
    pub(super) fn open_project_settings(&mut self) {
        self.quality.settings_project = true;
        self.quality.settings_open = true;
        self.focus_editor = false;
        let mut error = None;
        let value = self
            .quality
            .project_path
            .as_ref()
            .and_then(
                |path| match projects::read_json::<Value>(path, 1024 * 1024) {
                    Ok(value) => value.get("settings").cloned(),
                    Err(e) => {
                        error = Some(e);
                        None
                    }
                },
            )
            .unwrap_or_else(|| serde_json::to_value(&self.quality.project_settings).unwrap());
        self.quality.settings_json = serde_json::to_string_pretty(&value).unwrap();
        self.quality.project_editor = match parse(&self.quality.settings_json) {
            Ok(draft) => Editor {
                draft,
                error,
                ..Default::default()
            },
            Err(e) => Editor {
                view: View::Json,
                error: Some(e),
                ..Default::default()
            },
        };
    }

    pub(super) fn switch_project_settings_view(&mut self, view: View) -> bool {
        if view == View::Gui {
            match parse(&self.quality.settings_json) {
                Ok(draft) => {
                    let editor = &mut self.quality.project_editor;
                    if editor
                        .scope
                        .as_ref()
                        .is_some_and(|s| !draft.languages.contains_key(s))
                    {
                        editor.scope = None;
                    }
                    editor.draft = draft;
                }
                Err(error) => {
                    self.quality.project_editor.error = Some(error);
                    return false;
                }
            }
        }
        self.quality.project_editor.view = view;
        self.quality.project_editor.error = None;
        true
    }

    pub(super) fn sync_project_settings_gui(&mut self) {
        match self
            .quality
            .project_editor
            .sync_json(&mut self.quality.settings_json)
        {
            Ok(()) => {
                self.quality.project_editor.dirty = true;
                self.quality.project_editor.saved = false;
                self.quality.project_editor.error = None;
            }
            Err(error) => self.quality.project_editor.error = Some(error),
        }
    }

    pub(super) fn save_project_settings_draft(&mut self) -> Result<(), String> {
        let draft = parse(&self.quality.settings_json)?;
        if let Some(path) = &self.quality.project_path {
            let mut definition: Value = projects::read_json(path, 1024 * 1024)?;
            let settings =
                serde_json::from_str(&self.quality.settings_json).map_err(|e| e.to_string())?;
            definition
                .as_object_mut()
                .ok_or("The project must be a JSON object.")?
                .insert("settings".into(), settings);
            // Validate the updated document, preserving unrelated project keys.
            serde_json::from_value::<projects::Project>(definition.clone())
                .map_err(|e| e.to_string())?;
            projects::atomic_json(path, &definition)?;
        }
        self.quality.project_settings = draft.clone();
        self.quality.project_editor.draft = draft;
        self.quality.project_editor.error = None;
        self.quality.project_editor.dirty = false;
        self.quality.project_editor.saved = true;
        self.changed();
        self.apply_format_preferences();
        self.notice = "Project settings saved.".into();
        Ok(())
    }

    pub(super) fn project_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut close = false;
        let mut save = false;
        let size = egui::vec2(
            (ctx.screen_rect().width() - 64.0).min(740.0),
            (ctx.screen_rect().height() - 100.0).min(440.0),
        );
        egui::Window::new("Project settings")
            .id(egui::Id::new("project-settings"))
            .open(&mut open)
            .collapsible(false)
            .default_size(size)
            .min_size([520.0, 300.0])
            .max_size(ctx.screen_rect().size() - egui::vec2(40.0, 70.0))
            .default_pos(ctx.screen_rect().center() - size * 0.5)
            .frame(
                egui::Frame::window(&ctx.style())
                    .inner_margin(12.0)
                    .corner_radius(8),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (view, label) in [(View::Gui, "GUI"), (View::Json, "JSON")] {
                        if ui
                            .selectable_label(self.quality.project_editor.view == view, label)
                            .clicked()
                        {
                            self.switch_project_settings_view(view);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.quality.project_editor.dirty {
                            ui.weak("Unsaved changes");
                        } else if self.quality.project_editor.saved {
                            ui.weak("Saved");
                        }
                    });
                });
                ui.separator();
                let error_height = if self.quality.project_editor.error.is_some() {
                    56.0
                } else {
                    0.0
                };
                let body_height = (ui.available_height() - 44.0 - error_height).max(60.0);
                let gui = self.quality.project_editor.view == View::Gui;
                let mut changed = false;
                egui::ScrollArea::both()
                    .id_salt(("project-settings-body", gui))
                    .max_height(body_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if gui {
                            changed = self.quality.project_editor.gui(ui);
                        } else {
                            let response = ui.add(
                                egui::TextEdit::multiline(&mut self.quality.settings_json)
                                    .id(egui::Id::new("project-settings-json"))
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(14),
                            );
                            if response.changed() {
                                self.quality.project_editor.dirty = true;
                                self.quality.project_editor.saved = false;
                                self.quality.project_editor.error = None;
                            }
                        }
                    });
                if changed {
                    self.sync_project_settings_gui();
                }
                if let Some(error) = &self.quality.project_editor.error {
                    egui::ScrollArea::vertical()
                        .id_salt("project-settings-error")
                        .max_height(48.0)
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(error).color(self.theme.error.color()),
                                )
                                .wrap(),
                            );
                        });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    let label = self.quality.project_path.as_ref().map_or(
                        "Current workspace".into(),
                        |p| {
                            p.file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned()
                        },
                    );
                    ui.add(egui::Label::new(RichText::new(label).small().weak()).truncate())
                        .on_hover_text(
                            self.quality.project_path.as_ref().map_or(
                                "Save a project to share these settings in a .hedit-project file."
                                    .into(),
                                |p| p.display().to_string(),
                            ),
                        );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        save = controls::primary_button(
                            ui,
                            &self.theme,
                            "Save settings",
                            !self.busy && !self.closing,
                        )
                        .clicked();
                        close = ui
                            .add_sized([72.0, 28.0], egui::Button::new("Close"))
                            .clicked();
                    });
                });
            });
        self.quality.settings_open = open && !close;
        if !self.quality.settings_open {
            self.focus_editor = true;
        }
        if save {
            if let Err(error) = self.save_project_settings_draft() {
                self.quality.project_editor.error = Some(error);
            }
        }
    }
}
