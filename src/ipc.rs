//! Per-session file mailbox. The recovery lock remains the authority for liveness.
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
#[derive(Serialize, Deserialize)]
pub struct WirePath(Vec<u8>);
impl WirePath {
    pub fn new(path: &Path) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            Self(path.as_os_str().as_bytes().to_vec())
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            Self(
                path.as_os_str()
                    .encode_wide()
                    .flat_map(u16::to_le_bytes)
                    .collect(),
            )
        }
        #[cfg(not(any(unix, windows)))]
        {
            Self(path.to_string_lossy().as_bytes().to_vec())
        }
    }
    pub fn path(&self) -> PathBuf {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            std::ffi::OsString::from_vec(self.0.clone()).into()
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(
                &self
                    .0
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect::<Vec<_>>(),
            )
            .into()
        }
        #[cfg(not(any(unix, windows)))]
        {
            String::from_utf8_lossy(&self.0).to_string().into()
        }
    }
}
#[derive(Serialize, Deserialize)]
pub struct Request {
    pub files: Vec<WirePath>,
    pub stdin: Option<WirePath>,
    pub new_buffer: bool,
}
pub struct Incoming {
    pub request: Request,
    reply: PathBuf,
}
impl Incoming {
    pub fn finish(self, result: Result<(), String>) -> Result<(), String> {
        crate::projects::atomic_json(&self.reply, &result)
    }
}
fn running(directory: &Path) -> Result<bool, String> {
    let lock = match File::options()
        .read(true)
        .write(true)
        .open(directory.join("session.lock"))
    {
        Ok(lock) => lock,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    match lock.try_lock() {
        Ok(()) => {
            let _ = lock.unlock();
            Ok(false)
        }
        Err(std::fs::TryLockError::WouldBlock) => Ok(true),
        Err(e) => Err(e.to_string()),
    }
}
pub fn forward(
    directory: &Path,
    options: &crate::cli::Options,
    stdin: Option<&Path>,
) -> Result<bool, String> {
    if !running(directory)? {
        return Ok(false);
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let request = Request {
        files: options
            .files
            .iter()
            .map(|p| {
                WirePath::new(&if p.is_absolute() {
                    p.clone()
                } else {
                    cwd.join(p)
                })
            })
            .collect(),
        stdin: stdin.map(WirePath::new),
        new_buffer: options.new_buffer,
    };
    let mailbox = directory.join("requests");
    fs::create_dir_all(&mailbox).map_err(|e| e.to_string())?;
    let mut stage = tempfile::Builder::new()
        .prefix("hedit-")
        .suffix(".tmp")
        .tempfile_in(&mailbox)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut stage, &request).map_err(|e| e.to_string())?;
    stage.flush().map_err(|e| e.to_string())?;
    let path = stage.path().with_extension("request");
    let reply = path.with_extension("reply");
    stage.persist_noclobber(&path).map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        if reply.exists() {
            let result = crate::projects::read_json::<Result<(), String>>(&reply, 1024 * 1024);
            let _ = fs::remove_file(&reply);
            let _ = fs::remove_file(&path);
            result??;
            return Ok(true);
        }
        if !running(directory)? {
            let _ = fs::remove_file(&path);
            return Ok(false);
        }
        if start.elapsed() > Duration::from_secs(60) {
            let _ = fs::remove_file(&path);
            return Err("The running editor has not acknowledged this request yet; check its window before retrying.".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
pub fn receive(directory: &Path) -> Option<Result<Incoming, String>> {
    let entries = fs::read_dir(directory.join("requests")).ok()?;
    for entry in entries.flatten().take(128) {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "request")
            || !entry.file_type().is_ok_and(|t| t.is_file())
        {
            continue;
        }
        let request = crate::projects::read_json(&path, 1024 * 1024);
        let _ = fs::remove_file(&path);
        return Some(request.map(|request| Incoming {
            request,
            reply: path.with_extension("reply"),
        }));
    }
    None
}
