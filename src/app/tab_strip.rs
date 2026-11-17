//! Derived tab geometry only; no document text or persistent UI cache.
use super::*;
use egui::Color32;
use std::path::Path;
#[derive(Default)]
pub(super) struct Cache {
    entries: Vec<Entry>,
    offsets: Vec<f32>,
    style: Option<(egui::FontId, f32, egui::Vec2)>,
    last_active: Option<(u64, usize)>,
    last_width: f32,
    dirty: bool,
    font_refresh: u8,
    height: f32,
}

fn title_job(
    title: &str,
    location: Option<&str>,
    color: Color32,
    muted: Color32,
    italics: bool,
    font: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        title,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::proportional(font),
            color,
            italics,
            ..Default::default()
        },
    );
    if let Some(location) = location {
        job.append(
            &format!(" · {}", hedit::bidi::display(location)),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional((font - 1.0).max(10.0)),
                color: muted,
                italics,
                ..Default::default()
            },
        );
    }
    job
}

fn locations(workspace: &Workspace) -> Vec<Option<String>> {
    let files: Vec<_> = workspace
        .open
        .iter()
        .map(|b| (b.view.name.as_str(), b.page.info.path.as_deref()))
        .collect();
    location_hints(&files)
}

/// Compare reversed parent components so neighbouring paths determine the shortest
/// distinguishing suffix. Only metadata changes rebuild these hints; no disk I/O.
fn location_hints(files: &[(&str, Option<&str>)]) -> Vec<Option<String>> {
    let mut groups = std::collections::BTreeMap::<&str, Vec<usize>>::new();
    for (i, (name, _)) in files.iter().enumerate() {
        groups.entry(name).or_default().push(i);
    }
    let mut hints = vec![None; files.len()];
    for group in groups.values().filter(|group| group.len() > 1) {
        let parents: std::collections::BTreeSet<_> = group
            .iter()
            .filter_map(|&i| files[i].1.map(parent_components))
            .collect();
        if parents.is_empty() {
            continue;
        }
        let parents: Vec<_> = parents.into_iter().collect();
        let mut labels = std::collections::BTreeMap::new();
        for (i, parent) in parents.iter().enumerate() {
            let common =
                |other: &Vec<String>| parent.iter().zip(other).take_while(|(a, b)| a == b).count();
            let shared = i
                .checked_sub(1)
                .map_or(0, |j| common(&parents[j]))
                .max(parents.get(i + 1).map_or(0, common));
            let suffix: PathBuf = parent[..(shared + 1).min(parent.len())]
                .iter()
                .rev()
                .collect();
            labels.insert(parent, suffix.to_string_lossy().into_owned());
        }
        for &i in group {
            hints[i] = Some(files[i].1.map_or_else(
                || "Unsaved".into(),
                |path| labels[&parent_components(path)].clone(),
            ));
        }
    }
    hints
}

