pub mod activation;
pub mod bidi;
pub mod brand;
pub mod build;
pub mod cli;
pub mod completion;
pub mod counterpart;
pub mod debugger;
pub mod document;
pub mod editing;
mod file_io;
pub mod file_reference;
pub mod folder;
pub mod fonts;
pub mod git;
pub mod intelligence;
pub mod ipc;
pub mod markup;
pub mod projects;
pub mod search;
pub mod session;
pub mod settings;
pub mod spelling;
pub mod syntax;
#[cfg(feature = "terminal")]
pub mod terminal;
pub mod text_commands;
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
    ephemeral: Mutex<std::collections::HashSet<u64>>,
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
    pub fn is_ephemeral(&self, id: u64) -> bool {
        self.ephemeral
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&id)
    }
    pub fn insert(&self, doc: Document) -> Result<(), String> {
        if doc.is_ephemeral() {
            self.ephemeral
                .lock()
                .map_err(|e| e.to_string())?
                .insert(doc.id);
        }
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

pub mod remote;
