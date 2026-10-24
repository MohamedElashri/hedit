//! Running builds and the build results panel, as in Sublime Text.
use super::*;
use hedit::build::{self, Event, Location, Running};
use std::path::Path;

/// Output beyond this is dropped, with a note, so a runaway build cannot
/// exhaust memory.
const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub(super) struct BuildState {
    pub open: bool,
    running: Option<Running>,
    /// The build waits for the active file to finish saving.
    pending: bool,
    pub name: String,
    pub output: String,
    /// Bytes of `output` already searched for locations.
    parsed: usize,
    parsed_lines: usize,
    pub locations: Vec<Location>,
    pub current: Option<usize>,
    regex: String,
    dir: PathBuf,
    pub status: String,
}

impl BuildState {
    pub(super) fn running(&self) -> bool {
        self.running.is_some()
    }
    /// Find locations in the complete lines received so far.
    fn parse(&mut self, all: bool) {
        let end = if all {
            self.output.len()
        } else {
            self.output.rfind('\n').map_or(0, |n| n + 1)
        };
        if end <= self.parsed {
            return;
        }
        let text = &self.output[self.parsed..end];
        self.locations.extend(build::locations(
            text,
            &self.regex,
            &self.dir,
            self.parsed_lines,
        ));
        self.parsed_lines += text.lines().count();
        self.parsed = end;
    }
}

