//! Settings pages for formatting, folders and search, key bindings, and snippets.
use super::preferences::{row, section};
use super::quality::{Binding, BindingCommand};
use super::*;
use hedit::settings::{FormatSettings, Settings};

/// Controls for one set of formatting rules.
fn format_fields(ui: &mut egui::Ui, id: &str, f: &mut FormatSettings) {
    const KEEP: &str = "Keep each file's";
    row(ui, "Indent with", |ui| {
        egui::ComboBox::from_id_salt((id, "indent"))
            .selected_text(match f.translate_tabs_to_spaces {
                None => KEEP,
                Some(true) => "Spaces",
                Some(false) => "Tabs",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut f.translate_tabs_to_spaces, None, KEEP);
                ui.selectable_value(&mut f.translate_tabs_to_spaces, Some(true), "Spaces");
                ui.selectable_value(&mut f.translate_tabs_to_spaces, Some(false), "Tabs");
            });
    });
    row(ui, "Tab size", |ui| {
        egui::ComboBox::from_id_salt((id, "tab-size"))
            .selected_text(f.tab_size.map_or(KEEP.into(), |n| n.to_string()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut f.tab_size, None, KEEP);
                for n in 1..=8 {
                    ui.selectable_value(&mut f.tab_size, Some(n), n.to_string());
                }
            });
    });
    for (key, label, value) in [
        (
            "trim",
            "Trim trailing whitespace on save",
            &mut f.trim_trailing_white_space_on_save,
        ),
        (
            "newline",
            "End the file with a newline on save",
            &mut f.ensure_newline_at_eof_on_save,
        ),
    ] {
        row(ui, label, |ui| {
            egui::ComboBox::from_id_salt((id, key))
                .selected_text(match value {
                    None => "Inherit file / project",
                    Some(true) => "Enabled",
                    Some(false) => "Disabled",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(value, None, "Inherit file / project");
                    ui.selectable_value(value, Some(true), "Enabled");
                    ui.selectable_value(value, Some(false), "Disabled");
                });
        });
    }
    row(ui, "Line endings on save", |ui| {
        egui::ComboBox::from_id_salt((id, "eol"))
            .selected_text(match f.line_ending.as_deref() {
                Some("lf") => "LF (Unix, macOS)",
                Some("crlf") => "CRLF (Windows)",
                _ => KEEP,
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut f.line_ending, None, KEEP);
                ui.selectable_value(&mut f.line_ending, Some("lf".into()), "LF (Unix, macOS)");
                ui.selectable_value(&mut f.line_ending, Some("crlf".into()), "CRLF (Windows)");
            });
    });
    row(ui, "Encoding on save", |ui| {
        egui::ComboBox::from_id_salt((id, "encoding"))
            .selected_text(f.encoding.as_deref().unwrap_or(KEEP))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut f.encoding, None, KEEP);
                for label in [
                    "utf-8",
                    "utf-8-bom",
                    "utf-16le",
                    "utf-16be",
                    "windows-1252",
                    "iso-8859-1",
                    "shift_jis",
                    "gbk",
                ] {
                    ui.selectable_value(&mut f.encoding, Some(label.into()), label);
                }
            });
    });
}

/// The `Primary+Shift+K` form of a pressed key, as keybindings.json stores it.
pub(super) fn binding_text(modifiers: Modifiers, key: Key) -> String {
    let mut parts = Vec::new();
    if modifiers.command {
        parts.push("Primary");
    }
    if cfg!(target_os = "macos") && modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    parts.push(key.name());
    parts.join("+")
}

