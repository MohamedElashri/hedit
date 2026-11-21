//! Exact-source/document/epoch binding for cached native owners.
use super::{Method, Owner};
use crate::{
    cancel::Cancel,
    resources::Resources,
    tool::{Binding, Grant},
    workspace::{Budget, Identity},
    Handle,
};
use hedit_plugin::{sdk, service::ResultSet};
use std::{
    collections::BTreeMap,
    io,
    sync::{Arc, Mutex, MutexGuard, TryLockError},
    time::{Duration, Instant},
};

#[derive(PartialEq, Eq)]
struct Scope {
    command: String,
    source: Handle,
    document: String,
    epoch: Vec<u8>,
}
struct Entry {
    key: (Binding, Handle),
    root: Option<Identity>,
    relative: Option<String>,
    owner: Option<Arc<Owner>>,
}
#[derive(Default)]
struct Contents {
    entries: BTreeMap<String, Entry>,
}
#[derive(Default)]
pub struct Registry {
    cancel: Arc<Cancel>,
    scope: Mutex<Option<Scope>>,
    lifetime: Mutex<Option<hedit_core::lifetime::Lease>>,
    contents: Mutex<Contents>,
}
impl Drop for Registry {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
pub(crate) struct Request<'a> {
    pub profile: &'a str,
    pub snapshot: &'a sdk::Snapshot,
    pub resources: Option<&'a Resources>,
    pub relative: Option<&'a str>,
    pub method: Method,
    pub position: usize,
    pub may_edit: bool,
    pub cancel: Arc<Cancel>,
    pub deadline: Instant,
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "The scoped language-server authority changed.",
    )
}
fn stopped() -> io::Error {
    io::Error::other("The scoped language server stopped; restart it explicitly.")
}
impl Registry {
    /// Register this cancellation with the host policy watcher before use.
    pub fn cancellation(&self) -> Arc<Cancel> {
        self.cancel.clone()
    }
    pub fn stop(&self) {
        self.cancel.cancel();
    }
    pub fn current(&self) -> bool {
        !self.cancel.is_cancelled()
            && self.lifetime.lock().is_ok_and(|lifetime| {
                lifetime
                    .as_ref()
                    .is_none_or(hedit_core::lifetime::Lease::current)
            })
    }
    pub fn settled(&self) -> bool {
        self.contents.try_lock().is_ok_and(|contents| {
            contents
                .entries
                .values()
                .all(|entry| entry.owner.as_ref().is_none_or(|owner| owner.settled()))
        })
    }
    pub fn bind_lifetime(&self, lifetime: &hedit_core::lifetime::Lease) -> io::Result<()> {
        if !lifetime.current() || self.cancel.is_cancelled() {
            self.stop();
            return Err(denied());
        }
        let mut known = self.lifetime.lock().map_err(|_| denied())?;
        if known.as_ref().is_some_and(|known| !known.same(lifetime)) {
            self.stop();
            return Err(denied());
        }
        *known = Some(lifetime.clone());
        Ok(())
    }
    pub(crate) fn bind(
        &self,
        command: &str,
        source: Handle,
        document: &str,
        epoch: &[u8],
    ) -> io::Result<()> {
        if !self.current() || epoch.is_empty() || epoch.len() > 256 {
            return Err(denied());
        }
        let mut known = self.scope.lock().map_err(|_| denied())?;
        let expected = Scope {
            command: command.into(),
            source,
            document: document.into(),
            epoch: epoch.into(),
        };
        match &*known {
            Some(scope) if *scope != expected => {
                self.stop();
                Err(denied())
            }
            Some(_) => Ok(()),
            None => {
                *known = Some(expected);
                Ok(())
            }
        }
    }
    fn lock(
        &self,
        request: &Request<'_>,
        budget: &Budget<'_>,
    ) -> io::Result<MutexGuard<'_, Contents>> {
        loop {
            budget.check()?;
            if self.cancel.is_cancelled() || request.cancel.is_cancelled() {
                return Err(denied());
            }
            match self.contents.try_lock() {
                Ok(contents) => return Ok(contents),
                Err(TryLockError::Poisoned(_)) => {
                    self.stop();
                    return Err(denied());
                }
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }
    pub(crate) fn execute(
        &self,
        grant: &Grant,
        request: Request<'_>,
        budget: &mut Budget<'_>,
        image_bytes: &mut u64,
    ) -> io::Result<ResultSet> {
        let lifetime = self.lifetime.lock().map_err(|_| denied())?.clone();
        if lifetime
            .as_ref()
            .is_some_and(|lifetime| !lifetime.current())
        {
            self.stop();
            return Err(denied());
        }
        let key = grant.key();
        let requested_relative = if grant.workspace() {
            request.relative.map(str::to_owned)
        } else {
            None
        };
        let root = match grant.workspace_identity(request.resources) {
            Ok(root) => root,
            Err(error) => {
                self.stop();
                return Err(error);
            }
        };
        if self
            .scope
            .lock()
            .map_err(|_| denied())?
            .as_ref()
            .is_none_or(|scope| scope.document != request.snapshot.document)
        {
            self.stop();
            return Err(denied());
        }
        let mut contents = self.lock(&request, budget)?;
        let owner = if let Some(entry) = contents.entries.get(request.profile) {
            if entry.key != key || entry.root != root || entry.relative != requested_relative {
                self.stop();
                return Err(denied());
            }
            entry
                .owner
                .as_ref()
                .filter(|owner| owner.running())
                .cloned()
                .ok_or_else(stopped)?
        } else {
            if contents.entries.len() >= 8 {
                return Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Scoped language-server profile capacity is exhausted.",
                ));
            }
            *image_bytes = image_bytes.checked_sub(grant.image_work()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "Native image work budget exhausted.",
                )
            })?;
            let (run, relative) = grant.prepare_server(&request, budget)?;
            // A failed attempted native start remains a tombstone. Later
            // callbacks cannot silently reset its CPU/lifetime/crash budget.
            contents.entries.insert(
                request.profile.into(),
                Entry {
                    key,
                    root,
                    relative: requested_relative,
                    owner: None,
                },
            );
            let owner = Arc::new(Owner::start(
                run,
                relative,
                request.snapshot.clone(),
                grant.workspace(),
                self.cancel.clone(),
                request.deadline,
                lifetime,
            )?);
            contents
                .entries
                .get_mut(request.profile)
                .ok_or_else(denied)?
                .owner = Some(owner.clone());
            owner
        };
        drop(contents);
        budget.check()?;
        budget.charge(request.snapshot.text.len())?;
        owner.request(
            request.snapshot,
            request.method,
            request.position,
            request.may_edit,
            request.cancel,
            request.deadline,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_authority_cannot_change_command_token_document_or_epoch() {
        for variant in 0..4 {
            let registry = Registry::default();
            let token = Handle::fresh().unwrap();
            registry
                .bind("sample.command", token, &"a".repeat(32), b"epoch")
                .unwrap();
            registry
                .bind("sample.command", token, &"a".repeat(32), b"epoch")
                .unwrap();
            let (command, token, document, epoch) = match variant {
                0 => (
                    "foreign.command",
                    token,
                    "a".repeat(32),
                    b"epoch".as_slice(),
                ),
                1 => (
                    "sample.command",
                    Handle::fresh().unwrap(),
                    "a".repeat(32),
                    b"epoch".as_slice(),
                ),
                2 => ("sample.command", token, "b".repeat(32), b"epoch".as_slice()),
                _ => (
                    "sample.command",
                    token,
                    "a".repeat(32),
                    b"changed".as_slice(),
                ),
            };
            assert!(registry.bind(command, token, &document, epoch).is_err());
            assert!(registry.cancellation().is_cancelled());
        }
    }
    #[test]
    fn a_reopened_document_cannot_revive_an_old_native_scope() {
        let registry = Registry::default();
        let first = hedit_core::lifetime::Lifetime::default();
        registry.bind_lifetime(&first.lease()).unwrap();
        registry.bind_lifetime(&first.lease()).unwrap();
        first.close();
        let reopened = hedit_core::lifetime::Lifetime::default();
        assert!(registry.bind_lifetime(&reopened.lease()).is_err());
        assert!(registry.cancellation().is_cancelled());
    }
}
