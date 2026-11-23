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
    Extensions,
    Tools,
    LanguageServers,
    Recovery,
}
impl PreferencesPage {
    pub(super) const ALL: [Self; 12] = [
        Self::General,
        Self::Terminal,
        Self::Editor,
        Self::Formatting,
        Self::Folders,
        Self::Appearance,
        Self::Keys,
        Self::Snippets,
        Self::Extensions,
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
            Self::Extensions => "Extensions",
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
    let m = hedit::ui_style::Metrics::for_ui(ui);
    let label_width = 170.0 * m.font / 13.0;
    if ui.available_width() < label_width + 280.0 {
        ui.label(label);
        add(ui);
        ui.add_space(4.0);
        return;
    }
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, m.control),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(label_width);
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
        self.settings_scope.scope = settings_scope::Scope::User;
        if !self.preferences_open {
            self.settings_scope.invalidate();
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
        self.poll_extension_manager();
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
                    self.settings_scope_selector(ui);
                    ui.separator();
                    if self.settings_scope.scope == settings_scope::Scope::Project {
                        self.project_scope_contents(ui);
                        return;
                    }
                    let m = hedit::ui_style::Metrics::for_ui(ui);
                    let text_width = |text: &str| {
                        ui.fonts(|fonts| {
                            fonts
                                .layout_no_wrap(
                                    text.into(),
                                    FontId::proportional(m.font),
                                    self.theme.text.color(),
                                )
                                .size()
                                .x
                        })
                    };
                    let header_controls = text_width("Modified")
                        + text_width("Clear")
                        + ui.spacing().icon_width
                        + ui.spacing().icon_spacing
                        + 2.0 * ui.spacing().button_padding.x
                        + 4.0 * ui.spacing().item_spacing.x
                        + 12.0;
                    let category_width = (PreferencesPage::ALL
                        .iter()
                        .filter(|p| p.available())
                        .map(|p| text_width(p.title()))
                        .fold(0.0_f32, f32::max)
                        + if self.settings_search_active() {
                            64.0
                        } else {
                            24.0
                        })
                    .max(150.0)
                    .min(ui.available_width() * 0.38);
                    ui.horizontal(|ui| {
                        let width = (ui.available_width() - header_controls).max(80.0);
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
                    let scope: String = if self.preferences_page == PreferencesPage::LanguageServers
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
                    };
                    ui.add(egui::Label::new(RichText::new(&scope).small()).truncate())
                        .on_hover_text(scope);
                    ui.separator();
                    let searching = self.settings_search_active();
                    let counts = if searching {
                        self.settings_category_counts()
                    } else {
                        [0; PreferencesPage::ALL.len()]
                    };
                    let footer_height = 2.0 * m.control + 3.0 * m.gap + 4.0;
                    let body_height = (ui.available_height() - footer_height).max(64.0);
                    ui.horizontal_top(|ui| {
                        ui.set_height(body_height);
                        ui.vertical(|ui| {
                            self.settings_navigation(
                                ui,
                                searching,
                                &counts,
                                body_height,
                                category_width,
                            )
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
                            let content_width = ui.available_width();
                            let mut scroll = egui::ScrollArea::both()
                                .max_width(content_width)
                                .id_salt((
                                    "preferences-body",
                                    self.preferences_page as u8,
                                    searching,
                                ))
                                .max_height((body_height - (m.font + 5.0) * 1.5 - m.gap).max(24.0))
                                .auto_shrink([false, false]);
                            if std::mem::take(&mut self.settings_browser.reset_scroll) {
                                scroll = scroll.vertical_scroll_offset(0.0);
                            }
                            scroll.show(ui, |ui| {
                                // Long binding/configuration rows can scroll without
                                // widening the window; prose still wraps to the pane.
                                ui.set_width(content_width);
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
                                        PreferencesPage::Extensions => {
                                            self.extension_preferences(ui);
                                            self.extension_options(ui);
                                        }
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
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
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
                        ui.add(
                            egui::Label::new(
                                RichText::new(
                                    if self.preferences_page == PreferencesPage::LanguageServers
                                        && !searching
                                    {
                                        "Server edits use Save configuration."
                                    } else if self.settings_files.broken {
                                        "Fix settings.json to resume saving."
                                    } else {
                                        "Valid changes save automatically."
                                    },
                                )
                                .weak(),
                            )
                            .wrap(),
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
                    ui.label("A native text and log editor with durable recovery, Git, and SSH workspaces.");
                    ui.add_space(4.0);
                    let tip = egui::Button::new(
                        egui::RichText::new(format!("♥ {}", controls::TIP_LABEL)).strong(),
                    );
                    if ui.add(tip).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(controls::TIP_URL));
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.hyperlink_to("Project website", "https://melashri.net/software/hedit/");
                        ui.hyperlink_to("Source code", "https://github.com/MohamedElashri/hedit");
                        ui.hyperlink_to(
                            "Report an issue",
                            "https://github.com/MohamedElashri/hedit/issues",
                        );
                        ui.hyperlink_to("Release notes", hedit::update::RELEASES_URL);
                    });
                    guide = ui.link("User guide").clicked();
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.weak("Maintained by Mohamed Elashri");
                        ui.hyperlink_to(
                            "MIT license",
                            "https://github.com/MohamedElashri/hedit/blob/main/LICENSE",
                        );
                    });
                    ui.add_space(8.0);
                    ui.separator();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = ui
                            .add_sized([72.0, 28.0], egui::Button::new("Close"))
                            .clicked();
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
