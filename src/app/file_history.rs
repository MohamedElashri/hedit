use super::*;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const TEXT_LIMIT: u64 = 1024 * 1024;
const PER_FILE: usize = 20;
const TOTAL_FILES: usize = 256;
const TOTAL_BYTES: u64 = 64 * 1024 * 1024;

pub(super) struct Version {
    path: PathBuf,
    key: String,
    timestamp: u128,
    digest: String,
    size: u64,
}

pub(super) struct Review {
    pub id: u64,
    pub revision: u64,
    pub path: PathBuf,
    versions: Vec<Version>,
    selected: Option<(String, String)>,
}

fn key(path: &Path) -> String {
    hex::encode(Sha256::digest(path.as_os_str().as_encoded_bytes()))
}

fn read(root: &Path) -> Result<Vec<Version>, String> {
    let entries = match fs::read_dir(root) {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(error) => return Err(error.to_string()),
    };
    let hex = |value: &str| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit());
    let mut versions = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((key, rest)) = name.split_once('-') else {
            continue;
        };
        let Some((stamp, digest)) = rest.split_once('-') else {
            continue;
        };
        if !hex(key) || !hex(digest) || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let Ok(timestamp) = u128::from_str_radix(stamp, 16) else {
            continue;
        };
        let size = entry.metadata().map_err(|error| error.to_string())?.len();
        versions.push(Version {
            path: entry.path(),
            key: key.into(),
            timestamp,
            digest: digest.into(),
            size,
        });
    }
    versions.sort_unstable_by_key(|version| std::cmp::Reverse(version.timestamp));
    Ok(versions)
}

fn write(root: &Path, key: &str, text: &str, versions: &mut Vec<Version>) -> Result<(), String> {
    let digest = hex::encode(Sha256::digest(text));
    if versions
        .iter()
        .find(|version| version.key == key)
        .is_some_and(|version| version.digest == digest)
    {
        return Ok(());
    }
    let timestamp = now();
    let path = root.join(format!("{key}-{timestamp:032x}-{digest}"));
    let mut temp = tempfile::NamedTempFile::new_in(root).map_err(|error| error.to_string())?;
    temp.write_all(text.as_bytes())
        .map_err(|error| error.to_string())?;
    temp.as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temp.persist_noclobber(&path)
        .map_err(|error| error.to_string())?;
    versions.insert(
        0,
        Version {
            path,
            key: key.to_owned(),
            timestamp,
            digest,
            size: text.len() as u64,
        },
    );
    Ok(())
}

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn record(root: &Path, path: &Path, prior: Option<&str>, saved: &str) -> Result<(), String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("lock"))
        .map_err(|error| error.to_string())?;
    lock.lock().map_err(|error| error.to_string())?;
    let mut versions = read(root)?;
    let key = key(path);
    if let Some(prior) = prior {
        write(root, &key, prior, &mut versions)?;
    }
    write(root, &key, saved, &mut versions)?;

    let mut files = 0;
    let mut bytes = 0u64;
    let mut counts = std::collections::HashMap::new();
    for version in versions {
        let count = counts.entry(version.key).or_insert(0);
        if *count >= PER_FILE || files >= TOTAL_FILES || version.size > TOTAL_BYTES - bytes {
            fs::remove_file(&version.path).map_err(|error| error.to_string())?;
        } else {
            *count += 1;
            files += 1;
            bytes += version.size;
        }
    }
    Ok(())
}

pub(super) fn save(
    doc: &mut Document,
    target: Option<PathBuf>,
    history: Option<PathBuf>,
) -> Result<hedit::document::Info, String> {
    let before = doc.info();
    let prior = history.as_ref().and_then(|_| {
        let path = target
            .as_deref()
            .or_else(|| before.path.as_deref().map(Path::new))?;
        let file = hedit_core::file_io::open_regular(path).ok()?;
        let bytes = hedit_core::file_io::read_bounded(file, TEXT_LIMIT).ok()?;
        let encoding = before
            .path
            .as_deref()
            .filter(|name| Path::new(name) == path)
            .map_or_else(
                || hedit::text_encoding::detect(&bytes),
                |_| before.encoding.as_str(),
            );
        hedit::text_encoding::decode(&bytes, encoding)
            .ok()
            .map(|(text, _)| text)
            .filter(|text| text.len() as u64 <= TEXT_LIMIT)
    });
    let info = doc.save(target)?;
    if let (Some(root), Some(path)) = (history, info.path.as_deref()) {
        if let Ok(saved) = doc.text(TEXT_LIMIT) {
            if let Err(error) = record(&root, Path::new(path), prior.as_deref(), &saved) {
                eprintln!("Could not record local file history: {error}");
            }
        }
    }
    Ok(info)
}

impl HeditApp {
    pub(super) fn file_history_root(&self) -> Option<PathBuf> {
        if self.workspace.remote.is_some() {
            return None;
        }
        #[cfg(not(test))]
        if let Some(dirs) = directories::ProjectDirs::from("", "hedit", "hedit") {
            return Some(dirs.data_local_dir().join("file-history"));
        }
        Some(self.workspace.store.directory().join("file-history"))
    }

    pub(super) fn open_file_history(&mut self, ctx: &egui::Context) {
        let Some(root) = self.file_history_root() else {
            return;
        };
        let buffer = self.active();
        let Some(path) = buffer.page.info.path.as_ref().map(PathBuf::from) else {
            return;
        };
        let (id, revision) = (buffer.view.id, buffer.page.info.revision);
        self.job(ctx, move || {
            let identity = key(&path);
            let versions = read(&root)?
                .into_iter()
                .filter(|version| version.key == identity)
                .take(PER_FILE)
                .collect();
            Ok(Action::Quality(Box::new(quality::Event::FileHistory(
                Review {
                    id,
                    revision,
                    path,
                    versions,
                    selected: None,
                },
            ))))
        });
    }

