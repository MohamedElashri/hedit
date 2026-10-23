//! Small, native command line. No shell is used to open input.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Default)]
pub struct Options {
    pub session_dir: Option<PathBuf>,
    pub files: Vec<PathBuf>,
    pub stdin: bool,
    pub new_buffer: bool,
    /// Keep the terminal waiting until the files are closed (`-w`), so hedit
    /// can be the editor for tools such as `git commit`.
    pub wait: bool,
    pub help: bool,
    pub version: bool,
}
impl Options {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, String> {
        let mut options = Self::default();
        let mut args = args.into_iter();
        let mut literal = false;
        while let Some(arg) = args.next() {
            if literal {
                options.files.push(arg.into());
                continue;
            }
            match arg.to_str() {
                Some("--") => literal = true,
                Some("--session-dir") => {
                    options.session_dir =
                        Some(args.next().ok_or("--session-dir needs a directory")?.into())
                }
                Some("--new") => options.new_buffer = true,
                Some("--wait" | "-w") => options.wait = true,
                Some("--help" | "-h") => options.help = true,
                Some("--version" | "-V") => options.version = true,
                Some("-") => options.stdin = true,
                Some(s) if s.starts_with('-') => {
                    return Err(format!(
                        "Unknown option: {s}. Use -- before a filename beginning with '-'."
                    ))
                }
                _ => options.files.push(arg.into()),
            }
        }
        Ok(options)
    }
}

pub fn location(path: &Path) -> (PathBuf, u64, u64) {
    if path.exists() {
        return (path.to_owned(), 1, 1);
    }
    let Some(s) = path.to_str() else {
        return (path.to_owned(), 1, 1);
    };
    let Some((prefix, last)) = s.rsplit_once(':') else {
        return (path.to_owned(), 1, 1);
    };
    let Some(last) = last.parse::<u64>().ok().filter(|n| *n > 0) else {
        return (path.to_owned(), 1, 1);
    };
    if let Some((file, line)) = prefix.rsplit_once(':') {
        if let Some(line) = line.parse::<u64>().ok().filter(|n| *n > 0) {
            return (PathBuf::from(file), line, last);
        }
    }
    (PathBuf::from(prefix), last, 1)
}
