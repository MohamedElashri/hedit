//! Explicit cleanup of interrupted preparation. No original may ever have been
//! captured in this state; receipts or unknown entries require normal recovery.
use super::*;
impl Journal {
    pub fn discard_unprepared(
        root: &workspace::Root,
        id: Handle,
        store: &crate::recovery_store::RecoveryStore,
        budget: &mut Budget<'_>,
    ) -> io::Result<()> {
        root.require(workspace::Right::Edit)?;
        root.current()?;
        budget.check()?;
        let lease = store.lease(id, root.identity())?;
        let record = lease.record();
        let directory = match root.directory().open_dir_nofollow(&record.stage_name) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound && record.stage.is_none() => {
                // The reservation preceded stage creation. A known stage that
                // disappeared could have been moved with its retained data.
                return store.release(record);
            }
            Err(error) => return Err(error),
        };
        hedit_core::private_io::verify(&directory.try_clone()?.into_std_file())?;
        let identity = workspace::identity::directory(&directory)?;
        if record.stage.is_some_and(|expected| identity != expected) {
            return Err(mismatch());
        }
        root.protected().insert(directory.try_clone()?)?;
        let mut names = Vec::new();
        let mut bytes = 0;
        for entry in directory.entries()? {
            budget.check()?;
            if record.stage.is_none() || names.len() >= crate::plan::MAX_PLAN_FILES {
                return Err(mismatch());
            }
            let name = entry?.file_name().into_string().map_err(|_| mismatch())?;
            let index = name.strip_suffix(".new").ok_or_else(mismatch)?;
            let index = index.parse::<usize>().map_err(|_| mismatch())?;
            if index >= crate::plan::MAX_PLAN_FILES || name != format!("{index}.new") {
                return Err(mismatch());
            }
            let file = directory.open_with(
                &name,
                cap_std::fs::OpenOptions::new()
                    .read(true)
                    .follow(FollowSymlinks::No)
                    .nonblock(true),
            )?;
            let stamp = workspace::identity::regular(&file)?;
            if stamp.bytes > workspace::MAX_FILE_BYTES as u64 {
                return Err(mismatch());
            }
            bytes += stamp.bytes;
            if bytes > record.payload_limit() {
                return Err(mismatch());
            }
            names.push(name);
        }
        // Fully validate before deleting anything. prepared.json (even partial),
        // captured originals, receipts, links and unknown files all fail closed.
        for name in names {
            budget.check()?;
            directory.remove_file(name)?;
        }
        native::sync_dir(&directory)?;
        root.protected().release_empty(&directory)?;
        let named = root.directory().open_dir_nofollow(&record.stage_name)?;
        if workspace::identity::directory(&named)? != identity {
            return Err(mismatch());
        }
        drop(named);
        drop(directory);
        root.directory().remove_dir(&record.stage_name)?;
        native::sync_dir(root.directory())?;
        store.release(record)
    }
}
