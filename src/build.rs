//! Build systems in the style of Sublime Text: a shell command per syntax, its
//! output streamed into a panel, and errors found with a `file_regex`.
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BuildSystem {
    pub name: String,
    /// Run through the shell, with variables such as `$file` expanded.
    pub shell_cmd: String,
    /// Where to run; defaults to the file's folder.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub working_dir: String,
    /// Captures file, line, and optionally column and message.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub file_regex: String,
    /// Syntax IDs this system builds automatically, such as `"cuda"`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub selector: Vec<String>,
}

const GCC_ERRORS: &str = r"^(..[^:\n]*):(\d+):(\d+): (.*)$";

/// The built-in build systems; settings can add more or replace these by name.
pub fn defaults() -> Vec<BuildSystem> {
    let system = |name: &str, cmd: &str, dir: &str, regex: &str, selector: &[&str]| BuildSystem {
        name: name.into(),
        shell_cmd: cmd.into(),
        working_dir: dir.into(),
        file_regex: regex.into(),
        selector: selector.iter().map(|s| s.to_string()).collect(),
    };
    vec![
        system(
            "CUDA (nvcc)",
            r#"nvcc -g -G "$file" -o "$file_base_name""#,
            "",
            r"^(..[^(\n]*)\((\d+)\): (.*)$",
            &["cuda"],
        ),
        system(
            "C++ (g++)",
            r#"g++ -g -Wall "$file" -o "$file_base_name""#,
            "",
            GCC_ERRORS,
            &["cpp"],
        ),
        system(
            "C (gcc)",
            r#"gcc -g -Wall "$file" -o "$file_base_name""#,
            "",
            GCC_ERRORS,
            &["c"],
        ),
        system(
            "Rust (cargo build)",
            "cargo build",
            "$folder",
            r"^\s*--> (.+?):(\d+):(\d+)$",
            &["rust", "toml"],
        ),
        system(
            "Python",
            r#"python3 -u "$file""#,
            "",
            r#"^\s*File "(.+)", line (\d+)"#,
            &["python"],
        ),
        system("Make", "make", "$folder", GCC_ERRORS, &["makefile"]),
    ]
}

/// User systems, then the built-in ones they do not replace by name. User
/// systems come first, so they win automatic selection.
pub fn all(user: &[BuildSystem]) -> Vec<BuildSystem> {
    let mut systems = user.to_vec();
    systems.extend(
        defaults()
            .into_iter()
            .filter(|d| !user.iter().any(|u| u.name == d.name)),
    );
    systems
}

/// The system to use: the chosen one by name, or the first whose selector
/// includes the syntax when `chosen` is "automatic".
pub fn choose<'a>(
    systems: &'a [BuildSystem],
    chosen: &str,
    syntax: &str,
) -> Option<&'a BuildSystem> {
    if chosen.eq_ignore_ascii_case("automatic") || chosen.is_empty() {
        systems
            .iter()
            .find(|s| s.selector.iter().any(|x| x == syntax))
    } else {
        systems.iter().find(|s| s.name == chosen)
    }
}

impl BuildSystem {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.shell_cmd.trim().is_empty() {
            return Err("Build systems need a name and a shell_cmd.".into());
        }
        if !self.file_regex.is_empty() {
            regex::Regex::new(&self.file_regex)
                .map_err(|e| format!("Build system “{}”: invalid file_regex: {e}", self.name))?;
        }
        Ok(())
    }
}

/// Variables for a build of `file` inside the workspace `folder`.
pub struct Variables {
    pub file: PathBuf,
    pub folder: PathBuf,
}

impl Variables {
    /// Replace `$file`, `$file_path`, `$file_name`, `$file_base_name`,
    /// `$file_extension`, and `$folder`, longest names first.
    pub fn expand(&self, text: &str) -> String {
        let name = |p: &Path| p.to_string_lossy().into_owned();
        let file = &self.file;
        let values = [
            (
                "$file_base_name",
                name(Path::new(file.file_stem().unwrap_or_default())),
            ),
            (
                "$file_extension",
                name(Path::new(file.extension().unwrap_or_default())),
            ),
            (
                "$file_name",
                name(Path::new(file.file_name().unwrap_or_default())),
            ),
            ("$file_path", name(file.parent().unwrap_or(Path::new(".")))),
            ("$folder", name(&self.folder)),
            ("$file", name(file)),
        ];
        let mut out = text.to_owned();
        for (key, value) in values {
            out = out.replace(key, &value);
        }
        out
    }
}

