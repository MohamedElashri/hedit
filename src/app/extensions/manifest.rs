use hedit::{projects, syntax::Language};
use regex::Regex;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::OnceLock,
};

const API_VERSION: u32 = 1;
const MAX_DIRECTORY_ENTRIES: usize = 256;
const MAX_MANIFESTS: usize = 32;
const MAX_MANIFEST_BYTES: u64 = 32 * 1024;
const MAX_COMMANDS: usize = 16;
const MAX_ID_BYTES: usize = 48;
const MAX_TITLE_BYTES: usize = 128;
const MAX_BODY_BYTES: usize = 2048;
const MAX_BODY_LINES: usize = 16;
const MAX_LANGUAGES: usize = 16;
const MAX_FILENAME_PATTERN_BYTES: usize = 128;

#[derive(Clone)]
pub(in crate::app) struct Contribution {
    pub id: String,
    pub title: String,
    pub action: ExtensionAction,
    pub when: Context,
}

#[derive(Clone, Default, Deserialize)]
#[serde(try_from = "ContextDefinition")]
pub(in crate::app) struct Context {
    languages: Vec<Language>,
    filename: Option<Regex>,
    selection: Option<bool>,
}

impl Context {
    pub(super) fn matches(
        &self,
        language: Language,
        filename: Option<&str>,
        selection: bool,
    ) -> bool {
        (self.languages.is_empty() || self.languages.contains(&language))
            && self
                .filename
                .as_ref()
                .is_none_or(|pattern| filename.is_some_and(|name| pattern.is_match(name)))
            && self.selection.is_none_or(|required| required == selection)
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ContextDefinition {
    languages: Vec<String>,
    filename: Option<String>,
    selection: Option<bool>,
}

impl TryFrom<ContextDefinition> for Context {
    type Error = String;

    fn try_from(definition: ContextDefinition) -> Result<Self, Self::Error> {
        if definition.languages.len() > MAX_LANGUAGES {
            return Err(format!(
                "Use at most {MAX_LANGUAGES} languages in an extension context."
            ));
        }
        let languages = definition
            .languages
            .iter()
            .map(|id| {
                Language::from_id(id).ok_or_else(|| format!("Unknown editor language ID {id:?}."))
            })
            .collect::<Result<_, _>>()?;
        let filename = definition
            .filename
            .as_deref()
            .map(filename_pattern)
            .transpose()?;
        Ok(Self {
            languages,
            filename,
            selection: definition.selection,
        })
    }
}

fn filename_pattern(pattern: &str) -> Result<Regex, String> {
    if pattern.is_empty()
        || pattern.len() > MAX_FILENAME_PATTERN_BYTES
        || pattern.contains(['/', '\\'])
        || pattern.chars().any(char::is_control)
    {
        return Err(format!(
            "Use a filename pattern of 1–{MAX_FILENAME_PATTERN_BYTES} bytes without directory separators or control characters."
        ));
    }
    let mut regex = String::from(r"(?s)\A");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            _ => regex.push_str(&regex::escape(&ch.to_string())),
        }
    }
    regex.push_str(r"\z");
    Regex::new(&regex).map_err(|error| error.to_string())
}

#[derive(Deserialize)]
struct Version {
    api_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    api_version: u32,
    id: String,
    commands: Vec<Command>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    id: String,
    title: String,
    action: ExtensionAction,
    #[serde(default)]
    when: Context,
}

#[derive(Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(in crate::app) enum ExtensionAction {
    InsertSnippet {
        body: String,
    },
    NewFile {
        body: String,
        #[serde(default, deserialize_with = "template_language")]
        language: Option<Language>,
    },
}

fn template_language<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Language>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|id| {
            Language::from_id(&id).ok_or_else(|| {
                serde::de::Error::custom(format!("Unknown editor language ID {id:?}."))
            })
        })
        .transpose()
}

fn paths(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index == MAX_DIRECTORY_ENTRIES {
            return Err(io::Error::other(format!(
                "The directory contains more than {MAX_DIRECTORY_ENTRIES} entries."
            )));
        }
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && entry.file_type()?.is_file()
        {
            paths.push(path);
        }
    }
    paths.sort_unstable();
    Ok(paths)
}

pub(super) fn valid_id(id: &str) -> bool {
    id.len() <= MAX_ID_BYTES
        && id.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn valid_body(body: &str, variables: bool) -> bool {
    static TOKEN: OnceLock<Regex> = OnceLock::new();
    if body.is_empty()
        || body.len() > MAX_BODY_BYTES
        || body.bytes().filter(|byte| *byte == b'\n').count() >= MAX_BODY_LINES
    {
        return false;
    }
    // Flat numbered fields bound expansion and reuse the same editor snippet syntax.
    let pattern = TOKEN.get_or_init(|| {
        Regex::new(
            r"(?x)
              [^$\\]+
            | \\[\\$}]
            | \$(?P<bare>[0-9]+)
            | \$\{(?P<braced>[0-9]+)(?::[^$\\{}\r\n]*)?\}
            | \$\{(?P<variable>SELECTION|FILE_BASENAME)\}",
        )
        .expect("valid extension snippet token pattern")
    });
    let mut end = 0;
    for token in pattern.captures_iter(body) {
        let span = token.get(0).unwrap();
        let field = token.name("bare").or_else(|| token.name("braced"));
        if span.start() != end
            || field.is_some_and(|field| field.as_str().parse::<u32>().is_err())
            || (!variables && token.name("variable").is_some())
        {
            return false;
        }
        end = span.end();
    }
    end == body.len()
}

