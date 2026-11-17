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
/// taken for Ctrl+Tab. Some keyboard layouts also type the key's character
/// with Alt, as in Alt+1 or Alt+Q; that key's paired text is dropped too.
pub(super) fn take_exact(ctx: &egui::Context, modifiers: Modifiers, key: Key) -> bool {
    ctx.input_mut(|i| {
        let matches = |e: &egui::Event| {
            matches!(e, egui::Event::Key { key: k, pressed: true, modifiers: m, .. } if *k == key && m.matches_exact(modifiers))
        };
        let found = i.events.iter().position(matches);
        let Some(n) = found else {
            return false;
        };
        // Winit emits a key's text immediately after its Key event. Shift or
        // the keyboard layout can change it (e.g. Alt+Shift+2 produces '@').
        // Suppress paired text for coalesced repeats too, while taking only one
        // key press. Never remove unrelated typing elsewhere in the batch.
        if modifiers.alt
            && !modifiers.ctrl
            && !modifiers.command
            && !modifiers.mac_cmd
            && key.name().chars().count() == 1
        {
            for index in (n..i.events.len().saturating_sub(1)).rev() {
                if matches(&i.events[index])
                    && matches!(i.events.get(index + 1), Some(egui::Event::Text(_)))
                {
                    i.events.remove(index + 1);
                }
            }
        }
        i.events.remove(n);
        true
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
        if index >= self.workspace.open.len() {
            return;
        }
        self.select_group_tab(index);
        self.leave_review();
        if index == self.workspace.active {
            self.focus_editor = true;
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
        self.sync_groups();
        let control =
            ctx.input(|i| i.modifiers.ctrl || (!cfg!(target_os = "macos") && i.modifiers.command));
        if self.tab_cycle.step.is_some() && !control {
            self.tab_cycle.step = None;
        }
        let open: Vec<u64> = self.workspace.open.iter().map(|b| b.view.id).collect();
        let open_ids: std::collections::HashSet<_> = open.iter().copied().collect();
        // Caches for closed files go too.
        self.fold_cache.retain(|view| open_ids.contains(&view));
        self.spell.forget_closed(&open_ids);
        let cycle = &mut self.tab_cycle;
        let mut seen = std::collections::HashSet::with_capacity(open.len());
        cycle
            .recent
            .retain(|id| open_ids.contains(id) && seen.insert(*id));
        for id in &open {
            if seen.insert(*id) {
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
        self.sync_groups();
        let local = self.group_tab_ids(self.panes.focused);
        let recent: Vec<_> = self
            .tab_cycle
            .recent
            .iter()
            .copied()
            .filter(|id| local.contains(id))
            .collect();
        let count = recent.len();
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
        let id = recent[step];
        if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == id) {
            self.select_tab(index);
        }
    }
    /// Ctrl+PageDown / Ctrl+PageUp: the next or previous tab in the tab strip.
    pub(super) fn step_tab(&mut self, forward: bool) {
        self.sync_groups();
        let ids = self.group_tab_ids(self.panes.focused);
        if ids.is_empty() {
            return;
        }
        let active = ids
            .iter()
            .position(|id| *id == self.active().view.id)
            .unwrap_or(0);
        self.select_tab_position((active + if forward { 1 } else { ids.len() - 1 }) % ids.len());
    }
    pub(super) fn select_tab_position(&mut self, position: usize) {
        self.sync_groups();
        if let Some(id) = self.group_tab_ids(self.panes.focused).get(position) {
            if let Some(index) = self.workspace.open.iter().position(|b| b.view.id == *id) {
                self.select_tab(index);
            }
        }
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
                self.execute(ctx, command);
            }
        }
    }
}
