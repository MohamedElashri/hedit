//! Local extension manifests contribute commands to the Command Palette.
mod activity;
mod catalog;
mod distribution;
mod lua;
mod manager;
mod manifest;
mod options;
mod templates;

use super::{snippet_fields, HeditApp};
use eframe::egui;
use hedit::intelligence;
use std::path::Path;

pub(crate) use distribution::run_cli as run_plugin_cli;
pub(super) use lua::LuaTransform;
pub(super) use manager::Manager;
use manifest::ExtensionAction;
pub(super) use manifest::{load, Contribution};
pub(super) use options::SaveResult;
pub(super) use templates::Template;

const MAX_INDENT_BYTES: usize = 4096;
const MAX_EXPANSION_BYTES: usize = 128 * 1024;

pub(super) fn valid_command_id(id: &str) -> bool {
    id.split_once('.').is_some_and(|(extension, command)| {
        manifest::valid_id(extension) && manifest::valid_id(command)
    })
}

impl HeditApp {
    pub(super) fn extension_commands(&self) -> impl Iterator<Item = &Contribution> {
        let editor = self.active();
        let info = &editor.page.info;
        let language = editor.language();
        let filename = info.path.as_ref().map(|_| info.name.as_str());
        let selection = editor.view.anchor != editor.view.head
            || editor
                .view
                .selections
                .iter()
                .any(|selection| selection.anchor != selection.head);
        let one_time = self.extension_manager.one_time_command();
        let available = (!self.extensions.is_empty() || one_time.is_some())
            && !self.busy
            && !self.closing
            && !self.preferences_open
            && !self.source_control.open
            && self
                .workspace
                .remote
                .as_ref()
                .is_none_or(|remote| remote.writable());
        let editable = available && !self.markdown_preview_only() && !editor.follow && !info.paged;
        one_time
            .into_iter()
            .chain(self.extensions.iter())
            .filter(move |command| {
                let enabled = match command.action {
                    ExtensionAction::InsertSnippet { .. } => {
                        editable && editor.view.selections.is_empty()
                    }
                    ExtensionAction::NewFile { .. } => available,
                    ExtensionAction::Lua(_) => {
                        editable
                            && selection
                            && self.workspace.remote.is_none()
                            && self.lua_transform.is_none()
                            && crate::plugin_runtime::SUPPORTED
                    }
                };
                enabled && command.when.matches(language, filename, selection)
            })
    }

    pub(super) fn extension_available_by_id(&self, id: &str) -> bool {
        self.extension_by_id(id).is_some()
    }

    fn extension_by_id(&self, id: &str) -> Option<&Contribution> {
        valid_command_id(id)
            .then(|| self.extension_commands().find(|command| command.id == id))
            .flatten()
    }

    pub(super) fn extension_file_templates(&self) -> Vec<(String, String)> {
        self.extension_commands()
            .filter(|command| matches!(command.action, ExtensionAction::NewFile { .. }))
            .map(|command| (command.title.clone(), command.id.clone()))
            .collect()
    }

    pub(super) fn run_extension_by_id(&mut self, ctx: &egui::Context, id: &str) -> bool {
        let Some(action) = self
            .extension_by_id(id)
            .map(|command| command.action.clone())
        else {
            return false;
        };
        match action {
            ExtensionAction::InsertSnippet { body } => self.run_extension_snippet(&body),
            ExtensionAction::Lua(plugin) => {
                self.extension_manager.consume_one_time(id);
                self.start_lua_transform(id, plugin);
            }
            ExtensionAction::NewFile { body, language } => {
                self.run_extension_template(ctx, &body, language)
            }
        }
        true
    }

