use super::*;
use hedit::{intelligence, search};
use std::collections::{BTreeMap, BTreeSet};

const LIMIT: usize = 20_000;
const FILE_BYTES: u64 = 2 * 1024 * 1024;
const TOTAL_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Index {
    roots: Vec<PathBuf>,
    cancel: Option<Arc<AtomicBool>>,
    pending: Option<std::sync::mpsc::Receiver<Result<Results, String>>>,
    results: Option<Result<Results, String>>,
}

pub(super) struct Results {
    pub symbols: Vec<intelligence::Symbol>,
    pub truncated: bool,
}

impl Drop for Index {
    fn drop(&mut self) {
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Index {
    pub fn start(
        &mut self,
        ctx: &egui::Context,
        roots: Vec<PathBuf>,
        unsaved: BTreeMap<PathBuf, String>,
    ) {
        if self.roots == roots && (self.pending.is_some() || self.results.is_some()) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = std::sync::mpsc::channel();
        *self = Self {
            roots: roots.clone(),
            cancel: Some(cancel.clone()),
            pending: Some(rx),
            results: None,
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(collect(&roots, unsaved, &cancel));
            ctx.request_repaint();
        });
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn get(&mut self) -> Option<&Result<Results, String>> {
        use std::sync::mpsc::TryRecvError;
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => self.results = Some(result),
                Err(TryRecvError::Disconnected) => {
                    self.results = Some(Err("Symbol indexing stopped.".into()))
                }
                Err(TryRecvError::Empty) => return None,
            }
            self.pending = None;
        }
        self.results.as_ref()
    }

    pub fn needs_start(&self, roots: &[PathBuf]) -> bool {
        self.roots != roots || (self.pending.is_none() && self.results.is_none())
    }
}

fn collect(
    roots: &[PathBuf],
    mut unsaved: BTreeMap<PathBuf, String>,
    cancel: &AtomicBool,
) -> Result<Results, String> {
    let files: BTreeSet<_> = search::walk(roots, "", cancel)?
        .into_iter()
        .chain(unsaved.keys().cloned())
        .collect();
    let mut symbols = Vec::new();
    let mut bytes_left = TOTAL_BYTES;
    let mut truncated = false;
    for file in files {
        if cancel.load(Ordering::Relaxed) {
            return Err("Symbol indexing cancelled.".into());
        }
        let text = match unsaved.remove(&file) {
            Some(text) => text,
            None => {
                let Ok(file_data) = hedit_core::file_io::open_regular(&file) else {
                    continue;
                };
                let Ok(bytes) = hedit_core::file_io::read_bounded(file_data, FILE_BYTES) else {
                    continue;
                };
                let Ok((text, _)) =
                    hedit::text_encoding::decode(&bytes, hedit::text_encoding::detect(&bytes))
                else {
                    continue;
                };
                text
            }
        };
        if text.len() > bytes_left {
            truncated = true;
            break;
        }
        bytes_left -= text.len();
        let file_symbols = intelligence::symbols(&text, &file);
        if file_symbols.len() > LIMIT - symbols.len() {
            symbols.extend(file_symbols.into_iter().take(LIMIT - symbols.len()));
            truncated = true;
            break;
        }
        symbols.extend(file_symbols);
    }
    Ok(Results { symbols, truncated })
}

impl HeditApp {
    pub(super) fn prepare_workspace_symbols(&mut self, ctx: &egui::Context) {
        let roots: Vec<_> = self
            .workspace
            .folders
            .iter()
            .map(|folder| PathBuf::from(&folder.path))
            .collect();
        if !self.workspace_symbols.needs_start(&roots) {
            return;
        }
        let mut bytes_left = TOTAL_BYTES;
        let unsaved = self
            .workspace
            .open
            .iter()
            .filter_map(|buffer| {
                let path = PathBuf::from(buffer.page.info.path.as_ref()?);
                let len = buffer.page.text.len();
                if buffer.page.info.paged
                    || !buffer.page.info.dirty
                    || len > FILE_BYTES as usize
                    || len > bytes_left
                    || !roots.iter().any(|root| path.starts_with(root))
                {
                    return None;
                }
                bytes_left -= len;
                Some((path, buffer.page.text.clone()))
            })
            .collect();
        self.workspace_symbols.start(ctx, roots, unsaved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_symbol_index_prefers_open_buffers() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("main.rs");
        std::fs::write(&file, "fn saved() {}\n").unwrap();
        std::fs::write(directory.path().join("lib.rs"), "fn from_disk() {}\n").unwrap();
        let results = collect(
            &[directory.path().to_owned()],
            BTreeMap::from([(file.clone(), "\nfn unsaved() {}\n".into())]),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(results.symbols.len(), 2);
        let unsaved = results
            .symbols
            .iter()
            .find(|symbol| symbol.path == file)
            .unwrap();
        assert_eq!(unsaved.name, "unsaved");
        assert_eq!(unsaved.line, 2);
        assert!(results
            .symbols
            .iter()
            .any(|symbol| symbol.name == "from_disk"));
    }
}