    pub(super) fn file_history_window(&mut self, ctx: &egui::Context) {
        let Some(review) = &mut self.quality.file_history else {
            return;
        };
        let mut open = true;
        let mut pick = None;
        let mut restore = false;
        let active = &self.workspace.open[self.workspace.active];
        let current = active.view.id == review.id
            && active.page.info.revision == review.revision
            && active
                .page
                .info
                .path
                .as_deref()
                .is_some_and(|path| Path::new(path) == review.path);
        let ready =
            !self.busy && !self.closing && current && !active.follow && !active.page.info.paged;
        egui::Window::new("File History")
            .open(&mut open)
            .collapsible(false)
            .default_width(640.0)
            .max_width((ctx.viewport_rect().width() - 32.0).max(300.0))
            .show(ctx, |ui| {
                ui.label(review.path.display().to_string());
                if review.versions.is_empty() {
                    ui.label("No saved versions recorded yet.");
                }
                if !current {
                    ui.label("The editor changed. Reopen File History to compare the latest text.");
                }
                egui::ScrollArea::vertical()
                    .max_height(130.0)
                    .show(ui, |ui| {
                        for (index, version) in review.versions.iter().enumerate() {
                            let minutes = now().saturating_sub(version.timestamp) / 60_000_000_000;
                            let when = match minutes {
                                0 => "just now".into(),
                                1..=59 => format!("{minutes} min ago"),
                                60..=1439 => format!("{} hr ago", minutes / 60),
                                _ => format!("{} days ago", minutes / 1440),
                            };
                            if ui
                                .add_enabled(
                                    ready,
                                    egui::Button::new(format!(
                                        "Version {} · {when}",
                                        review.versions.len() - index
                                    )),
                                )
                                .clicked()
                            {
                                pick = Some(version.path.clone());
                            }
                        }
                    });
                if let Some((_, diff)) = &review.selected {
                    ui.separator();
                    if diff.is_empty() {
                        ui.label("This version matches the current editor.");
                    } else {
                        egui::ScrollArea::both().max_height(250.0).show(ui, |ui| {
                            let mut text = diff.as_str();
                            ui.add(
                                egui::TextEdit::multiline(&mut text)
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                        });
                    }
                    restore = ui
                        .add_enabled(
                            ready && !diff.is_empty(),
                            egui::Button::new("Restore into Editor"),
                        )
                        .clicked();
                }
            });
        if !open {
            self.quality.file_history = None;
            return;
        }
        let (id, revision) = (review.id, review.revision);
        let source = review.path.clone();
        let editor = Arc::clone(&self.workspace.editor);
        if let Some(path) = pick {
            self.job(ctx, move || {
                let text = String::from_utf8(
                    hedit_core::file_io::read_bounded(
                        hedit_core::file_io::open_regular(&path)
                            .map_err(|error| error.to_string())?,
                        TEXT_LIMIT,
                    )
                    .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let shared = editor.get(id)?;
                let doc = shared.lock().map_err(|error| error.to_string())?;
                let info = doc.info();
                if info.revision != revision
                    || info.path.as_deref().is_none_or(|p| Path::new(p) != source)
                {
                    return Err("The file changed. Reopen File History to compare versions.".into());
                }
                let current = doc.text(TEXT_LIMIT)?;
                drop(doc);
                let diff = hedit::search::diff(&text, &current, &source);
                Ok(Action::Quality(Box::new(quality::Event::FileHistoryDiff(
                    id, revision, text, diff,
                ))))
            });
        } else if restore {
            let Some((text, _)) = review.selected.take() else {
                return;
            };
            self.quality.file_history = None;
            self.job(ctx, move || {
                let shared = editor.get(id)?;
                let mut doc = shared.lock().map_err(|error| error.to_string())?;
                let info = doc.info();
                if info.revision != revision
                    || info.path.as_deref().is_none_or(|p| Path::new(p) != source)
                {
                    return Err(
                        "The file changed. Reopen File History to restore a version.".into(),
                    );
                }
                doc.replace(0, info.size, text, revision)?;
                Ok(Action::Page(id, doc.page(0)?, None))
            });
        }
    }

    pub(super) fn finish_file_history_diff(
        &mut self,
        id: u64,
        revision: u64,
        text: String,
        diff: String,
    ) {
        if let Some(review) = self
            .quality
            .file_history
            .as_mut()
            .filter(|review| review.id == id && review.revision == revision)
        {
            review.selected = Some((text, diff));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_history_keeps_the_twenty_most_recent_distinct_saves() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.txt");
        let root = directory.path().join("history");
        fs::write(&path, "original").unwrap();
        let mut doc = Document::open(1, &path).unwrap();
        for index in 1..=23 {
            let info = doc.info();
            doc.replace(0, info.size, format!("revision {index}"), info.revision)
                .unwrap();
            save(&mut doc, None, Some(root.clone())).unwrap();
        }
        save(&mut doc, None, Some(root.clone())).unwrap();
        let versions = read(&root).unwrap();
        assert_eq!(versions.len(), PER_FILE);
        assert_eq!(
            fs::read_to_string(&versions[0].path).unwrap(),
            "revision 23"
        );
        assert_eq!(
            fs::read_to_string(&versions[19].path).unwrap(),
            "revision 4"
        );
    }
}
