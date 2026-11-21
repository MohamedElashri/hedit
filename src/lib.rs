pub mod activation;
pub mod bidi;
pub mod brand;
pub mod cli;
pub mod completion;
pub mod counterpart;
pub mod debugger;
pub mod editing;
pub mod file_reference;
pub mod fonts;
pub mod icons;
pub mod intelligence;
pub mod ipc;
pub mod markup;
pub mod plugin_options;
pub mod projects;
pub mod session;
pub mod settings;
pub mod spelling;
pub mod syntax;
#[cfg(feature = "terminal")]
pub mod terminal;
pub mod text_commands;
pub mod theme;
pub mod ui_style;
pub mod update;
pub mod windows;
pub mod workspace;

use hedit_core::file_io;
pub use hedit_core::{build, document, folder, git, language_server, search, text_encoding};

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
    membership: Mutex<Membership>,
}

#[derive(Default)]
struct Membership {
    generation: u64,
    commit: Option<(u128, Vec<u64>)>,
    followed: std::collections::BTreeSet<u64>,
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
        let mut membership = self.membership.lock().map_err(|e| e.to_string())?;
        Self::membership_writable(&membership)?;
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
        membership.generation = membership.generation.wrapping_add(1);
        Ok(())
    }
    pub fn remove(&self, id: u64) -> Result<(), String> {
        let mut membership = self.membership.lock().map_err(|e| e.to_string())?;
        Self::membership_writable(&membership)?;
        self.documents
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&id);
        membership.generation = membership.generation.wrapping_add(1);
        Ok(())
    }
    fn membership_writable(membership: &Membership) -> Result<(), String> {
        if membership.commit.is_some() {
            Err("Buffer membership is awaiting a workspace commit.".into())
        } else {
            Ok(())
        }
    }
    pub fn set_followed(&self, id: u64, followed: bool) -> Result<(), String> {
        let mut membership = self
            .membership
            .try_lock()
            .map_err(|_| "Buffer membership is busy.")?;
        if membership
            .commit
            .as_ref()
            .is_some_and(|(_, ids)| ids.contains(&id))
        {
            return Err("This buffer is awaiting a workspace commit.".into());
        }
        if followed {
            membership.followed.insert(id);
        } else {
            membership.followed.remove(&id);
        }
        Ok(())
    }
    pub fn check_membership_writable(&self) -> Result<(), String> {
        Self::membership_writable(
            &*self
                .membership
                .try_lock()
                .map_err(|_| "Buffer membership is busy.")?,
        )
    }
    /// Rendering never blocks on document I/O or commit setup. A briefly busy
    /// membership lock disables editing for that frame.
    pub fn workspace_document_frozen(&self, id: u64) -> bool {
        self.membership.try_lock().map_or(true, |membership| {
            membership
                .commit
                .as_ref()
                .is_some_and(|(_, ids)| ids.contains(&id))
        })
    }
    pub fn workspace_commit_pending(&self) -> bool {
        self.membership
            .try_lock()
            .map_or(true, |membership| membership.commit.is_some())
    }
    pub fn document_set(&self) -> Result<(u64, Vec<SharedDocument>), String> {
        let membership = self.membership.lock().map_err(|e| e.to_string())?;
        Self::membership_writable(&membership)?;
        let documents = self
            .documents
            .lock()
            .map_err(|e| e.to_string())?
            .values()
            .cloned()
            .collect();
        Ok((membership.generation, documents))
    }
    /// Call only on the commit I/O thread. Structural changes cannot introduce
    /// a new alias after the host inspects every open or shelved document.
    pub fn freeze_workspace(
        self: &Arc<Self>,
        generation: u64,
        lease: u128,
        expected: &[(u64, u64)],
    ) -> Result<WorkspaceFreeze, String> {
        let mut membership = self.membership.lock().map_err(|e| e.to_string())?;
        Self::membership_writable(&membership)?;
        if membership.generation != generation {
            return Err("The open buffer set changed. Review the workspace edit again.".into());
        }
        if expected
            .iter()
            .any(|(id, _)| membership.followed.contains(id))
        {
            return Err("Stop following affected files before workspace editing.".into());
        }
        let documents = expected
            .iter()
            .map(|(id, revision)| Ok((self.get(*id)?, *revision)))
            .collect::<Result<Vec<_>, String>>()?;
        let mut frozen: Vec<SharedDocument> = Vec::new();
        for (document, revision) in documents {
            let result = document
                .lock()
                .map_err(|e| e.to_string())
                .and_then(|mut document| document.freeze_workspace(revision, lease));
            if let Err(error) = result {
                for document in &frozen {
                    document
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .thaw_workspace(lease);
                }
                return Err(error);
            }
            frozen.push(document);
        }
        membership.commit = Some((lease, expected.iter().map(|(id, _)| *id).collect()));
        Ok(WorkspaceFreeze {
            editor: self.clone(),
            lease,
            documents: frozen,
        })
    }
}
/// Owned and dropped on the commit I/O thread, including errors and cancellation.
pub struct WorkspaceFreeze {
    editor: Arc<Editor>,
    lease: u128,
    documents: Vec<SharedDocument>,
}
impl Drop for WorkspaceFreeze {
    fn drop(&mut self) {
        let mut membership = self
            .editor
            .membership
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for document in &self.documents {
            document
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .thaw_workspace(self.lease);
        }
        if membership
            .commit
            .as_ref()
            .is_some_and(|(lease, _)| *lease == self.lease)
        {
            membership.commit = None;
        }
    }
}
pub mod advanced;

pub mod remote;
