pub(super) use hedit::plugin_options::valid_id;
use hedit::plugin_options::{Value as SettingValue, MAX_SETTINGS};
use hedit::syntax::Language;
use regex::Regex;
use serde::{de, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

const DECLARATIVE_API_VERSION: u32 = 1;
const LUA_API_VERSION: u32 = 2;
pub(super) const MAX_DIRECTORY_ENTRIES: usize = 256;
pub(super) const MAX_MANIFESTS: usize = 32;
pub(super) const MAX_MANIFEST_BYTES: u64 = 32 * 1024;
pub(super) const MAX_COMMANDS: usize = 16;
const MAX_TITLE_BYTES: usize = 128;
const MAX_BODY_BYTES: usize = 2048;
const MAX_BODY_LINES: usize = 16;
const MAX_LUA_SOURCE_BYTES: usize = 16 * 1024;
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
struct DeclarativeDefinition {
    api_version: u32,
    id: String,
    commands: Vec<Command>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LuaDefinition {
    api_version: u32,
    id: String,
    commands: Vec<Command>,
    permissions: Vec<Permission>,
    #[serde(deserialize_with = "unique_scripts")]
    scripts: BTreeMap<String, Arc<str>>,
    #[serde(default)]
    settings: SettingDefinitions,
}

pub(super) type SettingDefinitions = BTreeMap<String, PluginSetting>;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PluginSetting {
    pub(super) label: String,
    pub(super) default: SettingValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub(super) enum Permission {
    #[serde(rename = "selection.read")]
    ReadSelection,
    #[serde(rename = "selection.replace")]
    ReplaceSelection,
}

pub(super) const LUA_CAPABILITIES: [Permission; 2] =
    [Permission::ReadSelection, Permission::ReplaceSelection];

pub(super) struct Manifest {
    pub(super) id: String,
    commands: Vec<Command>,
    scripts: BTreeMap<String, Arc<str>>,
    pub(super) lua_digest: Option<String>,
    settings: Arc<SettingDefinitions>,
}

impl Manifest {
    pub(super) fn command_labels(&self) -> Vec<(String, String)> {
        self.commands
            .iter()
            .map(|command| (format!("{}.{}", self.id, command.id), command.title.clone()))
            .collect()
    }

    pub(super) fn fixture_settings(
        &self,
        overrides: &BTreeMap<String, SettingValue>,
    ) -> Result<BTreeMap<String, SettingValue>, String> {
        let mut settings = self
            .settings
            .iter()
            .map(|(key, definition)| (key.clone(), definition.default.clone()))
            .collect::<BTreeMap<_, _>>();
        for (key, value) in overrides {
            let setting = settings.get_mut(key).ok_or("Unknown fixture setting.")?;
            if !setting.same_type(value) || !value.valid() {
                return Err("Invalid fixture setting value.".into());
            }
            *setting = value.clone();
        }
        Ok(settings)
    }

    pub(super) fn lua_source(&self, id: &str) -> Option<&str> {
        let (extension, name) = id.split_once('.')?;
        if extension != self.id {
            return None;
        }
        let command = self.commands.iter().find(|command| command.id == name)?;
        let ActionDefinition::Lua { source } = &command.action else {
            return None;
        };
        self.scripts.get(source).map(AsRef::as_ref)
    }

    pub(super) fn script_digests(&self) -> BTreeMap<String, String> {
        self.scripts
            .iter()
            .map(|(name, source)| (name.clone(), digest(source.as_bytes())))
            .collect()
    }

    pub(super) fn lua_command_labels(&self) -> impl Iterator<Item = (String, &str)> {
        self.commands
            .iter()
            .filter(|command| matches!(command.action, ActionDefinition::Lua { .. }))
            .map(|command| {
                (
                    format!("{}.{}", self.id, command.id),
                    command.title.as_str(),
                )
            })
    }

    pub(super) fn into_contributions(
        self,
        installed_path: PathBuf,
        mut grants: BTreeMap<String, super::catalog::Grant>,
    ) -> impl Iterator<Item = Contribution> {
        self.commands.into_iter().filter_map(move |command| {
            let id = format!("{}.{}", self.id, command.id);
            let action = match command.action {
                ActionDefinition::InsertSnippet { body } => ExtensionAction::InsertSnippet { body },
                ActionDefinition::NewFile { body, language } => {
                    ExtensionAction::NewFile { body, language }
                }
                ActionDefinition::Lua { source } => ExtensionAction::Lua(LuaPlugin {
                    source: self.scripts.get(&source)?.clone(),
                    plugin_id: self.id.clone(),
                    settings: self.settings.clone(),
                    on_save: command.on_save,
                    installed_path: installed_path.clone(),
                    grant: Arc::new(grants.remove(&id)?),
                    one_time: None,
                }),
            };
            Some(Contribution {
                id,
                title: command.title,
                action,
                when: command.when,
            })
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    id: String,
    title: String,
    action: ActionDefinition,
    #[serde(default)]
    when: Context,
    #[serde(default)]
    on_save: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ActionDefinition {
    InsertSnippet {
        body: String,
    },
    NewFile {
        body: String,
        #[serde(default, deserialize_with = "template_language")]
        language: Option<Language>,
    },
    Lua {
        source: String,
    },
}

#[derive(Clone)]
pub(in crate::app) enum ExtensionAction {
    InsertSnippet {
        body: String,
    },
    NewFile {
        body: String,
        language: Option<Language>,
    },
    Lua(LuaPlugin),
}

#[derive(Clone)]
pub(in crate::app) struct LuaPlugin {
    pub source: Arc<str>,
    pub(super) plugin_id: String,
    pub(super) settings: Arc<SettingDefinitions>,
    pub(super) on_save: bool,
    pub(super) installed_path: PathBuf,
    pub(super) grant: Arc<super::catalog::Grant>,
    pub(super) one_time: Option<Arc<super::catalog::OneTimeApproval>>,
}

impl LuaPlugin {
    pub(super) fn is_one_time(&self) -> bool {
        self.one_time.is_some()
    }

    pub(super) fn approval_id(&self) -> Option<String> {
        (!self.is_one_time()).then(|| self.grant.approval_id())
    }
}

fn unique_scripts<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, Arc<str>>, D::Error> {
    struct Scripts;
    impl<'de> de::Visitor<'de> for Scripts {
        type Value = BTreeMap<String, Arc<str>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("an object mapping Lua script names to source text")
        }

        fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            let mut scripts = BTreeMap::new();
            while let Some((name, source)) = map.next_entry::<String, String>()? {
                if scripts.len() == MAX_COMMANDS {
                    return Err(de::Error::custom(format!(
                        "Use at most {MAX_COMMANDS} Lua scripts."
                    )));
                }
                if !name.strip_suffix(".lua").is_some_and(valid_id) {
                    return Err(de::Error::custom(
                        "Lua script names must be lowercase IDs ending in .lua.",
                    ));
                }
                if source.trim().is_empty()
                    || source.len() > MAX_LUA_SOURCE_BYTES
                    || source.contains(['\0', '\u{1b}'])
                {
                    return Err(de::Error::custom(format!(
                        "Lua scripts must contain 1–{MAX_LUA_SOURCE_BYTES} bytes of source text."
                    )));
                }
                if scripts.insert(name, Arc::from(source)).is_some() {
                    return Err(de::Error::custom("Duplicate Lua script name."));
                }
            }
            Ok(scripts)
        }
    }
    deserializer.deserialize_map(Scripts)
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

pub(super) fn paths(directory: &Path) -> io::Result<(Vec<PathBuf>, usize)> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    let mut count = 0;
    for (index, entry) in entries.enumerate() {
        if index == MAX_DIRECTORY_ENTRIES {
            return Err(io::Error::other(format!(
                "The directory contains more than {MAX_DIRECTORY_ENTRIES} entries."
            )));
        }
        count = index + 1;
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
    Ok((paths, count))
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

pub(super) fn digest(text: impl AsRef<[u8]>) -> String {
    Sha256::digest(text.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn parse_manifest(text: &str) -> Result<Manifest, String> {
    if text.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!(
            "Manifest exceeds the {MAX_MANIFEST_BYTES}-byte limit."
        ));
    }
    let version: Version = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let (id, commands, scripts, lua_digest, settings) = match version.api_version {
        DECLARATIVE_API_VERSION => {
            let definition: DeclarativeDefinition =
                serde_json::from_str(text).map_err(|error| error.to_string())?;
            debug_assert_eq!(definition.api_version, DECLARATIVE_API_VERSION);
            (
                definition.id,
                definition.commands,
                BTreeMap::new(),
                None,
                BTreeMap::new(),
            )
        }
        LUA_API_VERSION => {
            let definition: LuaDefinition =
                serde_json::from_str(text).map_err(|error| error.to_string())?;
            debug_assert_eq!(definition.api_version, LUA_API_VERSION);
            if definition.permissions.len() != LUA_CAPABILITIES.len()
                || !LUA_CAPABILITIES
                    .iter()
                    .all(|capability| definition.permissions.contains(capability))
            {
                return Err("Lua plugins must request selection.read and selection.replace exactly once each.".into());
            }
            (
                definition.id,
                definition.commands,
                definition.scripts,
                Some(digest(text)),
                definition.settings,
            )
        }
        version => {
            return Err(format!(
                "API version {version} is unsupported. This editor supports versions {DECLARATIVE_API_VERSION} and {LUA_API_VERSION}."
            ));
        }
    };
    if !valid_id(&id) {
        return Err("Invalid extension ID. Use 1–48 lowercase letters, digits, or hyphens, starting with a letter.".into());
    }
    if !(1..=MAX_COMMANDS).contains(&commands.len()) {
        return Err(format!("Declare between 1 and {MAX_COMMANDS} commands."));
    }
    if settings.len() > MAX_SETTINGS
        || settings.iter().any(|(key, setting)| {
            !valid_id(key)
                || setting.label.is_empty()
                || setting.label.len() > MAX_TITLE_BYTES
                || setting.label.trim() != setting.label
                || setting.label.chars().any(char::is_control)
                || !setting.default.valid()
        })
    {
        return Err("Invalid Lua settings. Declare up to 16 labeled scalar defaults.".into());
    }
    let mut command_ids = HashSet::new();
    let mut used_scripts = HashSet::new();
    for command in &commands {
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
        if command.on_save && !matches!(command.action, ActionDefinition::Lua { .. }) {
            return Err("Only Lua commands can support save hooks.".into());
        }
        match &command.action {
            ActionDefinition::InsertSnippet { body } => validate_body(&command.id, body, true)?,
            ActionDefinition::NewFile { body, .. } => validate_body(&command.id, body, false)?,
            ActionDefinition::Lua { source } => {
                if lua_digest.is_none() {
                    return Err("Lua commands require API version 2.".into());
                }
                if !scripts.contains_key(source) {
                    return Err(format!(
                        "Command {} references an undeclared Lua script.",
                        command.id
                    ));
                }
                used_scripts.insert(source.as_str());
            }
        }
    }
    if lua_digest.is_some() && (used_scripts.is_empty() || used_scripts.len() != scripts.len()) {
        return Err("An API version 2 manifest needs a Lua command, and every declared Lua script needs a command.".into());
    }
    Ok(Manifest {
        id,
        commands,
        scripts,
        lua_digest,
        settings: Arc::new(settings),
    })
}

fn validate_body(id: &str, body: &str, variables: bool) -> Result<(), String> {
    if body.contains('\0') {
        return Err(format!(
            "Command {id} has a snippet containing a NUL character."
        ));
    }
    if !valid_body(body, variables) {
        return Err(format!("Command {id} has an invalid snippet. Use 1–{MAX_BODY_BYTES} bytes, up to {MAX_BODY_LINES} lines, and flat fields numbered 0–{}.", u32::MAX));
    }
    Ok(())
}

pub(in crate::app) fn load(config: Option<&Path>) -> (Vec<Contribution>, Vec<String>) {
    let catalog = config.map(super::catalog::read).unwrap_or_default();
    (catalog.contributions, catalog.view.startup_errors())
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

    fn lua_definition() -> Value {
        json!({
            "api_version": 2,
            "id": "uppercase",
            "permissions": ["selection.read", "selection.replace"],
            "scripts": {"upper.lua": "return function(text) return string.upper(text) end"},
            "commands": [{
                "id": "transform",
                "title": "Uppercase selection",
                "action": {"kind": "lua", "source": "upper.lua"}
            }]
        })
    }

    #[test]
    fn lua_manifest_resolves_script_and_keeps_exact_digest() {
        let definition = lua_definition();
        let compact = definition.to_string();
        let pretty = serde_json::to_string_pretty(&definition).unwrap();
        let manifest = parse_manifest(&compact).unwrap();
        assert_eq!(manifest.id, "uppercase");
        assert_eq!(
            manifest.lua_digest.as_deref(),
            Some(digest(&compact).as_str())
        );
        assert_ne!(
            manifest.lua_digest,
            parse_manifest(&pretty).unwrap().lua_digest
        );
        assert!(matches!(
            &manifest.commands[0].action,
            ActionDefinition::Lua { source }
                if &*manifest.scripts[source] == definition["scripts"]["upper.lua"].as_str().unwrap()
        ));
    }

    #[test]
    fn lua_manifest_rejects_undeclared_and_duplicate_permissions() {
        for permissions in [
            json!([]),
            json!(["selection.read"]),
            json!(["selection.replace", "selection.replace"]),
            json!(["selection.read", "selection.replace", "selection.read"]),
            json!(["selection.read", "workspace.read"]),
        ] {
            let mut definition = lua_definition();
            definition["permissions"] = permissions;
            assert!(parse_manifest(&definition.to_string()).is_err());
        }
        let mut definition = lua_definition();
        definition.as_object_mut().unwrap().remove("permissions");
        assert!(parse_manifest(&definition.to_string()).is_err());
    }

    #[test]
    fn lua_manifest_rejects_unbounded_or_ambiguous_scripts() {
        for scripts in [
            json!({}),
            json!({"../upper.lua": "return 1"}),
            json!({"upper.lua": "   "}),
            json!({"upper.lua": "\u{1b}Lua"}),
            json!({"upper.lua": "null\0byte"}),
            json!({"upper.lua": "x".repeat(MAX_LUA_SOURCE_BYTES + 1)}),
            json!({"upper.lua": "return 1", "unused.lua": "return 2"}),
        ] {
            let mut definition = lua_definition();
            definition["scripts"] = scripts;
            assert!(parse_manifest(&definition.to_string()).is_err());
        }
        let duplicate = lua_definition().to_string().replace(
            "{\"upper.lua\":",
            "{\"upper.lua\":\"return 0\",\"upper.lua\":",
        );
        let Err(error) = parse_manifest(&duplicate) else {
            panic!("duplicate script name was accepted");
        };
        assert!(error.contains("Duplicate Lua script name."), "{error}");
    }

    #[test]
    fn v1_extensions_cannot_declare_executable_actions() {
        let mut definition = lua_definition();
        definition["api_version"] = json!(1);
        assert!(parse_manifest(&definition.to_string()).is_err());
        let fields = definition.as_object_mut().unwrap();
        fields.remove("permissions");
        fields.remove("scripts");
        let Err(error) = parse_manifest(&definition.to_string()) else {
            panic!("Lua was accepted in a v1 manifest");
        };
        assert!(
            error.contains("Lua commands require API version 2."),
            "{error}"
        );
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
                r#"{"api_version":3,"future_schema":true}"#.into(),
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
            "API version 3 is unsupported",
            "exceeds",
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
