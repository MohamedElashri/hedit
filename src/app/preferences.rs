use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreferencesPage {
    General,
    Terminal,
    Editor,
    Formatting,
    Folders,
    Appearance,
    Keys,
    Snippets,
    Tools,
    LanguageServers,
    Recovery,
}
impl PreferencesPage {
    pub(super) const ALL: [Self; 11] = [
        Self::General,
        Self::Terminal,
        Self::Editor,
        Self::Formatting,
        Self::Folders,
        Self::Appearance,
        Self::Keys,
        Self::Snippets,
        Self::Tools,
        Self::LanguageServers,
        Self::Recovery,
    ];
    pub(super) fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Terminal => "Terminal",
            Self::Editor => "Editor",
            Self::Formatting => "Formatting",
            Self::Folders => "Folders & Search",
            Self::Appearance => "Appearance",
            Self::Keys => "Key Bindings",
            Self::Snippets => "Snippets",
            Self::Tools => "CUDA Tools",
            Self::LanguageServers => "Language Servers",
            Self::Recovery => "Recovery",
        }
    }
    pub(super) fn available(self) -> bool {
        self != Self::Tools || hedit::debugger::LOCAL_TOOLS_SUPPORTED
    }
}

/// A row of a settings page: a fixed-width label, then the control.
pub(super) fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    if ui.available_width() < 460.0 {
        ui.label(label);
        add(ui);
        ui.add_space(4.0);
        return;
    }
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
        self.settings_browser.query.clear();
        self.settings_browser.modified = false;
        self.settings_browser.category = None;
        self.settings_browser.reset_scroll = true;
        self.settings_browser.focus_search = page != PreferencesPage::LanguageServers;
        self.preferences_open = true;
        self.focus_editor = false;
    }
    pub(super) fn preferences(&mut self, ctx: &egui::Context) {
        if self.preferences_open {
            let before = self.theme.clone();
            let mut open = true;
            let available =
                (ctx.screen_rect().size() - egui::vec2(32.0, 80.0)).max(egui::vec2(320.0, 240.0));
            egui::Window::new("Settings")
                .id(egui::Id::new("preferences"))
                .open(&mut open)
                .default_size(egui::vec2(960.0, 680.0).min(available))
                .min_size(egui::vec2(520.0, 320.0).min(available))
                .max_size(available)
                .resizable(true)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - 150.0).max(120.0);
                        let search = ui.add(
                            egui::TextEdit::singleline(&mut self.settings_browser.query)
                                .id(egui::Id::new("settings-search"))
                                .hint_text("Search settings by name or key…")
                                .desired_width(width)
                                .char_limit(256),
                        );
                        if std::mem::take(&mut self.settings_browser.focus_search) {
                            search.request_focus();
                        }
                        let changed = search.changed();
                        let modified = ui
                            .checkbox(&mut self.settings_browser.modified, "Modified")
                            .changed();
                        if ui.small_button("Clear").clicked() {
                            self.settings_browser.query.clear();
                            self.settings_browser.modified = false;
                            self.settings_browser.category = None;
                            self.settings_browser.reset_scroll = true;
                        }
                        if changed || modified {
                            self.settings_browser.category = None;
                            self.settings_browser.reset_scroll = true;
                        }
                    });
                    ui.small(
                        if self.preferences_page == PreferencesPage::LanguageServers
                            && !self.settings_search_active()
                        {
                            self.workspace
                                .remote
                                .as_ref()
                                .map(|r| {
                                    format!("Language servers · SSH host: {}", r.config.target.host)
                                })
                                .unwrap_or_else(|| "Language servers · This computer".into())
                        } else {
                            "User settings · This computer · Applies across projects".into()
                        },
                    );
                    ui.separator();
                    let searching = self.settings_search_active();
                    let counts = if searching {
                        self.settings_category_counts()
                    } else {
                        [0; 11]
                    };
                    let body_height = (ui.available_height() - 35.0).max(120.0);
                    ui.horizontal_top(|ui| {
                        ui.set_height(body_height);
                        ui.vertical(|ui| {
                            ui.set_width(150.0);
                            egui::ScrollArea::vertical()
                                .id_salt("settings-categories")
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if searching
                                        && ui
                                            .add_sized(
                                                [145.0, 28.0],
                                                egui::Button::new("All results").selected(
                                                    self.settings_browser.category.is_none(),
                                                ),
                                            )
                                            .clicked()
                                    {
                                        self.settings_browser.category = None;
                                        self.settings_browser.reset_scroll = true;
                                    }
                                    for page in PreferencesPage::ALL {
                                        if !page.available() {
                                            continue;
                                        }
                                        let label = if searching {
                                            format!("{} ({})", page.title(), counts[page as usize])
                                        } else {
                                            page.title().into()
                                        };
                                        let selected = if searching {
                                            self.settings_browser.category == Some(page)
                                        } else {
                                            self.preferences_page == page
                                        };
                                        if ui
                                            .add_sized(
                                                [145.0, 28.0],
                                                egui::Button::new(label).selected(selected),
                                            )
                                            .clicked()
                                        {
                                            self.preferences_page = page;
                                            self.settings_browser.category =
                                                searching.then_some(page);
                                            self.settings_browser.reset_scroll = true;
                                        }
                                    }
                                });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            ui.set_width(ui.available_width().max(0.0));
                            ui.heading(if searching && self.settings_browser.category.is_none() {
                                "Search results"
                            } else {
                                self.preferences_page.title()
                            });
                            ui.add_space(8.0);
                            let mut scroll = egui::ScrollArea::vertical()
                                .id_salt((
                                    "preferences-body",
                                    self.preferences_page as u8,
                                    searching,
                                ))
                                .max_height(body_height - 34.0)
                                .auto_shrink([false, false]);
                            if std::mem::take(&mut self.settings_browser.reset_scroll) {
                                scroll = scroll.vertical_scroll_offset(0.0);
                            }
                            scroll.show(ui, |ui| {
                                if searching {
                                    self.settings_catalog(ui, self.settings_browser.category);
                                } else {
                                    match self.preferences_page {
                                        PreferencesPage::General => self.general_preferences(ui),
                                        PreferencesPage::Terminal => self.terminal_preferences(ui),
                                        PreferencesPage::Editor => self.editor_preferences(ui),
                                        PreferencesPage::Formatting => {
                                            self.formatting_preferences(ui)
                                        }
                                        PreferencesPage::Folders => self.folder_preferences(ui),
                                        PreferencesPage::Keys => self.key_preferences(ui, ctx),
                                        PreferencesPage::Snippets => self.snippet_preferences(ui),
                                        PreferencesPage::Appearance => {
                                            self.theme_preferences(ui, ctx)
                                        }
                                        PreferencesPage::Tools => self.tool_preferences(ui),
                                        PreferencesPage::LanguageServers => {
                                            self.language_preferences(ui)
                                        }
                                        PreferencesPage::Recovery => {
                                            self.recovery_preferences(ui, ctx)
                                        }
                                    }
                                }
                            });
                        });
                    });
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
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
                        ui.weak(
                            if self.preferences_page == PreferencesPage::LanguageServers
                                && !searching
                            {
                                "Server edits use Save configuration."
                            } else if self.settings_files.broken {
                                "Fix settings.json to resume saving."
                            } else {
                                "Valid changes save automatically."
                            },
                        );
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
            let mut guide = false;
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
                        guide = ui.link("User guide").clicked();
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
            if guide {
                self.execute(ctx, Command::UserGuide);
            }
        }
        self.user_guide(ctx);
    }
    fn general_preferences(&mut self, ui: &mut egui::Ui) {
        self.settings_catalog(ui, Some(PreferencesPage::General));
    }
    fn editor_preferences(&mut self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style())
            .inner_margin(12)
            .show(ui, |ui| {
                ui.strong("Editor font preview");
                if self.editor_font_loading.is_some() {
                    ui.weak("Loading editor font…");
                }
                self.font_setting(ui);
                ui.label(
                    RichText::new(font_picker::SAMPLE)
                        .font(FontId::monospace(self.settings.font_size))
                        .color(self.theme.text.color()),
                );
            });
        ui.add_space(8.0);
        self.settings_catalog(ui, Some(PreferencesPage::Editor));
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
