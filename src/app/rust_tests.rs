//! Choose Rust harness tests using source spans and conventional Cargo targets.
use super::*;
use std::path::{Path, PathBuf};
use syn::{spanned::Spanned, Item};

const LIMIT: usize = 512 * 1024;

pub(super) struct Run {
    pub directory: PathBuf,
    pub file: PathBuf,
    pub command: String,
    pub name: String,
}

fn executable_word(value: &str) -> Result<&str, String> {
    if !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        Ok(value)
    } else {
        Err("This Cargo target name is unsupported.".into())
    }
}

fn rust_name(value: &str) -> Result<&str, String> {
    let value = value.strip_prefix("r#").unwrap_or(value);
    if !value.is_empty() && value.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
        Ok(value)
    } else {
        Err("This Rust module or test name is unsupported.".into())
    }
}

fn tests(
    items: &[Item],
    path: &mut Vec<String>,
    line: Option<usize>,
    found: &mut Vec<String>,
) -> Result<(), String> {
    for item in items {
        match item {
            Item::Mod(module) => {
                if let Some((_, items)) = &module.content {
                    path.push(rust_name(&module.ident.to_string())?.into());
                    tests(items, path, line, found)?;
                    path.pop();
                }
            }
            Item::Fn(function)
                if line.is_none_or(|line| {
                    (function.span().start().line..=function.block.span().end().line)
                        .contains(&line)
                }) && function.attrs.iter().any(|attr| {
                    attr.path()
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "test")
                }) =>
            {
                path.push(rust_name(&function.sig.ident.to_string())?.into());
                found.push(path.join("::"));
                path.pop();
            }
            _ => {}
        }
    }
    Ok(())
}

fn target(relative: &Path, manifest: &Path) -> Result<(String, Vec<String>), String> {
    let parts: Vec<_> = relative.iter().filter_map(|part| part.to_str()).collect();
    let stem = |part: &str| {
        part.strip_suffix(".rs")
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "The file is not a Rust source file.".to_owned())
    };
    let (target, parents) = match parts.as_slice() {
        ["src", "lib.rs"] => ("--lib".into(), vec![]),
        ["src", "main.rs"] => {
            let file =
                hedit_core::file_io::open_regular(manifest).map_err(|err| err.to_string())?;
            let bytes = hedit_core::file_io::read_bounded(file, 256 * 1024)
                .map_err(|err| err.to_string())?;
            let manifest = std::str::from_utf8(&bytes)
                .map_err(|err| err.to_string())?
                .parse::<toml_edit::DocumentMut>()
                .map_err(|err| err.to_string())?;
            let name = manifest
                .get("package")
                .and_then(|package| package.get("name"))
                .and_then(toml_edit::Item::as_str)
                .ok_or("Cargo package name is missing.")?;
            (format!("--bin {}", executable_word(name)?), vec![])
        }
        ["src", "bin", binary] => (
            format!("--bin {}", executable_word(&stem(binary)?)?),
            vec![],
        ),
        ["src", "bin", binary, "main.rs"] => {
            (format!("--bin {}", executable_word(binary)?), vec![])
        }
        ["tests", test] => (format!("--test {}", executable_word(&stem(test)?)?), vec![]),
        ["tests", test, "main.rs"] => (format!("--test {}", executable_word(test)?), vec![]),
        ["src", dirs @ .., file] if dirs.first() != Some(&"bin") => {
            let mut parents: Vec<String> = dirs
                .iter()
                .map(|name| rust_name(name).map(str::to_owned))
                .collect::<Result<_, _>>()?;
            if *file != "mod.rs" {
                parents.push(rust_name(&stem(file)?)?.into());
            }
            let target = if manifest.with_file_name("src").join("lib.rs").is_file() {
                "--lib"
            } else {
                "--bins"
            };
            (target.into(), parents)
        }
        _ => return Err("Choose a Rust file in a conventional Cargo target.".into()),
    };
    Ok((target, parents))
}

fn prepare(file: PathBuf, source: &str, line: Option<usize>) -> Result<Run, String> {
    let directory = file
        .ancestors()
        .skip(1)
        .find(|dir| dir.join("Cargo.toml").is_file())
        .ok_or("Could not find Cargo.toml above this file.")?;
    let relative = file
        .strip_prefix(directory)
        .map_err(|err| err.to_string())?;
    let (target, mut parents) = target(relative, &directory.join("Cargo.toml"))?;
    let parsed =
        syn::parse_file(source).map_err(|err| format!("Could not parse the Rust file: {err}"))?;
    let mut found = vec![];
    tests(&parsed.items, &mut parents, line, &mut found)?;
    if found.is_empty() {
        return Err(if line.is_some() {
            "Place the cursor inside a Rust test."
        } else {
            "No Rust tests found in this file."
        }
        .into());
    }
    // All target and filter words are validated above because the shared build
    // runner invokes a shell. File paths are passed only as working directories.
    let command = format!("cargo test {target} -- --exact {}", found.join(" "));
    if command.len() > 4096 {
        return Err("Choose a single test. This file has too many test names.".into());
    }
    let name = if line.is_some() {
        found.remove(0)
    } else {
        format!(
            "{} tests",
            file.file_name().unwrap_or_default().to_string_lossy()
        )
    };
    Ok(Run {
        directory: directory.to_owned(),
        file,
        command,
        name,
    })
}

impl HeditApp {
    pub(super) fn rust_tests_available(&self) -> bool {
        let buffer = self.active();
        self.workspace.remote.is_none()
            && buffer.language() == syntax::Language::Rust
            && buffer.page.info.path.is_some()
            && !buffer.follow
            && !buffer.page.info.paged
            && buffer.page.text.len() <= LIMIT
    }

    pub(super) fn run_rust_tests(&mut self, ctx: &egui::Context, entire_file: bool) {
        let buffer = self.active();
        if buffer.page.info.dirty {
            self.error = Some("Save the Rust file before running its tests.".into());
            return;
        }
        let Some(file) = buffer.page.info.path.as_ref().map(PathBuf::from) else {
            return;
        };
        let source = buffer.page.text.clone();
        let line = (!entire_file).then(|| {
            source
                .chars()
                .take(buffer.view.head)
                .filter(|&ch| ch == '\n')
                .count()
                + 1
        });
        self.job(ctx, move || {
            let run = prepare(file, &source, line)?;
            Ok(Action::Quality(Box::new(quality::Event::RunRustTests(run))))
        });
    }
}

#[cfg(test)]
#[test]
fn rust_test_at_cursor_selects_only_the_enclosing_case() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"sample\"\n",
    )
    .unwrap();
    let path = dir.path().join("src/lib.rs");
    let source = "mod example {\n#[test]\nfn first() {}\n#[test]\nfn second() {}\n}";
    let single = prepare(path.clone(), source, Some(3)).unwrap();
    assert_eq!(single.command, "cargo test --lib -- --exact example::first");
    let file = prepare(path, source, None).unwrap();
    assert_eq!(
        file.command,
        "cargo test --lib -- --exact example::first example::second"
    );
}
