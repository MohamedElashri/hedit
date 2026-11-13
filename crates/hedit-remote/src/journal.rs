//! Host-only transactional edit history. A reply is constructed after commit.
use crate::{
    host,
    protocol::{Action, DocumentInfo, Error, Result, MAX_DOCUMENT, MAX_INSERT, PAGE_BYTES},
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
    sync::Arc,
    time::Duration,
};

const MAX_DOCUMENTS: i64 = 128;
const MAX_EVENTS: u64 = i64::MAX as u64 - 1;
const MAX_JOURNAL: u64 = 128 * 1024 * 1024;
const APPLICATION_ID: i64 = 0x48454452;
type SaveReceipt = (u64, u64, Vec<u8>, Vec<u8>, i64);
type WorkspaceIdentity = (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Change {
    start: u64,
    removed: Arc<str>,
    inserted: Arc<str>,
    before_version: u64,
    after_version: u64,
    before_format: (String, bool),
    after_format: (String, bool),
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Document {
    info: DocumentInfo,
    text: String,
    undo: Vec<Change>,
    redo: Vec<Change>,
    logical: u64,
    saved_logical: u64,
}

fn byte(text: &str, index: u64) -> Result<usize> {
    let index = usize::try_from(index).map_err(|_| Error::Limit)?;
    if index == text.chars().count() {
        return Ok(text.len());
    }
    text.char_indices()
        .nth(index)
        .map(|(i, _)| i)
        .ok_or(Error::Conflict)
}

impl Document {
    fn edit(&mut self, start: u64, end: u64, text: &str) -> Result<Change> {
        if start > end || text.len() > MAX_INSERT {
            return Err(Error::Limit);
        }
        let (a, z) = (byte(&self.text, start)?, byte(&self.text, end)?);
        self.edit_bytes(a as u64, z as u64, text)
    }
    fn edit_bytes(&mut self, start: u64, end: u64, text: &str) -> Result<Change> {
        let (a, z) = (
            usize::try_from(start).map_err(|_| Error::Limit)?,
            usize::try_from(end).map_err(|_| Error::Limit)?,
        );
        if a > z
            || !self.text.is_char_boundary(a)
            || !self.text.is_char_boundary(z)
            || text.len() > MAX_INSERT
        {
            return Err(Error::Conflict);
        }
        if self.text.len() - (z - a) + text.len() > MAX_DOCUMENT {
            return Err(Error::Limit);
        }
        let change = Change {
            start,
            removed: Arc::from(&self.text[a..z]),
            inserted: Arc::from(text),
            before_version: self.logical,
            after_version: self.info.revision + 1,
            before_format: (self.info.encoding.clone(), self.info.bom),
            after_format: (self.info.encoding.clone(), self.info.bom),
        };
        self.text.replace_range(a..z, text);
        self.logical = change.after_version;
        Ok(change)
    }
    fn apply(&mut self, action: &Action) -> Result<()> {
        match action {
            Action::Edit { start, end, text } => {
                let change = self.edit(*start, *end, text)?;
                self.undo.push(change);
                self.redo.clear();
            }
            Action::EditBytes { start, end, text } => {
                let change = self.edit_bytes(*start, *end, text)?;
                self.undo.push(change);
                self.redo.clear();
            }
            Action::Undo {} => {
                let change = self.undo.last().cloned().ok_or(Error::Conflict)?;
                // Undo may restore more than MAX_INSERT bytes after a large deletion.
                self.replace_history(&change, true)?;
                self.undo.pop();
                self.redo.push(change);
            }
            Action::Redo {} => {
                let change = self.redo.last().cloned().ok_or(Error::Conflict)?;
                self.replace_history(&change, false)?;
                self.redo.pop();
                self.undo.push(change);
            }
            Action::Format { encoding, bom } => {
                let encoding = hedit_core::text_encoding::canonical(encoding)
                    .map_err(|_| Error::Unsupported)?
                    .to_string();
                hedit_core::text_encoding::encode(&self.text, &encoding, *bom)
                    .map_err(|_| Error::Unsupported)?;
                self.undo.push(Change {
                    start: 0,
                    removed: Arc::from(""),
                    inserted: Arc::from(""),
                    before_version: self.logical,
                    after_version: self.info.revision + 1,
                    before_format: (self.info.encoding.clone(), self.info.bom),
                    after_format: (encoding.clone(), *bom),
                });
                self.logical = self.info.revision + 1;
                self.info.encoding = encoding;
                self.info.bom = *bom;
                self.redo.clear();
            }
        }
        while self.undo.len() > 256
            || self
                .undo
                .iter()
                .chain(&self.redo)
                .map(|c| c.removed.len() + c.inserted.len() + 128)
                .sum::<usize>()
                > 16 * 1024 * 1024
        {
            if !self.undo.is_empty() {
                self.undo.remove(0);
            } else if !self.redo.is_empty() {
                self.redo.remove(0);
            } else {
                break;
            }
        }
        self.info.revision = self.info.revision.checked_add(1).ok_or(Error::Limit)?;
        self.info.bytes = self.text.len() as u64;
        self.info.dirty = self.logical != self.saved_logical;
        Ok(())
    }
    fn replace_history(&mut self, change: &Change, undo: bool) -> Result<()> {
        let (from, to) = if undo {
            (&change.inserted, &change.removed)
        } else {
            (&change.removed, &change.inserted)
        };
        let a = usize::try_from(change.start).map_err(|_| Error::Corrupt)?;
        let z = a.checked_add(from.len()).ok_or(Error::Corrupt)?;
        if self.text.get(a..z) != Some(from.as_ref())
            || self.text.len() - from.len() + to.len() > MAX_DOCUMENT
        {
            return Err(Error::Corrupt);
        }
        self.text.replace_range(a..z, to);
        let format = if undo {
            &change.before_format
        } else {
            &change.after_format
        };
        self.info.encoding = format.0.clone();
        self.info.bom = format.1;
        self.logical = if undo {
            change.before_version
        } else {
            change.after_version
        };
        Ok(())
    }
}

pub struct Journal {
    db: Connection,
    source_cache: HashMap<u64, (host::Stamp, Vec<u8>)>,
    session: String,
    _writer_lock: File,
    directory: File,
    root: Option<File>,
    root_path: Option<std::path::PathBuf>,
    _workspace_lock: Option<File>,
    state_path: std::path::PathBuf,
    documents: HashMap<u64, Document>,
}

fn storage(_: rusqlite::Error) -> Error {
    Error::Storage
}
fn number(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}
fn sql(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::Limit)
}

impl Journal {
    pub fn open(state: &Path) -> Result<Self> {
        let directory = host::private_directory(state)?;
        let writer_lock = host::private_file(&directory, "writer.lock")?;
        writer_lock.try_lock().map_err(|_| Error::Busy)?;
        let db_file = host::private_file(&directory, "recovery.sqlite3")?;
        // Validate sidecars before SQLite can follow/open them.
        for name in [
            "recovery.sqlite3-wal",
            "recovery.sqlite3-shm",
            "recovery.sqlite3-journal",
        ] {
            let p = state.join(name);
            if p.symlink_metadata().is_ok() {
                host::private_file(&directory, name)?;
            }
        }
        if rusqlite::version_number() < 3_051_003 {
            return Err(Error::Unsupported);
        }
        let db = Connection::open_with_flags(
            state.join("recovery.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(storage)?;
        db.busy_timeout(Duration::from_millis(100))
            .map_err(storage)?;
        let mut version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(storage)?;
        let app: i64 = db
            .query_row("PRAGMA application_id", [], |r| r.get(0))
            .map_err(storage)?;
        let tables: i64 = db
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
            .map_err(storage)?;
        if ![1, 2, 3].contains(&version) && (version != 0 || app != 0 || tables != 0) {
            return Err(Error::Unsupported);
        }
        if version != 0 && app != APPLICATION_ID {
            return Err(Error::Corrupt);
        }
        let mode: String = db
            .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
            .map_err(storage)?;
        if mode != "wal" {
            return Err(Error::Unsupported);
        }
        db.execute_batch(
            "PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;
            PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON;
            PRAGMA wal_autocheckpoint=256; PRAGMA journal_size_limit=1048576;
            PRAGMA max_page_count=65536;",
        )
        .map_err(storage)?;
        #[cfg(target_os = "macos")]
        db.execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")
            .map_err(storage)?;
        let synchronous: i64 = db
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .map_err(storage)?;
        if synchronous != 2 {
            return Err(Error::Storage);
        }
        if version == 0 {
            db.execute_batch("BEGIN IMMEDIATE;
                CREATE TABLE state (id INTEGER PRIMARY KEY CHECK(id=1), epoch INTEGER NOT NULL, used INTEGER NOT NULL,
                    root BLOB, device BLOB, inode BLOB, session BLOB NOT NULL DEFAULT (randomblob(32)));
                INSERT INTO state(id,epoch,used) VALUES(1,0,0);
                CREATE TABLE documents (id INTEGER PRIMARY KEY, path BLOB NOT NULL UNIQUE, baseline TEXT NOT NULL, baseline_digest BLOB NOT NULL,
                    sequence INTEGER NOT NULL DEFAULT 0, bytes INTEGER NOT NULL);
                CREATE TABLE edits (document INTEGER NOT NULL REFERENCES documents(id), sequence INTEGER NOT NULL,
                    base INTEGER NOT NULL, payload BLOB NOT NULL, digest BLOB NOT NULL, bytes INTEGER NOT NULL,
                    PRIMARY KEY(document,sequence));
                PRAGMA user_version=1; PRAGMA application_id=1212499026;
                COMMIT;").map_err(storage)?;
        }
        if version == 0 {
            version = 1;
        }
        if version == 1 {
            db.execute_batch("BEGIN IMMEDIATE;
                ALTER TABLE documents ADD COLUMN encoding TEXT NOT NULL DEFAULT 'UTF-8';
                ALTER TABLE documents ADD COLUMN bom INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE documents ADD COLUMN base_encoding TEXT NOT NULL DEFAULT 'UTF-8';
                ALTER TABLE documents ADD COLUMN base_bom INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE documents ADD COLUMN source_digest BLOB;
                ALTER TABLE documents ADD COLUMN logical INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE documents ADD COLUMN save_version INTEGER NOT NULL DEFAULT 0;
                ALTER TABLE documents ADD COLUMN saved_revision INTEGER NOT NULL DEFAULT 0;
                UPDATE documents SET source_digest=baseline_digest,logical=-1;
                CREATE TABLE operations(id TEXT PRIMARY KEY,document INTEGER NOT NULL,sequence INTEGER NOT NULL,path BLOB NOT NULL,digest BLOB NOT NULL,status INTEGER NOT NULL DEFAULT 0);
                CREATE TABLE metadata(key TEXT PRIMARY KEY,value BLOB NOT NULL);
                PRAGMA user_version=2; COMMIT;").map_err(storage)?;
        }
        if version <= 2 {
            db.execute_batch("BEGIN IMMEDIATE; ALTER TABLE operations ADD COLUMN logical INTEGER NOT NULL DEFAULT 0; CREATE TABLE moves(id TEXT PRIMARY KEY,source BLOB NOT NULL,target BLOB NOT NULL,device BLOB NOT NULL,inode BLOB NOT NULL,status INTEGER NOT NULL DEFAULT 0); CREATE TABLE snapshots(document INTEGER PRIMARY KEY REFERENCES documents(id),sequence INTEGER NOT NULL,payload BLOB NOT NULL,digest BLOB NOT NULL); PRAGMA user_version=3; COMMIT;").map_err(storage)?;
        }
        db_file.sync_all().map_err(|_| Error::Storage)?;
        directory.sync_all().map_err(|_| Error::Storage)?;
        let check: String = db
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(|_| Error::Corrupt)?;
        if check != "ok" {
            return Err(Error::Corrupt);
        }
        let session: String = db
            .query_row("SELECT hex(session) FROM state", [], |r| r.get(0))
            .map_err(storage)?;
        if session.len() != 64 || !session.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::Corrupt);
        }
        Ok(Self {
            db,
            source_cache: HashMap::new(),
            session,
            _writer_lock: writer_lock,
            directory,
            root: None,
            root_path: None,
            _workspace_lock: None,
            state_path: state.to_owned(),
            documents: HashMap::new(),
        })
    }

    pub fn changed(&mut self, id: u64) -> Result<bool> {
        let info = self.info(id)?;
        if info.path.first() == Some(&0) {
            return Ok(false);
        }
        let Ok(file) = host::open_source(self.root.as_ref().ok_or(Error::Missing)?, &info.path)
        else {
            return Ok(true);
        };
        let stamp = host::stamp(&file.metadata().map_err(|_| Error::Storage)?);
        let digest = if let Some((old, digest)) =
            self.source_cache.get(&id).filter(|(old, _)| *old == stamp)
        {
            let _ = old;
            digest.clone()
        } else {
            let Ok((_, _, _, digest)) =
                host::read_encoded(self.root.as_ref().ok_or(Error::Missing)?, &info.path)
            else {
                return Ok(true);
            };
            self.source_cache.insert(id, (stamp, digest.clone()));
            digest
        };
        let expected: Vec<u8> = self
            .db
            .query_row(
                "SELECT source_digest FROM documents WHERE id=?1",
                [sql(id)?],
                |r| r.get(0),
            )
            .map_err(storage)?;
        Ok(digest != expected)
    }
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn root_file(&self) -> Result<File> {
        self.root
            .as_ref()
            .ok_or(Error::Missing)?
            .try_clone()
            .map_err(|_| Error::Storage)
    }
    pub fn root_path(&self) -> Result<std::path::PathBuf> {
        self.root_path.clone().ok_or(Error::Missing)
    }
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }
    pub fn dirty_paths(&self) -> Result<Vec<Vec<u8>>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(|d| d.dirty)
            .map(|d| d.path)
            .collect())
    }
    pub fn text(&mut self, id: u64) -> Result<String> {
        Ok(self.load(id)?.text.clone())
    }
    pub fn metadata(&mut self, value: Option<&serde_json::Value>) -> Result<serde_json::Value> {
        if let Some(value) = value {
            let bytes =
                crate::protocol::encode_limit(value, 256 * 1024).map_err(|_| Error::Limit)?;
            self.db.execute("INSERT INTO metadata(key,value) VALUES('view',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[bytes]).map_err(storage)?;
        }
        let bytes: Option<Vec<u8>> = self
            .db
            .query_row("SELECT value FROM metadata WHERE key='view'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(storage)?;
        bytes
            .map(|b| serde_json::from_slice(&b).map_err(|_| Error::Corrupt))
            .unwrap_or(Ok(serde_json::Value::Null))
    }
    pub fn scratch(&mut self) -> Result<DocumentInfo> {
        let mut path = vec![0];
        path.extend_from_slice(host::random_id()?.as_bytes());
        if self.list()?.len() >= MAX_DOCUMENTS as usize {
            return Err(Error::Limit);
        }
        let digest = Sha256::digest(b"").to_vec();
        self.db.execute("INSERT INTO documents(path,baseline,baseline_digest,bytes,encoding,bom,base_encoding,base_bom,source_digest) VALUES(?1,'',?2,0,'UTF-8',0,'UTF-8',0,?2)",params![path,digest]).map_err(storage)?;
        self.info(self.db.last_insert_rowid() as u64)
    }
    pub fn new_document(&mut self, path: &[u8]) -> Result<DocumentInfo> {
        let (parent, name) = host::parent(self.root.as_ref().ok_or(Error::Missing)?, path)?;
        let file: File = rustix::fs::openat(
            &parent,
            &name,
            rustix::fs::OFlags::WRONLY
                | rustix::fs::OFlags::CREATE
                | rustix::fs::OFlags::EXCL
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::from_bits_truncate(0o600),
        )
        .map(File::from)
        .map_err(|_| Error::Conflict)?;
        file.sync_all().map_err(|_| Error::Storage)?;
        parent.sync_all().map_err(|_| Error::Storage)?;
        self.open_document(path)
    }
    pub fn save_document(
        &mut self,
        id: u64,
        sequence: u64,
        operation: &str,
        target: Option<&[u8]>,
    ) -> Result<DocumentInfo> {
        if operation.len() != 64 || !operation.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Protocol);
        }
        let info = self.info(id)?;
        let doc = self.load(id)?.clone();
        if info.revision != sequence {
            return Err(Error::Conflict);
        }
        let path = target.unwrap_or(&info.path).to_vec();
        let bytes = hedit_core::text_encoding::encode(&doc.text, &info.encoding, info.bom)
            .map_err(|_| Error::Unsupported)?;
        let digest = Sha256::digest(&bytes).to_vec();
        let prior: Option<SaveReceipt> = self
            .db
            .query_row(
                "SELECT document,sequence,path,digest,status FROM operations WHERE id=?1",
                [operation],
                |r| {
                    Ok((
                        number(r, 0)?,
                        number(r, 1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(storage)?;
        if let Some((old_id, old_seq, old_path, old_digest, status)) = prior {
            if old_id != id || old_seq != sequence || old_path != path || old_digest != digest {
                return Err(Error::Conflict);
            }
            if status == 1 {
                return self.info(id);
            }
            // A previous attempt crossed an unknown publication boundary. Inspect
            // the destination; never blindly repeat a replacement.
            if host::read_encoded(self.root.as_ref().ok_or(Error::Missing)?, &path)
                .ok()
                .is_some_and(|(_, _, _, hash)| hash == digest)
            {
                self.finish_save(id, sequence, doc.logical, &path, &digest, operation)?;
                return self.info(id);
            }
            return Err(Error::Conflict);
        }
        if self.used()?.saturating_add(path.len() as u64 + 256) > MAX_JOURNAL {
            return Err(Error::Limit);
        }
        self.db.execute("INSERT INTO operations(id,document,sequence,path,digest,logical) VALUES(?1,?2,?3,?4,?5,?6)",params![operation,sql(id)?,sql(sequence)?,path,digest,sql(doc.logical)?]).map_err(storage)?;
        let expected: Option<Vec<u8>> = if target.is_none() || target == Some(info.path.as_slice())
        {
            Some(
                self.db
                    .query_row(
                        "SELECT source_digest FROM documents WHERE id=?1",
                        [sql(id)?],
                        |r| r.get(0),
                    )
                    .map_err(storage)?,
            )
        } else {
            None
        };
        host::atomic_save(
            self.root.as_ref().ok_or(Error::Missing)?,
            &path,
            &bytes,
            expected.as_deref(),
        )?;
        #[cfg(feature = "test-faults")]
        fault_gate("save_after_publish");
        self.finish_save(id, sequence, doc.logical, &path, &digest, operation)?;
        self.info(id)
    }
    fn finish_save(
        &mut self,
        id: u64,
        sequence: u64,
        logical: u64,
        path: &[u8],
        digest: &[u8],
        operation: &str,
    ) -> Result<()> {
        let tx = self.db.transaction().map_err(storage)?;
        tx.execute("UPDATE documents SET source_digest=?1,saved_revision=?2,save_version=?3,path=?4 WHERE id=?5",params![digest,sql(sequence)?,sql(logical)?,path,sql(id)?]).map_err(storage)?;
        tx.execute("UPDATE operations SET status=1 WHERE id=?1", [operation])
            .map_err(storage)?;
        tx.execute("UPDATE state SET used=used+?1", [path.len() as i64 + 256])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        if let Some(doc) = self.documents.get_mut(&id) {
            doc.saved_logical = logical;
            doc.info.saved_revision = sequence;
            doc.info.path = path.to_vec();
            doc.info.dirty = doc.logical != logical;
        }
        Ok(())
    }
    pub fn reload_document(
        &mut self,
        id: u64,
        discard: bool,
        expected: Option<&str>,
    ) -> Result<DocumentInfo> {
        let info = self.info(id)?;
        if info.dirty && !discard {
            return Err(Error::Conflict);
        }
        let (text, encoding, bom, digest) =
            host::read_encoded(self.root.as_ref().ok_or(Error::Missing)?, &info.path)?;
        if expected.is_some_and(|value| value != text) {
            return Err(Error::Conflict);
        }
        let length = self.load(id)?.text.chars().count() as u64;
        let mut next = self.apply(
            id,
            info.revision + 1,
            info.revision,
            &Action::Edit {
                start: 0,
                end: length,
                text,
            },
        )?;
        if next.encoding != encoding || next.bom != bom {
            next = self.apply(
                id,
                next.revision + 1,
                next.revision,
                &Action::Format { encoding, bom },
            )?;
        }
        let logical = self.load(id)?.logical;
        self.db.execute("UPDATE documents SET source_digest=?1,saved_revision=?2,save_version=?3 WHERE id=?4",params![digest,sql(next.revision)?,sql(logical)?,sql(id)?]).map_err(storage)?;
        self.documents.remove(&id);
        self.info(id)
    }
    fn retarget_transaction(tx: &rusqlite::Transaction<'_>, from: &[u8], to: &[u8]) -> Result<()> {
        let mut stmt = tx
            .prepare("SELECT id,path FROM documents")
            .map_err(storage)?;
        let rows = stmt
            .query_map([], |r| Ok((number(r, 0)?, r.get::<_, Vec<u8>>(1)?)))
            .map_err(storage)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?;
        drop(stmt);
        for (id, old) in rows {
            if old == from || (old.starts_with(from) && old.get(from.len()) == Some(&b'/')) {
                let mut path = to.to_vec();
                path.extend_from_slice(&old[from.len()..]);
                tx.execute(
                    "UPDATE documents SET path=?1 WHERE id=?2",
                    params![path, sql(id)?],
                )
                .map_err(storage)?;
            }
        }
        Ok(())
    }
    fn identity(&self, path: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        let (p, n) = host::parent(self.root.as_ref().ok_or(Error::Missing)?, path)?;
        let st = rustix::fs::statat(&p, &n, rustix::fs::AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|_| Error::Missing)?;
        Ok((
            st.st_dev.to_be_bytes().to_vec(),
            st.st_ino.to_be_bytes().to_vec(),
        ))
    }
    pub fn prepare_move(&mut self, from: &[u8], to: &[u8]) -> Result<String> {
        let (device, inode) = self.identity(from)?;
        let id = host::random_id()?;
        if self.identity(to).is_ok() {
            return Err(Error::Conflict);
        }
        self.db
            .execute(
                "INSERT INTO moves(id,source,target,device,inode) VALUES(?1,?2,?3,?4,?5)",
                params![id, from, to, device, inode],
            )
            .map_err(storage)?;
        Ok(id)
    }
    pub fn finish_move(&mut self, id: &str) -> Result<()> {
        let (from, to, device, inode): (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) = self
            .db
            .query_row(
                "SELECT source,target,device,inode FROM moves WHERE id=?1 AND status=0",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(storage)?
            .ok_or(Error::Missing)?;
        let moved = self.identity(&to).ok() == Some((device.clone(), inode.clone()))
            && self.identity(&from).is_err();
        let untouched = self.identity(&from).ok() == Some((device, inode));
        if !moved && !untouched {
            return Err(Error::Conflict);
        }
        let tx = self.db.transaction().map_err(storage)?;
        if moved {
            Self::retarget_transaction(&tx, &from, &to)?;
        }
        tx.execute("DELETE FROM moves WHERE id=?1", [id])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.documents.clear();
        Ok(())
    }
    fn reconcile(&mut self) -> Result<()> {
        let ids = {
            let mut stmt = self
                .db
                .prepare("SELECT id FROM moves WHERE status=0")
                .map_err(storage)?;
            let values = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?;
            values
        };
        for id in ids {
            self.finish_move(&id)?;
        }
        let pending = {
            let mut stmt=self.db.prepare("SELECT id,document,sequence,path,digest,logical FROM operations WHERE status=0").map_err(storage)?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        number(r, 1)?,
                        number(r, 2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, Vec<u8>>(4)?,
                        number(r, 5)?,
                    ))
                })
                .map_err(storage)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(storage)?;
            rows
        };
        for (id, doc, sequence, path, digest, logical) in pending {
            if host::read_encoded(self.root.as_ref().ok_or(Error::Missing)?, &path)
                .ok()
                .is_some_and(|(_, _, _, hash)| hash == digest)
            {
                self.finish_save(doc, sequence, logical, &path, &digest, &id)?;
            }
        }
        Ok(())
    }

    pub fn acquire(&mut self) -> Result<u64> {
        let tx = self.db.transaction().map_err(storage)?;
        let prior: u64 = tx
            .query_row("SELECT epoch FROM state", [], |r| number(r, 0))
            .map_err(storage)?;
        let next = prior
            .checked_add(1)
            .filter(|n| *n < i64::MAX as u64)
            .ok_or(Error::Limit)?;
        tx.execute("UPDATE state SET epoch=?1", [next as i64])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(next)
    }

    pub fn workspace(&mut self, bytes: &[u8]) -> Result<()> {
        let path = host::path(bytes)?;
        let root = host::absolute_directory(&path)?;
        let m = root.metadata().map_err(|_| Error::Storage)?;
        let (dev, ino) = (m.dev().to_be_bytes(), m.ino().to_be_bytes());
        let prior: WorkspaceIdentity = self
            .db
            .query_row("SELECT root,device,inode FROM state", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(storage)?;
        if let Some(old) = prior.0 {
            if old != path.as_os_str().as_bytes()
                || prior.1.as_deref() != Some(dev.as_slice())
                || prior.2.as_deref() != Some(ino.as_slice())
            {
                return Err(Error::Conflict);
            }
        } else {
            self.db
                .execute(
                    "UPDATE state SET root=?1,device=?2,inode=?3",
                    params![bytes, dev.as_slice(), ino.as_slice()],
                )
                .map_err(storage)?;
        }
        if self._workspace_lock.is_none() {
            self._workspace_lock = Some(host::workspace_lock(&root)?);
        }
        self.root_path = Some(path);
        self.root = Some(root);
        self.reconcile()
    }

    pub fn open_document(&mut self, path: &[u8]) -> Result<DocumentInfo> {
        let existing: Option<u64> = self
            .db
            .query_row("SELECT id FROM documents WHERE path=?1", [path], |r| {
                number(r, 0)
            })
            .optional()
            .map_err(storage)?;
        if let Some(id) = existing {
            return self.info(id);
        }
        let count: i64 = self
            .db
            .query_row("SELECT count(*) FROM documents", [], |r| r.get(0))
            .map_err(storage)?;
        if count >= MAX_DOCUMENTS {
            return Err(Error::Limit);
        }
        let (text, encoding, bom, source_digest) =
            host::read_encoded(self.root.as_ref().ok_or(Error::Missing)?, path)?;
        let used = self.used()?;
        let cost = text.len() as u64 + path.len() as u64 + 512;
        if used.saturating_add(cost) > MAX_JOURNAL {
            return Err(Error::Limit);
        }
        let tx = self.db.transaction().map_err(storage)?;
        tx.execute(
            "INSERT INTO documents(path,baseline,baseline_digest,bytes,encoding,bom,base_encoding,base_bom,source_digest) VALUES(?1,?2,?3,?4,?5,?6,?5,?6,?7)",
            params![
                path,
                text,
                Sha256::digest(text.as_bytes()).as_slice(),
                text.len() as i64,encoding,bom,source_digest
            ],
        )
        .map_err(storage)?;
        let id = tx.last_insert_rowid() as u64;
        tx.execute("UPDATE state SET used=used+?1", [cost as i64])
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        let info = self.info(id)?;
        self.documents.insert(
            id,
            Document {
                info: info.clone(),
                text,
                undo: Vec::new(),
                redo: Vec::new(),
                logical: 0,
                saved_logical: 0,
            },
        );
        Ok(info)
    }

    fn used(&self) -> Result<u64> {
        self.db
            .query_row("SELECT used FROM state", [], |r| number(r, 0))
            .map_err(storage)
    }

    pub fn list(&self) -> Result<Vec<DocumentInfo>> {
        let mut stmt = self
            .db
            .prepare("SELECT id FROM documents ORDER BY id LIMIT 129")
            .map_err(storage)?;
        let ids = stmt
            .query_map([], |r| number(r, 0))
            .map_err(storage)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage)?;
        if ids.len() > MAX_DOCUMENTS as usize {
            return Err(Error::Corrupt);
        }
        ids.into_iter().map(|id| self.info(id)).collect()
    }
    pub fn info(&self, id: u64) -> Result<DocumentInfo> {
        self.db.query_row("SELECT id,sequence,bytes,path,saved_revision,encoding,bom,logical!=save_version FROM documents WHERE id=?1",[sql(id)?],|r|Ok(DocumentInfo {document:number(r,0)?,revision:number(r,1)?,bytes:number(r,2)?,path:r.get(3)?,saved_revision:number(r,4)?,encoding:r.get(5)?,bom:r.get(6)?,dirty:r.get(7)?})).optional().map_err(storage)?.ok_or(Error::Missing)
    }
    fn load(&mut self, id: u64) -> Result<&Document> {
        if !self.documents.contains_key(&id) {
            let mut info = self.info(id)?;
            if info.revision > MAX_EVENTS || info.bytes > MAX_DOCUMENT as u64 {
                return Err(Error::Corrupt);
            }
            let (text, digest): (String, Vec<u8>) = self
                .db
                .query_row(
                    "SELECT baseline,baseline_digest FROM documents WHERE id=?1",
                    [sql(id)?],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(storage)?;
            if text.len() > MAX_DOCUMENT || Sha256::digest(text.as_bytes()).as_slice() != digest {
                return Err(Error::Corrupt);
            }
            let mut doc = Document {
                info: DocumentInfo {
                    document: id,
                    revision: 0,
                    bytes: text.len() as u64,
                    dirty: false,
                    encoding: self
                        .db
                        .query_row(
                            "SELECT base_encoding FROM documents WHERE id=?1",
                            [sql(id)?],
                            |r| r.get(0),
                        )
                        .map_err(storage)?,
                    bom: self
                        .db
                        .query_row(
                            "SELECT base_bom FROM documents WHERE id=?1",
                            [sql(id)?],
                            |r| r.get(0),
                        )
                        .map_err(storage)?,
                    ..info.clone()
                },
                text,
                undo: Vec::new(),
                redo: Vec::new(),
                logical: 0,
                saved_logical: 0,
            };
            let snapshot: Option<(u64, Vec<u8>, Vec<u8>)> = self
                .db
                .query_row(
                    "SELECT sequence,payload,digest FROM snapshots WHERE document=?1",
                    [sql(id)?],
                    |r| Ok((number(r, 0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()
                .map_err(storage)?;
            if let Some((sequence, payload, digest)) = snapshot {
                if payload.len() > crate::protocol::MAX_VALUE
                    || Sha256::digest(&payload).as_slice() != digest
                {
                    return Err(Error::Corrupt);
                }
                doc = serde_json::from_slice(&payload).map_err(|_| Error::Corrupt)?;
                if doc.info.document != id
                    || doc.info.revision != sequence
                    || sequence > info.revision
                    || doc.text.len() != doc.info.bytes as usize
                    || doc.text.len() > MAX_DOCUMENT
                    || doc.undo.len() > 256
                    || doc.redo.len() > 256
                {
                    return Err(Error::Corrupt);
                }
                doc.info.path = info.path.clone();
                doc.info.saved_revision = info.saved_revision;
            }
            let mut stmt = self.db.prepare("SELECT sequence,base,payload,digest,bytes FROM edits WHERE document=?1 AND sequence>?2 ORDER BY sequence LIMIT 1000001").map_err(storage)?;
            let mut rows = stmt
                .query(params![sql(id)?, sql(doc.info.revision)?])
                .map_err(storage)?;
            while let Some(row) = rows.next().map_err(storage)? {
                let sequence: u64 = number(row, 0).map_err(storage)?;
                let base: u64 = number(row, 1).map_err(storage)?;
                let payload: Vec<u8> = row.get(2).map_err(storage)?;
                let digest: Vec<u8> = row.get(3).map_err(storage)?;
                let size: u64 = number(row, 4).map_err(storage)?;
                if sequence != doc.info.revision + 1
                    || base != doc.info.revision
                    || payload.len() > crate::protocol::MAX_VALUE
                    || Sha256::digest(&payload).as_slice() != digest
                {
                    return Err(Error::Corrupt);
                }
                let action: Action =
                    serde_json::from_slice(&payload).map_err(|_| Error::Corrupt)?;
                doc.apply(&action).map_err(|_| Error::Corrupt)?;
                if doc.info.bytes != size {
                    return Err(Error::Corrupt);
                }
            }
            let (logical, saved): (i64, u64) = self
                .db
                .query_row(
                    "SELECT logical,save_version FROM documents WHERE id=?1",
                    [sql(id)?],
                    |r| Ok((r.get(0)?, number(r, 1)?)),
                )
                .map_err(storage)?;
            let logical = if logical < 0 {
                self.db
                    .execute(
                        "UPDATE documents SET logical=?1 WHERE id=?2",
                        params![sql(doc.logical)?, sql(id)?],
                    )
                    .map_err(storage)?;
                info = self.info(id)?;
                doc.logical
            } else {
                logical as u64
            };
            if doc.logical != logical {
                return Err(Error::Corrupt);
            }
            doc.saved_logical = saved;
            doc.info.dirty = logical != saved;
            if doc.info != info {
                return Err(Error::Corrupt);
            }
            self.documents.insert(id, doc);
        }
        self.documents.get(&id).ok_or(Error::Missing)
    }

    pub fn read(&mut self, id: u64, offset: u64) -> Result<(DocumentInfo, String, bool)> {
        let doc = self.load(id)?;
        let start = usize::try_from(offset).map_err(|_| Error::Limit)?;
        if !doc.text.is_char_boundary(start) {
            return Err(Error::Conflict);
        }
        // Worst-case JSON escaping expands each byte sixfold.
        let mut end = start.saturating_add(PAGE_BYTES / 6).min(doc.text.len());
        while !doc.text.is_char_boundary(end) {
            end -= 1;
        }
        Ok((
            doc.info.clone(),
            doc.text[start..end].into(),
            end == doc.text.len(),
        ))
    }

    pub fn apply(
        &mut self,
        id: u64,
        sequence: u64,
        base: u64,
        action: &Action,
    ) -> Result<DocumentInfo> {
        if sequence == 0 || sequence > MAX_EVENTS || base >= i64::MAX as u64 {
            return Err(Error::Limit);
        }
        if let Action::Edit { text, .. } = action {
            if text.len() > MAX_INSERT {
                return Err(Error::Limit);
            }
        }
        let payload = crate::protocol::encode_limit(action, crate::protocol::MAX_VALUE)
            .map_err(|_| Error::Limit)?;
        let digest = Sha256::digest(&payload);
        let prior: Option<(u64, Vec<u8>, u64)> = self
            .db
            .query_row(
                "SELECT base,digest,bytes FROM edits WHERE document=?1 AND sequence=?2",
                params![sql(id)?, sql(sequence)?],
                |r| Ok((number(r, 0)?, r.get(1)?, number(r, 2)?)),
            )
            .optional()
            .map_err(storage)?;
        if let Some((old_base, old_digest, size)) = prior {
            if old_base != base || old_digest != digest.as_slice() {
                return Err(Error::Conflict);
            }
            // A matching receipt is insufficient if its recovery baseline or
            // history cannot be reconstructed after a restart.
            self.load(id)?;
            let mut prior = self.info(id)?;
            prior.revision = sequence;
            prior.bytes = size;
            return Ok(prior);
        }
        let mut next = self.load(id)?.clone();
        if next.info.revision != base || sequence != base + 1 {
            return Err(Error::Conflict);
        }
        next.apply(action)?;
        let cost = payload.len() as u64 + 512;
        if self.used()?.saturating_add(cost) > MAX_JOURNAL {
            self.compact(id)?;
            if self.used()?.saturating_add(cost) > MAX_JOURNAL {
                return Err(Error::Limit);
            }
        }
        let tx = self.db.transaction().map_err(storage)?;
        tx.execute(
            "INSERT INTO edits VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                sql(id)?,
                sql(sequence)?,
                sql(base)?,
                payload,
                digest.as_slice(),
                sql(next.info.bytes)?
            ],
        )
        .map_err(storage)?;
        tx.execute(
            "UPDATE documents SET sequence=?1,bytes=?2,encoding=?4,bom=?5,logical=?6 WHERE id=?3",
            params![
                sql(sequence)?,
                sql(next.info.bytes)?,
                sql(id)?,
                next.info.encoding,
                next.info.bom,
                sql(next.logical)?
            ],
        )
        .map_err(storage)?;
        tx.execute("UPDATE state SET used=used+?1", [cost as i64])
            .map_err(storage)?;
        #[cfg(feature = "test-faults")]
        fault_gate("before_commit");
        tx.commit().map_err(storage)?;
        #[cfg(feature = "test-faults")]
        fault_gate("after_commit");
        let info = next.info.clone();
        self.documents.insert(id, next);
        if sequence.is_multiple_of(512) {
            let _ = self.compact(id);
        }
        Ok(info)
    }

    pub fn compact(&mut self, id: u64) -> Result<()> {
        let doc = self.load(id)?.clone();
        let payload = crate::protocol::encode_limit(&doc, crate::protocol::MAX_VALUE)
            .map_err(|_| Error::Limit)?;
        let digest = Sha256::digest(&payload);
        let tx = self.db.transaction().map_err(storage)?;
        tx.execute("INSERT INTO snapshots(document,sequence,payload,digest) VALUES(?1,?2,?3,?4) ON CONFLICT(document) DO UPDATE SET sequence=excluded.sequence,payload=excluded.payload,digest=excluded.digest",params![sql(id)?,sql(doc.info.revision)?,payload,digest.as_slice()]).map_err(storage)?;
        tx.execute(
            "DELETE FROM edits WHERE document=?1 AND sequence<=?2",
            params![sql(id)?, sql(doc.info.revision.saturating_sub(256))?],
        )
        .map_err(storage)?;
        tx.execute("UPDATE state SET used=(SELECT COALESCE(sum(length(baseline)+length(path)+512),0) FROM documents)+(SELECT COALESCE(sum(length(payload)+512),0) FROM edits)+(SELECT COALESCE(sum(length(payload)+128),0) FROM snapshots)+(SELECT COALESCE(sum(length(path)+256),0) FROM operations)",[]).map_err(storage)?;
        #[cfg(feature = "test-faults")]
        fault_gate("compact_before_commit");
        tx.commit().map_err(storage)?;
        #[cfg(feature = "test-faults")]
        fault_gate("compact_after_commit");
        Ok(())
    }
    pub fn checkpoint(&self) -> Result<()> {
        let (busy, _, _): (i64, i64, i64) = self
            .db
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(storage)?;
        if busy != 0 {
            return Err(Error::Busy);
        }
        self.directory.sync_all().map_err(|_| Error::Storage)
    }
}

#[cfg(feature = "test-faults")]
fn fault_gate(stage: &str) {
    if std::env::var("HEDIT_REMOTE_TEST_STAGE").ok().as_deref() != Some(stage) {
        return;
    }
    let Some(marker) = std::env::var_os("HEDIT_REMOTE_TEST_GATE") else {
        return;
    };
    let marker = std::path::PathBuf::from(marker);
    if std::fs::write(&marker, stage).is_err() {
        return;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !marker.with_extension("continue").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Journal, u64) {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap();
        let root = path.join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("file"), "base").unwrap();
        let mut journal = Journal::open(&path.join("state")).unwrap();
        journal.workspace(root.as_os_str().as_bytes()).unwrap();
        let id = journal.open_document(b"file").unwrap().document;
        (temp, journal, id)
    }
    fn edit() -> Action {
        Action::Edit {
            start: 0,
            end: 0,
            text: "new".into(),
        }
    }

    #[test]
    fn failed_transaction_never_publishes_text_sequence_or_dedup_state() {
        let (_temp, mut j, id) = setup();
        j.db.execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON documents BEGIN SELECT RAISE(ABORT,'injected'); END").unwrap();
        assert_eq!(j.apply(id, 1, 0, &edit()), Err(Error::Storage));
        assert_eq!(j.read(id, 0).unwrap().1, "base");
        assert_eq!(j.info(id).unwrap().revision, 0);
        let rows: i64 =
            j.db.query_row("SELECT count(*) FROM edits", [], |r| r.get(0))
                .unwrap();
        assert_eq!(rows, 0);
        j.db.execute_batch("DROP TRIGGER fail_update").unwrap();
        j.apply(id, 1, 0, &edit()).unwrap();
        assert_eq!(j.read(id, 0).unwrap().1, "newbase");
    }

    #[test]
    fn compaction_reclaims_quota_before_accepting_an_edit() {
        let (_temp, mut j, id) = setup();
        j.db.execute("UPDATE state SET used=?1", [MAX_JOURNAL as i64])
            .unwrap();
        assert!(j.apply(id, 1, 0, &edit()).is_ok());
        assert_eq!(j.read(id, 0).unwrap().1, "newbase");
        assert!(j.used().unwrap() < MAX_JOURNAL);
    }

    #[test]
    fn valid_sqlite_with_corrupt_baseline_or_event_is_not_silently_recovered() {
        for baseline in [true, false] {
            let (temp, mut j, id) = setup();
            j.apply(id, 1, 0, &edit()).unwrap();
            if baseline {
                j.db.execute("UPDATE documents SET baseline='fake'", [])
                    .unwrap();
            } else {
                j.db.execute(
                    "UPDATE edits SET payload=?1",
                    [br#"{"kind":"undo"}"#.as_slice()],
                )
                .unwrap();
            }
            drop(j);
            let state = temp.path().canonicalize().unwrap().join("state");
            let mut j = Journal::open(&state).unwrap();
            assert_eq!(j.read(id, 0).err(), Some(Error::Corrupt));
            assert_eq!(j.apply(id, 1, 0, &edit()), Err(Error::Corrupt));
        }
    }

    #[test]
    fn future_schema_is_preserved_and_rejected() {
        let (temp, j, _) = setup();
        j.db.execute_batch("PRAGMA user_version=200").unwrap();
        drop(j);
        let state = temp.path().canonicalize().unwrap().join("state");
        assert!(matches!(Journal::open(&state), Err(Error::Unsupported)));
        let db = Connection::open(state.join("recovery.sqlite3")).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 200);
    }
}
