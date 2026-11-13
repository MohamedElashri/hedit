//! Bounded framing. EOF inside a frame is never an accepted request.
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 64 * 1024;
pub const MAX_INSERT: usize = 8 * 1024 * 1024;
pub const MAX_DOCUMENT: usize = 8 * 1024 * 1024;
pub const MAX_VALUE: usize = 32 * 1024 * 1024;
pub const PAGE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Protocol,
    Limit,
    Permission,
    Unsupported,
    Conflict,
    Busy,
    Fenced,
    Missing,
    Storage,
    Corrupt,
    Unavailable,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Edit { start: u64, end: u64, text: String },
    EditBytes { start: u64, end: u64, text: String },
    Undo {},
    Redo {},
    Format { encoding: String, bom: bool },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Hello {},
    Ping {},
    Workspace {
        path: Vec<u8>,
    },
    Acquire {
        takeover: bool,
    },
    Open {
        epoch: u64,
        path: Vec<u8>,
    },
    List {},
    Changes {
        documents: Vec<u64>,
    },
    Content {
        document: u64,
    },
    Read {
        document: u64,
        offset: u64,
    },
    Apply {
        session: String,
        epoch: u64,
        document: u64,
        sequence: u64,
        base_revision: u64,
        action: Action,
    },
    Bind {
        session: String,
        epoch: u64,
        token: String,
    },
    Trust {
        epoch: u64,
        allow: bool,
    },
    Save {
        session: String,
        epoch: u64,
        document: u64,
        sequence: u64,
        operation: String,
        target: Option<Vec<u8>>,
    },
    Reload {
        session: String,
        epoch: u64,
        document: u64,
        discard: bool,
        expected_revision: u64,
        expected: Option<String>,
    },
    New {
        epoch: u64,
        path: Vec<u8>,
    },
    Scratch {
        epoch: u64,
    },
    Metadata {
        epoch: u64,
        value: Option<serde_json::Value>,
    },
    TaskStart {
        session: String,
        epoch: u64,
        call: hedit_core::backend::Call,
    },
    TaskPoll {
        id: u64,
    },
    TaskCancel {
        id: u64,
    },
    ProcessStart {
        session: String,
        epoch: u64,
        directory: Vec<u8>,
        command: Option<String>,
        file: Option<Vec<u8>>,
        cols: u16,
        rows: u16,
    },
    ProcessPoll {
        id: u64,
    },
    ProcessInput {
        id: u64,
        bytes: Vec<u8>,
    },
    ProcessResize {
        id: u64,
        cols: u16,
        rows: u16,
    },
    ProcessStop {
        id: u64,
    },
    UploadStart {
        length: u64,
    },
    UploadWrite {
        id: u64,
        #[serde(with = "binary")]
        bytes: Vec<u8>,
    },
    UploadRun {
        id: u64,
    },
    Download {
        id: u64,
        offset: u64,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub id: u64,
    pub request: Request,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocumentInfo {
    pub document: u64,
    pub revision: u64,
    pub bytes: u64,
    pub path: Vec<u8>,
    pub saved_revision: u64,
    pub encoding: String,
    pub bom: bool,
    pub dirty: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Response {
    Hello {
        session: String,
        worker_pid: u32,
        version: u32,
        max_frame: usize,
    },
    Pong {},
    Workspace {},
    Owner {
        epoch: u64,
        token: String,
    },
    Document {
        info: DocumentInfo,
    },
    Documents {
        items: Vec<DocumentInfo>,
    },
    Buffer {
        info: DocumentInfo,
        text: String,
    },
    Page {
        info: DocumentInfo,
        offset: u64,
        text: String,
        eof: bool,
    },
    Durable {
        info: DocumentInfo,
        sequence: u64,
    },
    Value {
        value: serde_json::Value,
    },
    Started {
        id: u64,
    },
    Pending {},
    TaskResult {
        result: std::result::Result<serde_json::Value, String>,
    },
    Process {
        bytes: Vec<u8>,
        done: bool,
        exit: Option<i32>,
    },
    Upload {
        id: u64,
    },
    Transfer {
        id: u64,
        length: u64,
    },
    Bytes {
        #[serde(with = "binary")]
        bytes: Vec<u8>,
        done: bool,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub version: u32,
    pub id: u64,
    pub result: Result<Response>,
}

/// Limits serialization too: escaping/control bytes cannot expand an outbound
/// message into an unbounded buffer before the length check.
pub fn encode(value: &impl Serialize) -> io::Result<Vec<u8>> {
    encode_limit(value, MAX_FRAME)
}
pub fn encode_limit(value: &impl Serialize, limit: usize) -> io::Result<Vec<u8>> {
    struct Bounded(Vec<u8>, usize);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.0.len().saturating_add(bytes.len()) > self.1 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "frame limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut out = Bounded(Vec::new(), limit);
    serde_json::to_writer(&mut out, value).map_err(io::Error::other)?;
    Ok(out.0)
}

pub fn write_frame(out: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let bytes = encode(value)?;
    out.write_all(&(bytes.len() as u32).to_be_bytes())?;
    out.write_all(&bytes)?;
    out.flush()
}

pub fn read_frame<T: DeserializeOwned>(input: &mut impl Read) -> io::Result<Option<T>> {
    let mut header = [0; 4];
    loop {
        match input.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    input.read_exact(&mut header[1..])?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame limit"));
    }
    let mut bytes = vec![0; len];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid frame"))
}

mod binary {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() > 56 * 1024 {
            return Err(serde::de::Error::custom("binary chunk too large"));
        }
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(serde::de::Error::custom)
    }
}
