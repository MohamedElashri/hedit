//! Go to definition (F12) and Go to references (Shift+F12) for the word at
//! the cursor, searching open files and then the open folders.
use super::*;
use hedit::intelligence::{self, Place};
use hedit::search;
use std::path::Path;

/// The most places listed.
const LIMIT: usize = 2_000;

#[derive(Clone, Debug)]
pub(super) struct Location {
    /// The open file it is in, for files without a path.
    pub view: Option<u64>,
    pub path: Option<PathBuf>,
    /// The file's name as shown in the list.
    pub file: String,
    pub place: Place,
}

/// An open file's text, for searching without reading it from disk.
struct OpenText {
    view: u64,
    path: Option<PathBuf>,
    file: String,
    text: String,
}

fn find_in(text: &str, name: &str, path: &Path, references: bool, room: usize) -> Vec<Place> {
    if references {
        intelligence::references(text, name, room)
    } else {
        let mut found = intelligence::definitions(text, name, path);
        found.truncate(room);
        found
    }
}

impl HeditApp {
    /// The word at the cursor, or the selected text on one line.
    fn lookup_name(&self) -> Option<String> {
        let b = self.active();
        let text = &b.page.text;
        let range = if b.view.anchor != b.view.head {
            hedit::advanced::Selection {
                anchor: b.view.anchor,
                head: b.view.head,
            }
        } else {
            hedit::advanced::word(text, b.view.head)
        };
        let start = hedit::advanced::byte(text, range.start());
        let end = start + hedit::advanced::byte(&text[start..], range.end() - range.start());
        let name = text[start..end].trim();
        (!name.is_empty() && !name.contains('\n') && name.chars().count() <= 200)
            .then(|| name.to_owned())
    }
    /// Find where the word at the cursor is defined, or every place it is used.
    pub(super) fn look_up(&mut self, ctx: &egui::Context, references: bool) {
        let Some(name) = self.lookup_name() else {
            self.notice = "Place the cursor on a name first.".into();
            return;
        };
        let roots: Vec<PathBuf> = self
            .workspace
            .folders
            .iter()
            .map(|f| PathBuf::from(&f.path))
            .collect();
        let active = self.active().view.id;
        // The current file first, then the other open files.
        let mut open: Vec<OpenText> = self
            .workspace
            .open
            .iter()
            .filter(|b| !b.page.info.paged)
            .map(|b| OpenText {
                view: b.view.id,
                path: b.page.info.path.as_ref().map(PathBuf::from),
                file: b.view.name.clone(),
                text: b.page.text.clone(),
            })
            .collect();
        open.sort_by_key(|o| o.view != active);
        let cancel = Arc::clone(&self.cancel);
        self.job(ctx, move || {
            let mut found: Vec<Location> = vec![];
            let mut seen = std::collections::HashSet::new();
            for o in &open {
                let path = o.path.clone().unwrap_or_default();
                if let Some(p) = &o.path {
                    seen.insert(p.clone());
                }
                for place in find_in(&o.text, &name, &path, references, LIMIT - found.len()) {
                    found.push(Location {
                        view: Some(o.view),
                        path: o.path.clone(),
                        file: o.file.clone(),
                        place,
                    });
                }
            }
            if !roots.is_empty() && found.len() < LIMIT {
                for file in search::walk(&roots, "", &cancel)? {
                    if seen.contains(&file) {
                        continue;
                    }
                    if std::fs::metadata(&file).map_or(true, |m| m.len() > search::FILE_LIMIT) {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(&file) else {
                        continue;
                    };
                    // Skip binary files.
                    if bytes.iter().take(8192).any(|&b| b == 0) {
                        continue;
                    }
                    let Ok((text, _)) =
                        hedit::text_encoding::decode(&bytes, hedit::text_encoding::detect(&bytes))
                    else {
                        continue;
                    };
                    let shown = hedit::folder::display_path(&roots, &file);
                    for place in find_in(&text, &name, &file, references, LIMIT - found.len()) {
                        found.push(Location {
                            view: None,
                            path: Some(file.clone()),
                            file: shown.clone(),
                            place,
                        });
                    }
                    if found.len() >= LIMIT {
                        break;
                    }
                }
            }
            Ok(Action::Quality(Box::new(quality::Event::Locations(
                references, name, found,
            ))))
        });
    }
    /// Jump to the only definition, or list them to choose from.
    pub(super) fn show_locations(
        &mut self,
        ctx: &egui::Context,
        references: bool,
        name: String,
        found: Vec<Location>,
    ) {
        match found.len() {
            0 if references => self.notice = format!("“{name}” is not used anywhere."),
            0 => self.notice = format!("No definition of “{name}” found."),
            1 if !references => self.open_location(ctx, found.into_iter().next().unwrap()),
            count => {
                let title = if references {
                    let more = if count >= LIMIT { "+" } else { "" };
                    format!("References to “{name}” ({count}{more})")
                } else {
                    format!("Definitions of “{name}”")
                };
                let items = found
                    .into_iter()
                    .map(|l| {
                        let detail = format!("{}:{}", l.file, l.place.line);
                        (l.place.text.clone(), detail, commands::Pick::Location(l))
                    })
                    .collect();
                self.open_pick_list(ctx, title, "Filter by text or file…", items);
            }
        }
    }
    /// Open a location's file, or switch to it, and put the cursor there.
    pub(super) fn open_location(&mut self, ctx: &egui::Context, location: Location) {
        self.navigation_mark();
        let open = self.workspace.open.iter().position(|b| {
            location.view == Some(b.view.id)
                || (location.path.is_some()
                    && b.page.info.path.as_ref().map(PathBuf::from) == location.path)
        });
        let Place { line, column, .. } = location.place;
        if let Some(index) = open {
            self.select_tab(index);
            self.jump_to_line(ctx, line, Some(column));
            self.focus_editor = true;
        } else if let Some(path) = location.path {
            self.pending_paths
                .push_back(PathBuf::from(format!("{}:{line}:{column}", path.display())));
        }
    }
}

/// Definitions shown while the pointer rests on a name.
pub(super) struct Hover {
    view: u64,
    /// The name's character range.
    word: (usize, usize),
    at: egui::Pos2,
    found: Vec<Location>,
    /// Where the list was drawn last frame.
    rect: Option<egui::Rect>,
}

impl Hover {
    #[cfg(test)]
    pub(super) fn found_lines(&self) -> Vec<usize> {
        self.found.iter().map(|l| l.place.line).collect()
    }
}

impl HeditApp {
    /// Definitions of the name at `index` in the open files, without the
    /// place being hovered.
    fn hover_lookup(&self, view: u64, word: (usize, usize)) -> Vec<Location> {
        let Some(b) = self.workspace.open.iter().find(|b| b.view.id == view) else {
            return vec![];
        };
        let text = &b.page.text;
        let start = hedit::advanced::byte(text, word.0);
        let end = start + hedit::advanced::byte(&text[start..], word.1 - word.0);
        let name = &text[start..end];
        let here = text[..start].matches('\n').count() + 1;
        let mut found = vec![];
        for o in std::iter::once(b).chain(self.workspace.open.iter().filter(|o| o.view.id != view))
        {
            if o.page.info.paged {
                continue;
            }
            let path = o.page.info.path.as_ref().map(PathBuf::from);
            for place in
                intelligence::definitions(&o.page.text, name, &path.clone().unwrap_or_default())
            {
                if o.view.id == view && place.line == here {
                    continue;
                }
                found.push(Location {
                    view: Some(o.view.id),
                    path: path.clone(),
                    file: o.view.name.clone(),
                    place,
                });
                if found.len() == 8 {
                    return found;
                }
            }
        }
        found
    }
    /// Show, keep, or close the definitions list for the hovered name.
    pub(super) fn hover_definitions(&mut self, ctx: &egui::Context) {
        let target = self.hover_target.take();
        if !self.settings.show_definitions || self.busy {
            self.hover = None;
            return;
        }
        let typing = ctx.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)))
        });
        let pointer = ctx.pointer_hover_pos();
        if let Some(hover) = &self.hover {
            let near = pointer.is_some_and(|p| {
                hover.rect.is_some_and(|r| r.expand(16.0).contains(p))
                    || p.distance(hover.at) < 24.0
            });
            let same =
                target.is_some_and(|(view, word, _)| view == hover.view && word == hover.word);
            if typing || !(near || same) {
                self.hover = None;
            }
        }
        if let (None, Some((view, word, at))) = (&self.hover, target) {
            let found = self.hover_lookup(view, word);
            if !found.is_empty() {
                self.hover = Some(Hover {
                    view,
                    word,
                    at,
                    found,
                    rect: None,
                });
            }
        }
        let Some(hover) = &mut self.hover else {
            return;
        };
        let theme = self.theme.clone();
        let mut chosen = None;
        let shown = egui::Area::new(egui::Id::new("definition-hover"))
            .order(egui::Order::Foreground)
            .fixed_pos(hover.at + egui::vec2(0.0, 18.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.panel.color())
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(if hover.found.len() == 1 {
                                "Definition"
                            } else {
                                "Definitions"
                            })
                            .small()
                            .color(theme.muted.color()),
                        );
                        for (index, l) in hover.found.iter().enumerate() {
                            let label = format!("{}:{}  {}", l.file, l.place.line, l.place.text);
                            let short: String = label.chars().take(100).collect();
                            if ui.link(short).clicked() {
                                chosen = Some(index);
                            }
                        }
                    });
            });
        hover.rect = Some(shown.response.rect);
        if let Some(index) = chosen {
            let location = hover.found[index].clone();
            self.hover = None;
            self.open_location(ctx, location);
        }
    }
}