/// One error or warning location found in the output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The output line it came from, counting from 0.
    pub output_line: usize,
    pub file: PathBuf,
    pub line: usize,
    pub column: Option<usize>,
    pub message: String,
}

/// Find locations in output lines with `file_regex`, resolving relative paths
/// against the build's working folder.
pub fn locations(output: &str, file_regex: &str, dir: &Path, first_line: usize) -> Vec<Location> {
    let Ok(regex) = regex::Regex::new(file_regex) else {
        return vec![];
    };
    output
        .lines()
        .enumerate()
        .filter_map(|(n, text)| {
            let c = regex.captures(text)?;
            let file = PathBuf::from(c.get(1)?.as_str().trim());
            let line = c.get(2)?.as_str().parse().ok()?;
            // A third numeric capture is the column; otherwise it is the message.
            let (column, message) = match c.get(3) {
                Some(m) if m.as_str().parse::<usize>().is_ok() => {
                    (m.as_str().parse().ok(), c.get(4).map(|m| m.as_str()))
                }
                other => (None, other.map(|m| m.as_str())),
            };
            Some(Location {
                output_line: first_line + n,
                file: if file.is_absolute() {
                    file
                } else {
                    dir.join(file)
                },
                line,
                column,
                message: message.unwrap_or("").trim().to_owned(),
            })
        })
        .collect()
}

pub enum Event {
    Output(String),
    /// The exit code (`None` when killed) and how long the build took.
    Finished(Option<i32>, Duration),
}

/// A running build. Dropping it does not stop the process; call [`Self::cancel`].
pub struct Running {
    child: Arc<Mutex<Option<Child>>>,
    /// The build's process group, shared by everything the shell starts.
    group: u32,
    /// Set once the build's output has ended.
    done: Arc<std::sync::atomic::AtomicBool>,
    pub events: mpsc::Receiver<Event>,
}

impl Running {
    /// Start `command` through the shell in `dir`, merging stdout and stderr.
    pub fn start(
        command: &str,
        dir: &Path,
        wake: impl Fn() + Send + Clone + 'static,
    ) -> Result<Self, String> {
        let mut shell = Command::new("sh");
        shell
            .arg("-c")
            // Merge stderr into stdout so messages keep their order.
            .arg(format!("exec 2>&1; {command}"))
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped());
        // A group of its own lets Cancel stop the compilers the shell starts.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut shell, 0);
        let mut child = shell
            .spawn()
            .map_err(|e| format!("Could not start the build: {e}"))?;
        let mut stdout = child.stdout.take().expect("stdout is piped");
        let group = child.id();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished = Arc::clone(&done);
        let child = Arc::new(Mutex::new(Some(child)));
        let (tx, rx) = mpsc::channel();
        let started = Instant::now();
        let waiter = Arc::clone(&child);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            let mut pending = Vec::new();
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buffer[..n]);
                        // Send whole UTF-8 sequences; keep a split one for later.
                        let valid = match std::str::from_utf8(&pending) {
                            Ok(_) => pending.len(),
                            Err(e) if e.error_len().is_none() => e.valid_up_to(),
                            Err(_) => pending.len(),
                        };
                        let text = String::from_utf8_lossy(&pending[..valid]).into_owned();
                        pending.drain(..valid);
                        let _ = tx.send(Event::Output(text));
                        wake();
                    }
                }
            }
            finished.store(true, std::sync::atomic::Ordering::Relaxed);
            let status = waiter
                .lock()
                .ok()
                .and_then(|mut c| c.take())
                .and_then(|mut c| c.wait().ok());
            let _ = tx.send(Event::Finished(
                status.and_then(|s| s.code()),
                started.elapsed(),
            ));
            wake();
        });
        Ok(Self {
            child,
            group,
            done,
            events: rx,
        })
    }
    /// Stop the build's shell. Its output thread then reports it finished.
    pub fn cancel(&self) {
        if let Ok(mut child) = self.child.lock() {
            if let Some(child) = child.as_mut() {
                kill_group(self.group);
                let _ = child.kill();
            }
        }
        // A command the shell was starting as the signal arrived can miss it
        // and keep the output open; repeat until the output ends.
        let (group, done) = (self.group, Arc::clone(&self.done));
        std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(100));
                if done.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                kill_group(group);
            }
        });
    }
}

/// Send SIGKILL to every process in the build's group.
fn kill_group(group: u32) {
    #[cfg(unix)]
    if let Some(group) = rustix::process::Pid::from_raw(group as i32) {
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
    }
    #[cfg(not(unix))]
    let _ = group;
}
