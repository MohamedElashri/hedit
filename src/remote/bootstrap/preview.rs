//! Read-only folder discovery before a workspace writer has been selected.
use super::{absolute, quote};
use hedit_remote::client::SshTarget;
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const PREFIX: &[u8] = b"HEDIT_DIRS_V1";
const DIRECTORY_BYTES: usize = 48 * 1024;
const OUTPUT_LIMIT: usize = 64 * 1024;
const MAX_DIRECTORIES: usize = 256;

#[derive(Debug)]
pub struct DirectoryPreview {
    pub path: String,
    pub directories: Vec<String>,
    pub incomplete: bool,
}

/// Browse with the same SSH authentication and host verification as setup.
/// This runs before helper setup and never opens a host journal.
pub fn browse_directories(
    target: &SshTarget,
    path: Option<&str>,
    cancel: &AtomicBool,
) -> Result<DirectoryPreview, String> {
    let command = target
        .command_for(&listing_command(path)?)
        .map_err(|error| error.to_string())?;
    parse(&capture(command, cancel)?)
}

fn listing_command(path: Option<&str>) -> Result<String, String> {
    let path = if let Some(path) = path {
        absolute(path)?;
        quote(path)
    } else {
        "\"$HOME\"".into()
    };
    // Prune every entry immediately. Keep NUL framing so names containing
    // newlines cannot impersonate other entries.
    let listing = format!(
        "set -eu; cd -P {path}; test -r .; printf 'HEDIT_DIRS_V1\\000%s\\000' \"$PWD\"; \
         find . ! -name . -prune -type d -print0 | head -c {}",
        DIRECTORY_BYTES + 1
    );
    // Run the known script with a POSIX shell on Bash, Zsh and fish accounts.
    Ok(format!("exec /bin/sh -c {}", quote(&listing)))
}

fn capture(mut command: Command, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Remote folder preview cancelled.".into());
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = bounded_read(child.stdout.take().expect("piped stdout"));
    let stderr = bounded_read(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + Duration::from_secs(30);
    let stop = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    let status = loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            stop(&mut child);
            return Err(if cancel.load(Ordering::Relaxed) {
                "Remote folder preview cancelled.".into()
            } else {
                "Reading the remote folder timed out.".into()
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                stop(&mut child);
                return Err(error.to_string());
            }
        }
    };
    let result = if status.success() { stdout } else { stderr }
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "The SSH folder preview did not finish.".to_owned())??;
    if !status.success() {
        let message: String = String::from_utf8_lossy(&result)
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(512)
            .collect();
        return Err(if message.trim().is_empty() {
            format!("Could not read the remote folder ({status}). Check the path and SSH access.")
        } else {
            message.trim().to_owned()
        });
    }
    Ok(result)
}

fn bounded_read(pipe: impl Read + Send + 'static) -> mpsc::Receiver<Result<Vec<u8>, String>> {
    let (tx, rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("ssh-folder-preview-output".into())
        .spawn(move || {
            let mut output = vec![];
            let result = pipe
                .take((OUTPUT_LIMIT + 1) as u64)
                .read_to_end(&mut output)
                .map_err(|error| error.to_string())
                .and_then(|count| {
                    if count <= OUTPUT_LIMIT {
                        Ok(output)
                    } else {
                        Err("The SSH folder preview exceeded its output limit.".into())
                    }
                });
            let _ = tx.send(result);
        });
    // An unsuccessful spawn drops the pipe and sender, waking the receiver.
    drop(thread);
    rx
}

fn parse(output: &[u8]) -> Result<DirectoryPreview, String> {
    let mut frames = output.split_inclusive(|&byte| byte == 0);
    let prefix = frames.next().and_then(|frame| frame.strip_suffix(&[0]));
    if prefix != Some(PREFIX) {
        return Err("Unexpected SSH folder response. Check for shell startup output.".into());
    }
    let path = frames
        .next()
        .and_then(|frame| frame.strip_suffix(&[0]))
        .and_then(|path| std::str::from_utf8(path).ok())
        .ok_or("The remote directory path is unavailable or is not UTF-8.")?;
    absolute(path)?;
    if path.chars().any(char::is_control) {
        return Err("The remote directory path contains control characters.".into());
    }
    let mut directories = Vec::new();
    let mut incomplete = output.len() > PREFIX.len() + path.len() + 2 + DIRECTORY_BYTES;
    for frame in frames {
        if directories.len() == MAX_DIRECTORIES {
            incomplete = true;
            break;
        }
        if let Some(name) = frame
            .strip_suffix(&[0])
            .and_then(|frame| frame.strip_prefix(b"./"))
            .and_then(|name| std::str::from_utf8(name).ok())
            .filter(|name| {
                !matches!(*name, "" | "." | "..")
                    && !name.contains('/')
                    && !name.chars().any(char::is_control)
            })
        {
            directories.push(name.to_owned());
        } else {
            incomplete = true;
        }
    }
    directories.sort_unstable();
    directories.dedup();
    Ok(DirectoryPreview {
        path: path.to_owned(),
        directories,
        incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_validates_paths_and_skips_invalid_entries() {
        assert!(listing_command(Some("/work/../else")).is_err());
        assert!(listing_command(Some("relative")).is_err());
        assert!(parse(b"login banner\nHEDIT_DIRS_V1\0/work\0").is_err());
        assert!(parse(b"HEDIT_DIRS_V1\0/work").is_err());
        let listing = parse(
            b"HEDIT_DIRS_V1\0/work\0./z dir\0./line\nbreak\0./nested/child\0./a\0./\xff\0./truncated",
        )
        .unwrap();
        assert_eq!(listing.path, "/work");
        assert_eq!(listing.directories, ["a", "z dir"]);
        assert!(listing.incomplete);
    }

    #[cfg(unix)]
    #[test]
    fn preview_lists_only_immediate_real_directories() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("host's workspace");
        std::fs::create_dir_all(root.join(".hidden/child")).unwrap();
        std::fs::create_dir(root.join("visible")).unwrap();
        std::fs::write(root.join("document"), "private").unwrap();
        std::os::unix::fs::symlink(root.join("visible"), root.join("link")).unwrap();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", &listing_command(root.to_str()).unwrap()]);
        let listing = parse(&capture(command, &AtomicBool::new(false)).unwrap()).unwrap();
        assert_eq!(listing.path, root.canonicalize().unwrap().to_str().unwrap());
        assert_eq!(listing.directories, [".hidden", "visible"]);
        assert!(!listing.incomplete);
    }
}
