use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Write},
    path::Path,
};

const LIMIT: u64 = 256 * 1024;
const MAX_SEARCHES: usize = 24;

#[derive(Clone, Deserialize, Serialize)]
struct Preset {
    name: String,
    query: String,
    filter: String,
    scope: Option<PathBuf>,
    options: search::Options,
    #[serde(default)]
    context: bool,
}

#[derive(Default)]
pub(in crate::app) struct Saved {
    path: Option<PathBuf>,
    entries: Vec<Preset>,
    name: String,
    error: Option<String>,
}

fn read(path: &Path) -> Result<Vec<Preset>, String> {
    let file = match hedit_core::file_io::open_regular(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.to_string()),
    };
    let bytes =
        hedit_core::file_io::read_bounded(file, LIMIT).map_err(|error| error.to_string())?;
    let entries: Vec<Preset> = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if entries.len() > MAX_SEARCHES {
        return Err("Too many saved searches.".into());
    }
    Ok(entries)
}

fn write(path: &Path, entries: &[Preset]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Saved search directory is unavailable.")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(entries).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Saved searches exceed the storage limit.".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|error| error.to_string())?;
    temp.write_all(&bytes).map_err(|error| error.to_string())?;
    temp.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temp.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

impl HeditApp {
    fn search_presets_path(&self) -> PathBuf {
        let mut hash = Sha256::new();
        if let Some(path) = &self.quality.project_path {
            hash.update(b"project\0");
            hash.update(path.as_os_str().as_encoded_bytes());
        } else {
            hash.update(b"folders\0");
            let mut roots: Vec<_> = self
                .workspace
                .folders
                .iter()
                .map(|root| root.path.as_str())
                .collect();
            roots.sort_unstable();
            for root in roots {
                hash.update(root.as_bytes());
                hash.update(b"\0");
            }
        }
        self.workspace
            .store
            .directory()
            .join("saved-searches")
            .join(format!("{}.json", hex::encode(hash.finalize())))
    }

    pub(super) fn saved_search_menu(&mut self, ui: &mut egui::Ui) -> bool {
        if self.workspace.remote.is_some() {
            return false;
        }
        let mut run = false;
        ui.menu_button("Saved Searches", |ui| {
            let path = self.search_presets_path();
            let saved = &mut self.quality.saved_searches;
            if saved.path.as_ref() != Some(&path) {
                match read(&path) {
                    Ok(entries) => {
                        *saved = Saved {
                            path: Some(path.clone()),
                            entries,
                            ..Default::default()
                        }
                    }
                    Err(error) => {
                        ui.label(error);
                        return;
                    }
                }
            }
            ui.add(
                egui::TextEdit::singleline(&mut saved.name)
                    .hint_text("Search name")
                    .desired_width(160.0),
            );
            let name = saved.name.trim();
            let existing = saved.entries.iter().position(|item| item.name == name);
            let ready = !name.is_empty()
                && name.len() <= 80
                && !name.chars().any(char::is_control)
                && !self.quality.folder_query.is_empty()
                && self.quality.folder_query.len() <= 4096
                && self.quality.filter.len() <= 4096
                && (existing.is_some() || saved.entries.len() < MAX_SEARCHES);
            let mut change = None;
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(if existing.is_some() {
                        "Update search"
                    } else {
                        "Save current search"
                    }),
                )
                .clicked()
            {
                let mut entries = saved.entries.clone();
                let preset = Preset {
                    name: name.to_owned(),
                    query: self.quality.folder_query.clone(),
                    filter: self.quality.filter.clone(),
                    scope: self.quality.folder_scope.clone(),
                    options: self.quality.folder_options.clone(),
                    context: self.quality.folder_context,
                };
                if let Some(index) = existing {
                    entries[index] = preset;
                } else {
                    entries.push(preset);
                }
                entries.sort_by(|left, right| left.name.cmp(&right.name));
                change = Some(entries);
            }
            let mut picked = None;
            egui::ScrollArea::vertical()
                .max_height(250.0)
                .show(ui, |ui| {
                    for (index, preset) in saved.entries.iter().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        !self.busy && !self.closing,
                                        egui::Button::new(&preset.name),
                                    )
                                    .clicked()
                                {
                                    picked = Some(preset.clone());
                                }
                                if ui.small_button("Remove").clicked() {
                                    let mut entries = saved.entries.clone();
                                    entries.remove(index);
                                    change = Some(entries);
                                }
                            });
                        });
                    }
                });
            if let Some(entries) = change {
                match write(&path, &entries) {
                    Ok(()) => {
                        saved.entries = entries;
                        saved.name.clear();
                        saved.error = None;
                    }
                    Err(error) => saved.error = Some(error),
                }
            }
            if let Some(error) = &saved.error {
                ui.label(error);
            }
            if let Some(preset) = picked {
                self.set_folder_search_scope(preset.scope);
                self.quality.folder_query = preset.query;
                self.quality.filter = preset.filter;
                self.quality.folder_options = preset.options;
                self.quality.folder_context = preset.context;
                self.quality.folder_replace = false;
                run = true;
                ui.close();
            }
        });
        run
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_search_round_trips_scope_and_matching_options() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join("saved-searches")
            .join("workspace.json");
        let preset = Preset {
            name: "Notes".into(),
            query: "TODO|FIXME".into(),
            filter: "*.rs".into(),
            scope: Some(directory.path().join("src")),
            options: search::Options {
                regex: true,
                whole_word: true,
                case_sensitive: false,
            },
            context: true,
        };
        assert!(read(&path).unwrap().is_empty());
        write(&path, std::slice::from_ref(&preset)).unwrap();
        let saved = read(&path).unwrap().pop().unwrap();
        assert_eq!(saved.name, preset.name);
        assert_eq!(saved.query, preset.query);
        assert_eq!(saved.filter, preset.filter);
        assert_eq!(saved.scope, preset.scope);
        assert_eq!(saved.options, preset.options);
        assert!(saved.context);
    }
}
