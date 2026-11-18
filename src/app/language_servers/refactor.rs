//! Review text changes before touching buffers. Applying keeps normal histories.
use super::*;
use hedit::language_server::workspace_edit::{Preview, QuickFix};
#[derive(Clone, PartialEq, Eq)]
struct Origin {
    view: u64,
    revision: u64,
    head: usize,
    anchor: usize,
    selections: Vec<hedit::advanced::Selection>,
    language: syntax::Language,
    pane: u64,
    path: PathBuf,
    root: PathBuf,
}
struct Rename {
    origin: Origin,
    name: String,
    focus: bool,
}
struct Review {
    origin: Origin,
    title: String,
    preview: Preview,
    selected: usize,
}
#[derive(Default)]
pub(in crate::app) struct State {
    rename: Option<Rename>,
    fixes: Option<(Origin, Vec<QuickFix>)>,
    review: Option<Review>,
    pub name: Option<String>,
}
impl State {
    pub fn visible(&self) -> bool {
        self.rename.is_some() || self.fixes.is_some() || self.review.is_some()
    }
}
pub(in crate::app) struct Applied {
    updates: Vec<(u64, hedit::document::Page)>,
    created: Vec<(hedit::document::Document, hedit::document::Page)>,
    error: Option<String>,
}
impl HeditApp {
    fn refactor_origin(&self) -> Option<Origin> {
        let b = self.active();
        let path = PathBuf::from(b.page.info.path.as_ref()?);
        Some(Origin {
            view: b.view.id,
            revision: b.page.info.revision,
            head: b.view.head,
            anchor: b.view.anchor,
            selections: b.view.selections.clone(),
            language: b.language(),
            pane: self.panes.focused_id(),
            root: self.language_root(&path),
            path,
        })
    }
    pub(in crate::app) fn rename_symbol(&mut self, ctx: &egui::Context) {
        if self.busy || self.closing || self.active().follow || self.active().page.info.paged {
            return;
        }
        let Some(origin) = self.refactor_origin() else {
            self.notice = "Save this file before renaming a symbol.".into();
            return;
        };
        self.close_transient(ctx);
        self.languages.refactor.rename = Some(Rename {
            origin,
            name: String::new(),
            focus: true,
        });
    }
    pub(in crate::app) fn review_refactor(
        &mut self,
        ctx: &egui::Context,
        title: String,
        preview: Preview,
    ) {
        if preview.files.is_empty() {
            self.notice = "No text changes were proposed.".into();
            return;
        }
        let Some(origin) = self.refactor_origin() else {
            return;
        };
        let bytes = preview
            .files
            .iter()
            .map(|f| f.original.len().saturating_add(f.updated.len()))
            .sum::<usize>();
        if preview.files.len() > 64
            || bytes > hedit::language_server::workspace_edit::MAX_PREVIEW_BYTES
            || preview
                .files
                .iter()
                .any(|f| !f.path.starts_with(&origin.root))
        {
            self.notice = "The refactor preview exceeds its project or size limits.".into();
            return;
        }
        self.close_transient(ctx);
        self.languages.refactor.review = Some(Review {
            origin,
            title,
            preview,
            selected: 0,
        });
    }
    pub(super) fn show_quick_fixes(&mut self, ctx: &egui::Context, fixes: Vec<QuickFix>) {
        if fixes.is_empty() {
            self.notice = "No quick fixes at this position.".into();
            return;
        }
        let Some(origin) = self.refactor_origin() else {
            return;
        };
        self.close_transient(ctx);
        self.languages.refactor.fixes = Some((origin, fixes));
    }
    pub(super) fn refactor_windows(&mut self, ctx: &egui::Context) {
        if let Some(mut draft) = self.languages.refactor.rename.take() {
            let mut open = true;
            let mut submit = false;
            egui::Window::new("Rename symbol")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("New symbol name");
                    let input = ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .id_salt("rename-symbol-name")
                            .char_limit(256),
                    );
                    if draft.focus {
                        input.request_focus();
                        draft.focus = false;
                    }
                    submit = ui
                        .add_enabled(
                            !draft.name.trim().is_empty(),
                            egui::Button::new("Preview rename"),
                        )
                        .clicked()
                        || (input.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)));
                    ui.weak("Review all text changes before applying.");
                });
            if open && submit {
                if self.refactor_origin() == Some(draft.origin) {
                    self.languages.refactor.name = Some(draft.name);
                    if !self.request_language_feature(ctx, Feature::Rename) {
                        self.notice =
                            "No language server supports symbol rename for this file.".into();
                        self.languages.refactor.name = None;
                    }
                } else {
                    self.notice = "The editor changed. Start rename again.".into();
                }
            } else if open {
                self.languages.refactor.rename = Some(draft);
            }
        }
        if let Some((origin, fixes)) = self.languages.refactor.fixes.take() {
            let mut open = true;
            let mut selected = None;
            egui::Window::new("Quick fixes")
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for (i, fix) in fixes.iter().enumerate() {
                                let response = ui.add_enabled(
                                    fix.preview.is_some(),
                                    egui::Button::new(&fix.title),
                                );
                                if response.clicked() {
                                    selected = Some(i);
                                }
                                if let Some(reason) = &fix.reason {
                                    ui.weak(reason);
                                }
                            }
                        });
                });
            if let Some(index) = selected {
                if self.refactor_origin() == Some(origin) {
                    let fix = &fixes[index];
                    if let Some(preview) = &fix.preview {
                        self.review_refactor(ctx, fix.title.clone(), preview.clone());
                    }
                } else {
                    self.notice = "The editor changed. Request quick fixes again.".into();
                }
            } else if open {
                self.languages.refactor.fixes = Some((origin, fixes));
            }
        }
        if let Some(mut review) = self.languages.refactor.review.take() {
            let mut open = true;
            let mut apply = false;
            let current = self.refactor_origin() == Some(review.origin.clone());
            egui::Window::new("Refactor preview")
                .open(&mut open)
                .default_size(egui::vec2(850.0, 500.0))
                .show(ctx, |ui| {
                    ui.strong(&review.title);
                    let count = review.preview.files.len();
                    ui.label(format!("{count} {} · Changes remain unsaved · Undo is available in each file", if count == 1 { "file" } else { "files" }));
                    egui::ComboBox::from_id_salt("refactor-file")
                        .selected_text(review.preview.files[review.selected].path.display().to_string())
                        .show_ui(ui, |ui| {
                            for (index, file) in review.preview.files.iter().enumerate() {
                                ui.selectable_value(&mut review.selected, index, file.path.display().to_string());
                            }
                        });
                    let file = &review.preview.files[review.selected];
                    ui.columns(2, |columns| {
                        let texts = [("Before", file.original.as_str()), ("After", file.updated.as_str())];
                        for (ui, (label, text)) in columns.iter_mut().zip(texts) {
                            ui.strong(label);
                            let height = ui.text_style_height(&egui::TextStyle::Monospace) + ui.spacing().item_spacing.y;
                            egui::ScrollArea::both()
                                .id_salt(("refactor-text", label, review.selected))
                                .max_height(350.0)
                                .show_rows(ui, height, text.lines().count().max(1), |ui, range| {
                                    for line in text.lines().skip(range.start).take(range.len()) {
                                        let line = line.chars().take(500).collect::<String>();
                                        ui.add(egui::Label::new(RichText::new(line).monospace()).wrap_mode(egui::TextWrapMode::Extend));
                                    }
                                });
                        }
                    });
                    ui.weak("Long lines show their first 500 characters. No files are saved automatically.");
                    if !current {
                        ui.colored_label(self.theme.warning.color(), "The editor changed. Request a fresh preview.");
                    }
                    apply = ui.add_enabled(current && !self.busy && !self.closing, egui::Button::new("Apply all text changes")).clicked();
                });
            if open && apply {
                self.apply_refactor(ctx, review);
            } else if open {
                self.languages.refactor.review = Some(review);
            }
        }
    }

    fn apply_refactor(&mut self, ctx: &egui::Context, review: Review) {
        if self.refactor_origin() != Some(review.origin.clone()) {
            self.notice = "The editor changed. Request a fresh preview.".into();
            return;
        }
        if let Err(e) = self.workspace.checkpoint() {
            self.error = Some(e);
            return;
        }
        let root = review.origin.root;
        let editor = self.workspace.editor.clone();
        let remote = self.workspace.remote.clone();
        let mut known = Vec::new();
        for view in self
            .workspace
            .open
            .iter()
            .map(|b| &b.view)
            .chain(self.workspace.closed.iter())
        {
            if let Ok(doc) = editor.get(view.id) {
                if let Ok(doc) = doc.lock() {
                    if let Some(path) = doc.info().path {
                        known.push((PathBuf::from(path), view.id));
                    }
                }
            }
        }
        self.cancel_opens();
        self.job(ctx, move || {
            let mut prepared = Vec::new();
            // Preflight every source before making the first edit.
            for file in review.preview.files {
                let existing = known
                    .iter()
                    .find(|(path, _)| text::same_path(path, &file.path))
                    .map(|(_, id)| *id);
                let mut doc = if let Some(id) = existing {
                    editor.get(id)?.lock().map_err(|e| e.to_string())?.clone()
                } else if let Some(remote) = &remote {
                    remote.open(editor.id(), &file.path)?
                } else {
                    hedit::document::Document::open(editor.id(), &file.path)?
                };
                let page = doc.page(0)?;
                if page
                    .info
                    .path
                    .as_ref()
                    .is_none_or(|path| !Path::new(path).starts_with(&root))
                {
                    return Err("The refactor source moved outside the project root.".into());
                }
                if page.info.paged
                    || page.text != file.original
                    || file.revision.is_some_and(|v| v != page.info.revision)
                {
                    return Err(format!(
                        "{} changed after preview. Request a fresh preview.",
                        file.path.display()
                    ));
                }
                prepared.push((existing, doc, file));
            }
            let mut result = Applied {
                updates: vec![],
                created: vec![],
                error: None,
            };
            let mut completed = Vec::new();
            for (existing, mut doc, file) in prepared {
                let applied = (|| {
                    let revision = doc.info().revision;
                    doc.replace(0, file.original.len() as u64, file.updated, revision)?;
                    let page = doc.page(0)?;
                    Ok::<_, String>(page)
                })();
                match applied {
                    Ok(page) => completed.push((existing, doc, page)),
                    Err(error) => {
                        result.error = Some(error);
                        break;
                    }
                }
            }
            // Local edits were made on clones: a failed batch commits nothing.
            if remote.is_none() {
                if let Some(error) = result.error.take() {
                    return Err(error);
                }
            }
            // SSH edits are journaled by the host; publish every completed edit
            // even if a later host operation fails, so no applied text is hidden.
            for (existing, doc, page) in completed {
                if let Some(id) = existing {
                    *editor.get(id)?.lock().map_err(|e| e.to_string())? = doc;
                    result.updates.push((id, page));
                } else {
                    result.created.push((doc, page));
                }
            }
            Ok(Action::RefactorApplied(Box::new(result)))
        });
    }
    pub(in crate::app) fn finish_refactor(&mut self, ctx: &egui::Context, applied: Applied) {
        let active = self.active().view.id;
        let count = applied.updates.len() + applied.created.len();
        for (id, page) in applied.updates {
            if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
                let buffer = &mut self.workspace.open[index];
                let selections = std::iter::once(hedit::advanced::Selection {
                    anchor: buffer.view.anchor,
                    head: buffer.view.head,
                })
                .chain(buffer.view.selections.iter().copied())
                .collect();
                buffer.cursor_undo.push(selections);
                if buffer.cursor_undo.len() > 256 {
                    buffer.cursor_undo.remove(0);
                }
                buffer.cursor_redo.clear();
                let head = buffer.view.head.min(page.text.chars().count());
                buffer.set_page(page);
                buffer.view.head = head;
                buffer.view.anchor = head;
            } else if let Some(index) = self.workspace.closed.iter().position(|v| v.id == id) {
                let view = self.workspace.closed.remove(index);
                self.workspace
                    .open
                    .push(hedit::workspace::Buffer::new(page, Some(view)));
                self.attach_open_tab(None, id);
            }
        }
        for (doc, page) in applied.created {
            let id = doc.id;
            let result = self.workspace.add(doc, page);
            self.report(result);
            self.attach_open_tab(None, id);
        }
        if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == active) {
            self.workspace.active = index;
        }
        self.languages.problem_rows = None;
        self.languages.explorer_badges = None;
        self.changed();
        self.focus_editor = true;
        self.notice = format!("Applied text changes in {count} files. Save each file when ready.");
        if let Some(error) = applied.error {
            self.error=Some(format!("Applied {count} files before the SSH operation failed: {error}. Undo is available in each changed file."));
        }
        self.checkpoint(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app, pump};
    use super::*;
    use crate::app::tests::{finish_app_job, frame, key};
    #[test]
    fn server_rename_previews_cross_file_changes_then_applies_with_undo_and_recovery() {
        let (dir, ctx, mut app) = app();
        let active = app.active().view.id;
        let source = PathBuf::from(app.active().page.info.path.as_ref().unwrap());
        let target = source.with_file_name("target.rs");
        app.batch(hedit::advanced::Batch {
            text: "fo // unsaved draft".into(),
            selections: vec![hedit::advanced::Selection::caret(2)],
        });
        let before = app.active().page.text.clone();
        app.languages.refactor.name = Some("renamed".into());
        assert!(app.request_language_feature(&ctx, Feature::Rename));
        pump(&ctx, &mut app, |a| a.languages.refactor.review.is_some());
        assert_eq!(app.active().page.text, before);
        assert!(!app.busy);
        let review = app.languages.refactor.review.take().unwrap();
        assert_eq!(review.preview.files.len(), 2);
        app.apply_refactor(&ctx, review);
        finish_app_job(&mut app, &ctx);
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.active().view.id, active);
        assert_eq!(app.active().page.text, "renamed // unsaved draft");
        assert!(app.active().page.info.dirty);
        assert_eq!(std::fs::read_to_string(&source).unwrap(), "fo");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "😀 target\n");
        app.history(&ctx, false);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, before);
        let index = app
            .workspace
            .open
            .iter()
            .position(|b| b.page.info.path.as_deref() == target.to_str())
            .unwrap();
        app.select_tab(index);
        assert!(app.active().page.text.starts_with("// renamed\n"));
        app.history(&ctx, false);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "😀 target\n");
        app.workspace.checkpoint().unwrap();
        let started = Instant::now();
        while app.checkpoint_pending {
            app.receive(&ctx);
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        drop(app);
        let (restored, errors) =
            Workspace::restore(dir.path().canonicalize().unwrap().join("session")).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert!(restored.open.iter().any(|b| b.page.text == before));
    }
    #[test]
    fn quick_fixes_require_review_and_never_execute_server_commands() {
        let (_dir, ctx, mut app) = app();
        assert!(app.request_language_feature(&ctx, Feature::QuickFixes));
        pump(&ctx, &mut app, |a| a.languages.refactor.fixes.is_some());
        let (_, fixes) = app.languages.refactor.fixes.take().unwrap();
        assert_eq!(fixes.len(), 2);
        assert!(fixes[1].preview.is_none());
        assert!(fixes[1].reason.as_ref().unwrap().contains("command"));
        app.review_refactor(
            &ctx,
            fixes[0].title.clone(),
            fixes[0].preview.clone().unwrap(),
        );
        assert_eq!(app.active().page.text, "fo");
        let review = app.languages.refactor.review.take().unwrap();
        app.apply_refactor(&ctx, review);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, "// fixed\nfo");
    }
    #[test]
    fn changes_to_any_file_after_preview_prevent_the_entire_local_batch() {
        let (_dir, ctx, mut app) = app();
        let target = PathBuf::from(app.active().page.info.path.as_ref().unwrap())
            .with_file_name("target.rs");
        app.languages.refactor.name = Some("renamed".into());
        assert!(app.request_language_feature(&ctx, Feature::Rename));
        pump(&ctx, &mut app, |a| a.languages.refactor.review.is_some());
        let review = app.languages.refactor.review.take().unwrap();
        std::fs::write(&target, "externally changed").unwrap();
        app.apply_refactor(&ctx, review);
        let started = Instant::now();
        while app.busy {
            app.receive(&ctx);
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.active().page.text, "fo");
        assert!(app
            .error
            .as_ref()
            .unwrap()
            .contains("changed after preview"));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "externally changed"
        );
    }
    #[test]
    fn escape_closes_rename_and_preview_and_cursor_changes_invalidate_review() {
        let (_dir, ctx, mut app) = app();
        app.rename_symbol(&ctx);
        frame(&ctx, &mut app, vec![]);
        assert!(app.languages.refactor.rename.is_some());
        frame(&ctx, &mut app, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!app.languages.refactor.visible());
        app.languages.refactor.name = Some("renamed".into());
        assert!(app.request_language_feature(&ctx, Feature::Rename));
        pump(&ctx, &mut app, |a| a.languages.refactor.review.is_some());
        let review = app.languages.refactor.review.take().unwrap();
        app.workspace.open[app.workspace.active].view.head = 0;
        app.apply_refactor(&ctx, review);
        assert!(!app.busy);
        assert_eq!(app.active().page.text, "fo");
        assert!(app.notice.contains("fresh preview"));
    }
}
