//! A script gets a selection snapshot and can propose one normal editor edit.
use super::{catalog, manifest::LuaPlugin};
use crate::{
    app::{clipboard_lines, HeditApp},
    plugin_runtime::{self, RunningPlugin},
};
use hedit::{advanced, editing};
use std::{
    fs::File,
    ops::Range,
    sync::mpsc::{self, Receiver, TryRecvError},
    thread,
};

pub(in crate::app) struct LuaTransform {
    proposal: Proposal,
    phase: Phase,
}

enum Phase {
    Checking(Receiver<Option<File>>),
    Running(RunningPlugin),
    Committing {
        approval: Receiver<Option<File>>,
        replacement: String,
    },
}

struct Proposal {
    target: Target,
    bytes: Range<usize>,
    plugin: LuaPlugin,
}

#[derive(Debug, PartialEq, Eq)]
struct Target {
    document: u64,
    revision: u64,
    group: u64,
    page_start: u64,
    language: &'static str,
    selection: advanced::Selection,
}

impl HeditApp {
    fn lua_target(&self) -> Option<Target> {
        let editor = self.active();
        let info = &editor.page.info;
        let selection = advanced::Selection {
            anchor: editor.view.anchor,
            head: editor.view.head,
        };
        if self.busy
            || self.closing
            || self.preferences_open
            || self.source_control.open
            || self.markdown_preview_only()
            || self.workspace.remote.is_some()
            || editor.follow
            || info.paged
            || !editor.view.selections.is_empty()
            || selection.anchor == selection.head
        {
            return None;
        }
        Some(Target {
            document: info.id,
            revision: info.revision,
            group: self.panes.focused_id(),
            page_start: editor.page.start,
            language: editor.language().id(),
            selection,
        })
    }

