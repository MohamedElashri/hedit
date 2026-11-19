use super::super::{commands::Pick, Action, HeditApp};
use eframe::egui;
use hedit::{document::Document, editing, intelligence, syntax::Language};

pub(in crate::app) struct Template {
    expansion: intelligence::Expansion,
    language: Option<Language>,
}

impl HeditApp {
    pub(in crate::app) fn choose_extension_template(&mut self, ctx: &egui::Context) {
        let items = self
            .extension_file_templates()
            .into_iter()
            .map(|(title, id)| (title, id.clone(), Pick::Extension(id)))
            .collect::<Vec<_>>();
        if items.is_empty() {
            self.notice =
                "No extension file templates are available in this editor context.".into();
            return;
        }
        self.open_pick_list(
            ctx,
            "New File from Template",
            "Find an extension template",
            items,
        );
    }

    pub(in crate::app) fn run_extension_template(
        &mut self,
        ctx: &egui::Context,
        body: &str,
        language: Option<Language>,
    ) {
        if self.busy || self.closing {
            return;
        }
        let id = self.workspace.editor.id();
        let remote = self.workspace.remote.clone();
        let body = body.to_owned();
        self.job(ctx, move || {
            let expansion = intelligence::expand_fields_bounded(
                &body.replace("\r\n", "\n"),
                "",
                None,
                super::MAX_EXPANSION_BYTES,
            )
            .ok_or("The expanded extension file template is too large.")?;
            let mut doc = match remote {
                Some(remote) => remote.scratch(id)?,
                None => Document::empty(id),
            };
            let page = doc.page(0)?;
            Ok(Action::AddTemplate(
                Box::new(doc),
                page,
                Template {
                    expansion,
                    language,
                },
            ))
        });
    }

    pub(in crate::app) fn fill_extension_template(&mut self, template: Template) {
        let intelligence::Expansion { text, fields } = template.expansion;
        let cursor = fields.last().map_or(0, |field| field[0].0);
        self.snippet = None;
        let inserted = self.apply_edit(
            editing::Edit {
                text,
                anchor: cursor,
                head: cursor,
            },
            None,
        );
        if let Some(language) = template.language {
            self.set_syntax(Some(language));
        }
        if inserted && fields.len() > 1 {
            self.start_snippet(0, fields);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::tests::finish_app_job;
    use hedit::advanced::Selection;

    #[test]
    fn file_template_creates_a_recoverable_unsaved_buffer_with_language_and_fields() {
        let directory = tempfile::tempdir().unwrap();
        let (mut workspace, _) =
            hedit::workspace::Workspace::restore(directory.path().into()).unwrap();
        workspace.open[0].page.text = "keep me".into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = HeditApp::from_context(&ctx, None, workspace, Vec::new(), Vec::new());

        app.run_extension_template(
            &ctx,
            "def ${1:test_λ}():\r\n    ${2:pass}  # $1\r\n$0",
            Some(Language::Python),
        );
        finish_app_job(&mut app, &ctx);

        assert_eq!(app.workspace.open[0].page.text, "keep me");
        assert_eq!(
            app.active().page.text,
            "def test_λ():\n    pass  # test_λ\n"
        );
        assert!(app.active().page.info.path.is_none());
        assert!(app.active().page.info.dirty);
        assert_eq!(app.active().language_override(), Some(Language::Python));
        assert_eq!(
            app.selections(),
            [
                Selection {
                    anchor: 4,
                    head: 10
                },
                Selection {
                    anchor: 26,
                    head: 32
                },
            ]
        );
        assert!(app.next_field(false));
        assert_eq!(
            app.selections(),
            [Selection {
                anchor: 18,
                head: 22
            }]
        );
        assert!(app.next_field(false));
        assert!(app.snippet.is_none());
        assert_eq!(app.active().view.head, 33);
        if let Some(worker) = app.checkpoint_worker.take() {
            worker.join().unwrap();
        }
        app.workspace.checkpoint().unwrap();
        let expected = app.active().page.text.clone();
        drop(app);

        let (reopened, _) = hedit::workspace::Workspace::restore(directory.path().into()).unwrap();
        let active = &reopened.open[reopened.active];
        assert_eq!(active.page.text, expected);
        assert_eq!(active.language_override(), Some(Language::Python));
        assert_eq!(active.view.head, 33);
    }
}
