use super::*;

use hedit::settings::{Settings, MAX_COLUMN};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreferencesPage {
    General,
    Editor,
    Formatting,
    Folders,
    Appearance,
    Keys,
    Snippets,
    Tools,
    Recovery,
}
impl PreferencesPage {
    pub(super) const ALL: [Self; 9] = [
        Self::General,
        Self::Editor,
        Self::Formatting,
        Self::Folders,
        Self::Appearance,
        Self::Keys,
        Self::Snippets,
        Self::Tools,
        Self::Recovery,
    ];
    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Editor => "Editor",
            Self::Formatting => "Formatting",
            Self::Folders => "Folders & Search",
            Self::Appearance => "Appearance",
            Self::Keys => "Key Bindings",
            Self::Snippets => "Snippets",
            Self::Tools => "CUDA Tools",
            Self::Recovery => "Recovery",
        }
    }
    fn available(self) -> bool {
        self != Self::Tools || hedit::debugger::LOCAL_TOOLS_SUPPORTED
    }
}

/// A row of a settings page: a fixed-width label, then the control.
pub(super) fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(170.0, 20.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(170.0);
                ui.add(egui::Label::new(label).truncate());
            },
        );
        add(ui);
    });
}

pub(super) fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong());
    ui.add_space(2.0);
}