fn read_manifest(path: &Path) -> Result<Manifest, String> {
    let text = projects::read_text(path, MAX_MANIFEST_BYTES)?;
    let version: Version = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if version.api_version != API_VERSION {
        return Err(format!(
            "API version {} is unsupported. This editor supports version {API_VERSION}.",
            version.api_version
        ));
    }
    let manifest: Manifest = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    debug_assert_eq!(manifest.api_version, version.api_version);
    if !valid_id(&manifest.id) {
        return Err("Invalid extension ID. Use 1–48 lowercase letters, digits, or hyphens, starting with a letter.".into());
    }
    if !(1..=MAX_COMMANDS).contains(&manifest.commands.len()) {
        return Err(format!("Declare between 1 and {MAX_COMMANDS} commands."));
    }
    let mut command_ids = HashSet::new();
    for command in &manifest.commands {
        if !valid_id(&command.id) {
            return Err(format!("Invalid command ID {:?}. Use 1–48 lowercase letters, digits, or hyphens, starting with a letter.", command.id));
        }
        if !command_ids.insert(command.id.as_str()) {
            return Err(format!("Duplicate command ID {:?}.", command.id));
        }
        if command.title.is_empty()
            || command.title.len() > MAX_TITLE_BYTES
            || command.title.trim() != command.title
            || command.title.chars().any(char::is_control)
        {
            return Err(format!("Command {} needs a title of 1–{MAX_TITLE_BYTES} bytes without surrounding whitespace or control characters.", command.id));
        }
        let (body, variables) = match &command.action {
            ExtensionAction::InsertSnippet { body } => (body, true),
            ExtensionAction::NewFile { body, .. } => (body, false),
        };
        if body.contains('\0') {
            return Err(format!(
                "Command {} has a snippet containing a NUL character.",
                command.id
            ));
        }
        if !valid_body(body, variables) {
            return Err(format!("Command {} has an invalid snippet. Use 1–{MAX_BODY_BYTES} bytes, up to {MAX_BODY_LINES} lines, and flat fields numbered 0–{}.", command.id, u32::MAX));
        }
    }
    Ok(manifest)
}

