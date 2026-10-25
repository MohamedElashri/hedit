//! Folder search: fixed controls and footer, with one bounded results area.
use super::*;
use hedit::search;

pub(super) const FIELDS: [&str; 3] = ["folders-find", "folders-replace", "folders-filter"];
const ROW_HEIGHT: f32 = 26.0;

#[derive(Default)]
pub(super) enum State {
    #[default]
    Idle,
    Searching,
    Ready,
    Cancelled,
    Failed(String),
    Applied(String),
}

fn field(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    text: &mut String,
    hint: &str,
) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add_sized(
            [48.0, 24.0],
            egui::Label::new(label).halign(egui::Align::LEFT),
        );
        ui.add_sized(
            [ui.available_width(), 24.0],
            egui::TextEdit::singleline(text)
                .id(egui::Id::new(id))
                .hint_text(hint),
        )
    })
    .inner
}

fn file_row(ui: &mut egui::Ui, theme: &Theme, label: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ROW_HEIGHT),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if selected {
        theme.selection.color()
    } else if response.hovered() {
        theme.elevated.color()
    } else {
        theme.panel.color()
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            if selected {
                theme.selection_text.color()
            } else {
                theme.text.color()
            },
        );
    response
}

impl HeditApp {
    pub(super) fn folder_preview_valid(&self) -> bool {
        self.quality
            .preview_query
            .as_ref()
            .is_some_and(|(q, r, f, o)| {
                q == &self.query
                    && r == &self.replacement
                    && f == &self.quality.filter
                    && o == &self.quality.options
            })
    }