/// Parse a comma-separated column list such as `80, 120`.
fn parse_columns(text: &str) -> Option<Vec<u16>> {
    let mut columns: Vec<u16> = text
        .split([',', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok().filter(|&c| (1..=MAX_COLUMN).contains(&c)))
        .collect::<Option<_>>()?;
    columns.sort_unstable();
    columns.dedup();
    (columns.len() <= 8).then_some(columns)
}
impl HeditApp {
    pub(super) fn open_preferences(&mut self, page: PreferencesPage) {
        if !self.preferences_open {
            self.appearance.original = self.theme.clone();
        }
        self.preferences_page = if page.available() {
            page
        } else {
            PreferencesPage::General
        };
        self.preferences_open = true;
        self.focus_editor = false;
    }
    pub(super) fn preferences(&mut self, ctx: &egui::Context) {
        if self.preferences_open {
            let before = self.theme.clone();
            let mut open = true;
            let body_height = (ctx.screen_rect().height() - 190.0).clamp(190.0, 520.0);
            let width = (ctx.screen_rect().width() - 64.0).clamp(540.0, 780.0);
            egui::Window::new("Settings")
                .id(egui::Id::new("preferences"))
                .open(&mut open)
                .fixed_size(egui::vec2(width, body_height + 74.0))
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.horizontal_top(|ui| {
                        ui.set_height(body_height);
                        ui.vertical(|ui| {
                            ui.set_width(130.0);
                            ui.add_space(8.0);
                            for page in PreferencesPage::ALL {
                                if !page.available() {
                                    continue;
                                }
                                if ui
                                    .add_sized(
                                        [130.0, 26.0],
                                        egui::Button::new(page.title())
                                            .selected(self.preferences_page == page),
                                    )
                                    .clicked()
                                {
                                    self.preferences_page = page;
                                }
                            }
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.set_min_width(350.0);
                            ui.heading(self.preferences_page.title());
                            ui.add_space(10.0);
                            egui::ScrollArea::vertical()
                                .id_salt(("preferences-body", self.preferences_page as u8))
                                .max_height(body_height - 40.0)
                                .show(ui, |ui| match self.preferences_page {
                                    PreferencesPage::General => self.general_preferences(ui),
                                    PreferencesPage::Editor => self.editor_preferences(ui),
                                    PreferencesPage::Formatting => self.formatting_preferences(ui),
                                    PreferencesPage::Folders => self.folder_preferences(ui),
                                    PreferencesPage::Keys => self.key_preferences(ui, ctx),
                                    PreferencesPage::Snippets => self.snippet_preferences(ui),
                                    PreferencesPage::Appearance => self.theme_preferences(ui, ctx),
                                    PreferencesPage::Tools => self.tool_preferences(ui),
                                    PreferencesPage::Recovery => self.recovery_preferences(ui, ctx),
                                });
                        });
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.settings_files.dir.is_some(),
                                egui::Button::new("Open settings.json"),
                            )
                            .on_hover_text(
                                self.settings_path()
                                    .map_or("".into(), |p| p.display().to_string()),
                            )
                            .clicked()
                        {
                            self.open_settings_file(ctx);
                        }
                        ui.weak("Changes save automatically.");
                    });
                });
            self.preferences_open &= open;
            if !open {
                self.focus_editor = true;
            }
            if self.theme != before {
                self.theme_edited(ctx, &before);
            }
        }
        self.navigation_dialog(ctx);
        if self.about_open {
            let mut open = true;
            let mut close = false;
            egui::Window::new("About hedit")
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
                    controls::info_header(
                        ui,
                        &self.theme,
                        "hedit",
                        &format!("Version {}", env!("CARGO_PKG_VERSION")),
                        false,
                    );
                    ui.add_space(8.0);
                    ui.weak("Keep your train of thought.");
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.hyperlink_to(
                            "User guide",
                            "https://github.com/MohamedElashri/hedit/blob/main/USER_GUIDE.md",
                        );
                        ui.weak("·");
                        ui.hyperlink_to("GitHub", "https://github.com/MohamedElashri/hedit");
                        ui.weak("·");
                        ui.hyperlink_to(
                            "MIT License",
                            "https://github.com/MohamedElashri/hedit/blob/main/LICENSE",
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            close = ui
                                .add_sized([72.0, 28.0], egui::Button::new("Close"))
                                .clicked();
                        });
                    });
                });
            self.about_open = open && !close;
            if !self.about_open {
                self.focus_editor = true;
            }
        }
    }
    fn general_preferences(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.settings;
        section(ui, "Window");
        ui.checkbox(&mut s.show_tab_bar, "Show tab bar");
        ui.checkbox(&mut s.show_status_bar, "Show status bar");
        ui.checkbox(&mut s.show_sidebar, "Show sidebar");
        let folders = ui.checkbox(&mut s.show_folders, "Show folders in the sidebar");
        ui.add_space(14.0);
        if ui.button("Restore defaults").clicked() {
            let d = Settings::default();
            let s = &mut self.settings;
            (
                s.show_tab_bar,
                s.show_status_bar,
                s.show_sidebar,
                s.show_folders,
            ) = (
                d.show_tab_bar,
                d.show_status_bar,
                d.show_sidebar,
                d.show_folders,
            );
            self.sync_folder_view();
        } else if folders.changed() {
            // Same as View → Side bar → Show folders: showing them shows the sidebar.
            self.settings.show_sidebar |= self.settings.show_folders;
            self.sync_folder_view();
        }
    }
    fn editor_preferences(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.settings;
        section(ui, "Text");
        let _ = s;
        row(ui, "Font", |ui| self.font_setting(ui));
        let s = &mut self.settings;
        row(ui, "Font name or file", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut s.font_face)
                    .hint_text("Built-in (Hack)")
                    .desired_width(240.0),
            )
            .on_hover_text(
                "An installed font family, such as JetBrains Mono, or a .ttf/.otf/.ttc path",
            );
        });
        ui.label(
            RichText::new(font_picker::SAMPLE)
                .font(FontId::monospace(s.font_size))
                .color(self.theme.text.color()),
        );
        ui.weak("Characters the font lacks, such as Arabic or CJK, come from system fonts.");
        row(ui, "Font size", |ui| {
            ui.add(
                egui::Slider::new(&mut s.font_size, hedit::settings::FONT_SIZES)
                    .step_by(1.0)
                    .suffix(" px"),
            );
        });
        ui.weak(format!(
            "{} and {} also change it while you work.",
            Command::ZoomIn.shortcut(),
            Command::ZoomOut.shortcut()
        ));
        row(ui, "Line spacing", |ui| {
            ui.add(
                egui::Slider::new(&mut s.line_padding, 0.0..=20.0)
                    .step_by(1.0)
                    .suffix(" px"),
            );
        });
        row(ui, "Cursor width", |ui| {
            ui.add(
                egui::Slider::new(&mut s.caret_width, 1.0..=4.0)
                    .step_by(0.5)
                    .suffix(" px"),
            );
        });
        ui.checkbox(&mut s.caret_blink, "Blink the cursor");
        ui.checkbox(&mut s.scroll_past_end, "Scroll past the last line");
        ui.checkbox(&mut s.line_numbers, "Show line numbers");
        ui.checkbox(&mut s.highlight_line, "Highlight the current line");
        ui.checkbox(&mut s.draw_white_space, "Show whitespace");
        ui.checkbox(&mut s.draw_indent_guides, "Show indentation guides");
        ui.checkbox(&mut s.show_minimap, "Show minimap");
        section(ui, "Wrapping and rulers");
        ui.checkbox(&mut s.word_wrap, "Wrap long lines");
        row(ui, "Wrap at column", |ui| {
            ui.add_enabled(
                s.word_wrap,
                egui::DragValue::new(&mut s.wrap_width).range(0..=MAX_COLUMN),
            );
            ui.weak(if s.wrap_width == 0 {
                "0 wraps at the window edge"
            } else {
                "columns"
            });
        });
        row(ui, "Rulers", |ui| {
            let id = ui.make_persistent_id("rulers-text");
            let mut text = ui.data_mut(|d| {
                d.get_temp::<String>(id).unwrap_or_else(|| {
                    s.rulers
                        .iter()
                        .map(u16::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
            });
            let response = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(120.0)
                    .hint_text("80, 120"),
            );
            let parsed = parse_columns(&text);
            if response.changed() {
                if let Some(columns) = &parsed {
                    s.rulers = columns.clone();
                }
            }
            if response.lost_focus() || !response.has_focus() {
                ui.data_mut(|d| d.remove::<String>(id));
            } else {
                ui.data_mut(|d| d.insert_temp(id, text));
            }
            if parsed.is_none() {
                ui.colored_label(self.theme.warning.color(), "Use up to 8 columns, 1–400");
            }
        });
        section(ui, "Typing");
        let s = &mut self.settings;
        ui.checkbox(
            &mut s.auto_indent,
            "Keep the indentation of the previous line",
        );
        ui.checkbox(
            &mut s.smart_indent,
            "Indent after an opening bracket or colon",
        );
        ui.checkbox(
            &mut s.smart_backspace,
            "Backspace removes one indentation level",
        );
        ui.checkbox(
            &mut s.trim_automatic_white_space,
            "Remove indentation left on lines that stay empty",
        );
        ui.checkbox(
            &mut s.copy_with_empty_selection,
            "Copy and cut the whole line when nothing is selected",
        );
        ui.checkbox(
            &mut s.auto_match_enabled,
            "Close brackets and quotes automatically",
        );
        ui.checkbox(&mut s.match_brackets, "Highlight matching brackets");
        ui.checkbox(
            &mut s.match_selection,
            "Outline other occurrences of the selected word",
        );
        ui.checkbox(&mut s.auto_complete, "Suggest completions while typing");
        ui.add_enabled(
            s.auto_complete,
            egui::Checkbox::new(
                &mut s.auto_complete_commit_on_tab,
                "Accept completions with Tab only; Enter starts a new line",
            ),
        );
        ui.checkbox(
            &mut s.show_definitions,
            "Show definitions when the pointer rests on a name",
        );
        ui.checkbox(
            &mut s.spell_check,
            "Check spelling in prose, comments, and strings",
        );
        ui.horizontal(|ui| {
            ui.label("Change markers compare with");
            for (value, label) in [
                ("auto", "Git, else the saved file"),
                ("saved", "The saved file"),
                ("off", "Nothing (off)"),
            ] {
                ui.radio_value(&mut s.mini_diff, value.to_owned(), label);
            }
        });
        ui.add_space(14.0);
        if ui.button("Restore defaults").clicked() {
            let d = Settings::default();
            let s = &mut self.settings;
            s.font_size = d.font_size;
            s.font_face = d.font_face;
            s.line_padding = d.line_padding;
            s.caret_width = d.caret_width;
            s.caret_blink = d.caret_blink;
            s.scroll_past_end = d.scroll_past_end;
            s.line_numbers = d.line_numbers;
            s.highlight_line = d.highlight_line;
            s.draw_white_space = d.draw_white_space;
            s.draw_indent_guides = d.draw_indent_guides;
            s.show_minimap = d.show_minimap;
            s.word_wrap = d.word_wrap;
            s.wrap_width = d.wrap_width;
            s.rulers = d.rulers;
            s.auto_indent = d.auto_indent;
            s.smart_indent = d.smart_indent;
            s.smart_backspace = d.smart_backspace;
            s.auto_match_enabled = d.auto_match_enabled;
            s.match_brackets = d.match_brackets;
            s.match_selection = d.match_selection;
            s.auto_complete = d.auto_complete;
            s.auto_complete_delay = d.auto_complete_delay;
            s.auto_complete_commit_on_tab = d.auto_complete_commit_on_tab;
            s.show_definitions = d.show_definitions;
            s.spell_check = d.spell_check;
            s.trim_automatic_white_space = d.trim_automatic_white_space;
            s.copy_with_empty_selection = d.copy_with_empty_selection;
            s.mini_diff = d.mini_diff;
        }
    }
    fn tool_preferences(&mut self, ui: &mut egui::Ui) {
        if !hedit::debugger::LOCAL_TOOLS_SUPPORTED {
            ui.label(
                "CUDA source editing is available. Local CUDA tools are supported on Linux only.",
            );
            return;
        }
        ui.weak("Configure NVIDIA tools here. Choose a program and arguments in the CUDA panel.");
        ui.add_space(12.0);
        ui.label("CUDA-GDB executable");
        let live = self
            .debug
            .as_ref()
            .is_some_and(|d| d.state != State::Exited);
        ui.add_enabled(
            !live,
            egui::TextEdit::singleline(&mut self.settings.cuda_gdb_path)
                .desired_width(f32::INFINITY)
                .hint_text("cuda-gdb or /path/to/cuda-gdb"),
        );
        if live {
            ui.weak("Stop the running tool session before changing the debugger path.");
        }
        ui.add_space(8.0);
        ui.label("Compute Sanitizer and nvidia-smi are found on PATH.");
        ui.label("Install the CUDA Toolkit and a compatible NVIDIA driver separately.");
        ui.add_space(14.0);
        if ui.button("Open CUDA panel").clicked() {
            self.gpu_open = true;
        }
    }
    fn recovery_preferences(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label("Unsaved and modified files are checkpointed every two seconds while work changes, and on normal exit.");
        ui.add_space(12.0);
        ui.label(if self.checkpoint_pending {
            "Checkpoint in progress…"
        } else if self.persisted == self.generation {
            "Recovery up to date"
        } else {
            "Changes waiting for a checkpoint"
        });
        if let Some(saved) = self.last_saved {
            ui.weak(format!(
                "Last completed checkpoint: {} seconds ago",
                saved.elapsed().as_secs()
            ));
        }
        if ui
            .add_enabled(
                !self.checkpoint_pending && !self.closing,
                egui::Button::new("Checkpoint now"),
            )
            .clicked()
        {
            self.checkpoint(ctx);
        }
        ui.add_space(12.0);
        ui.label("Closed files stay on the shelf. The 50-file history limit expands to keep unsaved notes, so closing a tab is never blocked by a full shelf.");
        if ui.button("Browse closed files…").clicked() {
            self.shelf_open = true;
        }
        ui.add_space(12.0);
        ui.weak("Recovery is periodic. A crash or power loss can lose edits since the last completed checkpoint. Keep independent backups for important work.");
    }
    fn navigation_dialog(&mut self, ctx: &egui::Context) {
        if !self.goto_open {
            return;
        }
        let mut open = true;
        let mut choice = controls::Choice::None;
        let shown = egui::Window::new("Go to line")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.label("Line number in the whole document");
                let input_id = egui::Id::new("goto-line-input");
                let enter = ctx.memory(|m| m.has_focus(input_id))
                    && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.goto)
                        .id(input_id)
                        .desired_width(f32::INFINITY)
                        .hint_text("1"),
                );
                if self.goto_focus {
                    response.request_focus();
                    self.goto_focus = false;
                }
                let valid = self.goto.parse::<u64>().is_ok_and(|line| line > 0);
                if !valid && !self.goto.is_empty() {
                    ui.colored_label(
                        self.theme.warning.color(),
                        "Enter a positive whole line number.",
                    );
                }
                choice =
                    controls::dialog_buttons(ui, &self.theme, "Go", !self.busy && valid, enter);
            });
        if choice == controls::Choice::Cancel {
            open = false;
        }
        if choice == controls::Choice::Confirm && !self.busy {
            if self.goto.parse::<u64>().is_ok_and(|line| line > 0) {
                self.goto_line(ctx);
                open = false;
            } else {
                self.notice = "Enter a positive line number.".into();
            }
        }
        if super::overlays::clicked_away(ctx, &shown) {
            open = false;
        }
        self.goto_open = open;
        if !open {
            self.focus_editor = true;
        }
    }
}
