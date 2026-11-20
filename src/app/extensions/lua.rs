//! A script gets selected text and can propose one normal editor edit.
use super::{
    activity::{Invocation, InvocationOutcome},
    catalog,
    manifest::LuaPlugin,
    options,
};
use crate::{
    app::{clipboard_lines, HeditApp},
    plugin_runtime::{self, RunningPlugin},
};
use eframe::egui;
use hedit::{
    advanced::{self, Selection},
    editing,
};
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

impl LuaTransform {
    pub(in crate::app) fn needs_poll(&self) -> bool {
        !matches!(self.phase, Phase::Preview { .. })
    }
}

enum Phase {
    Checking(Receiver<Option<File>>),
    Running(RunningPlugin),
    Preview {
        diff: Vec<DiffLine>,
        replacements: Vec<String>,
    },
    Committing {
        approval: Receiver<Option<File>>,
        replacements: Vec<String>,
    },
}

struct DiffLine {
    added: bool,
    text: String,
}

struct Proposal {
    target: Target,
    ranges: Vec<Range<usize>>,
    plugin: LuaPlugin,
    invocation: Invocation,
}

#[derive(Debug, PartialEq, Eq)]
struct Target {
    document: u64,
    revision: u64,
    group: u64,
    page_start: u64,
    language: &'static str,
    primary: Selection,
    extra: Vec<Selection>,
}