impl HeditApp {
    pub(super) fn formatting_preferences(&mut self, ui: &mut egui::Ui) {
        self.effective_settings_ui(ui);
        ui.weak("Defaults for every file. A project or .editorconfig can override them. Unset options leave each file's style alone.");
        section(ui, "All files");
        ui.checkbox(
            &mut self.settings.detect_indentation,
            "Use the indentation each file already has",
        );
        format_fields(ui, "all", &mut self.settings.format);
        section(ui, "Per syntax");
        ui.weak("Rules for one syntax override the defaults above.");
        let mut remove = None;
        for (syntax, rules) in self.settings.syntax_specific.iter_mut() {
            let label = syntax::Language::ALL
                .into_iter()
                .find(|l| l.id() == syntax)
                .map_or(syntax.as_str(), |l| l.label());
            egui::CollapsingHeader::new(label)
                .id_salt(("syntax-rules", syntax))
                .show(ui, |ui| {
                    format_fields(ui, syntax, rules);
                    if ui.small_button("Remove these rules").clicked() {
                        remove = Some(syntax.clone());
                    }
                });
        }
        if let Some(syntax) = remove {
            self.settings.syntax_specific.remove(&syntax);
        }
        let mut add = None;
        egui::ComboBox::from_id_salt("add-syntax-rules")
            .selected_text("Add rules for a syntax…")
            .height(320.0)
            .show_ui(ui, |ui| {
                for language in syntax::Language::ALL {
                    if language != syntax::Language::PlainText
                        && !self.settings.syntax_specific.contains_key(language.id())
                        && ui.selectable_label(false, language.label()).clicked()
                    {
                        add = Some(language.id().to_owned());
                    }
                }
            });
        if let Some(syntax) = add {
            self.settings
                .syntax_specific
                .insert(syntax, FormatSettings::default());
        }
        ui.add_space(14.0);
        if ui.button("Restore defaults").clicked() {
            self.settings.format = FormatSettings::default();
            self.settings.syntax_specific.clear();
            self.settings.detect_indentation = Settings::default().detect_indentation;
        }
    }
    pub(super) fn folder_preferences(&mut self, ui: &mut egui::Ui) {
        self.settings_catalog(ui, Some(PreferencesPage::Folders));
    }
    pub(super) fn key_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        use hedit::settings::KeybindingPreset;
        preferences::row(ui, "Shortcut preset", |ui| {
            egui::ComboBox::from_id_salt("keybinding-preset")
                .selected_text(match self.settings.keybinding_preset {
                    KeybindingPreset::Hedit => "Hedit",
                    KeybindingPreset::Vscode => "VS Code · common commands",
                })
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (KeybindingPreset::Hedit, "Hedit"),
                        (KeybindingPreset::Vscode, "VS Code · common commands"),
                    ] {
                        if ui
                            .selectable_value(&mut self.settings.keybinding_preset, value, label)
                            .changed()
                        {
                            self.chord = None;
                            self.reset_terminal_binding();
                        }
                    }
                });
        });
        ui.weak("Personal bindings take priority. Reset restores the selected preset. Other commands keep Hedit bindings.");
        ui.weak("Click a shortcut, then press the new keys. Esc cancels. Unbinding removes a shortcut entirely.");
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.key_filter)
                .hint_text("Filter commands or shortcuts…")
                .desired_width(240.0),
        );
        let filter = self.key_filter.to_lowercase();
        let mut shortcuts: Vec<(BindingCommand, String, String)> = Command::ALL
            .iter()
            .map(|&command| {
                (
                    command.into(),
                    command.label().to_owned(),
                    self.shortcut_text(command),
                )
            })
            .collect();
        let mut extension_ids = std::collections::BTreeSet::new();
        extension_ids.extend(self.extensions.iter().map(|command| command.id.as_str()));
        extension_ids.extend(
            self.quality
                .config
                .keybindings
                .iter()
                .filter_map(|binding| match &binding.command {
                    BindingCommand::Extension(id) => Some(id.as_str()),
                    BindingCommand::Builtin(_) => None,
                }),
        );
        for id in extension_ids {
            let command = BindingCommand::Extension(id.to_owned());
            let label = self.keybinding_label(&command);
            let shortcut = self
                .quality
                .config
                .keybindings
                .iter()
                .find(|binding| binding.command == command && !binding.shortcut.is_empty())
                .map_or_else(String::new, |binding| {
                    shortcut_preset::display(&binding.shortcut)
                });
            shortcuts.push((command, label, shortcut));
        }
        let mut change = None;
        egui::Grid::new("key-bindings")
            .num_columns(3)
            .striped(true)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (command, label, shortcut) in &shortcuts {
                    if !filter.is_empty()
                        && !label.to_lowercase().contains(&filter)
                        && !shortcut.to_lowercase().contains(&filter)
                        && !matches!(command, BindingCommand::Extension(id) if id.contains(&filter))
                    {
                        continue;
                    }
                    let name = ui.label(label);
                    if let BindingCommand::Extension(id) = command {
                        name.on_hover_text(id);
                    }
                    let capturing = self.key_capture.as_ref() == Some(command);
                    let text = if capturing {
                        match &self.capture_first {
                            Some((first, _)) => format!(
                                "{}, …",
                                first.replace(
                                    "Primary",
                                    if cfg!(target_os = "macos") {
                                        "Cmd"
                                    } else {
                                        "Ctrl"
                                    }
                                )
                            ),
                            None => "Press keys…".to_owned(),
                        }
                    } else if shortcut.is_empty() {
                        "—".to_owned()
                    } else {
                        shortcut.clone()
                    };
                    let clash = !shortcut.is_empty()
                        && shortcuts
                            .iter()
                            .any(|(other, _, s)| other != command && s == shortcut);
                    let response = ui
                        .add(
                            egui::Button::new(RichText::new(text).monospace())
                                .selected(capturing)
                                .min_size(egui::vec2(150.0, 0.0)),
                        )
                        .on_hover_text(if clash {
                            "Another command uses this shortcut"
                        } else {
                            "Click, then press the new keys"
                        });
                    if response.clicked() {
                        self.key_capture = (!capturing).then(|| command.clone());
                        self.capture_first = None;
                    }
                    ui.horizontal(|ui| {
                        if clash {
                            ui.colored_label(self.theme.warning.color(), "⚠");
                        }
                        if let Some(builtin) = command.builtin() {
                            if self.personally_bound(builtin) && ui.small_button("Reset").clicked()
                            {
                                change = Some((command.clone(), None));
                            }
                            if !shortcut.is_empty() && ui.small_button("Unbind").clicked() {
                                change = Some((command.clone(), Some(String::new())));
                            }
                        } else if self
                            .quality
                            .config
                            .keybindings
                            .iter()
                            .any(|binding| binding.command == *command)
                            && ui.small_button("Unbind").clicked()
                        {
                            change = Some((command.clone(), None));
                        }
                    });
                    ui.end_row();
                }
            });
        if let Some((command, shortcut)) = change {
            self.set_binding(command, shortcut);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Reset all").clicked() {
                self.quality.config.keybindings.clear();
                self.chord = None;
                self.reset_terminal_binding();
            }
            if let Some(dir) = self.settings_files.dir.clone() {
                if ui.button("Open keybindings.json").clicked() {
                    self.close_transient(ctx);
                    self.open(ctx, Some(dir.join(settings_store::KEYBINDINGS_FILE)));
                }
            }
        });
    }
    fn keybinding_label(&self, command: &BindingCommand) -> String {
        match command {
            BindingCommand::Builtin(command) => command.label().to_owned(),
            BindingCommand::Extension(id) => self
                .extensions
                .iter()
                .find(|command| command.id == *id)
                .map_or_else(|| id.clone(), |command| command.title.clone()),
        }
    }

    /// Apply a personal binding and clear commands previously using its keys.
    pub(super) fn set_binding(
        &mut self,
        command: impl Into<BindingCommand>,
        shortcut: Option<String>,
    ) {
        let command = command.into();
        let mut conflicts = Vec::new();
        if let Some(shortcut) = shortcut.as_deref().filter(|text| !text.is_empty()) {
            let keys = chords::parse(shortcut).unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            let effective = self.configured_bindings();
            // Include saved extension bindings even while their target is unavailable.
            for binding in effective.iter().chain(
                self.quality
                    .config
                    .keybindings
                    .iter()
                    .filter(|binding| binding.command.builtin().is_none()),
            ) {
                if binding.command != command
                    && shortcut_preset::bindings_collide(&keys, &binding.shortcut)
                    && seen.insert(binding.command.clone())
                {
                    conflicts.push(binding.command.clone());
                }
            }
            for &builtin in Command::ALL {
                let target = BindingCommand::Builtin(builtin);
                if target != command
                    && !seen.contains(&target)
                    && shortcut_preset::bindings_collide(&keys, &self.shortcut_text(builtin))
                {
                    conflicts.push(target);
                }
            }
        }
        let notice = conflicts
            .last()
            .zip(shortcut.as_deref())
            .map(|(other, shortcut)| {
                format!(
                    "{} was used by “{}”, which is now unbound.",
                    shortcut_preset::display(shortcut),
                    self.keybinding_label(other),
                )
            });
        let bindings = &mut self.quality.config.keybindings;
        let editor_only = command.builtin().is_none()
            || bindings
                .iter()
                .find(|b| b.command == command)
                .is_some_and(|b| b.editor_only);
        bindings
            .retain(|binding| binding.command != command && !conflicts.contains(&binding.command));
        if let Some(shortcut) = shortcut {
            bindings.push(Binding {
                command,
                shortcut,
                editor_only,
            });
        }
        for command in conflicts
            .into_iter()
            .filter(|command| command.builtin().is_some())
        {
            bindings.push(Binding {
                command,
                shortcut: String::new(),
                editor_only: false,
            });
        }
        if let Some(notice) = notice {
            self.notice = notice;
        }
        self.chord = None;
        self.reset_terminal_binding();
    }
    /// While a key binding is being recorded, the next key press becomes the
    /// shortcut and no other shortcut fires. Returns true while recording.
    pub(super) fn capture_binding(&mut self, ctx: &egui::Context) -> bool {
        let Some(command) = self.key_capture.clone() else {
            return false;
        };
        if !self.preferences_open {
            self.key_capture = None;
            self.capture_first = None;
            return false;
        }
        let pressed = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } => Some((*key, *modifiers)),
                _ => None,
            })
        });
        // Keep the keys away from menus, the editor, and other shortcuts.
        ctx.input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
        });
        // A second key soon after the first records a two-key shortcut.
        const CHORD_WAIT: Duration = Duration::from_millis(900);
        match pressed {
            Some((Key::Escape, m)) if m.is_none() => {
                self.key_capture = None;
                self.capture_first = None;
            }
            Some((key, modifiers)) => {
                let text = binding_text(modifiers, key);
                match self.capture_first.take() {
                    Some((first, _)) => {
                        self.key_capture = None;
                        self.set_binding(command, Some(format!("{first}, {text}")));
                    }
                    None => {
                        self.capture_first = Some((text, Instant::now()));
                        ctx.request_repaint_after(CHORD_WAIT);
                    }
                }
            }
            None => {
                if let Some((first, at)) = &self.capture_first {
                    if at.elapsed() >= CHORD_WAIT {
                        let first = first.clone();
                        self.capture_first = None;
                        self.key_capture = None;
                        self.set_binding(command, Some(first));
                    } else {
                        ctx.request_repaint_after(CHORD_WAIT - at.elapsed());
                    }
                }
            }
        }
        true
    }
    pub(super) fn snippet_preferences(&mut self, ui: &mut egui::Ui) {
        ui.weak("Type a snippet's trigger and press Tab, or use Tools → Insert snippet…. ${SELECTION} inserts the selected text, $1, $2, … or ${1:text} are fields that Tab moves through, and $0 marks where the cursor ends.");
        ui.add_space(6.0);
        let mut remove = None;
        let count = self.quality.config.snippets.len();
        for (index, snippet) in self.quality.config.snippets.iter_mut().enumerate() {
            let title = if snippet.description.is_empty() {
                snippet.trigger.clone()
            } else {
                format!("{} — {}", snippet.trigger, snippet.description)
            };
            egui::CollapsingHeader::new(title)
                .id_salt(("snippet", index))
                .default_open(index + 1 == count && snippet.trigger == "new")
                .show(ui, |ui| {
                    row(ui, "Trigger", |ui| {
                        ui.add(egui::TextEdit::singleline(&mut snippet.trigger).char_limit(256));
                    });
                    row(ui, "Description", |ui| {
                        ui.text_edit_singleline(&mut snippet.description);
                    });
                    row(ui, "Syntax", |ui| {
                        let selected = snippet.language.as_deref().map_or("Any", |id| {
                            syntax::Language::ALL
                                .into_iter()
                                .find(|l| l.id() == id)
                                .map_or(id, |l| l.label())
                        });
                        egui::ComboBox::from_id_salt(("snippet-language", index))
                            .selected_text(selected)
                            .height(320.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut snippet.language, None, "Any");
                                for language in syntax::Language::ALL {
                                    ui.selectable_value(
                                        &mut snippet.language,
                                        Some(language.id().to_owned()),
                                        language.label(),
                                    );
                                }
                            });
                    });
                    ui.add(
                        egui::TextEdit::multiline(&mut snippet.body)
                            .code_editor()
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                    if ui.small_button("Remove snippet").clicked() {
                        remove = Some(index);
                    }
                });
        }
        if let Some(index) = remove {
            self.quality.config.snippets.remove(index);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Add snippet").clicked() {
                self.quality
                    .config
                    .snippets
                    .push(hedit::intelligence::Snippet {
                        trigger: "new".into(),
                        description: String::new(),
                        body: "$0".into(),
                        language: None,
                    });
            }
            if ui.button("Restore default snippets").clicked() {
                self.quality.config.snippets = hedit::intelligence::default_snippets();
            }
        });
    }
}

