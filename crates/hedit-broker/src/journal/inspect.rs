//! Inspection never repairs, removes, or resumes a journal.
use super::*;
use crate::{plan::pages, resources::Reservation};
use std::sync::{atomic::AtomicUsize, Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Current {
    Original,
    Replacement,
    Missing,
    Conflict,
}
pub struct FileView {
    pub relative: String,
    pub original: Identity,
    pub replacement: Identity,
    pub current: Current,
    pub can_resume: bool,
    before: String,
    after: String,
    before_pages: pages::Pages,
    after_pages: pages::Pages,
}
impl FileView {
    pub fn before(&self) -> &str {
        &self.before
    }
    pub fn after(&self) -> &str {
        &self.after
    }
    pub fn before_page(&self, page: usize) -> (&str, usize) {
        self.before_pages.get(&self.before, page)
    }
    pub fn after_page(&self, page: usize) -> (&str, usize) {
        self.after_pages.get(&self.after, page)
    }
}
pub struct View {
    pub owner: Option<Owner>,
    pub files: Vec<FileView>,
    pub root: Identity,
    pub stage: Identity,
    _memory: Reservation,
}

/// Read exactly one bounded, stable regular file, independently authenticating
/// its physical identity and contents. Never follow a private-stage symlink.
fn read(file: File, budget: &mut Budget<'_>) -> io::Result<(workspace::Stamp, [u8; 32], String)> {
    let stamp = workspace::identity::regular(&file)?;
    if stamp.bytes > workspace::MAX_FILE_BYTES as u64 {
        return Err(mismatch());
    }
    let mut bytes = Vec::with_capacity(stamp.bytes as usize);
    let mut chunk = [0; 8192];
    let mut reader = &file;
    loop {
        budget.check()?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        budget.charge(n)?;
        if bytes.len().saturating_add(n) > stamp.bytes as usize {
            return Err(conflict());
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    if bytes.len() as u64 != stamp.bytes || workspace::identity::regular(&file)? != stamp {
        return Err(conflict());
    }
    let hash = Sha256::digest(&bytes).into();
    let text = String::from_utf8(bytes).map_err(|_| mismatch())?;
    if text.contains('\0') {
        return Err(mismatch());
    }
    Ok((stamp, hash, text))
}
fn staged(
    directory: &Dir,
    name: &str,
    expected: &workspace::Stamp,
    hash: [u8; 32],
    budget: &mut Budget<'_>,
) -> io::Result<Option<String>> {
    let file = match directory.open_with(
        name,
        cap_std::fs::OpenOptions::new()
            .read(true)
            .follow(FollowSymlinks::No)
            .nonblock(true),
    ) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let (actual, actual_hash, text) = read(file, budget)?;
    if !same(&actual, actual_hash, expected, hash) {
        return Err(mismatch());
    }
    Ok(Some(text))
}
impl Journal {
    /// A view is host-owned, bounded, and unauthoritative: every later action
    /// must reopen the selected root and revalidate the journal and live files.
    pub fn inspect(&self, root: &workspace::Root, budget: &mut Budget<'_>) -> io::Result<View> {
        root.require(workspace::Right::Read)?;
        root.current()?;
        if root.identity() != self.ledger.root {
            return Err(mismatch());
        }
        let bytes = self
            .ledger
            .files
            .iter()
            .try_fold(0usize, |sum, item| {
                sum.checked_add(item.old.bytes as usize)?
                    .checked_add(item.new.bytes as usize)?
                    .checked_add(
                        pages::INDEX_BYTES + item.relative.len() + std::mem::size_of::<FileView>(),
                    )
            })
            .ok_or_else(mismatch)?;
        // Reserve before allocating text, including one temporary workspace
        // snapshot per file. Reservations share the broker's global 64 MiB cap.
        let memory = Reservation::new(
            bytes
                .checked_add(workspace::MAX_FILE_BYTES)
                .ok_or_else(mismatch)?,
            Arc::new(AtomicUsize::new(0)),
        )?;
        let mut files = Vec::with_capacity(self.ledger.files.len());
        for (index, item) in self.ledger.files.iter().enumerate() {
            budget.check()?;
            let mut before = staged(
                &self.directory,
                &format!("{index}.old"),
                &item.old,
                item.old_hash,
                budget,
            )?;
            let mut after = staged(
                &self.directory,
                &format!("{index}.new"),
                &item.new,
                item.new_hash,
                budget,
            )?;
            let removed = staged(
                &self.directory,
                &format!("{index}.removed"),
                &item.new,
                item.new_hash,
                budget,
            )?;
            if after.is_some() && removed.is_some() {
                return Err(mismatch());
            }
            let pending = after.is_some();
            after = after.or(removed);
            let current = match root.read(&item.relative, budget) {
                Ok(snapshot)
                    if same(&snapshot.stamp, snapshot.digest, &item.old, item.old_hash) =>
                {
                    if before.is_none() {
                        before = Some(snapshot.text);
                    }
                    Current::Original
                }
                Ok(snapshot)
                    if same(&snapshot.stamp, snapshot.digest, &item.new, item.new_hash) =>
                {
                    if after.is_none() {
                        after = Some(snapshot.text);
                    }
                    Current::Replacement
                }
                Ok(_) => Current::Conflict,
                Err(error) if error.kind() == io::ErrorKind::NotFound => Current::Missing,
                Err(error) => return Err(error),
            };
            let before = before.ok_or_else(mismatch)?;
            let after = after.ok_or_else(mismatch)?;
            files.push(FileView {
                relative: item.relative.clone(),
                original: item.old.identity,
                replacement: item.new.identity,
                current,
                can_resume: pending || current == Current::Replacement,
                before_pages: pages::Pages::new(&before),
                after_pages: pages::Pages::new(&after),
                before,
                after,
            });
        }
        root.current()?;
        Ok(View {
            owner: self.ledger.owner.clone(),
            files,
            root: self.ledger.root,
            stage: self.ledger.stage,
            _memory: memory,
        })
    }
}
