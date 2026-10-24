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

/// Scripts egui's built-in fonts lack, each with characters a fallback font
/// must have. Arabic includes joined letter forms and the lam-alef ligature,
/// which [`crate::bidi::shape`] draws with.
const SCRIPTS: &[(&str, &str)] = &[
    ("Arabic", "\u{0628}\u{06CC}\u{FE91}\u{FEF4}\u{FEFB}\u{FB58}"),
    ("Hebrew", "\u{05D0}\u{05E9}"),
    ("Devanagari", "\u{0915}\u{093F}"),
    ("Thai", "\u{0E01}\u{0E32}"),
    ("CJK", "\u{4E00}\u{3042}\u{AC00}"),
];

/// Font files tried first on macOS, which has no fontconfig.
const MACOS_FONTS: &[&str] = &[
    "/System/Library/Fonts/SFArabic.ttf",
    "/System/Library/Fonts/GeezaPro.ttc",
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/SFHebrew.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Kohinoor.ttc",
    "/System/Library/Fonts/Thonburi.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
];

/// A system font that fills in a script, as egui font data.
pub struct Fallback {
    pub name: String,
    pub bytes: Vec<u8>,
    pub index: u32,
}

/// Whether the face at `index` in `bytes` draws every character of `sample`.
fn covers(bytes: &[u8], index: u32, sample: &str) -> bool {
    use ab_glyph::Font;
    ab_glyph::FontRef::try_from_slice_and_index(bytes, index)
        .is_ok_and(|font| sample.chars().all(|c| font.glyph_id(c).0 != 0))
}

/// Candidate (file, face index) pairs for `sample`, best first.
fn candidates(sample: &str) -> Vec<(PathBuf, u32)> {
    if cfg!(target_os = "macos") {
        return MACOS_FONTS.iter().map(|p| (PathBuf::from(p), 0)).collect();
    }
    let charset: Vec<String> = sample
        .chars()
        .map(|c| format!("{:04x}", c as u32))
        .collect();
    let pattern = format!("sans-serif:style=Regular:charset={}", charset.join(" "));
    let Ok(output) = std::process::Command::new("fc-match")
        .args(["-s", &pattern, "-f", "%{file}|%{index}|%{style}\n"])
        .output()
    else {
        return vec![("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf".into(), 0)];
    };
    let mut found: Vec<(PathBuf, u32)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .take(12)
        .filter_map(|line| {
            let mut parts = line.splitn(3, '|');
            let (file, index, style) = (parts.next()?, parts.next()?, parts.next()?);
            let style = style.to_ascii_lowercase();
            (style.starts_with("regular") || style.starts_with("book"))
                .then(|| (PathBuf::from(file), index.parse().unwrap_or(0)))
        })
        .collect();
    // Noto fonts are designed for these scripts; prefer them to broad fonts.
    found.sort_by_key(|(path, _)| !path.to_string_lossy().contains("Noto"));
    found
}

/// System fonts covering the scripts in [`SCRIPTS`], one per script and each
/// file at most once. Reads font files, so call it off the UI thread.
pub fn fallbacks() -> Vec<Fallback> {
    let mut loaded: Vec<Fallback> = Vec::new();
    for (script, sample) in SCRIPTS {
        if loaded.iter().any(|f| covers(&f.bytes, f.index, sample)) {
            continue;
        }
        for (path, index) in candidates(sample) {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if bytes.len() > 64 * 1024 * 1024 || !covers(&bytes, index, sample) {
                continue;
            }
            loaded.push(Fallback {
                name: format!("fallback-{script}"),
                bytes,
                index,
            });
            break;
        }
    }
    loaded
}