    fn run_extension_snippet(&mut self, body: &str) {
        if body.contains('\n') {
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
        let info = &self.active().page.info;
        let basename = info
            .path
            .as_ref()
            .and_then(|_| Path::new(&info.name).file_stem())
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_owned();
        self.insert_snippet_with(body, 0, |body, selection| {
            intelligence::expand_fields_bounded(
                body,
                selection,
                Some(&basename),
                MAX_EXPANSION_BYTES,
            )
        });
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

    pub(super) fn app_with(
        text: &str,
        config: Option<&Path>,
    ) -> (tempfile::TempDir, egui::Context, HeditApp) {
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
                action: ExtensionAction::InsertSnippet {
                    body: "wrong command".into(),
                },
                when: Default::default(),
            },
            Contribution {
                id: "second.insert".into(),
                title: "Insert note".into(),
                action: ExtensionAction::InsertSnippet {
                    body: "correct command".into(),
                },
                when: Default::default(),
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
        let (_session, ctx, mut app) = app_with("", Some(config.path()));
        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(app.run_extension_by_id(&ctx, "test.insert"));
        assert_eq!(app.active().page.text, "αα");
        assert!(app.next_field(false));
        assert_eq!(app.selections(), [Selection::caret(1)]);
    }

    #[test]
    fn extension_commands_only_edit_an_editable_single_selection() {
        let (_session, ctx, mut app) = app_with("left right", None);
        app.extensions.push(Contribution {
            id: "test.insert".into(),
            title: "Insert text".into(),
            action: ExtensionAction::InsertSnippet {
                body: "text".into(),
            },
            when: Default::default(),
        });
        assert_eq!(app.extension_commands().count(), 1);
        let index = app.workspace.active;
        app.workspace.open[index].follow = true;
        assert!(app.extension_commands().next().is_none());
        assert!(!app.run_extension_by_id(&ctx, "test.insert"));
        app.workspace.open[index].follow = false;
        app.workspace.open[index].page.info.paged = true;
        assert!(app.extension_commands().next().is_none());
        assert!(!app.run_extension_by_id(&ctx, "test.insert"));
        app.workspace.open[index].page.info.paged = false;
        app.set_selections(vec![Selection::caret(0), Selection::caret(5)]);
        assert!(app.extension_commands().next().is_none());
        assert!(!app.run_extension_by_id(&ctx, "test.insert"));
        assert_eq!(app.active().page.text, "left right");
    }

    #[test]
    fn extension_context_and_literal_variables_are_applied_at_invocation() {
        let configuration = tempfile::tempdir().unwrap();
        let manifests = configuration.path().join("extensions");
        std::fs::create_dir(&manifests).unwrap();
        std::fs::write(
            manifests.join("links.json"),
            r#"{"api_version":1,"id":"links","commands":[{
                "id":"wrap","title":"Wrap selection",
                "when":{"languages":["markdown"],"filename":"note-*.md","selection":true},
                "action":{"kind":"insert_snippet","body":"[${SELECTION}](${1:url}) ${FILE_BASENAME}$0"}
            },{
                "id":"other","title":"links.wrap","action":{"kind":"insert_snippet","body":"wrong"}
            }]}"#,
        ).unwrap();
        let (session, ctx, mut app) = app_with("", Some(configuration.path()));
        let filename = session.path().join("note-λ.md");
        let source = "$1 } café";
        std::fs::write(&filename, source).unwrap();
        app.open(&ctx, Some(filename));
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, source);
        assert!(!app.extension_available_by_id("links.wrap"));
        select_palette_command(&ctx, &mut app, "links.wrap");
        assert_eq!(app.active().page.text, source);
        app.close_transient(&ctx);
        app.set_selections(vec![Selection {
            anchor: 0,
            head: source.chars().count(),
        }]);
        assert!(app.extension_available_by_id("links.wrap"));
        assert!(app.run_extension_by_id(&ctx, "links.wrap"));
        assert_eq!(app.active().page.text, "[$1 } café](url) note-λ");
        assert!(app.snippet.is_some());
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, source);
    }

    #[test]
    fn file_template_picker_runs_an_installed_manifest_action() {
        let config = tempfile::tempdir().unwrap();
        let manifests = config.path().join("extensions");
        std::fs::create_dir(&manifests).unwrap();
        std::fs::write(
            manifests.join("tests.json"),
            r#"{"api_version":1,"id":"test-tools","commands":[{
                "id":"python-test","title":"Python test",
                "action":{"kind":"new_file","language":"python","body":"def test_${1:sample}():\n    pass\n$0"}
            }]}"#,
        )
        .unwrap();
        let (_session, ctx, mut app) = app_with("original", Some(config.path()));
        let previous = app.active().view.id;
        app.execute(&ctx, Command::NewFileFromTemplate);
        frame(&ctx, &mut app, Vec::new());
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::Text("Python test".into())],
        );
        frame(
            &ctx,
            &mut app,
            vec![key(egui::Key::Enter, egui::Modifiers::NONE)],
        );
        finish_app_job(&mut app, &ctx);
        assert_ne!(app.active().view.id, previous);
        assert_eq!(app.workspace.open[0].page.text, "original");
        assert_eq!(app.active().language(), hedit::syntax::Language::Python);
        assert!(app.active().page.info.path.is_none());
        assert_eq!(app.active().page.text, "def test_sample():\n    pass\n");
        assert!(app.snippet.is_some());
    }
}
