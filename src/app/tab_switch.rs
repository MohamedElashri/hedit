//! Switching tabs from the keyboard: in recently used order (Ctrl+Tab), in
//! tab order (Ctrl+PageUp/PageDown), or by position (Alt+1–9, Cmd+1–9 on macOS).
use super::*;

#[derive(Default)]
pub(super) struct TabCycle {
    /// Open views, most recently used first.
    recent: Vec<u64>,
    /// How far Ctrl+Tab has stepped through `recent` while Ctrl is held.
    step: Option<usize>,
}

/// Take one key press whose modifiers match exactly, so Ctrl+Shift+Tab is not
/// taken for Ctrl+Tab.
pub(super) fn take_exact(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| {
        let found = i.events.iter().position(|e| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. } if *k == key && m.matches_exact(modifiers))
        });
        found.map(|n| i.events.remove(n)).is_some()
    })
}

/// The key that selects a tab by position, for tabs 1 to 9.
pub(super) const TAB_KEYS: [(Command, Key); 9] = [
    (Command::Tab1, Key::Num1),
    (Command::Tab2, Key::Num2),
    (Command::Tab3, Key::Num3),
    (Command::Tab4, Key::Num4),
    (Command::Tab5, Key::Num5),
    (Command::Tab6, Key::Num6),
    (Command::Tab7, Key::Num7),
    (Command::Tab8, Key::Num8),
    (Command::Tab9, Key::Num9),
];

impl HeditApp {
    /// Make a tab the active one, as clicking it does.
    pub(super) fn select_tab(&mut self, index: usize) {
        if index >= self.workspace.open.len() || index == self.workspace.active {
            return;
        }
        self.navigation_mark();
        self.workspace.active = index;
        self.navigation_mark();
        self.changed();
        self.focus_editor = true;
    }
    /// Keep the recently used order current. While Ctrl is held after Ctrl+Tab
    /// the order stays put, so repeated presses walk further back; releasing
    /// Ctrl makes the chosen tab the most recent.
    pub(super) fn track_recent_tabs(&mut self, ctx: &egui::Context) {
        let cycle = &mut self.tab_cycle;
        let control =
            ctx.input(|i| i.modifiers.ctrl || (!cfg!(target_os = "macos") && i.modifiers.command));
        if cycle.step.is_some() && !control {
            cycle.step = None;
        }
        let open: Vec<u64> = self.workspace.open.iter().map(|b| b.view.id).collect();
        cycle.recent.retain(|id| open.contains(id));
        for id in &open {
            if !cycle.recent.contains(id) {
                cycle.recent.push(*id);
            }
        }
        let current = self.workspace.open[self.workspace.active].view.id;
        if cycle.step.is_none() && cycle.recent.first() != Some(&current) {
            cycle.recent.retain(|id| *id != current);
            cycle.recent.insert(0, current);
        }
    }
    /// Ctrl+Tab: the next tab back in recently used order.
    pub(super) fn cycle_recent(&mut self, back: bool) {
        let count = self.tab_cycle.recent.len();
        if count < 2 {
            return;
        }
        let step = self.tab_cycle.step.unwrap_or(0);
        let step = if back {
            (step + count - 1) % count
        } else {
            (step + 1) % count
        };
        self.tab_cycle.step = Some(step);
        let id = self.tab_cycle.recent[step];
        if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
            self.select_tab(index);
        }
    }
    /// Ctrl+PageDown / Ctrl+PageUp: the next or previous tab in the tab strip.
    pub(super) fn step_tab(&mut self, forward: bool) {
        let count = self.workspace.open.len();
        let active = self.workspace.active;
        self.select_tab(if forward {
            (active + 1) % count
        } else {
            (active + count - 1) % count
        });
    }
    pub(super) fn tab_shortcuts(&mut self, ctx: &egui::Context) {
        // Control+Tab on every platform, as Cmd+Tab switches applications on macOS.
        let control = if cfg!(target_os = "macos") {
            Modifiers::CTRL
        } else {
            Modifiers::COMMAND
        };
        let control_shift = Modifiers {
            shift: true,
            ..control
        };
        let position = if cfg!(target_os = "macos") {
            Modifiers::COMMAND
        } else {
            Modifiers::ALT
        };
        let mut keys = vec![
            (control_shift, Key::Tab, Command::RecentTabBack),
            (control, Key::Tab, Command::RecentTab),
            (control, Key::PageDown, Command::NextTab),
            (control, Key::PageUp, Command::PreviousTab),
        ];
        keys.extend(TAB_KEYS.map(|(command, key)| (position, key, command)));
        for (modifiers, key, command) in keys {
            if !self.overridden(command) && take_exact(ctx, modifiers, key) {
                // Some keyboard layouts also type the digit for Alt+1.
                let digit = key.name();
                if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) {
                    ctx.input_mut(|i| {
                        i.events
                            .retain(|e| !matches!(e, egui::Event::Text(t) if t == digit))
                    });
                }
                self.execute(ctx, command);
            }
        }
    }
}
