//! Settings pages for formatting, folders and search, key bindings, and snippets.
use super::preferences::{row, section};
use super::quality::Binding;
use super::*;
use hedit::settings::{FormatSettings, Settings};

/// A multi-line list of glob patterns, one per line, edited as text.
fn pattern_list(ui: &mut egui::Ui, id: &str, patterns: &mut Vec<String>, warning: egui::Color32) {
    let id = ui.make_persistent_id(id);
    let mut text = ui.data_mut(|d| {
        d.get_temp::<String>(id)
            .unwrap_or_else(|| patterns.join("\n"))
    });
    let response = ui.add(
        egui::TextEdit::multiline(&mut text)
            .font(egui::TextStyle::Monospace)
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
    let parsed: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect();
    let error = hedit::folder::validate_excludes(&parsed, &[]).err();
    if response.changed() && error.is_none() {
        *patterns = parsed;
    }
    if response.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    if let Some(error) = error {
        ui.colored_label(warning, error);
    }
}

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
    let mut trim = f.trim_trailing_white_space_on_save == Some(true);
    if ui
        .checkbox(&mut trim, "Trim trailing whitespace on save")
        .changed()
    {
        f.trim_trailing_white_space_on_save = trim.then_some(true);
    }
    let mut newline = f.ensure_newline_at_eof_on_save == Some(true);
    if ui
        .checkbox(&mut newline, "End the file with a newline on save")
        .changed()
    {
        f.ensure_newline_at_eof_on_save = newline.then_some(true);
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
        let warning = self.theme.warning.color();
        section(ui, "Hidden folders");
        ui.weak("One name or pattern per line, such as node_modules or *.egg-info. Matching folders are left out of the folder tree, Go to file, and Find in folders.");
        pattern_list(
            ui,
            "folder-excludes",
            &mut self.settings.folder_exclude_patterns,
            warning,
        );
        section(ui, "Hidden files");
        pattern_list(
            ui,
            "file-excludes",
            &mut self.settings.file_exclude_patterns,
            warning,
        );
        section(ui, "Find");
        ui.weak("New searches start with these options; changes in the Find bar are remembered.");
        ui.checkbox(&mut self.quality.options.case_sensitive, "Case sensitive");
        ui.checkbox(&mut self.quality.options.regex, "Regular expressions");
        ui.checkbox(&mut self.quality.options.whole_word, "Whole words");
        ui.add_space(14.0);
        if ui.button("Restore defaults").clicked() {
            let d = Settings::default();
            self.settings.folder_exclude_patterns = d.folder_exclude_patterns;
            self.settings.file_exclude_patterns = d.file_exclude_patterns;
            self.quality.options.case_sensitive = d.find_case_sensitive;
            self.quality.options.regex = d.find_regex;
            self.quality.options.whole_word = d.find_whole_word;
        }
    }
    pub(super) fn key_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.weak("Click a shortcut, then press the new keys. Esc cancels. Unbinding removes a shortcut entirely.");
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.key_filter)
                .hint_text("Filter commands or shortcuts…")
                .desired_width(240.0),
        );
        let filter = self.key_filter.to_lowercase();
        let shortcuts: Vec<(Command, String)> = Command::ALL
            .iter()
            .map(|&c| (c, self.shortcut_text(c)))
            .collect();
        let mut change: Option<(Command, Option<String>)> = None;
        egui::Grid::new("key-bindings")
            .num_columns(3)
            .striped(true)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (command, shortcut) in &shortcuts {
                    if !filter.is_empty()
                        && !command.label().to_lowercase().contains(&filter)
                        && !shortcut.to_lowercase().contains(&filter)
                    {
                        continue;
                    }
                    ui.label(command.label());
                    let capturing = self.key_capture == Some(*command);
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
                            .any(|(other, s)| other != command && s == shortcut);
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
                        self.key_capture = (!capturing).then_some(*command);
                    }
                    ui.horizontal(|ui| {
                        if clash {
                            ui.colored_label(self.theme.warning.color(), "⚠");
                        }
                        if self.overridden(*command) && ui.small_button("Reset").clicked() {
                            change = Some((*command, None));
                        }
                        if !shortcut.is_empty() && ui.small_button("Unbind").clicked() {
                            change = Some((*command, Some(String::new())));
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
            }
            if let Some(dir) = self.settings_files.dir.clone() {
                if ui.button("Open keybindings.json").clicked() {
                    self.close_transient(ctx);
                    self.open(ctx, Some(dir.join(settings_store::KEYBINDINGS_FILE)));
                }
            }
        });
    }
    /// Set a command's shortcut (`None` restores its default), unbinding any
    /// other command that used the same keys.
    pub(super) fn set_binding(&mut self, command: Command, shortcut: Option<String>) {
        let bindings = &mut self.quality.config.keybindings;
        let editor_only = bindings
            .iter()
            .find(|b| b.command == command)
            .is_some_and(|b| b.editor_only);
        bindings.retain(|b| b.command != command);
        let Some(shortcut) = shortcut else {
            return;
        };
        if !shortcut.is_empty() {
            let label = Binding {
                command,
                shortcut: shortcut.clone(),
                editor_only,
            };
            self.quality.config.keybindings.push(label);
            let taken = self.shortcut_text(command);
            let others: Vec<Command> = Command::ALL
                .iter()
                .copied()
                .filter(|&c| c != command && self.shortcut_text(c) == taken)
                .collect();
            for other in others {
                self.quality
                    .config
                    .keybindings
                    .retain(|b| b.command != other);
                self.quality.config.keybindings.push(Binding {
                    command: other,
                    shortcut: String::new(),
                    editor_only: false,
                });
                self.notice = format!(
                    "{taken} was used by “{}”, which is now unbound.",
                    other.label()
                );
            }
        } else {
            self.quality.config.keybindings.push(Binding {
                command,
                shortcut,
                editor_only,
            });
        }
    }
    /// While a key binding is being recorded, the next key press becomes the
    /// shortcut and no other shortcut fires. Returns true while recording.
    pub(super) fn capture_binding(&mut self, ctx: &egui::Context) -> bool {
        let Some(command) = self.key_capture else {
            return false;
        };
        if !self.preferences_open {
            self.key_capture = None;
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
