//! Macros: record what is typed and the keys pressed in the editor, play it
//! back, and save it under a name in macros.json.
use super::*;
use serde::{Deserialize, Serialize};

/// One recorded step: typed text, a key with its modifiers (written as in
/// keybindings.json, such as `Primary+Shift+Left`), or a clipboard action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Step {
    Text(String),
    Key(String),
    Copy,
    Cut,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Macro {
    pub name: String,
    pub steps: Vec<Step>,
}

#[derive(Default)]
pub(super) struct Macros {
    /// Steps recorded so far, while recording.
    pub recording: Option<Vec<Step>>,
    /// The macro recorded last, for Play macro.
    pub last: Vec<Step>,
    /// Steps still to play, one batch per frame.
    pub playing: VecDeque<Step>,
    /// The name being typed in the Save macro window.
    pub naming: Option<String>,
    naming_focus: bool,
}

/// The keys that record and play macros, which are never recorded.
pub(super) fn macro_keys() -> (Modifiers, Modifiers) {
    let record = if cfg!(target_os = "macos") {
        Modifiers::CTRL
    } else {
        Modifiers::COMMAND | Modifiers::ALT
    };
    (
        record,
        Modifiers {
            shift: true,
            ..record
        },
    )
}

impl HeditApp {
    /// Ctrl+Alt+Q: start recording, or stop and keep the recording.
    pub(super) fn toggle_recording(&mut self) {
        match self.macros.recording.take() {
            Some(steps) if steps.is_empty() => self.notice = "Nothing was recorded.".into(),
            Some(steps) => {
                self.notice = format!("Macro recorded: {} steps.", steps.len());
                self.macros.last = steps;
            }
            None => {
                self.macros.recording = Some(vec![]);
                self.notice = format!(
                    "Recording a macro… {} stops.",
                    Command::RecordMacro.shortcut()
                );
            }
        }
    }
    /// Play the last recording, or a saved macro.
    pub(super) fn play_macro(&mut self, steps: Vec<Step>) {
        if self.macros.recording.is_some() {
            self.notice = "Stop recording before playing a macro.".into();
            return;
        }
        if steps.is_empty() {
            self.notice = "Record a macro first.".into();
            return;
        }
        self.macros.playing = steps.into();
        self.focus_editor = true;
    }
    /// Record this frame's typing and keys while the editor has focus.
    pub(super) fn record_input(&mut self, ctx: &egui::Context) {
        if self.macros.recording.is_none()
            || !self.macros.playing.is_empty()
            || !ctx.memory(|m| m.has_focus(self.editor_id()))
        {
            return;
        }
        let (record, play) = macro_keys();
        let events = ctx.input(|i| i.events.clone());
        let steps = self.macros.recording.as_mut().unwrap();
        for event in events {
            let step = match event {
                egui::Event::Text(text) | egui::Event::Paste(text) => Step::Text(text),
                egui::Event::Copy => Step::Copy,
                egui::Event::Cut => Step::Cut,
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if key == Key::Q
                        && (modifiers.matches_exact(record) || modifiers.matches_exact(play))
                    {
                        continue;
                    }
                    Step::Key(settings_pages::binding_text(modifiers, key))
                }
                _ => continue,
            };
            match (steps.last_mut(), &step) {
                (Some(Step::Text(previous)), Step::Text(text)) => previous.push_str(text),
                _ => steps.push(step),
            }
        }
    }
    /// Feed the next recorded step to the editor, as if it were typed now.
    pub(super) fn play_step(&mut self, ctx: &egui::Context) {
        let Some(step) = self.macros.playing.pop_front() else {
            return;
        };
        let event = match step {
            Step::Text(text) => egui::Event::Text(text),
            Step::Copy => egui::Event::Copy,
            Step::Cut => egui::Event::Cut,
            Step::Key(name) => match quality_windows::shortcut(&name) {
                Ok(Some(shortcut)) => egui::Event::Key {
                    key: shortcut.logical_key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: shortcut.modifiers,
                },
                _ => {
                    self.notice = format!("A macro step is not a key: {name}");
                    self.macros.playing.clear();
                    return;
                }
            },
        };
        ctx.input_mut(|i| i.events.insert(0, event));
        if self.macros.playing.is_empty() {
            self.notice = "Macro played.".into();
        } else {
            ctx.request_repaint();
        }
    }
    /// The Save macro window: name the last recording.
    pub(super) fn macro_name_window(&mut self, ctx: &egui::Context) {
        let Some(mut name) = self.macros.naming.take() else {
            return;
        };
        let mut open = true;
        let mut choice = controls::Choice::None;
        let shown = egui::Window::new("Save macro")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 90.0])
            .show(ctx, |ui| {
                ui.label("Name");
                let id = egui::Id::new("macro-name-input");
                let enter = ctx.memory(|m| m.has_focus(id))
                    && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                let response = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .id(id)
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut self.macros.naming_focus) {
                    response.request_focus();
                }
                if self
                    .quality
                    .config
                    .macros
                    .iter()
                    .any(|m| m.name == name.trim())
                {
                    ui.weak("A macro with this name will be replaced.");
                }
                let valid = !name.trim().is_empty();
                choice = controls::dialog_buttons(ui, &self.theme, "Save", valid, enter);
            });
        if choice == controls::Choice::Confirm {
            let name = name.trim().to_owned();
            let saved = Macro {
                name: name.clone(),
                steps: self.macros.last.clone(),
            };
            let macros = &mut self.quality.config.macros;
            match macros.iter_mut().find(|m| m.name == name) {
                Some(existing) => *existing = saved,
                None => macros.push(saved),
            }
            self.notice = format!("Saved macro “{name}”.");
            self.focus_editor = true;
            return;
        }
        if open && choice != controls::Choice::Cancel && !overlays::clicked_away(ctx, &shown) {
            self.macros.naming = Some(name);
        } else {
            self.focus_editor = true;
        }
    }
    pub(super) fn open_macro_name(&mut self) {
        if self.macros.last.is_empty() {
            self.notice = "Record a macro first.".into();
            return;
        }
        self.macros.naming = Some(String::new());
        self.macros.naming_focus = true;
        self.focus_editor = false;
    }
}
