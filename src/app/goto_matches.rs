//! Memory-only current-file matches. Large scans use one worker; only an exact
//! document revision and query can populate the picker.
use super::goto_anything::{text_matches, Target};
use super::*;
type Matches = Arc<Vec<(String, usize)>>;
type Symbols = Arc<Vec<hedit::intelligence::Symbol>>;
#[derive(Clone, PartialEq, Eq)]
struct DocumentKey {
    id: u64,
    revision: u64,
    start: u64,
    path: PathBuf,
}
#[derive(Clone, PartialEq, Eq)]
struct Key {
    doc: DocumentKey,
    target: Target,
}
struct Reply {
    key: Key,
    matches: Matches,
    symbols: Option<Symbols>,
}
#[derive(Default)]
pub(super) struct TargetCache {
    result: Option<Reply>,
    presented: bool,
    symbols: Option<(DocumentKey, Symbols)>,
    job: Option<Receiver<Reply>>,
}
fn compute(key: Key, text: String, mut symbols: Option<Symbols>) -> Reply {
    let matches = match &key.target {
        Target::Symbol(query) => {
            let index = symbols.get_or_insert_with(|| {
                Arc::new(hedit::intelligence::symbols(&text, &key.doc.path))
            });
            let query = query.to_lowercase();
            let mut matches: Vec<_> = index
                .iter()
                .filter_map(|s| {
                    let score = if query.is_empty() {
                        0
                    } else {
                        hedit::folder::fuzzy_score(&s.name, &query)?
                    };
                    Some((score, s.name.clone(), s.line))
                })
                .collect();
            matches.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
            matches
                .into_iter()
                .take(500)
                .map(|(_, name, line)| (name, line))
                .collect()
        }
        Target::Text(query) => text_matches(&text, query, 500),
        Target::Line(..) => vec![],
    };
    Reply {
        key,
        matches: Arc::new(matches),
        symbols,
    }
}
impl TargetCache {
    pub fn get(
        &mut self,
        ctx: &egui::Context,
        buffer: &Buffer,
        target: &Target,
    ) -> Option<(Matches, bool)> {
        let key = Key {
            doc: DocumentKey {
                id: buffer.view.id,
                revision: buffer.page.info.revision,
                start: buffer.page.start,
                path: buffer
                    .page
                    .info
                    .path
                    .as_ref()
                    .map(PathBuf::from)
                    .unwrap_or_default(),
            },
            target: target.clone(),
        };
        if let Some(job) = &self.job {
            match job.try_recv() {
                Ok(reply) => {
                    if let Some(symbols) = &reply.symbols {
                        self.symbols = Some((reply.key.doc.clone(), symbols.clone()));
                    }
                    self.result = Some(reply);
                    self.presented = false;
                    self.job = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.job = None,
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        if let Some(reply) = &self.result {
            if reply.key == key {
                let new = !self.presented;
                self.presented = true;
                return Some((reply.matches.clone(), new));
            }
        }
        if self.job.is_some() {
            return None;
        }
        let symbols = self
            .symbols
            .as_ref()
            .filter(|(doc, _)| *doc == key.doc)
            .map(|(_, s)| s.clone());
        let text = if matches!(target, Target::Symbol(_)) && symbols.is_some() {
            String::new()
        } else {
            buffer.page.text.clone()
        };
        if buffer.page.text.len() > 64 * 1024 {
            let (tx, rx) = mpsc::channel();
            self.job = Some(rx);
            let ctx = ctx.clone();
            thread::spawn(move || {
                let _ = tx.send(compute(key, text, symbols));
                ctx.request_repaint();
            });
            None
        } else {
            let reply = compute(key, text, symbols);
            if let Some(symbols) = &reply.symbols {
                self.symbols = Some((reply.key.doc.clone(), symbols.clone()));
            }
            self.result = Some(reply);
            self.presented = true;
            Some((self.result.as_ref().unwrap().matches.clone(), true))
        }
    }
    #[cfg(test)]
    pub fn pending(&self) -> bool {
        self.job.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn buffer(text: &str) -> Buffer {
        let mut doc = Document::empty(1);
        doc.replace(0, 0, text.into(), 0).unwrap();
        Buffer::new(doc.page(0).unwrap(), None)
    }
    #[test]
    fn unchanged_matches_and_symbol_indexes_are_reused_across_queries() {
        let ctx = egui::Context::default();
        let buffer = buffer("fn alpha() {}\nfn beta() {}\n");
        let mut cache = TargetCache::default();
        let query = Target::Symbol("alpha".into());
        let (first, new) = cache.get(&ctx, &buffer, &query).unwrap();
        assert!(new);
        let (second, new) = cache.get(&ctx, &buffer, &query).unwrap();
        assert!(!new);
        assert!(Arc::ptr_eq(&first, &second));
        let symbols = cache.symbols.as_ref().unwrap().1.clone();
        let (beta, _) = cache
            .get(&ctx, &buffer, &Target::Symbol("beta".into()))
            .unwrap();
        assert_eq!(&**beta, &[("beta".into(), 2)]);
        assert!(Arc::ptr_eq(&symbols, &cache.symbols.as_ref().unwrap().1));
    }
    #[test]
    fn a_stale_worker_result_never_populates_a_newer_document_revision() {
        let ctx = egui::Context::default();
        let mut buffer = buffer("old\n");
        let old = Key {
            doc: DocumentKey {
                id: buffer.view.id,
                revision: buffer.page.info.revision,
                start: 0,
                path: PathBuf::new(),
            },
            target: Target::Text("fresh".into()),
        };
        let (tx, rx) = mpsc::channel();
        let mut cache = TargetCache {
            job: Some(rx),
            ..Default::default()
        };
        tx.send(Reply {
            key: old,
            matches: Arc::new(vec![("stale".into(), 1)]),
            symbols: None,
        })
        .unwrap();
        buffer.page.info.revision += 1;
        buffer.page.text = format!("{}fresh content\n", "line\n".repeat(14000));
        let query = Target::Text("fresh".into());
        assert!(cache.get(&ctx, &buffer, &query).is_none());
        let until = Instant::now() + Duration::from_secs(5);
        let matches = loop {
            if let Some((matches, _)) = cache.get(&ctx, &buffer, &query) {
                break matches;
            }
            assert!(Instant::now() < until);
            thread::yield_now();
        };
        assert_eq!(&**matches, &[("fresh content".into(), 14001)]);
    }
}
