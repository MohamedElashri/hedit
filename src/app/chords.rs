//! Two-key shortcuts such as Ctrl+K, Ctrl+D, as in Sublime Text.
use super::*;
use egui::KeyboardShortcut;

/// Sublime's default chords. `Primary` is Ctrl on Linux and Cmd on macOS.
pub(super) const DEFAULT_CHORDS: [(Command, &str); 8] = [
    (Command::DeleteToLineEnd, "Primary+K, Primary+K"),
    (Command::DeleteToLineStart, "Primary+K, Primary+Backspace"),
    (Command::Sidebar, "Primary+K, Primary+B"),
    (Command::SkipOccurrence, "Primary+K, Primary+D"),
    (Command::Uppercase, "Primary+K, Primary+U"),
    (Command::Lowercase, "Primary+K, Primary+L"),
    (Command::FoldAll, "Primary+K, Primary+1"),
    (Command::UnfoldAll, "Primary+K, Primary+J"),
];

/// A shortcut of one key or a two-key chord, as written in keybindings.json.
pub(super) fn parse(value: &str) -> Result<Vec<KeyboardShortcut>, String> {
    if value.trim().is_empty() {
        return Ok(vec![]);
    }
    let keys = value
        .split(',')
        .map(|part| {
            quality_windows::shortcut(part)?.ok_or_else(|| format!("Invalid shortcut: {value}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if keys.len() > 2 {
        return Err(format!("Shortcuts have at most two keys: {value}"));
    }
    Ok(keys)
}

/// The first key pressed this frame, with the modifiers it was pressed with.
fn pressed(ctx: &egui::Context) -> Option<(Key, Modifiers)> {
    ctx.input(|i| {
        i.events.iter().find_map(|e| match e {
            egui::Event::Key {
                key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } => Some((*key, *modifiers)),
            _ => None,
        })
    })
}

fn matches(shortcut: &KeyboardShortcut, key: Key, modifiers: Modifiers) -> bool {
    shortcut.logical_key == key && modifiers.matches_exact(shortcut.modifiers)
}

fn consume_keys(ctx: &egui::Context) {
    ctx.input_mut(|i| {
        i.events
            .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
    });
}

impl HeditApp {
    /// Every active chord: user bindings, then defaults not overridden.
    fn chords(&self) -> Vec<(Command, KeyboardShortcut, KeyboardShortcut)> {
        let user = self.quality.config.keybindings.iter().filter_map(|b| {
            match parse(&b.shortcut).ok()?.as_slice() {
                [first, second] => Some((b.command, *first, *second)),
                _ => None,
            }
        });
        let defaults = DEFAULT_CHORDS
            .iter()
            .filter(|(command, _)| !self.overridden(*command))
            .filter_map(|(command, text)| match parse(text).ok()?.as_slice() {
                [first, second] => Some((*command, *first, *second)),
                _ => None,
            });
        user.chain(defaults).collect()
    }
    /// Start or finish a chord. Returns true when the keys were used, so no
    /// other shortcut sees them.
    pub(super) fn handle_chords(&mut self, ctx: &egui::Context) -> bool {
        let Some((key, modifiers)) = pressed(ctx) else {
            return false;
        };
        let label = |s: &KeyboardShortcut| ctx.format_shortcut(s);
        if let Some(first) = self.chord.take() {
            consume_keys(ctx);
            self.focus_editor = true;
            if key == Key::Escape && modifiers.is_none() {
                self.notice = "Shortcut cancelled.".into();
                return true;
            }
            let found = self
                .chords()
                .into_iter()
                .find(|(_, a, b)| *a == first && matches(b, key, modifiers));
            match found {
                Some((command, _, _)) => {
                    self.notice.clear();
                    self.execute(ctx, command);
                }
                None => {
                    self.notice = format!(
                        "{} is not followed by that key in any shortcut.",
                        label(&first)
                    );
                }
            }
            return true;
        }
        let first = self
            .chords()
            .into_iter()
            .map(|(_, a, _)| a)
            .find(|a| matches(a, key, modifiers));
        if let Some(first) = first {
            consume_keys(ctx);
            self.notice = format!("{} was pressed. Waiting for the second key…", label(&first));
            self.chord = Some(first);
            return true;
        }
        false
    }
}