impl HeditApp {
    fn lua_target(&self) -> Option<Target> {
        let editor = self.active();
        let info = &editor.page.info;
        let primary = Selection {
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
            || (primary.anchor == primary.head
                && editor
                    .view
                    .selections
                    .iter()
                    .all(|range| range.anchor == range.head))
        {
            return None;
        }
        Some(Target {
            document: info.id,
            revision: info.revision,
            group: self.panes.focused_id(),
            page_start: editor.page.start,
            language: editor.language().id(),
            primary,
            extra: editor.view.selections.clone(),
        })
    }

    pub(super) fn start_lua_transform(&mut self, command: &str, plugin: LuaPlugin) {
        let proposal = self.lua_proposal(command, plugin);
        match proposal {
            Ok(proposal) => match self.request_lua_approval(&proposal.plugin) {
                Ok(approval) => {
                    self.lua_transform = Some(LuaTransform {
                        proposal,
                        phase: Phase::Checking(approval),
                    });
                    self.notice = "Running extension command…".into();
                }
                Err(error) => {
                    self.extension_manager
                        .record(proposal.invocation, InvocationOutcome::Denied);
                    self.error = Some(error);
                }
            },
            Err(error) => {
                self.extension_manager
                    .record(Invocation::new(command), InvocationOutcome::Denied);
                self.error = Some(error);
            }
        }
    }

    fn lua_proposal(&self, command: &str, plugin: LuaPlugin) -> Result<Proposal, String> {
        let target = self
            .lua_target()
            .ok_or("Select text in a local editable buffer.")?;
        let ranges = selected_ranges(&self.active().page.text, self.selections())?;
        Ok(Proposal {
            target,
            ranges,
            plugin,
            invocation: Invocation::new(command),
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

    pub(in crate::app) fn poll_lua_transform(&mut self, ctx: &egui::Context) {
        let Some(mut job) = self.lua_transform.take() else {
            return;
        };
        if self.lua_target().as_ref() != Some(&job.proposal.target) {
            self.extension_manager
                .record(job.proposal.invocation, InvocationOutcome::Cancelled);
            self.notice = "Extension command canceled because the editor context changed.".into();
            return;
        }
        match self.advance_lua_transform(ctx, &mut job) {
            Ok(None) => self.lua_transform = Some(job),
            Ok(Some(outcome)) => self
                .extension_manager
                .record(job.proposal.invocation, outcome),
            Err(error) => {
                let outcome = if error.contains("approval changed") {
                    InvocationOutcome::Denied
                } else {
                    InvocationOutcome::worker_failure(&error)
                };
                self.extension_manager
                    .record(job.proposal.invocation, outcome);
                self.notice = "Extension command failed.".into();
                self.error = Some(error);
            }
        }
    }

    fn advance_lua_transform(
        &mut self,
        ctx: &egui::Context,
        job: &mut LuaTransform,
    ) -> Result<Option<InvocationOutcome>, String> {
        let proposal = &job.proposal;
        match &mut job.phase {
            Phase::Checking(approval) => {
                if let Some(approval) = receive_approval(approval)? {
                    let selection = proposal
                        .ranges
                        .iter()
                        .map(|range| self.active().page.text[range.clone()].to_owned())
                        .collect();
                    let input = options::input(
                        &self.settings.extensions,
                        &proposal.plugin,
                        proposal.target.language,
                        selection,
                    );
                    job.phase = Phase::Running(plugin_runtime::start(input, approval)?);
                }
            }
            Phase::Running(worker) => {
                if let Some(replacements) = worker.try_result() {
                    let replacements = replacements?;
                    if self.settings.extensions.preview_edits {
                        let diff = edit_diff(self.active(), &proposal.ranges, &replacements);
                        if diff.is_empty() {
                            self.notice = "The extension proposed no changes.".into();
                            return Ok(Some(InvocationOutcome::Applied));
                        }
                        job.phase = Phase::Preview { diff, replacements };
                        ctx.request_repaint();
                    } else {
                        job.phase = Phase::Committing {
                            replacements,
                            approval: self.request_lua_approval(&proposal.plugin)?,
                        };
                    }
                }
            }
            Phase::Preview { diff, replacements } => match show_preview(ctx, diff) {
                Some(true) => {
                    job.phase = Phase::Committing {
                        replacements: std::mem::take(replacements),
                        approval: self.request_lua_approval(&proposal.plugin)?,
                    };
                }
                Some(false) => {
                    self.notice = "Extension changes discarded.".into();
                    self.focus_editor = true;
                    return Ok(Some(InvocationOutcome::Discarded));
                }
                None => {}
            },
            Phase::Committing {
                approval,
                replacements,
            } => {
                if let Some(_approval) = receive_approval(approval)? {
                    let result = self.apply_lua_replacements(&proposal.ranges, replacements);
                    return Ok(Some(if result {
                        InvocationOutcome::Applied
                    } else {
                        InvocationOutcome::Failed
                    }));
                }
            }
        }
        Ok(None)
    }

    fn apply_lua_replacements(&mut self, ranges: &[Range<usize>], replacements: &[String]) -> bool {
        let editor = self.active();
        let (text, selections) = build_edit(
            &editor.page.text,
            ranges,
            replacements,
            editor.page.info.eol == "CRLF",
        );
        let cursor = selections.last().map_or(0, |selection| selection.head);
        let edit = editing::Edit {
            text,
            anchor: cursor,
            head: cursor,
        };
        if self.apply_edit(edit, None) {
            if selections.len() > 1 {
                self.set_selections(selections);
            }
            self.snippet = None;
            self.notice = "Extension command finished.".into();
            return true;
        }
        false
    }
}

fn build_edit(
    text: &str,
    ranges: &[Range<usize>],
    replacements: &[String],
    crlf: bool,
) -> (String, Vec<Selection>) {
    let mut edited = String::with_capacity(text.len());
    let mut selections = Vec::with_capacity(ranges.len());
    let mut previous = 0;
    let mut cursor = 0;
    for (range, replacement) in ranges.iter().zip(replacements) {
        let before = &text[previous..range.start];
        let replacement = clipboard_lines::line_endings(replacement, crlf);
        edited.push_str(before);
        edited.push_str(&replacement);
        cursor += before.chars().count() + replacement.chars().count();
        selections.push(Selection::caret(cursor));
        previous = range.end;
    }
    edited.push_str(&text[previous..]);
    (edited, selections)
}

fn edit_diff(
    buffer: &hedit::workspace::Buffer,
    ranges: &[Range<usize>],
    replacements: &[String],
) -> Vec<DiffLine> {
    let crlf = buffer.page.info.eol == "CRLF";
    let mut diff = Vec::new();
    for (range, replacement) in ranges.iter().zip(replacements) {
        let original = &buffer.page.text[range.clone()];
        let replacement = clipboard_lines::line_endings(replacement, crlf);
        if original != replacement {
            for (added, text) in [(false, original), (true, replacement.as_str())] {
                for line in text.split_inclusive('\n') {
                    let line = line
                        .strip_suffix("\r\n")
                        .or_else(|| line.strip_suffix('\n'))
                        .unwrap_or(line)
                        .replace('\r', "␍");
                    let prefix = if added { '+' } else { '−' };
                    diff.push(DiffLine {
                        added,
                        text: format!("{prefix} {line}"),
                    });
                }
            }
        }
    }
    diff
}

fn show_preview(ctx: &egui::Context, diff: &[DiffLine]) -> Option<bool> {
    let modal = egui::Modal::new(egui::Id::new("lua-edit-preview")).show(ctx, |ui| {
        ui.set_width((ctx.screen_rect().width() - 64.0).clamp(224.0, 740.0));
        ui.heading("Extension edit preview");
        let height = ui.text_style_height(&egui::TextStyle::Monospace);
        egui::ScrollArea::both()
            .max_height((ctx.screen_rect().height() * 0.52).max(70.0))
            .show_rows(ui, height, diff.len(), |ui, lines| {
                for line in &diff[lines] {
                    let color = if line.added {
                        ui.visuals().strong_text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    ui.add(
                        egui::Label::new(egui::RichText::new(&line.text).monospace().color(color))
                            .extend(),
                    );
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Discard").clicked() {
                Some(false)
            } else if ui.button("Apply changes").clicked() {
                Some(true)
            } else {
                None
            }
        })
        .inner
    });
    if modal.should_close() {
        Some(false)
    } else {
        modal.inner
    }
}

// The returned lock serializes delivery or application with a revocation.
fn receive_approval(approval: &Receiver<Option<File>>) -> Result<Option<File>, String> {
    match approval.try_recv() {
        Ok(Some(guard)) => Ok(Some(guard)),
        Ok(None) => Err("Extension approval changed. Review it in Settings → Extensions.".into()),
        Err(TryRecvError::Empty) => Ok(None),
        Err(TryRecvError::Disconnected) => Err("The extension approval check stopped.".into()),
    }
}

fn selected_ranges(text: &str, selections: Vec<Selection>) -> Result<Vec<Range<usize>>, String> {
    let selections: Vec<_> = selections
        .into_iter()
        .filter(|selection| selection.start() != selection.end())
        .collect();
    if selections.len() > plugin_runtime::MAX_SELECTIONS {
        return Err("A Lua extension accepts at most 16 selections.".into());
    }
    let ranges: Vec<_> = selections
        .into_iter()
        .map(|selection| selection_bytes(text, selection))
        .collect::<Result<_, _>>()?;
    if ranges.iter().map(Range::len).sum::<usize>() > plugin_runtime::MAX_SELECTION_BYTES {
        return Err("Selected text exceeds the Lua input limit.".into());
    }
    Ok(ranges)
}

fn selection_bytes(text: &str, selection: Selection) -> Result<Range<usize>, String> {
    let start = advanced::byte(text, selection.start());
    let end = advanced::byte(text, selection.end());
    if start == end {
        return Err("Select text to run this extension command.".into());
    }
    if end - start > plugin_runtime::MAX_SELECTION_BYTES {
        return Err("The selection is too large for a Lua extension.".into());
    }
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
    use std::path::Path;

    const ID: &str = "markdown-quote";
    const COMMAND: &str = "markdown-quote.quote";
    const MANIFEST: &str = include_str!("../../../examples/extensions/markdown-quote.json");

    fn set_approval(config: &Path, digest: &str, approved: bool) {
        catalog::set_lua_approved(
            config,
            &config.join("extensions/markdown-quote.json"),
            ID,
            COMMAND,
            digest,
            approved,
        )
        .unwrap();
    }

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
        set_approval(config, &digest, true);
        let (session, ctx, mut app) = app_with(text, Some(config));
        app.settings.extensions.preview_edits = false;
        assert_eq!(app.extensions.len(), 1);
        (session, ctx, app, digest)
    }

    fn fake_result(app: &mut HeditApp, text: &str) {
        let plugin = match &app.extensions[0].action {
            super::super::ExtensionAction::Lua(plugin) => plugin.clone(),
            _ => panic!("expected a Lua command"),
        };
        let proposal = app.lua_proposal(COMMAND, plugin).unwrap();
        app.lua_transform = Some(LuaTransform {
            proposal,
            phase: Phase::Running(RunningPlugin::from_test_result(Ok(vec![text.into()]))),
        });
    }

    fn finish_transform(app: &mut HeditApp, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.lua_transform.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "Lua proposal did not finish"
            );
            app.poll_lua_transform(ctx);
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn preview_discards_or_applies_bounded_multi_selection_edits_atomically() {
        use crate::app::tests::{click_label, frame};

        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, _) = approved(config.path(), "α one β two");
        app.settings.extensions.preview_edits = true;
        let original = app.active().page.text.clone();
        app.set_selections(vec![
            Selection { anchor: 0, head: 1 },
            Selection { anchor: 6, head: 7 },
        ]);
        let plugin = match &app.extensions[0].action {
            super::super::ExtensionAction::Lua(plugin) => plugin.clone(),
            _ => unreachable!(),
        };
        let inject = |app: &mut HeditApp| {
            let proposal = app.lua_proposal(COMMAND, plugin.clone()).unwrap();
            app.lua_transform = Some(LuaTransform {
                proposal,
                phase: Phase::Running(RunningPlugin::from_test_result(Ok(vec![
                    "λ".into(),
                    "東京".into(),
                ]))),
            });
        };

        inject(&mut app);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        assert_eq!(app.active().page.text, original);
        click_label(&ctx, &mut app, "Discard");
        assert_eq!(app.active().page.text, original);
        assert!(app.lua_transform.is_none());

        inject(&mut app);
        frame(&ctx, &mut app, vec![]);
        frame(&ctx, &mut app, vec![]);
        click_label(&ctx, &mut app, "Apply changes");
        finish_transform(&mut app, &ctx);
        assert_eq!(app.active().page.text, "λ one 東京 two");
        app.execute(&ctx, Command::Undo);
        finish_app_job(&mut app, &ctx);
        assert_eq!(app.active().page.text, original);
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
        finish_transform(&mut app, &ctx);
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
        let (_session, ctx, mut app, _) = approved(config.path(), "abc def");
        let selection = Selection { anchor: 0, head: 3 };
        app.set_selections(vec![selection]);
        fake_result(&mut app, "replacement");
        app.set_selections(vec![Selection { anchor: 4, head: 7 }]);
        app.poll_lua_transform(&ctx);
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
        app.poll_lua_transform(&ctx);
        assert_eq!(app.active().page.text, "abc def!");
        assert!(app.lua_transform.is_none());
    }

    #[test]
    fn revoke_and_reapprove_discards_an_in_flight_lua_proposal() {
        let config = tempfile::tempdir().unwrap();
        let (_session, ctx, mut app, digest) = approved(config.path(), "selected");
        app.set_selections(vec![Selection { anchor: 0, head: 8 }]);
        fake_result(&mut app, "replacement");
        set_approval(config.path(), &digest, false);
        set_approval(config.path(), &digest, true);
        finish_transform(&mut app, &ctx);
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