    pub(super) fn start_lua_transform(&mut self, plugin: LuaPlugin) {
        let result = self.lua_proposal(plugin).and_then(|proposal| {
            let approval = self.request_lua_approval(&proposal.plugin)?;
            Ok(LuaTransform {
                proposal,
                phase: Phase::Checking(approval),
            })
        });
        match result {
            Ok(job) => {
                self.lua_transform = Some(job);
                self.notice = "Running extension command…".into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn lua_proposal(&self, plugin: LuaPlugin) -> Result<Proposal, String> {
        let target = self
            .lua_target()
            .ok_or("Select text in a local editable buffer.")?;
        let bytes = selection_bytes(&self.active().page.text, target.selection)?;
        Ok(Proposal {
            target,
            bytes,
            plugin,
        })
    }

    fn request_lua_approval(&self, plugin: &LuaPlugin) -> Result<Receiver<Option<File>>, String> {
        let config = self
            .settings_files
            .dir
            .clone()
            .ok_or("Extension approval requires a user configuration folder.")?;
        let plugin = plugin.clone();
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("hedit-lua-approval".into())
            .spawn(move || {
                let _ = sender.send(catalog::lua_authorization(&config, &plugin));
            })
            .map_err(|_| "The extension approval check could not start.")?;
        Ok(receiver)
    }

    pub(in crate::app) fn poll_lua_transform(&mut self) {
        let Some(job) = self.lua_transform.take() else {
            return;
        };
        if self.lua_target().as_ref() != Some(&job.proposal.target) {
            self.notice = "Extension command canceled because the editor context changed.".into();
            return;
        }
        match self.advance_lua_transform(job) {
            Ok(next) => self.lua_transform = next,
            Err(error) => {
                self.notice = "Extension command failed.".into();
                self.error = Some(error);
            }
        }
    }

    fn advance_lua_transform(&mut self, job: LuaTransform) -> Result<Option<LuaTransform>, String> {
        let LuaTransform {
            proposal,
            mut phase,
        } = job;
        match &mut phase {
            Phase::Checking(approval) => {
                if let Some(_approval) = receive_approval(approval)? {
                    let selection = self.active().page.text[proposal.bytes.clone()].to_owned();
                    phase = Phase::Running(plugin_runtime::start(
                        proposal.plugin.source.to_string(),
                        selection,
                        proposal.target.language.into(),
                    )?);
                }
            }
            Phase::Running(worker) => {
                if let Some(result) = worker.try_result() {
                    phase = Phase::Committing {
                        replacement: result?,
                        approval: self.request_lua_approval(&proposal.plugin)?,
                    };
                }
            }
            Phase::Committing {
                approval,
                replacement,
            } => {
                if let Some(_approval) = receive_approval(approval)? {
                    self.apply_lua_replacement(proposal, replacement);
                    return Ok(None);
                }
            }
        }
        Ok(Some(LuaTransform { proposal, phase }))
    }

    fn apply_lua_replacement(&mut self, proposal: Proposal, replacement: &str) {
        let editor = self.active();
        let replacement =
            clipboard_lines::line_endings(replacement, editor.page.info.eol == "CRLF");
        let mut text = editor.page.text.clone();
        text.replace_range(proposal.bytes, &replacement);
        let caret = proposal.target.selection.start() + replacement.chars().count();
        let edit = editing::Edit {
            text,
            anchor: caret,
            head: caret,
        };
        if self.apply_edit(edit, None) {
            self.snippet = None;
            self.notice = "Extension command finished.".into();
        }
    }
}

// The returned lock serializes launch or edit application with a revocation.
fn receive_approval(approval: &Receiver<Option<File>>) -> Result<Option<File>, String> {
    match approval.try_recv() {
        Ok(Some(guard)) => Ok(Some(guard)),
        Ok(None) => Err("Extension approval changed. Review it in Settings → Extensions.".into()),
        Err(TryRecvError::Empty) => Ok(None),
        Err(TryRecvError::Disconnected) => Err("The extension approval check stopped.".into()),
    }
}

fn selection_bytes(text: &str, selection: advanced::Selection) -> Result<Range<usize>, String> {
    let start = advanced::byte(text, selection.start());
    let end = advanced::byte(text, selection.end());
    if start == end {
        return Err("Select text to run this extension command.".into());
    }
    if end - start > plugin_runtime::MAX_SELECTION_BYTES {
        return Err("The selection is too large for a Lua extension.".into());
    }
    // A cursor at the visual end of a CRLF line can land inside its separator.
    for at in [start, end] {
        if at != 0 && text.as_bytes().get(at - 1..=at) == Some(b"\r\n") {
            return Err("Select the whole line break before running this command.".into());
        }
    }
    Ok(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::app::{extensions::tests::app_with, tests::finish_app_job, Command};
    use eframe::egui;
    use hedit::advanced::Selection;
    use std::path::Path;

    const ID: &str = "markdown-quote";
    const COMMAND: &str = "markdown-quote.quote";
    const MANIFEST: &str = include_str!("../../../examples/extensions/markdown-quote.json");

    fn approved(config: &Path, text: &str) -> (tempfile::TempDir, egui::Context, HeditApp, String) {
        let directory = config.join("extensions");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("markdown-quote.json"), MANIFEST).unwrap();
        let catalog = catalog::read(config);
        assert!(catalog.contributions.is_empty());
        let digest = catalog.view.files[0]
            .details
            .as_ref()
            .unwrap()
            .lua
            .as_ref()
            .unwrap()
            .digest
            .clone();
        catalog::set_lua_approved(config, ID, &digest, true).unwrap();
        let (session, ctx, app) = app_with(text, Some(config));
        assert_eq!(app.extensions.len(), 1);
        (session, ctx, app, digest)
    }

    fn fake_result(app: &mut HeditApp, text: &str) {
        let plugin = match &app.extensions[0].action {
            super::super::ExtensionAction::Lua(plugin) => plugin.clone(),
            _ => panic!("expected a Lua command"),
        };
        let proposal = app.lua_proposal(plugin).unwrap();
        app.lua_transform = Some(LuaTransform {
            proposal,
            phase: Phase::Running(RunningPlugin::from_test_result(Ok(text.into()))),
        });
    }

    fn finish_transform(app: &mut HeditApp) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.lua_transform.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "Lua proposal did not finish"
            );
            app.poll_lua_transform();
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn approved_lua_result_replaces_a_single_unicode_selection_and_undoes_once() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "head\nλ\nβ\ntail\n");
        app.edit_command(editing::Operation::ToCrlf);
        let original = app.active().page.text.clone();
        assert!(!app.extension_available_by_id(COMMAND));
        app.set_selections(vec![Selection {
            anchor: 10,
            head: 6,
        }]);
        assert_eq!(
            app.extension_available_by_id(COMMAND),
            plugin_runtime::SUPPORTED
        );
        fake_result(&mut app, "> λ\n> β");
        finish_transform(&mut app);
        assert_eq!(app.active().page.text, "head\r\n> λ\r\n> β\r\ntail\r\n");
        assert_eq!(app.active().page.info.eol, "CRLF");
        assert_eq!(app.selections(), [Selection::caret(14)]);
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, original);
    }

    #[test]
    fn changing_revision_or_selection_discards_a_late_lua_result() {
        let config = tempfile::tempdir().unwrap();
        let (_session, _ctx, mut app, _) = approved(config.path(), "abc def");
        let selection = Selection { anchor: 0, head: 3 };
        app.set_selections(vec![selection]);
        fake_result(&mut app, "replacement");
        app.set_selections(vec![Selection { anchor: 4, head: 7 }]);
        app.poll_lua_transform();
        assert_eq!(app.active().page.text, "abc def");
        assert!(app.lua_transform.is_none());
        assert!(app.notice.contains("context changed"));

        app.set_selections(vec![selection]);
        fake_result(&mut app, "replacement");
        assert!(app.apply_edit(
            editing::Edit {
                text: "abc def!".into(),
                anchor: selection.anchor,
                head: selection.head,
            },
            None
        ));
        app.poll_lua_transform();
        assert_eq!(app.active().page.text, "abc def!");
        assert!(app.lua_transform.is_none());
    }

    #[test]
    fn revoke_and_reapprove_discards_an_in_flight_lua_proposal() {
        let config = tempfile::tempdir().unwrap();
        let (_session, _ctx, mut app, digest) = approved(config.path(), "selected");
        app.set_selections(vec![Selection { anchor: 0, head: 8 }]);
        fake_result(&mut app, "replacement");
        catalog::set_lua_approved(config.path(), ID, &digest, false).unwrap();
        catalog::set_lua_approved(config.path(), ID, &digest, true).unwrap();
        finish_transform(&mut app);
        assert_eq!(app.active().page.text, "selected");
        assert!(app.error.as_deref().unwrap().contains("approval changed"));
    }

    #[test]
    fn selection_limits_count_utf8_bytes_and_preserve_crlf_pairs() {
        let text = "λ\r\n";
        let selected = |anchor, head| selection_bytes(text, advanced::Selection { anchor, head });
        assert_eq!(selected(1, 0).unwrap(), 0..2);
        assert_eq!(selected(1, 3).unwrap(), 2..4);
        assert!(selected(0, 2).is_err());
        assert!(selected(2, 3).is_err());
        assert!(selected(3, 4).is_err());

        let text = "λ".repeat(plugin_runtime::MAX_SELECTION_BYTES / 2 + 1);
        assert!(selection_bytes(
            &text,
            advanced::Selection {
                anchor: 0,
                head: plugin_runtime::MAX_SELECTION_BYTES / 2,
            }
        )
        .is_ok());
        assert!(selection_bytes(
            &text,
            advanced::Selection {
                anchor: 0,
                head: plugin_runtime::MAX_SELECTION_BYTES / 2 + 1,
            }
        )
        .is_err());
    }
}
