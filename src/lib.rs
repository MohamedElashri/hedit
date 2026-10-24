pub mod bidi;
pub mod brand;
pub mod build;
pub mod cli;
pub mod debugger;
pub mod document;
pub mod editing;
pub mod folder;
pub mod fonts;
pub mod intelligence;
pub mod ipc;
pub mod projects;
pub mod search;
pub mod session;
pub mod settings;
pub mod syntax;
pub mod text_encoding;
pub mod theme;
pub mod update;
pub mod windows;
pub mod workspace;

use document::Document;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};

pub type SharedDocument = Arc<Mutex<Document>>;

#[derive(Default)]
pub struct Editor {
    documents: Mutex<HashMap<u64, SharedDocument>>,
    next_id: AtomicU64,
}

impl Editor {
    pub fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub fn get(&self, id: u64) -> Result<SharedDocument, String> {
        self.documents
            .lock()
            .map_err(|e| e.to_string())?
            .get(&id)
            .cloned()
            .ok_or_else(|| "The document is no longer open.".into())
    }
    pub fn insert(&self, doc: Document) -> Result<(), String> {
        self.documents
            .lock()
            .map_err(|e| e.to_string())?
            .insert(doc.id, Arc::new(Mutex::new(doc)));
        Ok(())
    }
    pub fn remove(&self, id: u64) -> Result<(), String> {
        self.documents
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&id);
        Ok(())
    }
}
pub mod advanced;
