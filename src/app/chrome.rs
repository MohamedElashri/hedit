use super::*;
use std::path::Path;

/// The Find bar's query field, which keeps focus while stepping through matches.
pub(super) const FIND_INPUT: &str = "find-input";

fn menu_item(ui: &mut egui::Ui, app: &HeditApp, command: Command) -> bool {
    if matches!(command, Command::Gpu | Command::ToolSettings)
        && !hedit::debugger::LOCAL_TOOLS_SUPPORTED
    {
        return false;
    }
    let enabled = app.command_enabled(command);
    // Reserve a layer under the entry so unavailable entries can still show
    // which row the pointer is on; egui gives disabled widgets no hover state.
    let backdrop = ui.painter().add(egui::Shape::Noop);
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(command.label())
            .selected(app.command_selected(command))
            .shortcut_text(app.shortcut_text(command)),
    );
    if !enabled && response.contains_pointer() {
        let hovered = &ui.visuals().widgets.hovered;
        ui.painter().set(
            backdrop,
            egui::Shape::rect_filled(
                response.rect.expand(hovered.expansion),
                hovered.corner_radius,
                app.theme.hover().color().gamma_multiply(0.6),
            ),
        );
    }
    response.clicked()
}
impl HeditApp {
    pub(super) fn chrome(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        self.observe_notifications();
        self.sync_groups();
        self.sync_review_group();
        self.apply_workbench_style(ctx);
        root.reset_style();
        let theme = self.theme.clone();
        let mut action = None;
        let mut open_menu = None;
        egui::Panel::top("menu-bar")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(8, 1)),
            )
            .show(root, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    if controls::sidebar_button(
                        ui,
                        &theme,
                        self.command_selected(Command::Sidebar),
                        &self.shortcut_text(Command::Sidebar),
                    )
                    .clicked()
                    {
                        action = Some(Command::Sidebar);
                    }
                    ui.separator();
                    for (title, groups) in [
                        (
                            "File",
                            vec![
                                vec![
                                    Command::New,
                                    Command::NewWindow,
                                    Command::Open,
                                    Command::OpenFolder,
                                ],
                                vec![Command::ConnectSsh],
                                vec![Command::RefreshFolder, Command::CloseFolder],
                                vec![
                                    Command::Save,
                                    Command::SaveAs,
                                    Command::Rename,
                                    Command::Encoding,
                                ],
                                vec![Command::OpenRecent],
                                vec![
                                    Command::Close,
                                    Command::CloseAll,
                                    Command::CloseUnmodified,
                                    Command::Reopen,
                                    Command::Shelf,
                                ],
                                if cfg!(target_os = "macos") {
                                    vec![]
                                } else {
                                    vec![Command::Quit]
                                },
                            ],
                        ),
                        ("Edit", vec![]),
                        (
                            "Find",
                            vec![
                                vec![
                                    Command::Find,
                                    Command::Replace,
                                    Command::FindPrevious,
                                    Command::FindFiles,
                                ],
                                vec![
                                    Command::SelectAllMatches,
                                    Command::UseSelectionForFind,
                                    Command::UseSelectionForReplace,
                                    Command::QuickFindNext,
                                    Command::QuickFindPrevious,
                                ],
                            ],
                        ),
                        ("View", vec![]),
                        (
                            "Goto",
                            vec![
                                vec![
                                    Command::Switcher,
                                    Command::Goto,
                                    Command::Symbols,
                                    Command::GotoDefinition,
                                    Command::GotoReferences,
                                    Command::SwitchCounterpart,
                                    Command::OpenFileReference,
                                ],
                                vec![Command::NextTab, Command::PreviousTab, Command::RecentTab],
                                vec![Command::NextChange, Command::PreviousChange],
                                vec![Command::NavBack, Command::NavForward],
                                vec![
                                    Command::Bookmark,
                                    Command::NextBookmark,
                                    Command::PreviousBookmark,
                                    Command::SelectBookmarks,
                                    Command::ClearBookmarks,
                                    Command::Bookmarks,
                                ],
                            ],
                        ),
                        (
                            "Project",
                            vec![
                                vec![
                                    Command::OpenProject,
                                    Command::SaveProject,
                                    Command::SaveProjectAs,
                                    Command::RecentProjects,
                                ],
                                vec![Command::ProjectSettings],
                            ],
                        ),
                        (
                            "Tools",
                            vec![
                                vec![
                                    Command::Palette,
                                    Command::Gpu,
                                    Command::SourceControl,
                                    Command::Terminal,
                                    Command::NewTerminal,
                                    Command::TerminalSettings,
                                    Command::GitDiff,
                                    Command::RevertChange,
                                    Command::CompareDisk,
                                    Command::Complete,
                                    Command::Snippets,
                                ],
                                vec![
                                    Command::RecordMacro,
                                    Command::PlayMacro,
                                    Command::SaveMacro,
                                    Command::RunMacro,
                                ],
                                vec![
                                    Command::Build,
                                    Command::BuildWith,
                                    Command::RunProjectTask,
                                    Command::RerunProjectTask,
                                    Command::CancelBuild,
                                    Command::BuildResults,
                                    Command::NextResult,
                                    Command::PreviousResult,
                                ],
                                vec![
                                    Command::LanguageHover,
                                    Command::LanguageFormat,
                                    Command::LanguageRename,
                                    Command::LanguageQuickFix,
                                    Command::LanguageProblems,
                                    Command::NextProblem,
                                    Command::PreviousProblem,
                                    Command::LanguageRestart,
                                ],
                                vec![Command::Checkpoint],
                            ],
                        ),
                        (
                            "Preferences",
                            vec![vec![
                                Command::Settings,
                                Command::Appearance,
                                Command::ChooseFont,
                                Command::ToolSettings,
                                Command::LanguageServers,
                                Command::KeyBindings,
                            ]],
                        ),
                        (
                            "Help",
                            vec![vec![Command::UserGuide, Command::Updates, Command::About]],
                        ),
                    ] {
                        // egui does not mark the open menu's title, so keep it filled
                        // like a hovered title while its menu is showing.
                        let mut button = egui::Button::new(title);
                        if self.open_menu == Some(title) {
                            button = button.fill(theme.hover().color());
                        }
                        egui::menu::MenuButton::from_button(button).ui(ui, |ui| {
                            // Contents run only while this menu is open.
                            open_menu = Some(title);
                            ui.set_min_width(245.0);
                            if title == "Edit" {
                                self.edit_menu(ui, &mut action);
                                return;
                            }
                            if title == "View" {
                                self.view_menu(ui, &mut action);
                                return;
                            }
                            for (index, group) in
                                groups.iter().filter(|g| !g.is_empty()).enumerate()
                            {
                                if index > 0 {
                                    ui.separator();
                                }
                                if group.contains(&Command::LanguageHover) {
                                    ui.menu_button("Language server", |ui| {
                                        for &command in group {
                                            if menu_item(ui, self, command) {
                                                action = Some(command);
                                                ui.close();
                                            }
                                        }
                                    });
                                    continue;
                                }
                                for &command in group {
                                    if menu_item(ui, self, command) {
                                        action = Some(command);
                                        ui.close();
                                    }
                                }
                            }
                            if title == "Help" {
                                ui.separator();
                                if ui
                                    .hyperlink_to(controls::TIP_LABEL, controls::TIP_URL)
                                    .clicked()
                                {
                                    ui.close();
                                }
                            }
                        });
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.remote_menu(ui)
                    });
                });
            });
        if self.open_menu != open_menu {
            self.open_menu = open_menu;
            ctx.request_repaint();
        }
        if let Some(command) = action {
            self.execute(ctx, command);
        }
        self.status_bar(root);
        self.activity_bar(root);
        self.buffer_sidebar(root);
        self.tabs(root);
        self.document_outline(root);
    }
    fn tabs(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        if !self.settings.show_tab_bar || self.distraction_free || self.panes.mode != 0 {
            return;
        }
        let theme = self.theme.clone();
        let mut select = None;
        let mut close = None;
        let mut action = None;
        let mut reorder = None;
        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin {
                        left: 6,
                        right: 6,
                        top: 2,
                        bottom: 0,
                    }),
            )
            .show(root, |ui| {
                egui::ScrollArea::horizontal()
                    .id_salt("tab-strip")
                    .show_viewport(ui, |ui, viewport| {
                        self.tab_strip.sync(ui, &self.workspace);
                        let visible = self.tab_strip.visible(viewport);
                        let total = self.tab_strip.width();
                        let active = self.workspace.active;
                        let origin = ui.cursor().min;
                        if self
                            .tab_strip
                            .reveal(self.active().view.id, active, viewport.width())
                        {
                            ui.scroll_to_rect(
                                egui::Rect::from_min_max(
                                    origin + egui::vec2(self.tab_strip.offset(active), 0.0),
                                    origin
                                        + egui::vec2(
                                            self.tab_strip.offset(active + 1),
                                            self.tab_strip.height(),
                                        ),
                                ),
                                None,
                            );
                        }
                        ui.set_min_size(egui::vec2(
                            total
                                + 28.0
                                + if self.source_control.review_tab {
                                    160.0
                                } else {
                                    0.0
                                },
                            self.tab_strip.height(),
                        ));
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.horizontal(|ui| {
                            ui.add_space(self.tab_strip.offset(visible.start));
                            for index in visible.clone() {
                                let buffer = &self.workspace.open[index];
                                let active =
                                    index == self.workspace.active && !self.source_control.open;
                                let can_close = self.workspace.can_close(index);
                                let button = self.tab_strip.button(
                                    ui,
                                    index,
                                    if active {
                                        theme.text.color()
                                    } else {
                                        theme.muted.color()
                                    },
                                    theme.muted.color(),
                                    self.quality.preview == Some(buffer.view.id),
                                );
                                let label = self.tab_strip.label(index).to_owned();
                                let tooltip = buffer.page.info.path.as_deref().map_or_else(
                                    || "Unsaved file · included in recovery".to_owned(),
                                    |path| {
                                        self.workspace
                                            .remote
                                            .as_ref()
                                            .and_then(|remote| {
                                                remote.relative(Path::new(path)).ok().map(
                                                    |relative| {
                                                        format!(
                                                            "{}:{}/{}",
                                                            remote.config.target.host,
                                                            remote
                                                                .config
                                                                .root
                                                                .trim_end_matches('/'),
                                                            String::from_utf8_lossy(&relative)
                                                        )
                                                    },
                                                )
                                            })
                                            .unwrap_or_else(|| path.to_owned())
                                    },
                                );
                                let shown = ui.push_id(buffer.view.id, |ui| {
                                    egui::Frame::new()
                                        .fill(if active {
                                            theme.background.color()
                                        } else {
                                            theme.panel.color()
                                        })
                                        .corner_radius(0)
                                        .inner_margin(egui::Margin::symmetric(10, 3))
                                        .show(ui, |ui| {
                                            ui.push_id(buffer.page.info.id, |ui| {
                                                ui.horizontal(|ui| {
                                                    let response =
                                                        ui.add(button).on_hover_text(tooltip);
                                                    response.widget_info(|| {
                                                        egui::WidgetInfo::labeled(
                                                            egui::Role::Button,
                                                            ui.is_enabled(),
                                                            &label,
                                                        )
                                                    });
                                                    #[cfg(target_os = "macos")]
                                                    ui.ctx().accesskit_node_builder(
                                                        response.id,
                                                        |node| {
                                                            node.set_role(
                                                                egui::accesskit::Role::Tab,
                                                            );
                                                            node.set_selected(active);
                                                            node.set_label(label.clone());
                                                            if buffer.view.pinned
                                                                && buffer.page.info.dirty
                                                            {
                                                                node.set_description(
                                                                    "Pinned; unsaved changes",
                                                                );
                                                            } else if buffer.view.pinned {
                                                                node.set_description("Pinned");
                                                            } else if buffer.page.info.dirty {
                                                                node.set_description(
                                                                    "Unsaved changes",
                                                                );
                                                            }
                                                        },
                                                    );
                                                    if response.drag_started() {
                                                        self.quality.tab_drag =
                                                            Some(buffer.view.id);
                                                    }
                                                    if response.contains_pointer()
                                                        && ui.input(|i| i.pointer.any_released())
                                                    {
                                                        if let Some(id) =
                                                            self.quality.tab_drag.take()
                                                        {
                                                            reorder = Some((id, index));
                                                        }
                                                    }
                                                    if response.double_clicked() {
                                                        self.quality.preview = None;
                                                    }
                                                    if response.clicked() {
                                                        select = Some(index);
                                                    }
                                                    if response
                                                        .clicked_by(egui::PointerButton::Middle)
                                                        && can_close
                                                    {
                                                        close = Some(index);
                                                    }
                                                    response.context_menu(|ui| {
                                                        for command in [
                                                            Command::PinTab,
                                                            Command::CloseOthers,
                                                            Command::CloseRight,
                                                            Command::CopyPath,
                                                            Command::CopyFileLocation,
                                                            Command::RevealInSidebar,
                                                        ] {
                                                            if matches!(
                                                                command,
                                                                Command::CopyPath
                                                                    | Command::CopyFileLocation
                                                                    | Command::RevealInSidebar
                                                            ) && buffer.page.info.path.is_none()
                                                            {
                                                                continue;
                                                            }
                                                            if command == Command::CopyFileLocation
                                                                && buffer.page.start != 0
                                                            {
                                                                continue;
                                                            }
                                                            if ui.button(command.label()).clicked()
                                                            {
                                                                select = Some(index);
                                                                action = Some(command);
                                                                ui.close();
                                                            }
                                                        }
                                                        if ui
                                                            .add_enabled(
                                                                can_close,
                                                                egui::Button::new("Close tab"),
                                                            )
                                                            .clicked()
                                                        {
                                                            close = Some(index);
                                                            ui.close();
                                                        }
                                                        if ui.button("Name unsaved file…").clicked()
                                                        {
                                                            select = Some(index);
                                                            action = Some(Command::Rename);
                                                            ui.close();
                                                        }
                                                        if ui
                                                            .button("Browse closed files…")
                                                            .clicked()
                                                        {
                                                            action = Some(Command::Shelf);
                                                            ui.close();
                                                        }
                                                    });
                                                    if can_close {
                                                        let close_response =
                                                            controls::tab_close_button(
                                                                ui,
                                                                buffer.page.info.dirty,
                                                            )
                                                            .on_hover_text(format!(
                                                                "Close {}",
                                                                label
                                                            ));
                                                        close_response.widget_info(|| {
                                                            egui::WidgetInfo::labeled(
                                                                egui::Role::Button,
                                                                ui.is_enabled(),
                                                                format!("Close {}", label),
                                                            )
                                                        });
                                                        if close_response.clicked() {
                                                            close = Some(index);
                                                        }
                                                    }
                                                });
                                            });
                                        });
                                });
                                if active {
                                    ui.painter().hline(
                                        shown.response.rect.x_range(),
                                        shown.response.rect.top() + 0.5,
                                        egui::Stroke::new(1.0_f32, theme.accent.color()),
                                    );
                                }
                                if self.tab_strip.measured(index, shown.response.rect.width()) {
                                    ui.ctx().request_repaint();
                                }
                            }
                            ui.add_space(total - self.tab_strip.offset(visible.end));
                            if self.source_control.review_tab {
                                ui.horizontal(|ui| {
                                    let review =
                                        ui.selectable_label(self.source_control.open, "Git review");
                                    if std::mem::take(&mut self.source_control.reveal_review) {
                                        review.scroll_to_me(Some(egui::Align::Max));
                                    }
                                    if review.clicked() {
                                        self.activate_review();
                                        self.focus_editor = false;
                                    }
                                    if controls::close_button(ui, "Close Git review").clicked() {
                                        self.leave_review();
                                        self.source_control.review_tab = false;
                                        self.focus_editor = true;
                                    }
                                });
                            }

                            if ui
                                .add_enabled_ui(!self.busy && !self.closing, |ui| {
                                    hedit::icons::button(ui, hedit::icons::Icon::Add, "New file")
                                })
                                .inner
                                .on_hover_text(format!(
                                    "New file · {}",
                                    self.shortcut_text(Command::New)
                                ))
                                .clicked()
                            {
                                action = Some(Command::New);
                            }
                        });
                    });
            });
        if !self.busy && !self.closing {
            if let Some((id, target)) = reorder {
                let active = self.active().view.id;
                if let Some(source) = self.workspace.open.iter().position(|b| b.view.id == id) {
                    let buffer = self.workspace.open.remove(source);
                    self.workspace
                        .open
                        .insert(target.min(self.workspace.open.len()), buffer);
                    self.workspace.active = self
                        .workspace
                        .open
                        .iter()
                        .position(|b| b.view.id == active)
                        .unwrap_or(0);
                    self.changed();
                }
                self.quality.tab_drag = None;
            }
            if let Some(index) = select {
                self.select_tab(index);
                self.focus_editor = true;
            }
            if let Some(index) = close {
                self.request_close(vec![self.workspace.open[index].view.id]);
            }
            if let Some(command) = action {
                self.execute(ctx, command);
            }
        }
    }
    fn status_bar(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        if !self.settings.show_status_bar || self.distraction_free {
            return;
        }
        let font = (self.settings.ui_font_size - 1.0).max(10.0);
        let mut status_action = None;
        let theme = self.theme.clone();
        egui::Panel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(10, 0)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let right = (340.0 * font / 11.0).min(ui.available_width() * 0.75);
                    let left =
                        (ui.available_width() - right - ui.spacing().item_spacing.x).max(60.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(left, 0.0),
                        egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            let buffer = &self.workspace.open[self.workspace.active];
                            let source_index = self.projections[self.panes.focused]
                                .source_index(&buffer.page.text);
                            let (line, col) = source_index.line_column(
                                &buffer.page.text,
                                buffer.view.head,
                                buffer.view.tab_size.unwrap_or(4) as usize,
                            );
                            let (line, col) = (line + 1, col + 1);
                            ui.label(
                                RichText::new(format!(
                                    "{}Line {line}, Column {col}",
                                    if buffer.page.info.paged { "Page " } else { "" }
                                ))
                                .size(font),
                            )
                            .on_hover_text(format!(
                                "{} · {}",
                                buffer.view.name,
                                format_bytes(buffer.page.info.size)
                            ));
                            if let Some((characters, lines)) = source_index.selection_counts(
                                hedit::advanced::Selection {
                                    anchor: buffer.view.anchor,
                                    head: buffer.view.head,
                                },
                                &buffer.view.selections,
                            ) {
                                let character_label = if characters == 1 {
                                    "character"
                                } else {
                                    "characters"
                                };
                                let line_label = if lines == 1 { "line" } else { "lines" };
                                ui.label(
                                    RichText::new(format!(
                                        "{characters} {character_label} · {lines} {line_label}"
                                    ))
                                    .size(font),
                                );
                            }
                            if !buffer.view.selections.is_empty() {
                                ui.label(
                                    RichText::new(format!(
                                        "{} cursors",
                                        buffer.view.selections.len() + 1
                                    ))
                                    .size((font - 1.0).max(10.0))
                                    .color(theme.accent.color()),
                                );
                            }
                            if buffer.follow {
                                ui.label(
                                    RichText::new("READ ONLY · FOLLOWING")
                                        .size((font - 1.0).max(10.0))
                                        .color(theme.accent.color()),
                                );
                            }
                            if ui
                                .small_button(format!(
                                    "Notifications {}",
                                    self.notifications.unread()
                                ))
                                .clicked()
                            {
                                self.toggle_notifications();
                            }
                            if self.busy {
                                ui.spinner();
                                if ui.small_button("Cancel").clicked() {
                                    self.cancel.store(true, Ordering::Relaxed);
                                }
                            }
                            // Show each new notice briefly, like Sublime's status messages.
                            if self.notice_shown.0 != self.notice {
                                self.notice_shown = (self.notice.clone(), Instant::now());
                            }
                            let age = self.notice_shown.1.elapsed();
                            if age < NOTICE_TIME && self.notice != "Ready." {
                                ui.separator();
                                let width = ui.available_width().max(0.0);
                                ui.add_sized(
                                    [width, font + 8.0],
                                    egui::Label::new(
                                        RichText::new(&self.notice)
                                            .size(font)
                                            .color(theme.muted.color()),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&self.notice);
                                ctx.request_repaint_after(NOTICE_TIME - age);
                            }
                        },
                    );
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true),
                        |ui| {
                            let remote = self.workspace.remote.as_ref().map(|r| r.status());
                            let (label, color) = if let Some((connected, _, pending, _)) = &remote {
                                if !connected {
                                    (
                                        "Disconnected · pending edits remain in memory",
                                        theme.warning,
                                    )
                                } else if *pending > 0 {
                                    ("Waiting for durable SSH host receipts", theme.warning)
                                } else {
                                    ("Recovery durable on SSH host", theme.muted)
                                }
                            } else if self.closing {
                                ("Finishing recovery…", theme.warning)
                            } else if self.checkpoint_pending {
                                ("Checkpointing…", theme.warning)
                            } else if self.persisted == self.generation {
                                ("Recovery up to date", theme.muted)
                            } else {
                                ("Recovery pending", theme.warning)
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("●").size(9.0).color(color.color()),
                                    )
                                    .frame(false),
                                )
                                .on_hover_text(format!("{label} · Recovery settings"))
                                .clicked()
                            {
                                self.open_preferences(PreferencesPage::Recovery);
                            }
                            self.language_menu(ui);
                            if let Some(count) = self.plugin_services.summary() {
                                if ui.small_button(format!("Extensions · {count}")).clicked() {
                                    self.plugin_services.open = true;
                                }
                            }
                            self.language_status_button(ui);
                            self.status_menus(ui, &mut status_action);
                        },
                    );
                });
            });
        if let Some(command) = status_action {
            self.execute(ctx, command);
        }
    }
    /// Indentation, line ending, and encoding, each clickable, like Sublime.
    fn status_menus(&self, ui: &mut egui::Ui, action: &mut Option<Command>) {
        let muted = self.theme.muted.color();
        let small = |text: String| {
            egui::Button::new(
                RichText::new(text)
                    .size((self.settings.ui_font_size - 1.0).max(10.0))
                    .color(muted),
            )
            .frame(false)
        };
        let info = &self.active().page.info;
        if ui
            .add(small(format!(
                "{}{}",
                info.encoding,
                if info.bom { " BOM" } else { "" }
            )))
            .on_hover_text("Reopen or save with another encoding")
            .clicked()
        {
            *action = Some(Command::Encoding);
        }
        egui::menu::MenuButton::from_button(small(info.eol.to_owned()))
            .ui(ui, |ui| {
                for command in [Command::ToLf, Command::ToCrlf] {
                    if menu_item(ui, self, command) {
                        *action = Some(command);
                        ui.close();
                    }
                }
            })
            .0
            .on_hover_text("Line endings");
        let view = &self.active().view;
        let width = view.tab_size.unwrap_or(4);
        let indent = if view.indent.as_deref() == Some("tabs") {
            format!("Tab width: {width}")
        } else {
            format!("Spaces: {width}")
        };
        egui::menu::MenuButton::from_button(small(indent))
            .ui(ui, |ui| {
                ui.set_min_width(220.0);
                for command in [Command::IndentSpaces, Command::IndentTabs] {
                    if menu_item(ui, self, command) {
                        *action = Some(command);
                        ui.close();
                    }
                }
                ui.separator();
                for command in [Command::Indent2, Command::Indent4, Command::Indent8] {
                    if menu_item(ui, self, command) {
                        *action = Some(command);
                        ui.close();
                    }
                }
                ui.separator();
                for command in [Command::DetectIndent, Command::ToSpaces, Command::ToTabs] {
                    if menu_item(ui, self, command) {
                        *action = Some(command);
                        ui.close();
                    }
                }
            })
            .0
            .on_hover_text("Indentation");
    }
    /// Choose the active file's syntax (`None` detects it from the name) and
    /// apply that syntax's formatting rules.
    pub(super) fn set_syntax(&mut self, language: Option<syntax::Language>) {
        if self.workspace.open[self.workspace.active].set_language(language) {
            self.changed();
            self.apply_format_preferences();
        }
    }
    fn language_menu(&mut self, ui: &mut egui::Ui) {
        let buffer = self.active();
        let language = buffer.language();
        let detected = syntax::Language::detect(&buffer.page.info.name);
        let selected = buffer.language_override();
        let popup_id = egui::Id::unique("syntax-popup");
        if ui.input(|input| input.key_pressed(Key::Escape)) {
            egui::Popup::close_id(ui.ctx(), popup_id);
        }
        let response = ui.add(
            egui::Button::new(
                RichText::new(format!("{} ⏷", language.label()))
                    .size((self.settings.ui_font_size - 1.0).max(10.0))
                    .color(
                        if selected.is_some() {
                            self.theme.accent
                        } else {
                            self.theme.muted
                        }
                        .color(),
                    ),
            )
            .frame(false)
            .min_size(egui::vec2(90.0, 22.0))
            .selected(egui::Popup::is_id_open(ui.ctx(), popup_id)),
        );
        if response.clicked() {
            egui::Popup::toggle_id(ui.ctx(), popup_id);
        }
        let first_frame = self.syntax_focus;
        let mut choice = None;
        let height =
            (response.rect.top() - ui.ctx().viewport_rect().top() - 150.0).clamp(80.0, 480.0);
        egui::Popup::from_response(&response)
            .id(popup_id)
            .open_memory(None)
            .align(egui::emath::RectAlign::TOP_START)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let width = (ui.ctx().viewport_rect().width() - 40.0).clamp(240.0, 380.0);
                ui.set_width(width);
                ui.spacing_mut().interact_size.y = hedit::ui_style::Metrics::for_ui(ui).row;
                ui.style_mut()
                    .text_styles
                    .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
                ui.label(RichText::new("Syntax for this file").size(16.0).strong());
                let search_id = egui::Id::unique("syntax-filter");
                if first_frame {
                    ui.memory_mut(|memory| memory.request_focus(search_id));
                    if !ui.is_sizing_pass() {
                        self.syntax_focus = false;
                    }
                }
                let search = ui.add_sized(
                    [width, 30.0],
                    egui::TextEdit::singleline(&mut self.syntax_query)
                        .id(search_id)
                        .hint_text("Filter by language or extension…")
                        .font(egui::FontId::proportional(14.0)),
                );
                let query_changed = self.syntax_query != self.syntax_last_query;
                if query_changed {
                    self.syntax_matches = syntax::Language::ALL
                        .into_iter()
                        .filter(|language| language.matches_query(&self.syntax_query))
                        .collect();
                    self.syntax_last_query.clone_from(&self.syntax_query);
                }
                let matches = &self.syntax_matches;
                if search.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                    if matches.len() == 1 {
                        choice = Some(Some(matches[0]));
                    } else {
                        search.request_focus();
                    }
                }
                if ui
                    .selectable_label(
                        selected.is_none(),
                        format!("Automatic ({})", detected.label()),
                    )
                    .clicked()
                {
                    choice = Some(None);
                }
                ui.separator();
                let row_height = hedit::ui_style::Metrics::for_ui(ui).row;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("syntax-choices")
                    .max_height(height)
                    .auto_shrink([false, false]);
                if first_frame || query_changed {
                    let offset = if first_frame && self.syntax_query.is_empty() {
                        let index = matches
                            .iter()
                            .position(|&language| language == selected.unwrap_or(detected))
                            .unwrap_or(0);
                        (index as f32 * (row_height + ui.spacing().item_spacing.y) - height / 2.0)
                            .max(0.0)
                    } else {
                        0.0
                    };
                    scroll = scroll.vertical_scroll_offset(offset);
                }
                if matches.is_empty() {
                    scroll.show(ui, |ui| {
                        ui.label("No matching syntax");
                    });
                } else {
                    scroll.show_rows(ui, row_height, matches.len(), |ui, visible| {
                        for &language in &matches[visible] {
                            let row = ui
                                .push_id(language.id(), |ui| {
                                    ui.selectable_label(
                                        selected == Some(language),
                                        language.label(),
                                    )
                                })
                                .inner;
                            if row.clicked() {
                                choice = Some(Some(language));
                            }
                        }
                    });
                }
            });
        if let Some(language) = choice {
            egui::Popup::close_id(ui.ctx(), popup_id);
            self.set_syntax(language);
            self.focus_editor = true;
        }
        if !egui::Popup::is_id_open(ui.ctx(), popup_id) {
            self.syntax_query.clear();
            self.syntax_focus = true;
        }
        response.on_hover_text(format!(
            "Change syntax · {}\nApplies only to this file and is kept in recovery.",
            if selected.is_some() {
                "manually selected"
            } else {
                "detected from filename"
            },
        ));
    }
    fn buffer_sidebar(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        if !self.settings.show_sidebar || self.distraction_free {
            return;
        }
        if self.sidebar == workbench_navigation::Sidebar::Search {
            self.folder_search_sidebar(root);
            return;
        }
        if self.sidebar == workbench_navigation::Sidebar::SourceControl {
            self.source_control_sidebar(root);
            return;
        }
        let theme = self.theme.clone();
        egui::Panel::left("buffers")
            .default_size(220.0)
            .min_size(140.0)
            .max_size(360.0)
            .frame(
                egui::Frame::new()
                    .fill(theme.panel.color())
                    .inner_margin(egui::Margin::symmetric(8, 10)),
            )
            .show(root, |ui| {
                ui.set_min_width(ui.available_width());
                if self.sidebar == workbench_navigation::Sidebar::Remote {
                    self.ensure_remote_catalog(ctx);
                    if !self.settings.show_activity_bar
                        && self.workspace_header(ui, false) == Some(true)
                    {
                        self.show_git_sidebar(ctx);
                    }
                    self.remote_explorer(ui);
                    return;
                }
                if self.workspace_header(ui, false) == Some(true) {
                    self.show_git_sidebar(ctx);
                }
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(format!(
                                "CLOSED FILES    {}",
                                self.workspace.closed.len()
                            ))
                            .size(hedit::ui_style::Metrics::for_ui(ui).small)
                            .color(theme.muted.color()),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Browse and reopen closed files")
                    .clicked()
                {
                    self.shelf_open = true;
                }
                ui.add_enabled_ui(!self.busy && !self.closing, |ui| self.folder_tree(ui));
            });
    }
    pub(super) fn search_panel(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        self.poll_live_find(ctx);
        if !self.find_open {
            return;
        }
        egui::Panel::bottom("find-panel")
            .frame(
                egui::Frame::new()
                    .fill(self.theme.surface.color())
                    .inner_margin(egui::Margin::symmetric(10, 6)),
            )
            .show(root, |ui| {
                // Wrap whole controls; wrapping checkbox text into the remaining
                // few pixels can grow a narrow panel beyond the viewport.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Find");
                    let width = (ui.available_width() - 225.0).max(100.0);
                    let input_id = egui::Id::unique(FIND_INPUT);
                    let focused = ctx.memory(|m| m.has_focus(input_id));
                    // Shift+Enter searches backwards, like Sublime Text.
                    let previous =
                        focused && ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter));
                    let enter =
                        focused && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                    let response = bidi_view::field(ui, &mut self.query, |edit| {
                        edit.id(input_id)
                            .desired_width(width)
                            .hint_text("Search text or regular expression")
                    });
                    if response.changed() {
                        self.find_as_you_type(ctx);
                    }
                    if self.find_focus {
                        response.request_focus();
                        self.find_focus = false;
                    }
                    wrap_search_control(ui, "Find next", 0.0);
                    let clicked = ui
                        .add_enabled(!self.busy && !self.closing, egui::Button::new("Find next"))
                        .clicked();
                    if (clicked || enter) && !self.busy && !self.closing {
                        self.find(ctx);
                    } else if previous && !self.busy && !self.closing {
                        self.find_advanced(ctx, true);
                    }
                    ui.toggle_value(&mut self.replace_open, "Replace");
                    if controls::close_button(ui, "Close search · Esc").clicked() {
                        self.find_open = false;
                        self.focus_editor = true;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    let options = [
                        ui.checkbox(&mut self.quality.options.regex, "Regex"),
                        ui.checkbox(&mut self.quality.options.case_sensitive, "Case sensitive"),
                        ui.checkbox(&mut self.quality.options.whole_word, "Whole words"),
                    ];
                    if options.iter().any(|r| r.changed()) {
                        self.find_as_you_type(ctx);
                    }
                    wrap_search_control(
                        ui,
                        "Selection only",
                        ui.spacing().icon_width + ui.spacing().icon_spacing,
                    );
                    let selection_changed = ui.add_enabled_ui(self.quality.scope.is_some(), |ui| {
                        ui.checkbox(&mut self.quality.selection_only, "Selection only")
                            .changed()
                    });
                    if selection_changed.inner {
                        self.find_as_you_type(ctx);
                    }
                    wrap_search_control(ui, "Previous", 0.0);
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Previous"))
                        .clicked()
                    {
                        self.find_advanced(ctx, true);
                    }
                    let status = ui.label(&self.quality.match_status);
                    if let Some(error) = &self.live_find.error {
                        status.on_hover_text(error);
                    }
                });
                if self.replace_open {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("With");
                        let buttons = ["Replace selected", "Replace all"].map(|label| {
                            egui::WidgetText::from(label).into_galley(
                                ui,
                                Some(egui::TextWrapMode::Extend),
                                f32::INFINITY,
                                egui::TextStyle::Button,
                            )
                        });
                        let button_width: f32 = buttons
                            .iter()
                            .map(|text| text.size().x + 2.0 * ui.spacing().button_padding.x)
                            .sum();
                        let margin = egui::Margin::symmetric(4, 2);
                        let width = (ui.available_width()
                            - button_width
                            - 2.0 * ui.spacing().item_spacing.x
                            - margin.sum().x)
                            .max(100.0);
                        let response = bidi_view::field(ui, &mut self.replacement, |edit| {
                            edit.desired_width(width)
                                .margin(margin)
                                .hint_text("Replacement text")
                        });
                        if response.lost_focus()
                            && ui.input(|i| i.key_pressed(Key::Enter))
                            && !self.busy
                            && !self.closing
                        {
                            self.find(ctx);
                            response.request_focus();
                        }
                        wrap_search_control(ui, "Replace selected", 0.0);
                        if ui
                            .add_enabled(
                                self.command_enabled(Command::Replace),
                                egui::Button::new(buttons[0].clone()),
                            )
                            .clicked()
                        {
                            self.replace_selection();
                            self.focus_editor = false;
                        }
                        wrap_search_control(ui, "Replace all", 0.0);
                        if ui
                            .add_enabled(
                                self.command_enabled(Command::Replace),
                                egui::Button::new(buttons[1].clone()),
                            )
                            .clicked()
                        {
                            self.replace_advanced(true);
                            self.focus_editor = false;
                        }
                    });
                }
            });
    }
}