    pub(super) fn folder_search_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut run = false;
        let mut apply = false;
        let mut undo = false;
        let mut navigate = None;
        let mut choose_folder = false;
        let searching = matches!(self.quality.folder_state, State::Searching);
        let roots: Vec<PathBuf> = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect();
        let size = egui::vec2(
            (ctx.screen_rect().width() - 64.0).min(900.0),
            (ctx.screen_rect().height() - 100.0).min(620.0),
        );
        egui::Window::new("Find in folders")
            .id(egui::Id::new("folder-search"))
            .open(&mut open).collapsible(false)
            .default_size(size).min_size([560.0, 300.0])
            .max_size(ctx.screen_rect().size() - egui::vec2(40.0, 70.0))
            .default_pos(ctx.screen_rect().center() - size * 0.5)
            .frame(egui::Frame::window(&ctx.style()).inner_margin(12.0).corner_radius(8))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_sized([48.0, 26.0], egui::Label::new("Find").halign(egui::Align::LEFT));
                    let response = ui.add_sized([(ui.available_width() - 106.0).max(120.0), 26.0],
                        egui::TextEdit::singleline(&mut self.query).id(egui::Id::new(FIELDS[0]))
                            .hint_text("Search text or regular expression"));
                    if self.quality.folder_focus {
                        response.request_focus();
                        self.quality.folder_focus = false;
                    }
                    if searching {
                        if ui.add_sized([98.0, 28.0], egui::Button::new("Cancel search")).clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                        }
                    } else {
                        run = controls::primary_button(ui, &self.theme,
                            if self.quality.folder_replace { "Preview" } else { "Find all" },
                            !self.busy && !self.closing && !self.query.is_empty() && !roots.is_empty()).clicked();
                    }
                });
                if self.quality.folder_replace {
                    field(ui, "Replace", FIELDS[1], &mut self.replacement, "Replacement text (empty deletes matches)");
                }
                field(ui, "Files", FIELDS[2], &mut self.quality.filter, "All files, or patterns such as *.rs, *.cu");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.quality.options.regex, "Regex");
                    ui.checkbox(&mut self.quality.options.case_sensitive, "Match case");
                    ui.checkbox(&mut self.quality.options.whole_word, "Whole words");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.toggle_value(&mut self.quality.folder_replace, "Replace");
                    });
                });
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if searching {
                        ui.spinner();
                        ui.strong("Searching…");
                    } else if matches!(self.quality.folder_state, State::Ready) {
                        let results = &self.quality.results;
                        ui.strong(format!("{} {} in {} {}", results.hits.len(),
                            if results.hits.len() == 1 { "match" } else { "matches" }, results.files.len(),
                            if results.files.len() == 1 { "file" } else { "files" }));
                        if results.truncated {
                            ui.colored_label(self.theme.warning.color(), "Result limit reached");
                        }
                    } else {
                        ui.strong("Results");
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let has_hits = !self.quality.results.hits.is_empty() && !self.busy;
                        if ui.add_enabled(has_hits, egui::Button::new("Next")).on_hover_text("F4").clicked() {
                            navigate = Some(true);
                        }
                        if ui.add_enabled(has_hits, egui::Button::new("Previous")).on_hover_text("Shift+F4").clicked() {
                            navigate = Some(false);
                        }
                        if !self.quality.folder_replace
                            && ui.checkbox(&mut self.quality.folder_context, "Context").changed() {
                            self.quality.result_shown = None;
                        }
                    });
                });
                if !self.quality.folder_replace && matches!(self.quality.folder_state, State::Ready)
                    && self.quality.folder_snapshot.as_ref().is_some_and(|(q, f, o)| q != &self.query || f != &self.quality.filter || o != &self.quality.options) {
                    ui.colored_label(self.theme.warning.color(), "Search changed. Find all to refresh results.");
                }
                // The footer never scrolls out of view, even with long results.
                let height = (ui.available_height() - 43.0).max(80.0);
                egui::Frame::new().fill(self.theme.background.color())
                    .stroke(egui::Stroke::new(1.0, self.theme.border.color()))
                    .corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                        ui.set_min_height((height - 12.0).max(60.0));
                        let body_height = (height - 12.0).max(60.0);
                        if self.quality.folder_replace && !self.quality.results.plans.is_empty() {
                            self.replacement_review(ui, &roots, body_height);
                        } else if !self.quality.results.files.is_empty() {
                            self.folder_result_rows(ui, &roots, body_height);
                        } else {
                            egui::ScrollArea::vertical().id_salt("folder-empty").max_height(body_height).show(ui, |ui| {
                                ui.add_space(18.0);
                                let (title, detail) = match &self.quality.folder_state {
                                    State::Searching => ("Searching folders…", "Results will appear here."),
                                    State::Failed(error) => ("Search failed", error.as_str()),
                                    State::Cancelled => ("Search cancelled", "Change the query or search again."),
                                    State::Applied(message) => ("Files updated", message.as_str()),
                                    State::Ready => ("No matches found", "Try a different query or file filter."),
                                    State::Idle if roots.is_empty() => ("No folders open", "Open a folder to search its files."),
                                    State::Idle => ("Find across your folders", "Enter a query and choose Find all."),
                                };
                                ui.label(RichText::new(title).strong());
                                ui.add(egui::Label::new(RichText::new(detail).color(self.theme.muted.color())).wrap());
                                if roots.is_empty() && !searching {
                                    ui.add_space(8.0);
                                    choose_folder = ui.button("Open folder…").clicked();
                                }
                            });
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let scope = match roots.as_slice() {
                        [] => "No folders open".into(),
                        [root] => root.file_name().unwrap_or(root.as_os_str()).to_string_lossy().into_owned(),
                        roots => format!("{} folders", roots.len()),
                    };
                    let scope = format!("{scope} · {} skipped", self.quality.results.skipped);
                    ui.add(egui::Label::new(RichText::new(scope).small().color(self.theme.muted.color())).truncate())
                        .on_hover_text(format!("{}\n\nRespects .gitignore. Includes unsaved text in open files. Binary files and files over 16 MiB are skipped.",
                            roots.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.quality.folder_replace && !self.quality.results.plans.is_empty() {
                            let count = self.quality.results.plans.iter().filter(|p| p.selected).count();
                            apply = controls::primary_button(ui, &self.theme, &format!("Apply to {count} {}", if count == 1 { "file" } else { "files" }),
                                !self.busy && !self.closing && self.folder_preview_valid() && !self.quality.results.truncated && count > 0).clicked();
                        }
                        if !self.quality.replace_undo.is_empty() {
                            undo = ui.add_enabled(!self.busy && !self.closing, egui::Button::new("Undo replacements")).clicked();
                        }
                    });
                });
            });
        self.quality.folders_open = open;
        if !open {
            self.focus_editor = true;
        }
        if run {
            self.folder_search(ctx, self.quality.folder_replace);
        }
        if apply {
            self.apply_folder_replacements(ctx);
        }
        if undo {
            self.undo_folder_replacements(ctx);
        }
        if let Some(forward) = navigate {
            self.quality.results_newer = true;
            self.next_result(forward);
        }
        if choose_folder {
            self.open_folder(ctx);
        }
    }

    fn folder_result_rows(&mut self, ui: &mut egui::Ui, roots: &[PathBuf], height: f32) {
        enum Row {
            File(usize),
            Line(usize, usize),
        }
        let current = self
            .quality
            .result_current
            .and_then(|i| self.quality.results.hits.get(i))
            .map(|h| (h.path.clone(), h.line));
        let scroll = self.quality.result_current != self.quality.result_shown;
        self.quality.result_shown = self.quality.result_current;
        if scroll {
            if let Some((path, _)) = &current {
                self.quality.folder_collapsed.remove(path);
            }
        }
        let files = &self.quality.results.files;
        let mut rows = vec![];
        let mut target = None;
        for (f, file) in files.iter().enumerate() {
            rows.push(Row::File(f));
            if self.quality.folder_collapsed.contains(&file.path) {
                continue;
            }
            for (l, line) in file.lines.iter().enumerate() {
                if !self.quality.folder_context && line.column.is_none() {
                    continue;
                }
                if current
                    .as_ref()
                    .is_some_and(|(p, n)| p == &file.path && *n == line.line)
                {
                    target = Some(rows.len());
                }
                rows.push(Row::Line(f, l));
            }
        }
        let regex = self.quality.folder_regex.as_ref();
        let mut area = egui::ScrollArea::vertical()
            .id_salt("folder-hits")
            .max_height(height)
            .auto_shrink([false, false]);
        if scroll {
            if let Some(row) = target {
                area =
                    area.vertical_scroll_offset((row as f32 * ROW_HEIGHT - height * 0.35).max(0.0));
            }
        }
        let mut clicked = None;
        ui.spacing_mut().item_spacing.y = 0.0;
        area.show_rows(ui, ROW_HEIGHT, rows.len(), |ui, visible| {
            for row in &rows[visible] {
                match *row {
                    Row::File(f) => {
                        let file = &files[f];
                        let collapsed = self.quality.folder_collapsed.contains(&file.path);
                        let name = hedit::folder::display_path(roots, &file.path);
                        let label = format!(
                            "{} {name}  ·  {} {}",
                            if collapsed { "▸" } else { "▾" },
                            file.count,
                            if file.count == 1 { "match" } else { "matches" }
                        );
                        let response = file_row(ui, &self.theme, &label, false);
                        if response.clicked() {
                            if collapsed {
                                self.quality.folder_collapsed.remove(&file.path);
                            } else {
                                self.quality.folder_collapsed.insert(file.path.clone());
                            }
                        }
                        response.on_hover_text(file.path.display().to_string());
                    }
                    Row::Line(f, l) => {
                        let file = &files[f];
                        let line = &file.lines[l];
                        let selected = current
                            .as_ref()
                            .is_some_and(|(p, n)| p == &file.path && *n == line.line);
                        let (rect, response) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), ROW_HEIGHT),
                            egui::Sense::click(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &line.text)
                        });
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                2.0,
                                if selected {
                                    self.theme.selection.color()
                                } else {
                                    self.theme.elevated.color()
                                },
                            );
                        }
                        let number = egui::Rect::from_min_max(
                            rect.min,
                            egui::pos2(rect.left() + 48.0, rect.bottom()),
                        );
                        ui.painter().text(
                            number.right_center() - egui::vec2(8.0, 0.0),
                            egui::Align2::RIGHT_CENTER,
                            line.line.to_string(),
                            FontId::monospace(12.0),
                            self.theme.muted.color(),
                        );
                        let text_rect = egui::Rect::from_min_max(
                            egui::pos2(number.right(), rect.top()),
                            rect.max,
                        );
                        let color = if selected {
                            self.theme.selection_text.color()
                        } else if line.column.is_none() {
                            self.theme.muted.color()
                        } else {
                            self.theme.text.color()
                        };
                        let job = highlighted(
                            &line.text,
                            regex.filter(|_| line.column.is_some()),
                            &self.theme,
                            color,
                        );
                        let galley = ui.painter().layout_job(job);
                        ui.painter()
                            .with_clip_rect(text_rect.intersect(ui.clip_rect()))
                            .galley(
                                egui::pos2(
                                    text_rect.left(),
                                    text_rect.center().y - galley.size().y * 0.5,
                                ),
                                galley,
                                color,
                            );
                        if response.clicked() {
                            clicked = Some((file.path.clone(), line.line, line.column));
                        }
                        response.on_hover_text(&line.text);
                    }
                }
            }
        });
        if let Some((path, line, column)) = clicked {
            let hit = self
                .quality
                .results
                .hits
                .iter()
                .position(|h| h.path == path && h.line == line);
            self.quality.result_current = hit;
            self.quality.result_shown = hit;
            self.quality.results_newer = true;
            self.pending_paths.push_back(PathBuf::from(format!(
                "{}:{line}:{}",
                path.display(),
                column.unwrap_or(1)
            )));
        }
    }

    fn replacement_review(&mut self, ui: &mut egui::Ui, roots: &[PathBuf], height: f32) {
        if self.quality.result_current != self.quality.result_shown {
            if let Some(hit) = self
                .quality
                .result_current
                .and_then(|i| self.quality.results.hits.get(i))
            {
                self.quality.preview_diff = self
                    .quality
                    .results
                    .plans
                    .iter()
                    .position(|p| p.path == hit.path);
            }
            self.quality.result_shown = self.quality.result_current;
        }
        let valid = self.folder_preview_valid();
        let mut header_height = 24.0;
        if !valid || self.quality.results.truncated {
            ui.colored_label(
                self.theme.warning.color(),
                if !valid {
                    "Settings changed. Preview again before applying."
                } else {
                    "Preview is incomplete. Narrow the search before applying."
                },
            );
            header_height += 24.0;
        }
        ui.horizontal(|ui| {
            let mut all = self.quality.results.plans.iter().all(|p| p.selected);
            if ui.checkbox(&mut all, "Select all files").changed() {
                for plan in &mut self.quality.results.plans {
                    plan.selected = all;
                }
            }
            ui.weak("Open files stay unsaved.").on_hover_text(
                "Other files are updated on disk. Undo replacements restores those files.",
            );
        });
        let height = (height - header_height).max(60.0);
        let width = ui.available_width();
        let left = (width * 0.34).clamp(170.0, 280.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(left, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left);
                    egui::ScrollArea::vertical()
                        .id_salt("replacement-files")
                        .max_height(height)
                        .max_width(left)
                        .auto_shrink([false, false])
                        .show_rows(
                            ui,
                            ROW_HEIGHT,
                            self.quality.results.plans.len(),
                            |ui, rows| {
                                for i in rows {
                                    let plan = &mut self.quality.results.plans[i];
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut plan.selected, "");
                                        let label = hedit::folder::display_path(roots, &plan.path);
                                        if file_row(
                                            ui,
                                            &self.theme,
                                            &label,
                                            self.quality.preview_diff == Some(i),
                                        )
                                        .on_hover_text(plan.path.display().to_string())
                                        .clicked()
                                        {
                                            self.quality.preview_diff = Some(i);
                                        }
                                    });
                                }
                            },
                        );
                },
            );
            let (divider, _) =
                ui.allocate_exact_size(egui::vec2(1.0, height), egui::Sense::hover());
            ui.painter().line_segment(
                [divider.center_top(), divider.center_bottom()],
                egui::Stroke::new(1.0, self.theme.border.color()),
            );
            ui.vertical(|ui| {
                ui.set_width((width - left - 20.0).max(100.0));
                let index = *self.quality.preview_diff.get_or_insert(0);
                if let Some(plan) = self.quality.results.plans.get(index) {
                    ui.add(
                        egui::Label::new(
                            RichText::new(hedit::folder::display_path(roots, &plan.path)).strong(),
                        )
                        .truncate(),
                    );
                    if self
                        .quality
                        .preview_rendered
                        .as_ref()
                        .is_none_or(|(i, _)| *i != index)
                    {
                        let display = PathBuf::from(hedit::folder::display_path(roots, &plan.path));
                        self.quality.preview_rendered =
                            Some((index, search::diff(&plan.before, &plan.after, &display)));
                    }
                    let diff = &self.quality.preview_rendered.as_ref().unwrap().1;
                    egui::ScrollArea::both()
                        .id_salt("replacement-diff")
                        .max_width((width - left - 20.0).max(100.0))
                        .max_height((height - 24.0).max(40.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add(
                                egui::Label::new(diff_job(diff, &self.theme))
                                    .extend()
                                    .selectable(true),
                            );
                        });
                }
            });
        });
    }
}