impl HeditApp {
    /// Build the active file with the chosen (or automatic) build system,
    /// saving it first.
    pub(super) fn build(&mut self, ctx: &egui::Context) {
        self.quality.results_newer = false;
        let b = self.active();
        let Some(path) = b.page.info.path.clone() else {
            self.error = Some("Save the file before building it.".into());
            return;
        };
        if b.page.info.dirty {
            self.build.pending = true;
            self.save_file(ctx, false);
            return;
        }
        let systems = build::all(&self.settings.build_systems);
        let language = b.language();
        let Some(system) =
            build::choose(&systems, &self.settings.build_system, language.id()).cloned()
        else {
            self.error = Some(format!(
                "No build system for {}. Choose one with Tools → Build with… (Ctrl+Shift+B).",
                language.label()
            ));
            return;
        };
        let file = PathBuf::from(&path);
        let folder = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .find(|f| file.starts_with(f))
            .or_else(|| file.parent().map(Path::to_path_buf))
            .unwrap_or_default();
        let variables = build::Variables { file, folder };
        let dir = if system.working_dir.is_empty() {
            variables
                .file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default()
        } else {
            PathBuf::from(variables.expand(&system.working_dir))
        };
        let command = variables.expand(&system.shell_cmd);
        if let Some(previous) = self.build.running.take() {
            previous.cancel();
        }
        let wake = {
            let ctx = ctx.clone();
            move || ctx.request_repaint()
        };
        match Running::start(&command, &dir, wake) {
            Ok(running) => {
                self.build = BuildState {
                    open: true,
                    running: Some(running),
                    name: system.name.clone(),
                    regex: system.file_regex.clone(),
                    dir,
                    status: format!("Building with {}…", system.name),
                    ..Default::default()
                };
            }
            Err(error) => self.error = Some(error),
        }
    }
    /// After a save finishes, run a build that was waiting for it.
    pub(super) fn build_after_save(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.build.pending) && !self.active().page.info.dirty {
            self.build(ctx);
        }
    }
    pub(super) fn cancel_build(&mut self) {
        if let Some(running) = &self.build.running {
            running.cancel();
            self.build.status = "Cancelling…".into();
        }
    }
    /// Take new output and the result of a running build.
    pub(super) fn poll_build(&mut self) {
        let Some(running) = &self.build.running else {
            return;
        };
        let mut finished = None;
        while let Ok(event) = running.events.try_recv() {
            match event {
                Event::Output(text) => {
                    if self.build.output.len() < OUTPUT_LIMIT {
                        self.build.output.push_str(&text);
                        if self.build.output.len() >= OUTPUT_LIMIT {
                            self.build.output.push_str("\n[Output truncated]\n");
                        }
                    }
                }
                Event::Finished(code, elapsed) => finished = Some((code, elapsed)),
            }
        }
        self.build.parse(false);
        if let Some((code, elapsed)) = finished {
            self.build.running = None;
            self.build.parse(true);
            let time = format!("{:.1}s", elapsed.as_secs_f32());
            let summary = match code {
                Some(0) => format!("[Finished in {time}]"),
                Some(code) => format!("[Finished in {time} with exit code {code}]"),
                None => format!("[Cancelled after {time}]"),
            };
            if !self.build.output.is_empty() && !self.build.output.ends_with('\n') {
                self.build.output.push('\n');
            }
            self.build.output.push_str(&summary);
            let errors = self.build.locations.len();
            self.build.status = match errors {
                0 => summary,
                1 => format!("{summary} · 1 location; F4 jumps to it"),
                n => format!("{summary} · {n} locations; F4 jumps to the next"),
            };
        }
    }
    /// Jump to the next (or previous) location in the build output.
    pub(super) fn next_result(&mut self, forward: bool) {
        // Find in folders results, when they are newer than the build's.
        let hits = self.quality.results.hits.len();
        if self.quality.results_newer && hits > 0 {
            let next = match (self.quality.result_current, forward) {
                (None, true) => 0,
                (None, false) => hits - 1,
                (Some(i), true) => (i + 1) % hits,
                (Some(i), false) => (i + hits - 1) % hits,
            };
            self.quality.result_current = Some(next);
            let hit = &self.quality.results.hits[next];
            self.pending_paths.push_back(PathBuf::from(format!(
                "{}:{}:{}",
                hit.path.display(),
                hit.line,
                hit.column
            )));
            self.notice = format!("Match {} of {hits}", next + 1);
            return;
        }
        let count = self.build.locations.len();
        if count == 0 {
            self.notice = "No build results to go to.".into();
            return;
        }
        let next = match (self.build.current, forward) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => (i + count - 1) % count,
        };
        self.go_to_location(next);
    }
    fn go_to_location(&mut self, index: usize) {
        self.build.current = Some(index);
        self.build.open = true;
        let location = &self.build.locations[index];
        let column = location.column.map_or(String::new(), |c| format!(":{c}"));
        self.pending_paths.push_back(PathBuf::from(format!(
            "{}:{}{column}",
            location.file.display(),
            location.line
        )));
        if !location.message.is_empty() {
            self.notice = location.message.clone();
        }
    }
    /// The build results panel above the status bar.
    pub(super) fn build_panel(&mut self, ctx: &egui::Context) {
        if !self.build.open {
            return;
        }
        let mut jump = None;
        let mut close = false;
        let mut cancel = false;
        egui::TopBottomPanel::bottom("build-panel")
            .resizable(true)
            .default_height(180.0)
            .height_range(80.0..=(ctx.screen_rect().height() * 0.6).max(100.0))
            .frame(
                egui::Frame::new()
                    .fill(self.theme.surface.color())
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.build.name).strong());
                    if self.build.running() {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.build.status).color(self.theme.muted.color()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = controls::close_button(ui, "Hide build results · Esc").clicked();
                        if self.build.running() {
                            cancel = ui.button("Cancel").clicked();
                        }
                    });
                });
                let lines: Vec<&str> = self.build.output.lines().collect();
                let current_line = self
                    .build
                    .current
                    .map(|i| self.build.locations[i].output_line);
                let row = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.build.running())
                    .show_rows(ui, row, lines.len(), |ui, range| {
                        for n in range {
                            let location =
                                self.build.locations.iter().position(|l| l.output_line == n);
                            let backdrop = ui.painter().add(egui::Shape::Noop);
                            let mut text = RichText::new(lines[n]).monospace();
                            if location.is_some() {
                                text = text.color(self.theme.warning.color());
                            }
                            let response = ui.add(
                                egui::Label::new(text)
                                    .sense(if location.is_some() {
                                        egui::Sense::click()
                                    } else {
                                        egui::Sense::hover()
                                    })
                                    .truncate(),
                            );
                            if current_line == Some(n) {
                                ui.painter().set(
                                    backdrop,
                                    egui::Shape::rect_filled(
                                        response.rect,
                                        2.0,
                                        self.theme.selection.color(),
                                    ),
                                );
                            }
                            if let Some(index) = location {
                                if response
                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                    .clicked()
                                {
                                    jump = Some(index);
                                }
                            }
                        }
                    });
            });
        if close {
            self.build.open = false;
            self.focus_editor = true;
        }
        if cancel {
            self.cancel_build();
        }
        if let Some(index) = jump {
            self.go_to_location(index);
        }
    }
}