#[cfg(test)]
mod formatting_override_tests {
    use super::super::tests::text_rect;
    use super::*;
    fn draw(
        ctx: &egui::Context,
        rules: &mut FormatSettings,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        crate::app::tests::run_pass(
            ctx,
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 500.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .show_in_context(ctx, |ui| format_fields(ui, "test", rules));
            },
        )
    }
    fn click(ctx: &egui::Context, rules: &mut FormatSettings, pos: egui::Pos2) {
        draw(
            ctx,
            rules,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        draw(
            ctx,
            rules,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::NONE,
            }],
        );
    }
    #[test]
    fn formatting_controls_can_explicitly_disable_inherited_save_rules() {
        let ctx = egui::Context::default();
        let mut rules = FormatSettings {
            trim_trailing_white_space_on_save: Some(true),
            ensure_newline_at_eof_on_save: Some(true),
            ..FormatSettings::default()
        };
        draw(&ctx, &mut rules, vec![]);
        let output = draw(&ctx, &mut rules, vec![]);
        let enabled = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "Enabled"))
            .unwrap()
            .center();
        click(&ctx, &mut rules, enabled);
        let output = draw(&ctx, &mut rules, vec![]);
        let disabled = output
            .shapes
            .iter()
            .find_map(|s| text_rect(&s.shape, "Disabled"))
            .unwrap()
            .center();
        click(&ctx, &mut rules, disabled);
        assert_eq!(rules.trim_trailing_white_space_on_save, Some(false));
        assert_eq!(rules.ensure_newline_at_eof_on_save, Some(true));
    }
}