fn highlighted(
    text: &str,
    regex: Option<&regex::Regex>,
    theme: &Theme,
    color: egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let normal = egui::TextFormat {
        font_id: FontId::monospace(12.0),
        color,
        ..Default::default()
    };
    let display = hedit::bidi::display(text);
    let mut start = 0;
    if let Some(regex) = regex {
        for found in regex.find_iter(&display) {
            job.append(&display[start..found.start()], 0.0, normal.clone());
            job.append(
                found.as_str(),
                0.0,
                egui::TextFormat {
                    background: theme.accent.color().gamma_multiply(0.22),
                    ..normal.clone()
                },
            );
            start = found.end();
        }
    }
    job.append(&display[start..], 0.0, normal);
    job
}

fn diff_job(diff: &str, theme: &Theme) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for line in diff.split_inclusive('\n') {
        let color = if line.starts_with('+') && !line.starts_with("+++") {
            theme.accent.color()
        } else if line.starts_with('-') && !line.starts_with("---") {
            theme.error.color()
        } else if line.starts_with("@@") {
            theme.muted.color()
        } else {
            theme.text.color()
        };
        job.append(
            line,
            0.0,
            egui::TextFormat {
                font_id: FontId::monospace(12.0),
                color,
                ..Default::default()
            },
        );
    }
    job
}
