//! Owner-local handles and aggregate immutable-snapshot retention.
use crate::{
    workspace::{Budget, Root, Snapshot},
    Handle,
};
use std::{
    collections::BTreeMap,
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
const SESSION_BYTES: usize = 16 * 1024 * 1024;
const GLOBAL_BYTES: usize = 64 * 1024 * 1024;
const MAX_RESOURCES: usize = 128;
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;
static RETAINED: AtomicUsize = AtomicUsize::new(0);
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Resource handle is unavailable in this session.",
    )
}
fn full() -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        "Extension snapshot retention limit reached. Release unused resources.",
    )
}
pub(crate) struct Reservation {
    bytes: usize,
    owner: Arc<AtomicUsize>,
}
impl Reservation {
    pub(crate) fn new(bytes: usize, owner: Arc<AtomicUsize>) -> io::Result<Self> {
        fn reserve(counter: &AtomicUsize, bytes: usize, maximum: usize) -> io::Result<()> {
            let mut old = counter.load(Ordering::Acquire);
            loop {
                let next = old
                    .checked_add(bytes)
                    .filter(|next| *next <= maximum)
                    .ok_or_else(full)?;
                match counter.compare_exchange_weak(old, next, Ordering::AcqRel, Ordering::Acquire)
                {
                    Ok(_) => return Ok(()),
                    Err(actual) => old = actual,
                }
            }
        }
        reserve(&owner, bytes, SESSION_BYTES)?;
        if let Err(error) = reserve(&RETAINED, bytes, GLOBAL_BYTES) {
            owner.fetch_sub(bytes, Ordering::AcqRel);
            return Err(error);
        }
        Ok(Self { bytes, owner })
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        RETAINED.fetch_sub(self.bytes, Ordering::AcqRel);
        self.owner.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
pub struct Resource {
    snapshot: Snapshot,
    version: Handle,
    _memory: Reservation,
}
impl Resource {
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn version(&self) -> Handle {
        self.version
    }
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Version {
    pub resource: Handle,
    pub version: Handle,
    pub relative: String,
    pub bytes: usize,
}
#[derive(Debug, serde::Serialize)]
pub struct Chunk {
    pub text: String,
    pub next: Option<usize>,
}
/// Allocate one instance per authenticated owner/grant/session. A guessed or
/// copied identifier cannot resolve in another instance's table.
pub struct Resources {
    root: Root,
    scope: Handle,
    values: BTreeMap<Handle, Arc<Resource>>,
    retained: Arc<AtomicUsize>,
}
impl Resources {
    pub fn new(root: Root) -> io::Result<Self> {
        Ok(Self {
            root,
            scope: Handle::fresh()?,
            values: BTreeMap::new(),
            retained: Arc::new(AtomicUsize::new(0)),
        })
    }
    pub(crate) fn reserve(&self, bytes: usize) -> io::Result<Reservation> {
        Reservation::new(bytes, self.retained.clone())
    }
    pub fn scope(&self) -> Handle {
        self.scope
    }
    pub fn root(&self, scope: Handle) -> io::Result<&Root> {
        if scope != self.scope {
            return Err(denied());
        }
        Ok(&self.root)
    }
    pub fn load(
        &mut self,
        scope: Handle,
        path: &str,
        budget: &mut Budget<'_>,
    ) -> io::Result<Version> {
        self.root(scope)?;
        if self.values.len() >= MAX_RESOURCES {
            return Err(full());
        }
        let snapshot = self.root.read(path, budget)?;
        let bytes = snapshot.text.capacity()
            + snapshot.relative.capacity()
            + std::mem::size_of::<Resource>();
        let memory = Reservation::new(bytes, self.retained.clone())?;
        let resource = Handle::fresh()?;
        let version = Handle::fresh()?;
        let view = Version {
            resource,
            version,
            relative: snapshot.relative.clone(),
            bytes: snapshot.text.len(),
        };
        self.values.insert(
            resource,
            Arc::new(Resource {
                snapshot,
                version,
                _memory: memory,
            }),
        );
        Ok(view)
    }
    pub fn get(&self, resource: Handle, version: Handle) -> io::Result<Arc<Resource>> {
        self.values
            .get(&resource)
            .filter(|value| value.version == version)
            .cloned()
            .ok_or_else(denied)
    }
    pub fn chunk(
        &self,
        resource: Handle,
        version: Handle,
        offset: usize,
        maximum: usize,
        budget: &mut Budget<'_>,
    ) -> io::Result<Chunk> {
        budget.check()?;
        self.root.current()?;
        let resource = self.get(resource, version)?;
        let text = &resource.snapshot.text;
        let boundary = |offset| {
            text.is_char_boundary(offset)
                && !(offset > 0 && text.as_bytes().get(offset - 1..=offset) == Some(b"\r\n"))
        };
        if maximum == 0 || maximum > MAX_CHUNK_BYTES || !boundary(offset) {
            return Err(denied());
        }
        let mut end = offset.saturating_add(maximum).min(text.len());
        while end > offset && !boundary(end) {
            end -= 1;
        }
        if end == offset && offset < text.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Chunk size cannot hold the next character or line break.",
            ));
        }
        budget.charge(end - offset)?;
        Ok(Chunk {
            text: text[offset..end].into(),
            next: (end < text.len()).then_some(end),
        })
    }
    pub fn release(&mut self, resource: Handle) -> io::Result<()> {
        self.values.remove(&resource).ok_or_else(denied)?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Policy, Protected, Right};
    use std::{sync::atomic::AtomicBool, time::Duration};
    fn session(path: &std::path::Path) -> Resources {
        Resources::new(
            Root::open_selected(
                path,
                Policy::new([Right::Read]).unwrap(),
                Protected::new(vec![]).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn foreign_versions_released_handles_and_split_unicode_cannot_disclose_data() {
        let directory = tempfile::tempdir().unwrap();
        let text = "λ\r\n東京🦀";
        std::fs::write(directory.path().join("text.txt"), text).unwrap();
        let mut a = session(directory.path());
        let b = session(directory.path());
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(2), 4096);
        let version = a.load(a.scope(), "text.txt", &mut budget).unwrap();
        assert!(b.get(version.resource, version.version).is_err());
        assert!(a.get(version.resource, Handle::fresh().unwrap()).is_err());
        assert!(a.load(b.scope(), "text.txt", &mut budget).is_err());
        assert!(a
            .chunk(version.resource, version.version, 1, 4, &mut budget)
            .is_err());
        assert!(a
            .chunk(version.resource, version.version, 3, 4, &mut budget)
            .is_err());
        let mut offset = 0;
        let mut reconstructed = String::new();
        loop {
            let chunk = a
                .chunk(version.resource, version.version, offset, 4, &mut budget)
                .unwrap();
            reconstructed.push_str(&chunk.text);
            if let Some(next) = chunk.next {
                offset = next;
            } else {
                break;
            }
        }
        assert_eq!(reconstructed, text);
        a.release(version.resource).unwrap();
        assert!(a.get(version.resource, version.version).is_err());
    }
    #[test]
    fn repeated_cached_chunks_still_consume_the_chain_budget() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("text.txt"), "fixture").unwrap();
        let mut session = session(directory.path());
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(2), 14);
        let version = session
            .load(session.scope(), "text.txt", &mut budget)
            .unwrap();
        session
            .chunk(version.resource, version.version, 0, 7, &mut budget)
            .unwrap();
        assert!(session
            .chunk(version.resource, version.version, 0, 7, &mut budget)
            .is_err());
    }
}