const NOTICE_TIME: std::time::Duration = std::time::Duration::from_secs(5);

fn command_group(
    ui: &mut egui::Ui,
    app: &HeditApp,
    title: &str,
    commands: &[Command],
    action: &mut Option<Command>,
) {
    ui.menu_button(title, |ui| {
        ui.set_min_width(250.0);
        for &command in commands {
            if menu_item(ui, app, command) {
                *action = Some(command);
                ui.close();
            }
        }
    });
}
impl HeditApp {
    fn edit_menu(&self, ui: &mut egui::Ui, action: &mut Option<Command>) {
        for command in [Command::Undo, Command::Redo] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close();
            }
        }
        ui.separator();
        for command in [
            Command::Cut,
            Command::Copy,
            Command::Paste,
            Command::PasteIndent,
            Command::PasteHistory,
        ] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close();
            }
        }
        ui.separator();
        command_group(
            ui,
            self,
            "Selection",
            &[
                Command::SelectAll,
                Command::SelectLine,
                Command::NextOccurrence,
                Command::SkipOccurrence,
                Command::AllOccurrences,
                Command::SplitLines,
                Command::AddCursorAbove,
                Command::AddCursorBelow,
                Command::Rectangle,
                Command::SingleSelection,
                Command::ExpandSelection,
                Command::ExpandToBrackets,
                Command::ExpandToIndentation,
                Command::ExpandToTag,
                Command::JumpToBracket,
                Command::SoftUndo,
                Command::SoftRedo,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Line",
            &[
                Command::Duplicate,
                Command::DeleteLine,
                Command::InsertBefore,
                Command::InsertAfter,
                Command::MoveUp,
                Command::MoveDown,
                Command::JoinLines,
                Command::DeleteToLineStart,
                Command::DeleteToLineEnd,
                Command::SortLines,
                Command::SortLinesIgnoreCase,
                Command::ReverseLines,
                Command::UniqueLines,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Text",
            &[
                Command::Uppercase,
                Command::Lowercase,
                Command::TitleCase,
                Command::SwapCase,
                Command::Transpose,
                Command::WrapLines,
                Command::Reindent,
                Command::TrimWhitespace,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Line endings",
            &[Command::ToLf, Command::ToCrlf],
            action,
        );
        ui.menu_button("Indentation", |ui| {
            egui::ScrollArea::vertical()
                .id_salt("indent-menu-scroll")
                .max_height((ui.ctx().viewport_rect().height() - 70.0).max(140.0))
                .show(ui, |ui| {
                    for commands in [
                        &[Command::Indent, Command::Unindent][..],
                        &[Command::IndentSpaces, Command::IndentTabs][..],
                        &[Command::Indent2, Command::Indent4, Command::Indent8][..],
                        &[Command::ToSpaces, Command::ToTabs][..],
                        &[
                            Command::AutoIndent,
                            Command::SmartIndent,
                            Command::SoftTabs,
                            Command::DetectIndent,
                        ][..],
                    ] {
                        for &command in commands {
                            if menu_item(ui, self, command) {
                                *action = Some(command);
                                ui.close();
                            }
                        }
                        ui.separator();
                    }
                });
        });
        ui.separator();
        for command in [Command::ToggleComment, Command::ToggleBlockComment] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close();
            }
        }
    }
    fn view_menu(&self, ui: &mut egui::Ui, action: &mut Option<Command>) {
        command_group(
            ui,
            self,
            "Markdown",
            &[
                Command::MarkdownSource,
                Command::MarkdownSplit,
                Command::MarkdownPreview,
                Command::MarkdownRefresh,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Layout",
            &[
                Command::SinglePane,
                Command::SplitColumns,
                Command::Columns3,
                Command::Columns4,
                Command::SplitRows,
                Command::Rows3,
                Command::Grid,
                Command::FocusPane,
                Command::MovePane,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Code folding",
            &[
                Command::Fold,
                Command::Unfold,
                Command::FoldAll,
                Command::FoldLevel1,
                Command::FoldLevel2,
                Command::FoldLevel3,
                Command::UnfoldAll,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Side bar",
            &[
                Command::FocusExplorer,
                Command::FocusSearch,
                Command::Sidebar,
                Command::FolderSidebar,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Panel",
            &[
                Command::Notifications,
                Command::TogglePanel,
                Command::MaximizePanel,
                Command::FocusEditor,
                Command::FocusTerminal,
                Command::LanguageProblems,
                Command::BuildResults,
                Command::Terminal,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Editor",
            &[
                Command::Tabs,
                Command::Breadcrumbs,
                Command::Outline,
                Command::StatusBar,
                Command::LineNumbers,
                Command::CurrentLine,
                Command::Whitespace,
                Command::IndentGuides,
                Command::Minimap,
                Command::Wrap,
                Command::ToggleSpellCheck,
                Command::NextMisspelling,
                Command::PreviousMisspelling,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Font size",
            &[Command::ZoomIn, Command::ZoomOut, Command::ResetZoom],
            action,
        );
        command_group(
            ui,
            self,
            "Ruler",
            &[
                Command::RulerNone,
                Command::Ruler80,
                Command::Ruler100,
                Command::Ruler120,
            ],
            action,
        );
        command_group(
            ui,
            self,
            "Word wrap column",
            &[
                Command::WrapWindow,
                Command::Wrap80,
                Command::Wrap100,
                Command::Wrap120,
            ],
            action,
        );
        ui.separator();
        for command in [
            Command::Fullscreen,
            Command::DistractionFree,
            Command::Follow,
        ] {
            if menu_item(ui, self, command) {
                *action = Some(command);
                ui.close();
            }
        }
    }
}

// Enabled scopes paint their child before the wrapping parent allocates it.
// Move the cursor first so compound controls never paint past the right edge.
fn wrap_search_control(ui: &mut egui::Ui, label: &str, extra: f32) {
    let width = egui::WidgetText::from(label)
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Button,
        )
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x
        + extra;
    if ui.available_size_before_wrap().x < width {
        ui.end_row();
    }
}
