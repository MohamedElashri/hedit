//! Local extension manifests contribute commands to the Command Palette.
mod manifest;
mod templates;

use super::{snippet_fields, HeditApp};
use hedit::intelligence::Snippet;

pub(super) use manifest::{load, Contribution};
pub(super) use templates::Template;

const MAX_INDENT_BYTES: usize = 4096;

impl HeditApp {
    pub(super) fn extension_commands(&self) -> &[Contribution] {
        if self.extensions.is_empty()
            || self.busy
            || self.closing
            || self.preferences_open
            || self.source_control.open
            || self.markdown_preview_only()
            || self.active().follow
            || self.active().page.info.paged
            || !self.active().view.selections.is_empty()
        {
            &[]
        } else {
            &self.extensions
        }
    }

    pub(super) fn run_extension(&mut self, index: usize) {
        let Some(command) = self.extension_commands().get(index) else {
            return;
        };
        if command.body.contains('\n') {
            let buffer = self.active();
            let text = &buffer.page.text;
            let start =
                snippet_fields::insertion_byte(text, buffer.view.anchor.min(buffer.view.head));
            let line = text[..start].rfind('\n').map_or(0, |index| index + 1);
            let indent_bytes = text[line..]
                .bytes()
                .take(MAX_INDENT_BYTES + 1)
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count();
            if indent_bytes > MAX_INDENT_BYTES {
                self.error = Some("Extension snippet cannot indent this line.".into());
                return;
            }
        }
        self.insert_snippet(
            Snippet {
                trigger: String::new(),
                description: command.title.clone(),
                body: command.body.clone(),
                language: None,
            },
            0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        settings_store,
        tests::{finish_app_job, frame, key},
        Command,
    };
    use eframe::egui;
    use hedit::{advanced::Selection, editing::Operation, workspace::Workspace};
    use std::path::Path;

    fn app_with(text: &str, config: Option<&Path>) -> (tempfile::TempDir, egui::Context, HeditApp) {
        let session = tempfile::tempdir().unwrap();
        let (mut workspace, _) = Workspace::restore(session.path().into()).unwrap();
        workspace.open[0].page.text = text.into();
        workspace.edit(0, "").unwrap();
        let ctx = egui::Context::default();
        let mut app = settings_store::TEST_CONFIG_DIR.with(|dir| {
            let previous = dir.replace(config.map(Path::to_path_buf));
            let app = HeditApp::from_context(&ctx, None, workspace, Vec::new(), Vec::new());
            dir.replace(previous);
            app
        });
        app.focus_editor = true;
        frame(&ctx, &mut app, Vec::new());
        (session, ctx, app)
    }

    fn select_palette_command(ctx: &egui::Context, app: &mut HeditApp, query: &str) {
        app.execute(ctx, Command::Palette);
        frame(ctx, app, Vec::new());
        frame(ctx, app, vec![egui::Event::Text(query.into())]);
        frame(ctx, app, vec![key(egui::Key::Enter, egui::Modifiers::NONE)]);
    }

    #[test]
    fn extension_command_palette_edits_crlf_and_undoes_in_one_step() {
        let config = tempfile::tempdir().unwrap();
        let extensions = config.path().join("extensions");
        std::fs::create_dir(&extensions).unwrap();
        std::fs::write(
            extensions.join("markdown-task.json"),
            include_str!("../../examples/extensions/markdown-task.json"),
        )
        .unwrap();
        let (_session, ctx, mut app) = app_with("head\nλ\ntail\n", Some(config.path()));
        assert!(app.error.is_none(), "{:?}", app.error);
        app.edit_command(Operation::ToCrlf);
        let original = app.active().page.text.clone();
        app.set_selections(vec![Selection { anchor: 6, head: 7 }]);
        select_palette_command(&ctx, &mut app, "markdown-task.insert-task");
        assert_eq!(
            app.active().page.text,
            "head\r\n- [ ] Describe the task\r\ntail\r\n"
        );
        assert!(app.snippet.is_some());
        assert_eq!(app.active().page.info.eol, "CRLF");
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, original);
    }

    #[test]
    fn extension_command_palette_prioritizes_an_exact_namespaced_id() {
        let (_session, ctx, mut app) = app_with("", None);
        app.extensions = vec![
            Contribution {
                id: "first.insert".into(),
                title: "second.insert".into(),
                body: "wrong command".into(),
            },
            Contribution {
                id: "second.insert".into(),
                title: "Insert note".into(),
                body: "correct command".into(),
            },
        ];
        select_palette_command(&ctx, &mut app, "second.insert");
        assert_eq!(app.active().page.text, "correct command");
    }

    #[test]
    fn extension_duplicate_defaults_keep_a_preceding_caret_marker() {
        let config = tempfile::tempdir().unwrap();
        let directory = config.path().join("extensions");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("duplicate.json"),
            r#"{"api_version":1,"id":"test","commands":[{"id":"insert","title":"Insert text",
             "action":{"kind":"insert_snippet","body":"${1:α}$0${1:β}"}}]}"#,
        )
        .unwrap();
        let (_session, _ctx, mut app) = app_with("", Some(config.path()));
        assert!(app.error.is_none(), "{:?}", app.error);
        app.run_extension(0);
        assert_eq!(app.active().page.text, "αα");
        assert!(app.next_field(false));
        assert_eq!(app.selections(), [Selection::caret(1)]);
    }

    #[test]
    fn extension_commands_only_edit_an_editable_single_selection() {
        let (_session, _ctx, mut app) = app_with("left right", None);
        app.extensions.push(Contribution {
            id: "test.insert".into(),
            title: "Insert text".into(),
            body: "text".into(),
        });
        assert_eq!(app.extension_commands().len(), 1);
        let index = app.workspace.active;
        app.workspace.open[index].follow = true;
        assert!(app.extension_commands().is_empty());
        app.run_extension(0);
        app.workspace.open[index].follow = false;
        app.workspace.open[index].page.info.paged = true;
        assert!(app.extension_commands().is_empty());
        app.run_extension(0);
        app.workspace.open[index].page.info.paged = false;
        app.set_selections(vec![Selection::caret(0), Selection::caret(5)]);
        assert!(app.extension_commands().is_empty());
        app.run_extension(0);
        assert_eq!(app.active().page.text, "left right");
    }
}
