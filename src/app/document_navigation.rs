//! File context uses metadata; the optional symbol index is bounded and on demand.
use super::*;
use hedit::icons::Icon;
use std::path::{Component, Path};
const OUTLINE_LIMIT: usize = 2 * 1024 * 1024;
type Crumbs = Arc<Vec<(String, PathBuf)>>;
struct Breadcrumbs {
    group: u64,
    id: u64,
    path: String,
    root: Option<String>,
    items: Crumbs,
}
#[derive(Default)]
pub(super) struct State {
    symbols: goto_matches::TargetCache,
    query: String,
    pub focus_query: bool,
    crumbs: VecDeque<Breadcrumbs>,
    #[cfg(test)]
    updates: usize,
}
fn path_crumbs(path: &Path, root: Option<&Path>) -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    let mut full = PathBuf::new();
    let relative = if let Some(root) = root.filter(|r| path.starts_with(r)) {
        full = root.to_owned();
        result.push((
            root.file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
                .into_owned(),
            full.clone(),
        ));
        path.strip_prefix(root).unwrap_or(path)
    } else {
        path
    };
    for component in relative.components() {
        if matches!(component, Component::CurDir) {
            continue;
        }
        full.push(component);
        result.push((
            component.as_os_str().to_string_lossy().into_owned(),
            full.clone(),
        ));
    }
    if result.len() > 8 {
        let skipped = result.len() - 7;
        let parent = result[skipped].1.clone();
        result.drain(1..=skipped);
        result.insert(1, ("…".into(), parent));
    }
    result
}
impl State {
    fn breadcrumbs(
        &mut self,
        group: u64,
        id: u64,
        path: &str,
        root: Option<&Path>,
    ) -> (Crumbs, bool) {
        let root = root.map(|r| r.to_string_lossy());
        if let Some(cached) = self.crumbs.iter().find(|c| {
            c.group == group && c.id == id && c.path == path && c.root.as_deref() == root.as_deref()
        }) {
            return (cached.items.clone(), false);
        }
        self.crumbs.retain(|c| c.group != group);
        let items = Arc::new(path_crumbs(Path::new(path), root.as_deref().map(Path::new)));
        if self.crumbs.len() == panes::MAX_GROUPS {
            self.crumbs.pop_front();
        }
        self.crumbs.push_back(Breadcrumbs {
            group,
            id,
            path: path.into(),
            root: root.map(|r| r.into_owned()),
            items: items.clone(),
        });
        (items, true)
    }
}
impl HeditApp {
    pub(super) fn document_breadcrumbs(&mut self, ui: &mut egui::Ui) {
        if !self.settings.show_breadcrumbs || self.distraction_free {
            return;
        }
        let b = &self.workspace.open[self.workspace.active];
        let Some(path) = b.page.info.path.as_deref() else {
            let name: String = b
                .view
                .name
                .chars()
                .take(160)
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            ui.add(
                egui::Label::new(RichText::new(format!("Unsaved · {name}")).small().weak())
                    .truncate(),
            );
            return;
        };
        if let Some(remote) = &self.workspace.remote {
            let relative = remote
                .relative(Path::new(path))
                .map(|p| String::from_utf8_lossy(&p).into_owned())
                .unwrap_or_else(|_| b.view.name.clone());
            let label = format!("{} · {relative}", remote.config.target.host);
            if ui
                .add(egui::Button::new(label).frame(false).truncate())
                .on_hover_text("Reveal this file in Files")
                .clicked()
            {
                self.sidebar = workbench_navigation::Sidebar::Files;
                self.reveal_in_sidebar();
            }
            return;
        }
        let root = self
            .workspace
            .folders
            .iter()
            .filter(|f| Path::new(path).starts_with(&f.path))
            .max_by_key(|f| f.path.len())
            .map(|f| Path::new(&f.path));
        let can_reveal = root.is_some();
        let group = self.panes.focused_id();
        let (crumbs, fresh) = self
            .document_navigation
            .breadcrumbs(group, b.view.id, path, root);
        let muted = self.theme.muted.color();
        let mut reveal = None;
        egui::ScrollArea::horizontal()
            .id_salt(("breadcrumbs", group))
            .max_height(hedit::ui_style::Metrics::for_ui(ui).control)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (index, (label, path)) in crumbs.iter().enumerate() {
                        if index > 0 {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(12.0, 16.0), egui::Sense::hover());
                            Icon::ChevronRight.paint(ui, rect, muted);
                        }
                        let visible: String = label
                            .chars()
                            .take(48)
                            .map(|c| if c.is_control() { ' ' } else { c })
                            .collect();
                        let response = ui
                            .add_enabled(
                                can_reveal,
                                egui::Button::new(
                                    RichText::new(hedit::bidi::display(&visible)).small(),
                                )
                                .frame(false),
                            )
                            .on_hover_text(format!("Reveal in Files · {}", path.display()));
                        if fresh && index + 1 == crumbs.len() {
                            response.scroll_to_me(Some(egui::Align::Max));
                        }
                        if response.clicked() {
                            reveal = Some(path.clone());
                        }
                    }
                });
            });
        if let Some(path) = reveal {
            self.sidebar = workbench_navigation::Sidebar::Files;
            self.reveal_path_in_sidebar(path.to_string_lossy().into_owned());
        }
    }
    pub(super) fn document_outline(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        if !self.settings.show_outline || self.distraction_free || self.source_control.open {
            return;
        }
        let mut close = false;
        let mut jump = None;
        let origin = (self.active().view.id, self.active().page.info.revision);
        egui::Panel::right("document-outline").default_size(240.0).min_size(160.0).max_size((ctx.viewport_rect().width()*0.3).max(160.0))
            .frame(egui::Frame::new().fill(self.theme.panel.color()).inner_margin(8)).show(root,|ui| {
                ui.horizontal(|ui| {
                    ui.strong("Outline");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui| { close=controls::close_button(ui,"Hide Outline").clicked(); });
                });
                if self.active().page.info.paged || self.active().page.text.len()>OUTLINE_LIMIT {
                    ui.weak("Outline is available for files up to 2 MiB. Use Go to symbol for larger files.");return;
                }
                let query=ui.add(egui::TextEdit::singleline(&mut self.document_navigation.query).id(egui::Id::unique("outline-filter")).hint_text("Filter symbols…").char_limit(128));
                if std::mem::take(&mut self.document_navigation.focus_query) { query.request_focus(); }
                if query.has_focus() { self.focus_editor=false; }
                let target=goto_anything::Target::Symbol(self.document_navigation.query.clone());
                let Some((rows,fresh))=self.document_navigation.symbols.get(ctx,&self.workspace.open[self.workspace.active],&target) else { ui.spinner();return; };
                #[cfg(test)]
                if fresh { self.document_navigation.updates+=1; }
                #[cfg(not(test))]
                let _=fresh;
                if rows.is_empty() { ui.weak("No symbols in this file."); }
                if rows.len()==500 { ui.weak("Showing the first 500 symbols."); }
                egui::ScrollArea::vertical().id_salt("outline-symbols").auto_shrink([false,false])
                    .show_rows(ui,hedit::ui_style::Metrics::for_ui(ui).row,rows.len(),|ui,range| {
                        for (name,line) in &rows[range] {
                            let visible:String=name.chars().take(160).collect();
                            if ui.add(egui::Label::new(format!("{visible} · {line}")).truncate().sense(egui::Sense::click())).on_hover_text(name).clicked() { jump=Some(*line); }
                        }
                    });
            });
        if close {
            self.settings.show_outline = false;
            self.focus_editor = true;
            ctx.memory_mut(|m| m.surrender_focus(egui::Id::unique("outline-filter")));
        }
        if let Some(line) =
            jump.filter(|_| origin == (self.active().view.id, self.active().page.info.revision))
        {
            self.jump_to_line(ctx, line, None);
            self.focus_editor = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::{click_label, finish_app_job, frame, key, text_rect};
    fn app() -> (tempfile::TempDir, egui::Context, HeditApp) {
        let dir = tempfile::tempdir().unwrap();
        let (workspace, _) = Workspace::restore(dir.path().join("session")).unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, vec![], vec![]);
        app.settings.auto_complete = false;
        app.set_folder(hedit::folder::read_directory(dir.path().to_owned()).unwrap());
        for (name, text) in [
            ("alpha.rs", "fn alpha() {}\n"),
            ("beta.rs", "\n\nfn beta() {}\n"),
        ] {
            let file = dir.path().join(name);
            std::fs::write(&file, text).unwrap();
            app.open(&ctx, Some(file));
            finish_app_job(&mut app, &ctx);
        }
        app.select_tab(1);
        (dir, ctx, app)
    }
    #[test]
    fn breadcrumbs_are_bounded_and_cache_only_path_metadata() {
        let root = Path::new("workspace");
        let file = (0..20)
            .fold(root.to_owned(), |p, n| p.join(format!("folder{n}")))
            .join("main.rs");
        let rows = path_crumbs(&file, Some(root));
        assert_eq!(rows.len(), 8);
        assert_eq!(rows[0].0, "workspace");
        assert_eq!(rows[1].0, "…");
        assert_eq!(rows.last().unwrap().1, file);
        let mut state = State::default();
        let (one, fresh) = state.breadcrumbs(1, 7, "workspace/src/main.rs", Some(root));
        assert!(fresh);
        let (two, fresh) = state.breadcrumbs(1, 7, "workspace/src/main.rs", Some(root));
        assert!(!fresh && Arc::ptr_eq(&one, &two));
        for group in 2..20 {
            state.breadcrumbs(group, 7, "workspace/src/main.rs", Some(root));
        }
        assert_eq!(state.crumbs.len(), panes::MAX_GROUPS);
    }
    #[test]
    fn outline_does_no_hidden_work_reuses_results_and_tracks_the_active_document() {
        let (_dir, ctx, mut app) = app();
        for _ in 0..4 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.document_navigation.updates, 0);
        app.execute(&ctx, Command::Outline);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        let updates = app.document_navigation.updates;
        assert_eq!(updates, 1);
        for _ in 0..10 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.document_navigation.updates, updates);
        app.select_tab(2);
        let out = frame(&ctx, &mut app, vec![]);
        assert!(out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "beta · 3").is_some()));
        assert!(!out
            .shapes
            .iter()
            .any(|s| text_rect(&s.shape, "alpha · 1").is_some()));
        click_label(&ctx, &mut app, "beta · 3");
        assert_eq!(app.active().view.head, 2);
        let updates = app.document_navigation.updates;
        app.execute(&ctx, Command::Outline);
        let previous = app.active().page.text.clone();
        app.workspace.open[app.workspace.active].page.text = "fn changed() {}\n".into();
        app.workspace.edit(app.workspace.active, &previous).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        assert_eq!(app.document_navigation.updates, updates);
    }
    #[test]
    fn outline_filter_escape_returns_typing_to_the_editor() {
        let (_dir, ctx, mut app) = app();
        app.execute(&ctx, Command::Outline);
        for _ in 0..3 {
            frame(&ctx, &mut app, vec![]);
        }
        frame(&ctx, &mut app, vec![egui::Event::Text("alp".into())]);
        assert_eq!(app.document_navigation.query, "alp");
        let before = app.active().page.text.clone();
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        frame(&ctx, &mut app, vec![egui::Event::Text("x".into())]);
        assert!(app.settings.show_outline);
        assert_eq!(app.document_navigation.query, "alp");
        assert_ne!(app.active().page.text, before);
    }
    #[test]
    fn paged_and_oversized_documents_do_not_start_outline_scans() {
        let (_dir, ctx, mut app) = app();
        app.settings.show_outline = true;
        for paged in [true, false] {
            let b = &mut app.workspace.open[app.workspace.active];
            b.page.info.paged = paged;
            if !paged {
                b.page.text = "x".repeat(OUTLINE_LIMIT + 1);
            }
            let _ = crate::app::tests::run_pass(
                &ctx,
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(720.0, 480.0),
                    )),
                    ..Default::default()
                },
                |ctx| ctx.root_ui(|root| app.document_outline(root)),
            );
            assert_eq!(app.document_navigation.updates, 0);
            assert!(!app.document_navigation.symbols.pending());
        }
    }
}
