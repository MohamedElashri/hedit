//! Installed fonts: list the families for the font picker and load one by
//! name for the `font_face` setting.
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
            if !matches!(extension.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
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

/// Font files hedit can read: TrueType and OpenType, alone or in collections.
fn readable(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".ttf", ".otf", ".ttc", ".otc"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// The best face from `fc-list` lines of `file|index|style`: a readable file,
/// the regular style first, then other upright styles, then any.
pub fn pick_face(listing: &str) -> Option<(PathBuf, u32)> {
    listing
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '|');
            let (file, index, style) = (parts.next()?, parts.next()?, parts.next()?);
            if !readable(file) {
                return None;
            }
            // fontconfig lists other weights with "Regular" as an alias, as in
            // "Black,Regular"; the face's own style is the first name.
            let style = style.to_ascii_lowercase();
            let own = style.split(',').next().unwrap_or("").trim();
            const OTHER: [&str; 11] = [
                "bold",
                "italic",
                "oblique",
                "black",
                "heavy",
                "thin",
                "light",
                "medium",
                "condensed",
                "narrow",
                "semi",
            ];
            let rank = if matches!(own, "regular" | "book" | "normal" | "roman") {
                0
            } else if OTHER.iter().any(|s| own.contains(s)) {
                2
            } else {
                1
            };
            Some((rank, PathBuf::from(file), index.parse().unwrap_or(0)))
        })
        .min_by(|a, b| (a.0, &a.1, a.2).cmp(&(b.0, &b.1, b.2)))
        .map(|(_, file, index)| (file, index))
}

/// The regular face of `family` as fontconfig knows it: file and face index.
/// None when fontconfig is missing or does not know the family.
fn fontconfig_face(family: &str) -> Option<(PathBuf, u32)> {
    let output = std::process::Command::new("fc-list")
        .args([family, "-f", "%{file}|%{index}|%{style}\n"])
        .output()
        .ok()?;
    pick_face(&String::from_utf8_lossy(&output.stdout))
}

/// Read the font for `family` (a family name or a font file path) with its
/// face index, refusing files over 64 MiB.
pub fn load(family: &str) -> Result<(Vec<u8>, u32), String> {
    let (path, index) = if Path::new(family).is_absolute() {
        (find(family, &[])?, 0)
    } else {
        match fontconfig_face(family) {
            Some(face) => face,
            None => (find(family, &font_dirs())?, 0),
        }
    };
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024 {
        return Err(format!("{} is too large.", path.display()));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !covers(&bytes, index, "") {
        return Err(format!("{} is not a font hedit can read.", path.display()));
    }
    Ok((bytes, index))
}

/// An installed font family, for the font picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    /// Every character has the same width, as code fonts do.
    pub monospace: bool,
    /// Scripts beyond Latin the family covers, such as "Arabic" or "CJK".
    pub scripts: Vec<&'static str>,
}

/// Languages fontconfig reports, and the script each stands for.
const LANGUAGES: &[(&str, &str)] = &[
    ("ar", "Arabic"),
    ("he", "Hebrew"),
    ("hi", "Devanagari"),
    ("th", "Thai"),
    ("zh-cn", "CJK"),
    ("ja", "CJK"),
    ("ko", "CJK"),
    ("el", "Greek"),
    ("ru", "Cyrillic"),
];

/// Families from `fc-list` output lines of `family|spacing|file|langs`,
/// leaving out families with no file hedit can read, such as web fonts.
pub fn parse_fc_list(output: &str) -> Vec<Family> {
    let mut families: Vec<Family> = vec![];
    for line in output.lines() {
        let mut parts = line.splitn(4, '|');
        let (Some(name), Some(spacing), Some(file), Some(langs)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if !readable(file) {
            continue;
        }
        let name = name.split(',').next().unwrap_or(name).trim();
        if name.is_empty() || name.starts_with('.') {
            continue;
        }
        let langs: Vec<&str> = langs.split('|').collect();
        let mut scripts: Vec<&'static str> = vec![];
        for (lang, script) in LANGUAGES {
            if langs.contains(lang) && !scripts.contains(script) {
                scripts.push(script);
            }
        }
        // 100 is fontconfig's monospace spacing; 90 is dual-width (CJK mono).
        let monospace = matches!(spacing.trim(), "100" | "90");
        match families.iter_mut().find(|f| f.name == name) {
            Some(family) => {
                family.monospace |= monospace;
                for script in scripts {
                    if !family.scripts.contains(&script) {
                        family.scripts.push(script);
                    }
                }
            }
            None => families.push(Family {
                name: name.to_owned(),
                monospace,
                scripts,
            }),
        }
    }
    families.sort_by(|a, b| {
        (!a.monospace, a.name.to_lowercase()).cmp(&(!b.monospace, b.name.to_lowercase()))
    });
    families
}

/// Installed font families, monospace ones first. Runs fontconfig or scans
/// the font folders, so call it off the UI thread.
pub fn families() -> Vec<Family> {
    let listed = std::process::Command::new("fc-list")
        .args([":", "-f", "%{family}|%{spacing}|%{file}|%{lang}\n"])
        .output()
        .ok()
        .filter(|o| o.status.success());
    if let Some(output) = listed {
        return parse_fc_list(&String::from_utf8_lossy(&output.stdout));
    }
    // Without fontconfig, name families after their files.
    let mut names = std::collections::BTreeSet::new();
    let mut pending: Vec<(PathBuf, usize)> = font_dirs().into_iter().map(|d| (d, 0)).collect();
    while let Some((dir, depth)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten().take(20_000) {
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
            if matches!(extension.as_deref(), Some("ttf" | "otf" | "ttc" | "otc")) {
                let stem = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                names.insert(stem.split('-').next().unwrap_or(&stem).to_owned());
            }
        }
    }
    names
        .into_iter()
        .map(|name| Family {
            monospace: name.to_lowercase().contains("mono"),
            name,
            scripts: vec![],
        })
        .collect()
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