pub(in crate::app) fn load(config: Option<&Path>) -> (Vec<Contribution>, Vec<String>) {
    let Some(directory) = config.map(|config| config.join("extensions")) else {
        return (Vec::new(), Vec::new());
    };
    let paths = match paths(&directory) {
        Ok(paths) => paths,
        Err(error) => {
            return (
                Vec::new(),
                vec![format!("Extensions in {}: {error}", directory.display())],
            );
        }
    };
    let mut errors = Vec::new();
    if paths.len() > MAX_MANIFESTS {
        errors.push(format!(
            "Extensions in {}: only the first {MAX_MANIFESTS} manifests by filename were read.",
            directory.display()
        ));
    }
    let mut extension_ids = HashSet::new();
    let mut contributions = Vec::new();
    for path in paths.into_iter().take(MAX_MANIFESTS) {
        match read_manifest(&path) {
            Ok(manifest) if extension_ids.insert(manifest.id.clone()) => {
                for command in manifest.commands {
                    contributions.push(Contribution {
                        id: format!("{}.{}", manifest.id, command.id),
                        title: command.title,
                        action: command.action,
                        when: command.when,
                    });
                }
            }
            Ok(manifest) => errors.push(format!(
                "Extension {}: duplicate extension ID {:?}.",
                path.display(),
                manifest.id
            )),
            Err(error) => errors.push(format!("Extension {}: {error}", path.display())),
        }
    }
    (contributions, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn command(id: &str, body: &str) -> Value {
        json!({"id": id, "title": "Insert note", "action": {"kind": "insert_snippet", "body": body}})
    }

    fn manifest(id: &str, commands: Vec<Value>) -> String {
        json!({"api_version": 1, "id": id, "commands": commands}).to_string()
    }

    #[test]
    fn extension_loader_isolates_invalid_manifests_and_resolves_duplicates() {
        let config = tempfile::tempdir().unwrap();
        let directory = config.path().join("extensions");
        fs::create_dir(&directory).unwrap();
        let valid = manifest("notes", vec![command("insert", "Το ${1:café}$0")]);
        for (name, value) in [
            ("a-good.json", valid.clone()),
            ("b-duplicate.json", valid),
            ("c-broken.json", "{".into()),
            (
                "d-future.json",
                r#"{"api_version":2,"future_schema":true}"#.into(),
            ),
            (
                "e-oversized.json",
                " ".repeat(MAX_MANIFEST_BYTES as usize + 1),
            ),
            (
                "f-command-duplicate.json",
                manifest("duplicate", vec![command("one", "a"), command("one", "b")]),
            ),
            (
                "g-invalid-action.json",
                manifest(
                    "bad",
                    vec![
                        json!({"id": "one", "title": "Action", "action": {"kind":"invalid","body":"x"}}),
                    ],
                ),
            ),
            ("ignored.txt", "ignored".into()),
        ] {
            fs::write(directory.join(name), value).unwrap();
        }
        fs::create_dir(directory.join("ignored.json")).unwrap();
        let (contributions, errors) = load(Some(config.path()));
        assert_eq!(contributions.len(), 1);
        assert_eq!(contributions[0].id, "notes.insert");
        assert!(matches!(&contributions[0].action,
            ExtensionAction::InsertSnippet { body } if body == "Το ${1:café}$0"));
        assert_eq!(errors.len(), 6, "{errors:?}");
        for (error, expected) in errors.iter().zip([
            "duplicate extension ID",
            "c-broken.json",
            "API version 2 is unsupported",
            "exceeds the",
            "Duplicate command ID",
            "unknown variant",
        ]) {
            assert!(error.contains(expected), "{error:?}");
        }
    }

    #[test]
    fn extension_loader_bounds_directory_discovery() {
        let config = tempfile::tempdir().unwrap();
        let directory = config.path().join("extensions");
        fs::create_dir(&directory).unwrap();
        for index in 0..=MAX_MANIFESTS {
            fs::write(
                directory.join(format!("{index:03}.json")),
                manifest(&format!("example-{index}"), vec![command("insert", "note")]),
            )
            .unwrap();
        }
        let (commands, errors) = load(Some(config.path()));
        assert_eq!(commands.len(), MAX_MANIFESTS);
        assert_eq!(commands.last().unwrap().id, "example-31.insert");
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("first 32 manifests"));
        for index in MAX_MANIFESTS..MAX_DIRECTORY_ENTRIES {
            fs::write(directory.join(format!("{index}.txt")), "").unwrap();
        }
        let (commands, errors) = load(Some(config.path()));
        assert!(commands.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("more than 256 entries"));
    }

    #[test]
    fn extension_template_allows_flat_fields_and_bounds_expansion() {
        assert!(valid_body(r"\$ ${1:café} $1 ${2} $0 \\ \}", false));
        assert!(valid_body("one\r\ntwo", false));
        assert!(valid_body("${SELECTION} ${FILE_BASENAME}$0", true));
        assert!(!valid_body("${FILE_BASENAME}", false));
        for body in [
            "",
            "${SELECTION}",
            "${1:${2:nested}}",
            "${1:mirrored $2}",
            "${1:missing-end",
            &"x".repeat(MAX_BODY_BYTES + 1),
            &"x\n".repeat(MAX_BODY_LINES),
        ] {
            assert!(!valid_body(body, false), "{body:?}");
        }
    }

    #[test]
    fn extension_loader_rejects_nul_in_plain_text_and_field_defaults() {
        let config = tempfile::tempdir().unwrap();
        let directory = config.path().join("extensions");
        fs::create_dir(&directory).unwrap();
        for (id, body) in [("plain", "before\0after"), ("field", "${1:before\0after}")] {
            fs::write(
                directory.join(format!("{id}.json")),
                manifest(id, vec![command("insert", body)]),
            )
            .unwrap();
        }
        let (commands, errors) = load(Some(config.path()));
        assert!(commands.is_empty());
        assert_eq!(errors.len(), 2);
        assert!(errors.iter().all(|error| error.contains("NUL")));
    }

    #[test]
    fn extension_template_rejects_overflowing_field_numbers() {
        assert!(valid_body("$4294967295 ${4294967295:valid}", false));
        assert!(valid_body(r"\$4294967296", false));
        for body in ["$4294967296", "${4294967296}", "${4294967296:invalid}"] {
            assert!(!valid_body(body, false), "{body:?}");
        }
    }

    #[test]
    fn context_selectors_match_language_filename_and_selection_together() {
        let context: Context = serde_json::from_value(json!({
            "languages": ["markdown"], "filename": "note-?.md", "selection": true
        }))
        .unwrap();
        assert!(context.matches(Language::Markdown, Some("note-λ.md"), true));
        assert!(!context.matches(Language::Python, Some("note-λ.md"), true));
        assert!(!context.matches(Language::Markdown, Some("note-long.md"), true));
        assert!(!context.matches(Language::Markdown, Some("note-λ.md"), false));
        assert!(!context.matches(Language::Markdown, None, true));
        for invalid in [
            json!({"languages": ["unknown"]}),
            json!({"filename": "folder/*.md"}),
            json!({"selection": "required"}),
            json!({"script": "anything"}),
        ] {
            assert!(serde_json::from_value::<Context>(invalid).is_err());
        }
    }
}
