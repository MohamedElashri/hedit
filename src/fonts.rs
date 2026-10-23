//! Find an installed font by family name, for the `font_face` setting.
use std::path::{Path, PathBuf};

/// Folders searched for installed fonts on this platform.
pub fn font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts".into(), "/Library/Fonts".into()]
    } else {
        vec!["/usr/share/fonts".into(), "/usr/local/share/fonts".into()]
    };
    if let Some(home) = home {
        if cfg!(target_os = "macos") {
            dirs.push(home.join("Library/Fonts"));
        } else {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The regular style of `family` in `dirs`, or `family` itself when it is a
/// font file path. Prefers a file named after the family alone or its Regular
/// style, and skips bold and italic styles.
pub fn find(family: &str, dirs: &[PathBuf]) -> Result<PathBuf, String> {
    let path = Path::new(family);
    if path.is_absolute() {
        return path
            .is_file()
            .then(|| path.to_owned())
            .ok_or_else(|| format!("{family} is not a font file."));
    }
    let wanted = normalize(family);
    if wanted.is_empty() {
        return Err("Font name is empty.".into());
    }
    let mut best: Option<(u8, PathBuf)> = None;
    let mut pending: Vec<(PathBuf, usize)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    let mut visited = 0;
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > 50_000 {
                break;
            }
            let path = entry.path();
            if path.is_dir() {
                if depth < 5 {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            if !matches!(extension.as_deref(), Some("ttf" | "otf")) {
                continue;
            }
            let stem = normalize(&path.file_stem().unwrap_or_default().to_string_lossy());
            let Some(style) = stem.strip_prefix(&wanted) else {
                continue;
            };
            let rank = match style {
                "" | "regular" | "book" => 0,
                s if s.contains("bold") || s.contains("italic") || s.contains("oblique") => {
                    continue
                }
                s if s.ends_with("regular") => 1,
                _ => 2,
            };
            if best.as_ref().is_none_or(|(r, p)| (rank, &path) < (*r, p)) {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, path)| path)
        .ok_or_else(|| format!("No installed font named “{family}” was found."))
}

/// Read a font file found by [`find`], refusing files over 32 MiB.
pub fn load(family: &str) -> Result<Vec<u8>, String> {
    let path = find(family, &font_dirs())?;
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 32 * 1024 * 1024 {
        return Err(format!("{} is too large.", path.display()));
    }
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}