fn parent_components(path: &str) -> Vec<String> {
    // SSH URIs contain a session identifier, not a user-facing directory name.
    let decoded = path.strip_prefix("hedit-ssh://").map(|tail| {
        let relative = tail.split_once('/').map_or("", |(_, path)| path);
        percent_encoding::percent_decode_str(relative)
            .decode_utf8_lossy()
            .into_owned()
    });
    let parent = Path::new(decoded.as_deref().unwrap_or(path))
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    parent
        .components()
        .rev()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect()
}
struct Entry {
    id: u64,
    name: String,
    path: Option<String>,
    location: Option<String>,
    label: String,
    pinned: bool,
    close: bool,
    pub title: String,
    width: f32,
}
impl Cache {
    pub fn invalidate_fonts(&mut self) {
        // egui installs new definitions on the next pass. Refresh both passes,
        // including offscreen tabs, before returning to cached geometry.
        self.font_refresh = 2;
    }
    pub fn sync(&mut self, ui: &egui::Ui, workspace: &Workspace) {
        let metrics = hedit::ui_style::Metrics::for_ui(ui);
        self.height = metrics.tab;
        let font = ui.style().text_styles[&egui::TextStyle::Button].clone();
        let style = (
            font,
            ui.ctx().pixels_per_point(),
            ui.spacing().button_padding,
        );
        let restyle = self.style.as_ref() != Some(&style) || self.font_refresh > 0;
        self.font_refresh = self.font_refresh.saturating_sub(1);
        let locations = (self.entries.len() != workspace.open.len()
            || self.entries.iter().zip(&workspace.open).any(|(e, b)| {
                e.id != b.view.id || e.name != b.view.name || e.path != b.page.info.path
            }))
        .then(|| locations(workspace));
        let mut changed = restyle || self.entries.len() != workspace.open.len() || self.dirty;
        for (i, b) in workspace.open.iter().enumerate() {
            let close = workspace.can_close(i);
            let location = locations
                .as_ref()
                .map_or_else(|| &self.entries[i].location, |locations| &locations[i]);
            if !restyle
                && self.entries.get(i).is_some_and(|e| {
                    e.id == b.view.id
                        && e.name == b.view.name
                        && e.path == b.page.info.path
                        && &e.location == location
                        && e.pinned == b.view.pinned
                        && e.close == close
                })
            {
                continue;
            }
            let visual = hedit::bidi::display(&b.view.name);
            let mut chars = visual.chars();
            let mut title: String = chars.by_ref().take(30).collect();
            if chars.next().is_some() {
                title.push('…');
            }
            if b.view.pinned {
                title.insert_str(0, "◆ ");
            }
            let label = location.as_ref().map_or_else(
                || b.view.name.clone(),
                |location| format!("{} · {location}", b.view.name),
            );
            let galley = egui::WidgetText::from(title_job(
                &title,
                location.as_deref(),
                Color32::WHITE,
                Color32::GRAY,
                false,
                metrics.font,
            ))
            .into_galley(
                ui,
                Some(egui::TextWrapMode::Extend),
                f32::INFINITY,
                egui::TextStyle::Button,
            );
            let width = 20.0
                + (galley.size().x + ui.spacing().button_padding.x * 2.0).max(96.0)
                + if close { metrics.control + 2.0 } else { 0.0 };
            let entry = Entry {
                id: b.view.id,
                name: b.view.name.clone(),
                path: b.page.info.path.clone(),
                location: location.clone(),
                label,
                pinned: b.view.pinned,
                close,
                title,
                width,
            };
            if i < self.entries.len() {
                self.entries[i] = entry;
            } else {
                self.entries.push(entry);
            }
            changed = true;
            self.last_active = None;
        }
        self.entries.truncate(workspace.open.len());
        if changed {
            self.offsets.clear();
            self.offsets.push(0.0);
            for entry in &self.entries {
                self.offsets
                    .push(self.offsets.last().copied().unwrap() + entry.width + 2.0);
            }
        }
        if restyle {
            self.last_active = None;
        }
        self.style = Some(style);
        self.dirty = false;
    }
    pub fn title(
        &self,
        i: usize,
        color: Color32,
        muted: Color32,
        italics: bool,
    ) -> egui::text::LayoutJob {
        let entry = &self.entries[i];
        title_job(
            &entry.title,
            entry.location.as_deref(),
            color,
            muted,
            italics,
            self.style.as_ref().map_or(13.0, |s| s.0.size),
        )
    }
    pub fn label(&self, i: usize) -> &str {
        &self.entries[i].label
    }
    pub fn offset(&self, i: usize) -> f32 {
        self.offsets[i]
    }
    pub fn entry_width(&self, i: usize) -> f32 {
        self.offsets[i + 1] - self.offsets[i]
    }
    pub fn width(&self) -> f32 {
        self.offsets.last().copied().unwrap_or(0.0)
    }
    pub fn height(&self) -> f32 {
        self.height
    }
    pub fn visible(&self, viewport: egui::Rect) -> std::ops::Range<usize> {
        let first = self
            .offsets
            .partition_point(|x| *x < viewport.left())
            .saturating_sub(2)
            .min(self.entries.len());
        let end =
            (self.offsets.partition_point(|x| *x <= viewport.right()) + 1).min(self.entries.len());
        first..end.max(first)
    }
    pub fn reveal(&mut self, id: u64, index: usize, width: f32) -> bool {
        let reveal = self.last_active != Some((id, index)) || self.last_width != width;
        self.last_active = Some((id, index));
        self.last_width = width;
        reveal
    }
    pub fn measured(&mut self, i: usize, width: f32) -> bool {
        if (self.entries[i].width - width).abs() > 0.01 {
            self.entries[i].width = width;
            self.dirty = true;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_names_expand_only_to_the_first_distinct_parent() {
        let hints = location_hints(&[
            ("config.json", Some("/project/client/src/config.json")),
            ("config.json", Some("/project/server/src/config.json")),
            ("config.json", Some("/project/tools/config.json")),
            ("unique.rs", Some("/project/client/src/unique.rs")),
        ]);
        for (hint, expected) in
            hints
                .iter()
                .zip([Some("client/src"), Some("server/src"), Some("tools"), None])
        {
            assert_eq!(
                hint.as_deref().map(|s| s.replace('\\', "/")),
                expected.map(str::to_owned)
            );
        }
    }

    #[test]
    fn nested_parents_repeated_paths_and_unsaved_files_stay_readable() {
        let hints = location_hints(&[
            ("a", Some("project/src/a")),
            ("a", Some("project/src/nested/src/a")),
            ("a", Some("project/src/a")),
            ("a", None),
            ("scratch", None),
            ("scratch", None),
        ]);
        assert_eq!(
            hints[0].as_deref().unwrap().replace('\\', "/"),
            "project/src"
        );
        assert_eq!(
            hints[1].as_deref().unwrap().replace('\\', "/"),
            "nested/src"
        );
        assert_eq!(hints[0], hints[2]);
        assert_eq!(hints[3].as_deref(), Some("Unsaved"));
        assert_eq!(&hints[4..], &[None, None]);
    }

    #[test]
    fn remote_paths_decode_folder_names_without_exposing_session_ids() {
        let hints = location_hints(&[
            (
                "a.rs",
                Some("hedit-ssh://private-session/client%20app/src/a.rs"),
            ),
            (
                "a.rs",
                Some("hedit-ssh://private-session/server%2Fsrc%2Fa.rs"),
            ),
        ]);
        assert_eq!(
            hints[0].as_deref().unwrap().replace('\\', "/"),
            "client app/src"
        );
        assert_eq!(
            hints[1].as_deref().unwrap().replace('\\', "/"),
            "server/src"
        );
    }

    #[test]
    fn unicode_location_keeps_logical_accessibility_and_visual_shaping() {
        let hints = location_hints(&[("a", Some("مشروع/a")), ("a", Some("שלום/a"))]);
        assert_eq!(hints[0].as_deref(), Some("مشروع"));
        let job = title_job(
            "a",
            hints[0].as_deref(),
            Color32::WHITE,
            Color32::GRAY,
            true,
            13.0,
        );
        assert_eq!(job.text, format!("a · {}", hedit::bidi::display("مشروع")));
        assert_eq!(job.sections[1].format.color, Color32::GRAY);
        assert!(job.sections.iter().all(|s| s.format.italics));
    }

    #[cfg(unix)]
    #[test]
    fn filesystem_root_and_relative_directory_are_visible() {
        let hints = location_hints(&[
            ("a", Some("/a")),
            ("a", Some("a")),
            ("a", Some("/project/a")),
        ]);
        assert_eq!(
            hints,
            [Some("/".into()), Some(".".into()), Some("project".into())]
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_drives_and_unc_roots_distinguish_identical_folders() {
        let hints = location_hints(&[
            ("a", Some(r"C:\src\a")),
            ("a", Some(r"D:\src\a")),
            ("a", Some(r"\\server\share\src\a")),
        ]);
        assert_eq!(
            hints,
            [
                Some(r"C:\src".into()),
                Some(r"D:\src".into()),
                Some(r"\\server\share\src".into())
            ]
        );
    }
}
