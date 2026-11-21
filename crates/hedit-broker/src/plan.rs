//! Immutable multi-file proposals. Preparing a preview never mutates files.
mod pages;
use crate::{
    resources::{Resource, Resources},
    workspace::{Budget, Right, MAX_FILE_BYTES},
    Handle,
};
use hedit_plugin::sdk;
use std::{
    collections::BTreeSet,
    io,
    sync::Arc,
    time::{Duration, Instant},
};
pub const MAX_PLAN_FILES: usize = 32;
pub const MAX_REPLACEMENT_BYTES: usize = 256 * 1024;
pub use hedit_plugin::broker::Change;
pub struct Proposed {
    resource: Arc<Resource>,
    text: String,
    _memory: crate::resources::Reservation,
    before_pages: pages::Pages,
    after_pages: pages::Pages,
}
impl Proposed {
    pub fn relative(&self) -> &str {
        &self.resource.snapshot().relative
    }
    pub fn before(&self) -> &str {
        &self.resource.snapshot().text
    }
    pub fn after(&self) -> &str {
        &self.text
    }
    pub fn identity(&self) -> crate::workspace::Identity {
        self.resource.snapshot().stamp.identity
    }
    pub fn before_page(&self, page: usize) -> (&str, usize) {
        self.before_pages.get(self.before(), page)
    }
    pub fn after_page(&self, page: usize) -> (&str, usize) {
        self.after_pages.get(self.after(), page)
    }
    pub(crate) fn snapshot(&self) -> &crate::workspace::Snapshot {
        self.resource.snapshot()
    }
}
pub struct Plan {
    id: Handle,
    scope: Handle,
    created: Instant,
    files: Vec<Proposed>,
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "Invalid workspace edit proposal.",
    )
}
pub(crate) fn conflict() -> io::Error {
    io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Workspace files changed. Review a new edit plan.",
    )
}
impl Plan {
    pub fn prepare(
        resources: &Resources,
        scope: Handle,
        changes: Vec<Change>,
        budget: &mut Budget<'_>,
    ) -> io::Result<Self> {
        let root = resources.root(scope)?;
        root.require(Right::Edit)?;
        if changes.is_empty() || changes.len() > MAX_PLAN_FILES {
            return Err(invalid());
        }
        let mut identities = BTreeSet::new();
        let mut files = Vec::with_capacity(changes.len());
        let mut replacement_bytes = 0usize;
        let mut normalized_bytes = 0usize;
        for change in changes {
            budget.check()?;
            let resource = resources.get(change.resource, change.version)?;
            let snapshot = resource.snapshot();
            if snapshot.stamp.identity.volume != root.identity().volume
                || !identities.insert(snapshot.stamp.identity)
                || change.edits.len() > sdk::MAX_EDITS
            {
                return Err(invalid());
            }
            let crlf = snapshot.text.contains("\r\n");
            for edit in &change.edits {
                replacement_bytes = replacement_bytes
                    .checked_add(edit.text.len())
                    .ok_or_else(invalid)?;
                normalized_bytes = normalized_bytes
                    .checked_add(sdk::normalized_replacement_bytes(&edit.text, crlf))
                    .ok_or_else(invalid)?;
                if replacement_bytes > MAX_REPLACEMENT_BYTES
                    || normalized_bytes > MAX_REPLACEMENT_BYTES
                {
                    return Err(invalid());
                }
            }
            let current = root.read(&snapshot.relative, budget)?;
            if current.stamp != snapshot.stamp || current.digest != snapshot.digest {
                return Err(conflict());
            }
            let memory = resources.reserve(
                snapshot
                    .text
                    .len()
                    .saturating_add(
                        change
                            .edits
                            .iter()
                            .map(|edit| edit.text.len() * 2)
                            .sum::<usize>(),
                    )
                    .saturating_add(pages::INDEX_BYTES),
            )?;
            let (text, _) = sdk::prepare_text_edits(&snapshot.text, crlf, &change.edits)
                .map_err(|_| invalid())?;
            if text.len() > MAX_FILE_BYTES {
                return Err(invalid());
            }
            if text != snapshot.text {
                files.push(Proposed {
                    before_pages: pages::Pages::new(&snapshot.text),
                    after_pages: pages::Pages::new(&text),
                    resource,
                    text,
                    _memory: memory,
                });
            }
        }
        if files.is_empty() {
            return Err(invalid());
        }
        Ok(Self {
            id: Handle::fresh()?,
            scope,
            created: Instant::now(),
            files,
        })
    }
    pub fn id(&self) -> Handle {
        self.id
    }
    pub fn scope(&self) -> Handle {
        self.scope
    }
    pub fn files(&self) -> &[Proposed] {
        &self.files
    }
    pub fn live(&self) -> bool {
        self.created.elapsed() < Duration::from_secs(120)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{Policy, Protected, Root};
    use hedit_plugin::sdk::Edit;
    use std::{path::Path, sync::atomic::AtomicBool};
    fn resources(path: &Path) -> Resources {
        Resources::new(
            Root::open_selected(
                path,
                Policy::new([Right::Read, Right::Edit]).unwrap(),
                Protected::new(vec![]).unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn proposals_use_shared_unicode_validation_and_do_not_write_before_acceptance() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("first.txt"), "λ\r\n東京").unwrap();
        std::fs::write(directory.path().join("second.txt"), "old name\n").unwrap();
        let mut resources = resources(directory.path());
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(2), 4096);
        let scope = resources.scope();
        let first = resources.load(scope, "first.txt", &mut budget).unwrap();
        let second = resources.load(scope, "second.txt", &mut budget).unwrap();
        let changes = vec![
            Change {
                resource: first.resource,
                version: first.version,
                edits: vec![Edit {
                    start: 4,
                    finish: 10,
                    text: "新しい\n行".into(),
                }],
            },
            Change {
                resource: second.resource,
                version: second.version,
                edits: vec![Edit {
                    start: 0,
                    finish: 3,
                    text: "new".into(),
                }],
            },
        ];
        let plan = Plan::prepare(&resources, scope, changes.clone(), &mut budget).unwrap();
        assert_eq!(plan.files()[0].after(), "λ\r\n新しい\r\n行");
        assert_eq!(plan.files()[1].after(), "new name\n");
        assert_eq!(
            std::fs::read_to_string(directory.path().join("second.txt")).unwrap(),
            "old name\n"
        );
        let mut bad = changes.clone();
        bad[1].edits[0].finish = 100;
        assert!(Plan::prepare(&resources, scope, bad, &mut budget).is_err());
        assert_eq!(
            std::fs::read_to_string(directory.path().join("first.txt")).unwrap(),
            "λ\r\n東京"
        );
        let mut duplicate = changes.clone();
        duplicate[1] = duplicate[0].clone();
        assert!(Plan::prepare(&resources, scope, duplicate, &mut budget).is_err());
        std::fs::write(directory.path().join("second.txt"), "concurrent change\n").unwrap();
        assert!(Plan::prepare(&resources, scope, changes, &mut budget).is_err());
    }
}
#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::workspace::{Policy, Protected, Root};
    use std::{sync::atomic::AtomicBool, time::Duration};
    #[test]
    fn aggregate_replacement_budget_counts_crlf_expansion_across_files() {
        let outer = tempfile::tempdir().unwrap();
        for name in ["first.txt", "second.txt"] {
            std::fs::write(outer.path().join(name), "old\r\n").unwrap();
        }
        let root = Root::open_selected(
            outer.path(),
            Policy::new([Right::Read, Right::Edit]).unwrap(),
            Protected::new([]).unwrap(),
        )
        .unwrap();
        let mut resources = Resources::new(root).unwrap();
        let scope = resources.scope();
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel, Duration::from_secs(3), 1024 * 1024);
        let mut changes = Vec::new();
        for name in ["first.txt", "second.txt"] {
            let version = resources.load(scope, name, &mut budget).unwrap();
            changes.push(Change {
                resource: version.resource,
                version: version.version,
                edits: vec![sdk::Edit {
                    start: 0,
                    finish: 0,
                    text: "\n".repeat(70_000),
                }],
            });
        }
        assert!(Plan::prepare(&resources, scope, changes, &mut budget).is_err());
        assert_eq!(
            std::fs::read_to_string(outer.path().join("first.txt")).unwrap(),
            "old\r\n"
        );
    }
}
