//! One bounded, cached diagnostic catalogue shared by Problems and Explorer.
use super::*;
const LIMIT: usize = 10_000;
const BYTE_LIMIT: usize = 8 * 1024 * 1024;
#[derive(Clone, Copy)]
enum Position {
    Language(Encoding, text::Position, u64),
    Build(usize, Option<usize>),
}
#[derive(Clone)]
pub(super) struct ProblemRow {
    path: PathBuf,
    source: String,
    severity: u8,
    message: String,
    line: u64,
    column: usize,
    position: Position,
}
#[derive(Clone, Default, PartialEq, Eq)]
struct Filter {
    text: String,
    severity: u8,
    source: String,
    current: bool,
    path: Option<PathBuf>,
}
pub(super) struct State {
    filter: Filter,
    grouped: bool,
    rows: Option<Arc<Vec<ProblemRow>>>,
    filtered_by: Filter,
    indices: Vec<usize>,
    sources: Vec<String>,
    display: Vec<(usize, bool)>,
    display_grouped: bool,
    counts: [usize; 4],
    current: Option<(PathBuf, String, u64, usize, String)>,
    limited: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            filter: Filter::default(),
            grouped: true,
            rows: None,
            filtered_by: Filter::default(),
            indices: vec![],
            sources: vec![],
            display: vec![],
            display_grouped: true,
            counts: [0; 4],
            current: None,
            limited: false,
        }
    }
}
impl ProblemRow {
    fn key(&self) -> (PathBuf, String, u64, usize, String) {
        (
            self.path.clone(),
            self.source.clone(),
            self.line,
            self.column,
            self.message.clone(),
        )
    }
    fn label(&self) -> String {
        format!(
            "{} · {} · {}:{} · {}",
            severity_name(self.severity),
            self.source,
            self.path.file_name().unwrap_or_default().to_string_lossy(),
            self.line,
            self.message
        )
    }
}
fn severity_name(value: u8) -> &'static str {
    match value {
        1 => "Error",
        2 => "Warning",
        4 => "Hint",
        _ => "Information",
    }
}
fn message(value: &str) -> String {
    value
        .chars()
        .take(500)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
fn build_severity(value: &str) -> u8 {
    let s = value.trim_start().to_ascii_lowercase();
    if s.starts_with("error") || s.starts_with("fatal error") {
        1
    } else if s.starts_with("warning") {
        2
    } else {
        3
    }
}
impl HeditApp {
    fn cached_problems(&mut self) -> Arc<Vec<ProblemRow>> {
        let build_key = (self.build.problems_revision, self.build.locations.len());
        if self.languages.problem_rows.is_none() || self.languages.problem_build_key != build_key {
            let mut rows = Vec::new();
            let mut bytes = 0usize;
            let mut limited = false;
            let mut push = |row: ProblemRow| {
                let size = row.path.as_os_str().len() + row.source.len() + row.message.len();
                if rows.len() >= LIMIT || bytes.saturating_add(size) > BYTE_LIMIT {
                    limited = true;
                    return;
                }
                bytes += size;
                rows.push(row);
            };
            for (path, sources) in &self.languages.diagnostics {
                for diagnostics in sources {
                    for item in &diagnostics.items {
                        let Ok(range) =
                            serde_json::from_value::<text::Range>(item["range"].clone())
                        else {
                            continue;
                        };
                        if (range.end.line, range.end.character)
                            < (range.start.line, range.start.character)
                        {
                            continue;
                        }
                        push(ProblemRow {
                            path: path.clone(),
                            source: diagnostics.server.clone(),
                            severity: item["severity"]
                                .as_u64()
                                .filter(|v| (1..=4).contains(v))
                                .unwrap_or(3) as u8,
                            message: message(item["message"].as_str().unwrap_or("")),
                            line: u64::from(range.start.line) + 1,
                            column: range.start.character as usize + 1,
                            position: Position::Language(
                                diagnostics.encoding,
                                range.start,
                                diagnostics.revision,
                            ),
                        });
                    }
                }
            }
            for location in &self.build.locations {
                push(ProblemRow {
                    path: location.file.clone(),
                    source: "Build".into(),
                    severity: build_severity(&location.message),
                    message: message(&location.message),
                    line: location.line as u64,
                    column: location.column.unwrap_or(1),
                    position: Position::Build(location.line, location.column),
                });
            }
            rows.sort_by(|a, b| {
                (&a.path, a.line, &a.source, &a.message)
                    .cmp(&(&b.path, b.line, &b.source, &b.message))
            });
            self.languages.problem_rows = Some(Arc::new(rows));
            self.languages.problem_build_key = build_key;
            self.languages.explorer_badges = None;
            self.languages.problems_ui.limited = limited;
        }
        self.languages.problem_rows.as_ref().unwrap().clone()
    }
    fn filtered_problems(&mut self) -> Arc<Vec<ProblemRow>> {
        let rows = self.cached_problems();
        let path = self.active().page.info.path.as_ref().map(PathBuf::from);
        let state = &mut self.languages.problems_ui;
        state.filter.path = if state.filter.current { path } else { None };
        let changed = state.rows.as_ref().is_none_or(|r| !Arc::ptr_eq(r, &rows));
        if changed {
            state.sources = rows.iter().map(|r| r.source.clone()).collect();
            state.sources.sort();
            state.sources.dedup();
            state.counts = [0; 4];
            for r in rows.iter() {
                state.counts[usize::from(r.severity - 1)] += 1;
            }
        }
        if changed || state.filter != state.filtered_by {
            let query = state.filter.text.to_lowercase();
            state.indices = rows
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    (state.filter.severity == 0 || r.severity == state.filter.severity)
                        && (state.filter.source.is_empty() || r.source == state.filter.source)
                        && (!state.filter.current || state.filter.path.as_ref() == Some(&r.path))
                        && (query.is_empty()
                            || r.message.to_lowercase().contains(&query)
                            || r.path.to_string_lossy().to_lowercase().contains(&query)
                            || r.source.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
            state.filtered_by = state.filter.clone();
            state.rows = Some(rows.clone());
            state.display_grouped = !state.grouped;
        }
        if state.display_grouped != state.grouped {
            state.display.clear();
            let mut previous = None;
            for &index in &state.indices {
                let row = &rows[index];
                if state.grouped && previous != Some(&row.path) {
                    state.display.push((index, true));
                    previous = Some(&row.path);
                }
                state.display.push((index, false));
            }
            state.display_grouped = state.grouped;
        }
        rows
    }
    pub(in crate::app) fn explorer_diagnostic_badges(
        &mut self,
    ) -> Arc<BTreeMap<PathBuf, (usize, u8)>> {
        let rows = self.cached_problems();
        if self.languages.explorer_badges.is_none() {
            let mut badges = BTreeMap::<PathBuf, (usize, u8)>::new();
            let mut bytes = 0usize;
            for row in rows.iter() {
                for path in row.path.ancestors().take(32) {
                    if let Some(value) = badges.get_mut(path) {
                        value.0 = value.0.saturating_add(1);
                        value.1 = value.1.min(row.severity);
                    } else {
                        let size = path.as_os_str().len().saturating_add(64);
                        if badges.len() >= 20_000 || bytes.saturating_add(size) > BYTE_LIMIT {
                            self.languages.problems_ui.limited = true;
                            continue;
                        }
                        bytes += size;
                        badges.insert(path.to_owned(), (1, row.severity));
                    }
                }
            }
            self.languages.explorer_badges = Some(Arc::new(badges));
        }
        self.languages.explorer_badges.as_ref().unwrap().clone()
    }
    pub(in crate::app) fn next_problem(&mut self, forward: bool) {
        let rows = self.filtered_problems();
        let state = &self.languages.problems_ui;
        if state.indices.is_empty() {
            self.notice = "No diagnostics match the Problems filters.".into();
            return;
        }
        let index = state
            .indices
            .iter()
            .position(|&i| Some(rows[i].key()) == state.current);
        let count = state.indices.len();
        let next = match (index, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => (i + count - 1) % count,
        };
        let row = rows[state.indices[next]].clone();
        self.jump_problem(row);
    }
    fn jump_problem(&mut self, row: ProblemRow) {
        match row.position {
            Position::Build(line, column) => {
                let column = column.map_or(String::new(), |c| format!(":{c}"));
                self.pending_paths.push_back(PathBuf::from(format!(
                    "{}:{line}{column}",
                    row.path.display()
                )));
            }
            Position::Language(encoding, position, revision) => {
                let Some(index) = self.workspace.open.iter().position(|b| {
                    b.page
                        .info
                        .path
                        .as_ref()
                        .is_some_and(|p| Path::new(p) == row.path)
                        && b.page.info.revision == revision
                }) else {
                    self.languages.problem_rows = None;
                    self.notice = "This diagnostic is out of date.".into();
                    return;
                };
                let text = &self.workspace.open[index].page.text;
                let Ok(byte) = encoding.byte(text, position) else {
                    self.notice = "The diagnostic position is invalid.".into();
                    return;
                };
                let scalar = text[..byte].chars().count();
                self.select_tab(index);
                self.set_selections(vec![hedit::advanced::Selection::caret(scalar)]);
            }
        }
        self.languages.problems_ui.current = Some(row.key());
        self.set_panel(workbench_panel::Panel::Problems);
        self.focus_editor = true;
    }
    pub(in crate::app) fn problems_contents(&mut self, ui: &mut egui::Ui) {
        let _ = self.filtered_problems();
        let state = &mut self.languages.problems_ui;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.filter.text)
                    .id_salt("problems-filter")
                    .hint_text("Filter message, file or source")
                    .desired_width(200.0)
                    .char_limit(256),
            );
            egui::ComboBox::from_id_salt("problem-severity")
                .selected_text(if state.filter.severity == 0 {
                    "All severities"
                } else {
                    severity_name(state.filter.severity)
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.filter.severity, 0, "All severities");
                    for value in 1..=4 {
                        ui.selectable_value(
                            &mut state.filter.severity,
                            value,
                            severity_name(value),
                        );
                    }
                });
            egui::ComboBox::from_id_salt("problem-source")
                .selected_text(if state.filter.source.is_empty() {
                    "All sources"
                } else {
                    &state.filter.source
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut state.filter.source, String::new(), "All sources");
                    for source in &state.sources {
                        ui.selectable_value(&mut state.filter.source, source.clone(), source);
                    }
                });
            ui.checkbox(&mut state.filter.current, "Current file");
            ui.checkbox(&mut state.grouped, "Group by file");
        });
        let rows = self.filtered_problems();
        let state = &self.languages.problems_ui;
        let mut navigate = None;
        ui.horizontal_wrapped(|ui| {
            ui.weak(format!(
                "{} of {} · {} errors · {} warnings · {} information · {} hints",
                state.indices.len(),
                rows.len(),
                state.counts[0],
                state.counts[1],
                state.counts[2],
                state.counts[3]
            ));
            if ui.small_button("Previous diagnostic").clicked() {
                navigate = Some(false)
            }
            if ui.small_button("Next diagnostic").clicked() {
                navigate = Some(true)
            }
            if state.limited {
                ui.colored_label(self.theme.warning.color(), "Diagnostic limit reached");
            }
        });
        let mut jump = None;
        if state.display.is_empty() {
            ui.weak(if rows.is_empty() {
                "No diagnostics."
            } else {
                "No diagnostics match these filters."
            });
        }
        egui::ScrollArea::vertical()
            .id_salt("unified-problems")
            .show_rows(
                ui,
                ui.text_style_height(&egui::TextStyle::Body) + ui.spacing().item_spacing.y,
                state.display.len(),
                |ui, range| {
                    for i in range {
                        let (index, header) = state.display[i];
                        let row = &rows[index];
                        if header {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(row.path.display().to_string()).strong(),
                                )
                                .truncate(),
                            )
                            .on_hover_text(row.path.display().to_string());
                        } else if ui
                            .selectable_label(
                                state.current.as_ref() == Some(&row.key()),
                                row.label(),
                            )
                            .on_hover_text(row.path.display().to_string())
                            .clicked()
                        {
                            jump = Some(row.clone());
                        }
                    }
                },
            );
        if let Some(row) = jump {
            self.jump_problem(row);
        } else if let Some(forward) = navigate {
            self.next_problem(forward);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn fixture() -> (tempfile::TempDir, egui::Context, HeditApp, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        settings_store::TEST_CONFIG_DIR.with(|d| *d.borrow_mut() = Some(root.join("config")));
        let file = root.join("main.rs");
        std::fs::write(&file, "a😀b\nsecond\n").unwrap();
        let (workspace, _) = Workspace::restore(root.join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.choose_terminal(false);
        app.open(&ctx, Some(file.clone()));
        crate::app::tests::finish_app_job(&mut app, &ctx);
        let revision = app.active().page.info.revision;
        app.languages.diagnostics.insert(file.clone(),vec![Diagnostics{server:"fixture".into(),root:root.clone(),revision,encoding:Encoding::Utf16,items:vec![
            json!({"range":{"start":{"line":0,"character":1},"end":{"line":0,"character":3}},"severity":1,"message":"first issue"}),
            json!({"range":{"start":{"line":1,"character":0},"end":{"line":1,"character":1}},"severity":2,"message":"second issue"})]}]);
        app.build.locations.push(hedit::build::Location {
            output_line: 0,
            file: root.join("other.rs"),
            line: 3,
            column: Some(2),
            message: "warning: build issue".into(),
        });
        (dir, ctx, app, file)
    }
    #[test]
    fn filters_counts_and_grouping_share_build_and_language_rows_without_starting_services() {
        let (_dir, _ctx, mut app, file) = fixture();
        let rows = app.filtered_problems();
        assert_eq!(rows.len(), 3);
        assert!(app.languages.service.is_none());
        assert_eq!(app.languages.problems_ui.counts, [1, 2, 0, 0]);
        assert_eq!(app.languages.problems_ui.display.len(), 5);
        let same = app.filtered_problems();
        assert!(Arc::ptr_eq(&rows, &same));
        app.languages.problems_ui.filter.severity = 2;
        app.filtered_problems();
        assert_eq!(app.languages.problems_ui.indices.len(), 2);
        app.languages.problems_ui.filter.source = "Build".into();
        app.filtered_problems();
        assert_eq!(app.languages.problems_ui.indices.len(), 1);
        app.languages.problems_ui.filter.current = true;
        app.filtered_problems();
        assert!(app.languages.problems_ui.indices.is_empty());
        app.languages.problems_ui.filter = Filter {
            text: "FIRST".into(),
            ..Default::default()
        };
        app.filtered_problems();
        assert_eq!(app.languages.problems_ui.indices.len(), 1);
        let badges = app.explorer_diagnostic_badges();
        assert_eq!(badges[&file], (2, 1));
        assert_eq!(badges[&file.parent().unwrap().join("other.rs")], (1, 2));
        app.languages.problems_ui.grouped = false;
        app.filtered_problems();
        assert_eq!(app.languages.problems_ui.display.len(), 1);
    }
    #[test]
    fn diagnostic_navigation_wraps_decodes_unicode_and_refuses_stale_positions() {
        let (_dir, _ctx, mut app, _file) = fixture();
        app.languages.problems_ui.filter.source = "fixture".into();
        app.next_problem(true);
        assert_eq!(app.active().view.head, 1);
        app.next_problem(true);
        assert_eq!(app.active().view.head, 4);
        app.next_problem(true);
        assert_eq!(app.active().view.head, 1);
        app.next_problem(false);
        assert_eq!(app.active().view.head, 4);
        let row = app.cached_problems()[0].clone();
        let old = app.active().page.text.clone();
        let i = app.workspace.active;
        app.workspace.open[i].page.text.push('!');
        app.workspace.edit(i, &old).unwrap();
        let head = app.active().view.head;
        app.jump_problem(row);
        assert_eq!(app.active().view.head, head);
        assert!(app.notice.contains("out of date"));
    }
    #[test]
    fn build_replacement_and_malformed_diagnostics_invalidate_cached_rows() {
        let (_dir, _ctx, mut app, file) = fixture();
        let original = app.cached_problems();
        app.build.locations[0].message = "error: replacement".into();
        app.build.problems_revision += 1;
        let changed = app.cached_problems();
        assert!(!Arc::ptr_eq(&original, &changed));
        assert_eq!(changed[2].severity, 1);
        app.languages.diagnostics.get_mut(&file).unwrap()[0].items.push(json!({"range":{"start":{"line":2,"character":0},"end":{"line":0,"character":0}},"message":"invalid"}));
        app.languages.problem_rows = None;
        assert_eq!(app.cached_problems().len(), 3);
    }
}
#[cfg(test)]
mod limit_tests {
    use super::tests::fixture;
    use super::*;
    #[test]
    fn catalogue_stays_bounded_and_build_badges_clear_after_replacement() {
        let (_dir, _ctx, mut app, _) = fixture();
        let base = app.build.locations[0].clone();
        app.build.locations = vec![base.clone(); LIMIT + 10];
        app.build.problems_revision += 1;
        assert_eq!(app.cached_problems().len(), LIMIT);
        assert!(app.languages.problems_ui.limited);
        app.build.locations.clear();
        app.build.problems_revision += 1;
        assert_eq!(app.cached_problems().len(), 2);
        assert!(!app.languages.problems_ui.limited);
        assert!(!app.explorer_diagnostic_badges().contains_key(&base.file));
    }
    #[test]
    fn current_file_filter_tracks_switches_and_reset_filters_restore_all_rows() {
        let (_dir, ctx, mut app, file) = fixture();
        app.languages.problems_ui.filter.current = true;
        app.filtered_problems();
        assert_eq!(app.languages.problems_ui.indices.len(), 2);
        app.execute(&ctx, Command::New);
        crate::app::tests::finish_app_job(&mut app, &ctx);
        app.filtered_problems();
        assert!(app.languages.problems_ui.indices.is_empty());
        assert_eq!(app.languages.problems_ui.filter.path, None);
        app.languages.problems_ui.filter = Filter::default();
        app.filtered_problems();
        assert_eq!(app.languages.problems_ui.indices.len(), 3);
        assert!(app.cached_problems().iter().any(|r| r.path == file));
    }
}
