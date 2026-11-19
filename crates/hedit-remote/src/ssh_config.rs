//! Bounded, read-only host discovery. OpenSSH remains responsible for resolving
//! Host/Match options at connection time. Discovery never executes Match exec.
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Component, Path, PathBuf},
};

#[derive(Default)]
pub struct Hosts {
    pub aliases: Vec<String>,
    pub warnings: Vec<String>,
}
/// Read a selected config, or the normal user and system configurations.
pub fn discover(config: Option<&Path>, home: &Path) -> Hosts {
    let mut scan = Scan {
        home,
        files: 0,
        bytes: 0,
        seen: BTreeSet::new(),
        aliases: BTreeSet::new(),
        warnings: vec![],
    };
    if let Some(path) = config {
        scan.file(path, 0, false);
    } else {
        scan.file(&home.join(".ssh/config"), 0, true);
        #[cfg(unix)]
        scan.file(Path::new("/etc/ssh/ssh_config"), 0, true);
        #[cfg(windows)]
        if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
            scan.file(&PathBuf::from(program_data).join("ssh/ssh_config"), 0, true);
        }
    }
    Hosts {
        aliases: scan.aliases.into_iter().collect(),
        warnings: scan.warnings,
    }
}
struct Scan<'a> {
    home: &'a Path,
    files: usize,
    bytes: usize,
    seen: BTreeSet<PathBuf>,
    aliases: BTreeSet<String>,
    warnings: Vec<String>,
}
impl Scan<'_> {
    fn file(&mut self, path: &Path, depth: usize, optional: bool) {
        if self.files >= 128 || depth > 8 || self.bytes >= 1024 * 1024 {
            if self.warnings.is_empty() {
                self.warnings
                    .push("SSH config discovery limit reached.".into());
            }
            return;
        }
        let path = match std::fs::canonicalize(path) {
            Ok(path) => path,
            Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                self.warnings.push(format!("{}: {e}", path.display()));
                return;
            }
        };
        if !self.seen.insert(path.clone()) {
            return;
        }
        self.files += 1;
        let result = (|| -> std::io::Result<String> {
            let file = hedit_core::file_io::open_regular(&path)?;
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("Not a regular SSH config"));
            }
            let mut bytes = vec![];
            file.take((1024 * 1024 - self.bytes + 1) as u64)
                .read_to_end(&mut bytes)?;
            self.bytes += bytes.len();
            if self.bytes > 1024 * 1024 {
                return Err(std::io::Error::other("SSH config exceeds discovery limit"));
            }
            String::from_utf8(bytes).map_err(std::io::Error::other)
        })();
        let text = match result {
            Ok(text) => text,
            Err(e) => {
                self.warnings.push(format!("{}: {e}", path.display()));
                return;
            }
        };
        for line in text.lines() {
            let fields = tokens(line);
            let Some(key) = fields.first() else {
                continue;
            };
            if key.eq_ignore_ascii_case("host") {
                for alias in &fields[1..] {
                    // Pattern and negated entries are selectors, not destinations.
                    if self.aliases.len() < 512 && valid_alias(alias) {
                        self.aliases.insert(alias.clone());
                    }
                }
            } else if key.eq_ignore_ascii_case("include") {
                for pattern in &fields[1..] {
                    let expanded = if let Some(tail) = pattern.strip_prefix("~/") {
                        self.home.join(tail)
                    } else {
                        let p = PathBuf::from(pattern);
                        if p.is_absolute() {
                            p
                        } else if path.starts_with("/etc/ssh") {
                            Path::new("/etc/ssh").join(p)
                        } else {
                            self.home.join(".ssh").join(p)
                        }
                    };
                    for included in expand(&expanded) {
                        self.file(&included, depth + 1, true);
                    }
                }
            }
        }
    }
}
fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && !alias.starts_with('-')
        && alias.len() <= 253
        && alias
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_:[]".contains(&b))
        && !alias.contains(['*', '?', '!'])
        && (!alias.contains('[') || alias.contains(':'))
}
/// Tokenize the discovery directives, accepting quotes, escapes, comments and =.
fn tokens(line: &str) -> Vec<String> {
    let mut result = vec![];
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for c in line.chars() {
        if escape {
            word.push(c);
            escape = false;
            continue;
        }
        if c == '\\' {
            escape = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                word.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '#' {
            break;
        } else if c.is_whitespace()
            || (c == '=' && (result.is_empty() || (result.len() == 1 && word.is_empty())))
        {
            if !word.is_empty() {
                result.push(std::mem::take(&mut word));
            }
        } else {
            word.push(c);
        }
    }
    if !word.is_empty() {
        result.push(word);
    }
    result
}
fn expand(pattern: &Path) -> Vec<PathBuf> {
    let mut paths = vec![PathBuf::new()];
    for component in pattern.components() {
        if let Component::Normal(name) = component {
            let name = name.to_string_lossy();
            if name.contains(['*', '?', '[']) {
                let mut regex = String::from("^");
                let mut class = false;
                for c in name.chars() {
                    match c {
                        '*' if !class => regex.push_str(".*"),
                        '?' if !class => regex.push('.'),
                        '[' => {
                            class = true;
                            regex.push('[');
                        }
                        ']' if class => {
                            class = false;
                            regex.push(']');
                        }
                        _ if class => regex.push(c),
                        _ => regex.push_str(&regex::escape(&c.to_string())),
                    }
                }
                regex.push('$');
                let Ok(regex) = regex::Regex::new(&regex) else {
                    return vec![];
                };
                paths = paths
                    .iter()
                    .flat_map(|p| std::fs::read_dir(p).into_iter().flatten().take(4096))
                    .filter_map(Result::ok)
                    .filter(|e| regex.is_match(&e.file_name().to_string_lossy()))
                    .take(128)
                    .map(|e| e.path())
                    .collect();
            } else {
                for p in &mut paths {
                    p.push(component.as_os_str());
                }
            }
        } else {
            for p in &mut paths {
                p.push(component.as_os_str());
            }
        }
    }
    paths.sort();
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_aliases_and_includes_without_evaluating_match_commands() {
        let dir = tempfile::tempdir().unwrap();
        let ssh = dir.path().join(".ssh");
        std::fs::create_dir_all(ssh.join("config.d")).unwrap();
        std::fs::write(ssh.join("config"), "Host=dev lab * !excluded wild?\n  Port 2222\nInclude config.d/*.conf\nMatch exec \"touch should-never-run\"\nHost 'quoted-host' # comment\nInclude config\n").unwrap();
        std::fs::write(
            ssh.join("config.d/a.conf"),
            "hOsT cluster\nInclude ~/extra\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("extra"), "Host extra\n").unwrap();
        let hosts = discover(Some(&ssh.join("config")), dir.path());
        assert_eq!(
            hosts.aliases,
            ["cluster", "dev", "extra", "lab", "quoted-host"]
        );
        assert!(hosts.warnings.is_empty(), "{:?}", hosts.warnings);
        assert!(!dir.path().join("should-never-run").exists());
    }
    #[test]
    fn selected_missing_or_oversized_config_reports_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        assert!(!discover(Some(&config), dir.path()).warnings.is_empty());
        std::fs::write(&config, "x".repeat(1024 * 1024 + 1)).unwrap();
        assert!(!discover(Some(&config), dir.path()).warnings.is_empty());
    }
}
