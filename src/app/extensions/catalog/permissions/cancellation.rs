//! Cancellation precedes an exclusive policy change. A separate lock serializes
//! writers while existing disclosure guards drain; a crashed writer stays denied
//! until a reader safely repairs its orphaned marker under both locks.
use super::*;
use std::{ops::Deref, path::PathBuf};

const STOP_LOCK: &str = "extensions-stop.lock";
pub(in crate::app::extensions) const STOP_FILE: &str = "extensions-stop.json";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    generation: [u8; 16],
    pending: bool,
}
fn marker(config: &Path) -> Result<Option<Marker>, String> {
    let file = match open_leaf(&config.join(STOP_FILE), OpenOptions::new().read(true)) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    verify_grant_owner(&file)?;
    let bytes = hedit_core::file_io::read_bounded(file, 512).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "Invalid extension cancellation marker.".into())
}
fn control_lock(config: &Path, name: &str) -> Result<File, String> {
    fs::create_dir_all(config).map_err(|e| e.to_string())?;
    let file = open_leaf(
        &config.join(name),
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true),
    )
    .map_err(|e| e.to_string())?;
    verify_grant_owner(&file)?;
    Ok(file)
}
fn write_marker(config: &Path, value: &Marker) -> Result<(), String> {
    // NamedTempFile starts owner-private and persist never follows the old leaf.
    let mut file = tempfile::NamedTempFile::new_in(config).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, value).map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(config.join(STOP_FILE))
        .map_err(|e| e.error.to_string())?;
    super::super::lifecycle::sync_directory(config)
}
fn fresh() -> Result<[u8; 16], String> {
    let mut generation = [0; 16];
    getrandom::fill(&mut generation).map_err(|_| "Could not cancel extension sessions.")?;
    Ok(generation)
}
/// A compound generation prevents a cleared marker from reviving an old session,
/// even if the requested mutation subsequently fails before changing grants.
pub(in crate::app::extensions) fn broker_epoch(
    config: &Path,
    lock: &File,
) -> Result<Vec<u8>, String> {
    let epoch = read_epoch(lock)?;
    let mut result = vec![epoch.len() as u8];
    result.extend_from_slice(&epoch);
    match marker(config)? {
        Some(marker) if marker.pending => return Err("Extension policy is changing.".into()),
        Some(marker) => {
            result.push(1);
            result.extend_from_slice(&marker.generation);
        }
        None => result.push(0),
    }
    Ok(result)
}
pub(in crate::app::extensions) struct MutationGuard {
    policy: File,
    _writer: File,
    config: PathBuf,
    generation: [u8; 16],
}
impl Deref for MutationGuard {
    type Target = File;
    fn deref(&self) -> &File {
        &self.policy
    }
}
impl Drop for MutationGuard {
    fn drop(&mut self) {
        // A failed write deliberately leaves pending=true (fail closed). Both
        // locks are still held, so this cannot clear another writer's marker.
        let _ = write_marker(
            &self.config,
            &Marker {
                generation: self.generation,
                pending: false,
            },
        );
    }
}
pub(in crate::app::extensions) fn mutation_lock(config: &Path) -> Result<MutationGuard, String> {
    let writer = control_lock(config, STOP_LOCK)?;
    hedit_core::file_io::lock_with_timeout(&writer, Duration::from_millis(250))
        .map_err(|_| "Another extension change is in progress. Try again.")?;
    let generation = fresh()?;
    write_marker(
        config,
        &Marker {
            generation,
            pending: true,
        },
    )?;
    let result = (|| {
        let policy = control_lock(config, LOCK_FILE)?;
        hedit_core::file_io::lock_with_timeout(&policy, Duration::from_secs(5))
            .map_err(|_| "Extension sessions did not stop. Try again.")?;
        advance_one_time_epoch(&policy)?;
        Ok(MutationGuard {
            policy,
            _writer: writer,
            config: config.to_owned(),
            generation,
        })
    })();
    if result.is_err() {
        // The writer handle was dropped by the closure, so reacquire before
        // clearing; another writer may already own the marker. Leaving it for
        // orphan repair is safer than clearing an unknown generation.
        repair_orphan(config)?;
    }
    result
}
pub(in crate::app::extensions) fn repair_orphan(config: &Path) -> Result<(), String> {
    if !marker(config)?.is_some_and(|marker| marker.pending) {
        return Ok(());
    }
    let writer = control_lock(config, STOP_LOCK)?;
    match writer.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    }
    // Re-read after serializing writers; the initial observation may be stale.
    if !marker(config)?.is_some_and(|marker| marker.pending) {
        return Ok(());
    }
    let policy = change_lock(config)?;
    advance_one_time_epoch(&policy)?;
    write_marker(
        config,
        &Marker {
            generation: fresh()?,
            pending: false,
        },
    )
}

#[cfg(test)]
mod tests;
