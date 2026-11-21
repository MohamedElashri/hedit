//! Bounded, portable bundles for locally distributed extensions.
use super::manifest::{self, Manifest, MAX_COMMANDS, MAX_MANIFEST_BYTES};
use crate::plugin_runtime::MAX_SOURCE_BYTES;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{de, Deserialize, Serialize};
use serde_json::value::{self, RawValue};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
};
use zeroize::Zeroizing;

pub(super) const MAX_INSTALL_BYTES: u64 = 256 * 1024;
const BUNDLE_VERSION: u32 = 1;
const LEGACY_INSTALL_BYTES: u64 = 96 * 1024;
const MAX_FIXTURE_BYTES: u64 = 512 * 1024;
const MAX_FIXTURES: usize = 32;
const SIGNATURE_DOMAIN: &[u8] = b"hedit extension bundle v1\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::extensions) struct Release {
    pub(in crate::app::extensions) version: semver::Version,
    pub(in crate::app::extensions) publisher: Option<Publisher>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app::extensions) struct Publisher {
    pub(in crate::app::extensions) name: String,
    pub(in crate::app::extensions) fingerprint: String,
}

impl Release {
    pub(super) fn check_update(&self, previous: &Self) -> Result<(), String> {
        if let Some(installed) = &previous.publisher {
            if self.publisher.as_ref() != Some(installed) {
                return Err(
                    "The update must use the installed publisher's signing key and name.".into(),
                );
            }
        }
        if self.version <= previous.version {
            return Err(format!(
                "Update to a version newer than the installed release {}.",
                previous.version
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    bundle_version: u32,
    release: String,
    manifest: String,
    publisher: Option<SignedBy>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SignedBy {
    name: String,
    public_key: String,
    signature: String,
}

impl Bundle {
    fn parse(self) -> Result<(Manifest, Release), String> {
        if self.bundle_version != BUNDLE_VERSION {
            return Err(format!(
                "Unsupported extension bundle format {}.",
                self.bundle_version
            ));
        }
        let release = version(&self.release)?;
        let publisher = self
            .publisher
            .as_ref()
            .map(|signer| signer.verify(&self.release, &self.manifest))
            .transpose()?;
        let manifest = manifest::parse_manifest(&self.manifest)?;
        Ok((
            manifest,
            Release {
                version: release,
                publisher,
            },
        ))
    }
}

impl SignedBy {
    fn verify(&self, release: &str, manifest: &str) -> Result<Publisher, String> {
        validate_name(&self.name)?;
        let key = decode::<32>(&self.public_key, "publisher key")?;
        let verifying = VerifyingKey::from_bytes(&key).map_err(|_| "Invalid publisher key.")?;
        let signature = Signature::from_bytes(&decode::<64>(&self.signature, "bundle signature")?);
        verifying
            .verify_strict(&signed_message(release, &self.name, manifest), &signature)
            .map_err(|_| "The extension bundle signature does not match its contents.")?;
        Ok(Publisher {
            name: self.name.clone(),
            fingerprint: manifest::digest(key),
        })
    }
}

fn decode<const N: usize>(text: &str, kind: &str) -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    if text.bytes().any(|byte| byte.is_ascii_uppercase())
        || hex::decode_to_slice(text, &mut bytes).is_err()
    {
        return Err(format!("Invalid {kind}."));
    }
    Ok(bytes)
}

fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 80
        || name.trim() != name
        || name.chars().any(|ch| {
            ch.is_control() || matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err("Choose a publisher name of 1–80 bytes without surrounding whitespace or control characters.".into());
    }
    Ok(())
}

fn version(text: &str) -> Result<semver::Version, String> {
    if text.len() > 64 {
        return Err("The extension release version is too long.".into());
    }
    let version = semver::Version::parse(text)
        .map_err(|_| "Use a semantic extension release version such as 1.0.0.")?;
    if !version.build.is_empty() {
        return Err("Extension release versions omit build metadata.".into());
    }
    Ok(version)
}

fn signed_message(release: &str, name: &str, manifest: &str) -> Vec<u8> {
    let mut bytes = SIGNATURE_DOMAIN.to_vec();
    for field in [release, name, manifest] {
        bytes.extend_from_slice(&(field.len() as u32).to_le_bytes());
        bytes.extend_from_slice(field.as_bytes());
    }
    bytes
}

/// Resolve the installed format without interpreting any paths in a bundle.
pub(super) fn parse_installable(text: &str) -> Result<(Manifest, Option<Release>), String> {
    if text.len() as u64 > MAX_INSTALL_BYTES {
        return Err("The extension bundle exceeds the size limit.".into());
    }
    #[derive(Deserialize)]
    struct Format<'a> {
        #[serde(borrow)]
        bundle_version: Option<&'a RawValue>,
    }
    let feature: Format<'_> = serde_json::from_str(text).map_err(|error| {
        if text.len() as u64 > MAX_MANIFEST_BYTES {
            format!("Manifest exceeds the {MAX_MANIFEST_BYTES}-byte limit.")
        } else {
            error.to_string()
        }
    })?;
    if feature.bundle_version.is_none() {
        return manifest::parse_manifest(text).map(|manifest| (manifest, None));
    }
    let bundle: Bundle = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let (mut manifest, release) = bundle.parse()?;
    if !manifest.sdk && text.len() as u64 > LEGACY_INSTALL_BYTES {
        return Err("The extension bundle exceeds the legacy size limit.".into());
    }
    if manifest.lua_digest.is_some() {
        manifest.lua_digest = Some(manifest::digest(text));
    }
    Ok((manifest, Some(release)))
}

struct ManifestFields(BTreeMap<String, Box<RawValue>>);

impl<'de> Deserialize<'de> for ManifestFields {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Fields;
        impl<'de> de::Visitor<'de> for Fields {
            type Value = ManifestFields;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an extension manifest object")
            }

            fn visit_map<M: de::MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut fields = BTreeMap::new();
                while let Some((key, value)) = map.next_entry()? {
                    if fields.insert(key, value).is_some() {
                        return Err(de::Error::custom("Duplicate manifest field."));
                    }
                }
                Ok(ManifestFields(fields))
            }
        }
        deserializer.deserialize_map(Fields)
    }
}

/// An author may refer to ordinary sibling Lua files. The portable form uses
/// the same inline manifest parser and bounds as local manifests.
fn authoring_manifest(path: &Path, original: &str) -> Result<String, String> {
    if original.len() as u64 > manifest::MAX_SDK_MANIFEST_BYTES {
        return Err("Manifest exceeds the SDK byte limit.".into());
    }
    let ManifestFields(mut fields) =
        serde_json::from_str(original).map_err(|error| error.to_string())?;
    let sdk = fields
        .get("api_version")
        .is_some_and(|v| serde_json::from_str::<u32>(v.get()).ok() == Some(3));
    if !sdk && original.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!(
            "Manifest exceeds the {MAX_MANIFEST_BYTES}-byte limit."
        ));
    }
    let directory = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut changed = false;
    for (file_field, inline_field) in [("script_files", "scripts"), ("module_files", "modules")] {
        let Some(files) = fields.remove(file_field) else {
            continue;
        };
        if file_field == "module_files" && !sdk {
            return Err("Module files require API version 3.".into());
        }
        if fields.contains_key(inline_field) {
            return Err(format!(
                "Declare either {file_field} or inline {inline_field}."
            ));
        }
        let filenames: Vec<String> =
            serde_json::from_str(files.get()).map_err(|error| error.to_string())?;
        if !(1..=MAX_COMMANDS).contains(&filenames.len()) {
            return Err(format!(
                "Declare between 1 and {MAX_COMMANDS} source files."
            ));
        }
        let mut sources = BTreeMap::new();
        for filename in filenames {
            if !filename
                .strip_suffix(".lua")
                .is_some_and(manifest::valid_id)
            {
                return Err(format!(
                    "Use a simple sibling Lua filename for {filename:?}."
                ));
            }
            let bytes = read_sibling(directory, &filename, MAX_SOURCE_BYTES as u64)?;
            let source =
                String::from_utf8(bytes).map_err(|_| "Lua files must contain UTF-8 source.")?;
            if sources.insert(filename, source).is_some() {
                return Err("Duplicate Lua source filename.".into());
            }
        }
        fields.insert(
            inline_field.into(),
            value::to_raw_value(&sources).map_err(|e| e.to_string())?,
        );
        changed = true;
    }
    if let Some(files) = fields.remove("resource_files") {
        if !sdk || fields.contains_key("resources") {
            return Err(
                "Resource files require API version 3 and cannot accompany inline resources."
                    .into(),
            );
        }
        let filenames: BTreeMap<String, String> =
            serde_json::from_str(files.get()).map_err(|e| e.to_string())?;
        if filenames.len() > crate::plugin_runtime::sdk::MAX_RESOURCES {
            return Err("Too many text resources.".into());
        }
        let mut resources = BTreeMap::new();
        for (name, filename) in filenames {
            let simple = filename.rsplit_once('.').is_some_and(|(stem, extension)| {
                manifest::valid_id(stem) && ["txt", "md", "json", "csv"].contains(&extension)
            });
            if !manifest::valid_id(&name) || !simple {
                return Err("Use a resource ID and a simple sibling text filename.".into());
            }
            let bytes = read_sibling(directory, &filename, MAX_SOURCE_BYTES as u64)?;
            resources.insert(
                name,
                String::from_utf8(bytes).map_err(|_| "Resources must be UTF-8 text.")?,
            );
        }
        fields.insert(
            "resources".into(),
            value::to_raw_value(&resources).map_err(|e| e.to_string())?,
        );
        changed = true;
    }
    if !changed {
        manifest::parse_manifest(original)?;
        return Ok(original.into());
    }
    let manifest = serde_json::to_string(&fields).map_err(|e| e.to_string())?;
    manifest::parse_manifest(&manifest)?;
    Ok(manifest)
}

fn read_sibling(directory: &Path, filename: &str, maximum: u64) -> Result<Vec<u8>, String> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let file: File = {
        use rustix::fs::{open, openat, Mode, OFlags};
        let base = open(
            directory,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| format!("Could not open the authoring directory ({error})."))?;
        openat(
            base,
            filename,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| format!("Could not open {filename} ({error})."))?
        .into()
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let file = open_regular_leaf(&directory.join(filename))
        .map_err(|error| format!("Could not open {filename} ({error})."))?;
    checked_read(file, maximum).map_err(|error| format!("Could not read {filename} ({error})."))
}

fn open_regular_leaf(path: &Path) -> io::Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(io::Error::other("Choose a regular file."));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use rustix::fs::OFlags;
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    options.open(path)
}

fn checked_read(file: File, maximum: u64) -> io::Result<Vec<u8>> {
    let properties = file.metadata()?;
    if !properties.is_file() {
        return Err(io::Error::other("Choose a regular file."));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if properties.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other(
                "Choose a regular file without a reparse point.",
            ));
        }
    }
    hedit_core::file_io::read_bounded(file, maximum)
}

fn read_cli_file(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    open_regular_leaf(path)
        .and_then(|file| checked_read(file, maximum))
        .map_err(|error| format!("Could not read {} ({error}).", path.display()))
}

fn open_source(path: &Path) -> Result<String, String> {
    String::from_utf8(read_cli_file(path, MAX_INSTALL_BYTES)?)
        .map_err(|_| format!("{} must be UTF-8.", path.display()))
}

fn load_cli_manifest(path: &Path) -> Result<(Manifest, Option<Release>), String> {
    let text = open_source(path)?;
    let authored = serde_json::from_str::<ManifestFields>(&text)
        .map_err(|error| error.to_string())?
        .0
        .keys()
        .any(|key| ["script_files", "module_files", "resource_files"].contains(&key.as_str()));
    if authored {
        let manifest = authoring_manifest(path, &text)?;
        parse_installable(&manifest)
    } else {
        parse_installable(&text)
    }
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|error| error.to_string())?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|error| error.to_string())?;
    file.persist_noclobber(path)
        .map_err(|error| format!("Could not create {} ({}).", path.display(), error.error))?;
    Ok(())
}

fn keygen(path: &Path) -> Result<(), String> {
    let mut secret = Zeroizing::new([0; 32]);
    getrandom::fill(secret.as_mut()).map_err(|_| "Could not create a signing key.")?;
    let signer = SigningKey::from_bytes(&secret);
    write_new_file(path, secret.as_ref())?;
    println!(
        "Publisher key fingerprint {}",
        manifest::digest(signer.verifying_key().as_bytes())
    );
    Ok(())
}

fn pack(
    path: &Path,
    output: &Path,
    release: &str,
    sign: Option<(&str, &Path)>,
) -> Result<(), String> {
    version(release)?;
    let original = open_source(path)?;
    let manifest = authoring_manifest(path, &original)?;
    let sdk = manifest::parse_manifest(&manifest)?.sdk;
    let publisher = if let Some((name, keyfile)) = sign {
        validate_name(name)?;
        let secret = Zeroizing::new(read_cli_file(keyfile, 32)?);
        let signer = SigningKey::from_bytes(
            secret
                .as_slice()
                .try_into()
                .map_err(|_| "Use a 32-byte hedit publisher key.")?,
        );
        let signature = signer.sign(&signed_message(release, name, &manifest));
        Some(SignedBy {
            name: name.to_owned(),
            public_key: hex::encode(signer.verifying_key().as_bytes()),
            signature: hex::encode(signature.to_bytes()),
        })
    } else {
        None
    };
    let bundle = Bundle {
        bundle_version: BUNDLE_VERSION,
        release: release.to_owned(),
        manifest,
        publisher,
    };
    let bytes = serde_json::to_vec(&bundle).map_err(|error| error.to_string())?;
    if bytes.len() as u64
        > if sdk {
            MAX_INSTALL_BYTES
        } else {
            LEGACY_INSTALL_BYTES
        }
    {
        return Err("The extension bundle exceeds the size limit.".into());
    }
    write_new_file(output, &bytes)?;
    println!("Created {}", output.display());
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    command: String,
    #[serde(default = "plain_text")]
    language: String,
    input: String,
    expected: String,
    #[serde(default)]
    settings: BTreeMap<String, hedit::plugin_options::Value>,
    #[serde(default)]
    arguments: BTreeMap<String, hedit::plugin_options::Value>,
    #[serde(default)]
    selections: Option<Vec<crate::plugin_runtime::sdk::Selection>>,
    #[serde(default)]
    storage: Option<crate::plugin_runtime::sdk::Storage>,
    #[serde(default)]
    expected_storage: Option<crate::plugin_runtime::sdk::Storage>,
}

fn plain_text() -> String {
    hedit::syntax::Language::PlainText.id().into()
}

fn test(path: &Path, fixtures: &Path) -> Result<(), String> {
    test_traced(path, fixtures, false)
}
fn test_traced(path: &Path, fixtures: &Path, timings: bool) -> Result<(), String> {
    let (manifest, _) = load_cli_manifest(path)?;
    let bytes = read_cli_file(fixtures, MAX_FIXTURE_BYTES)?;
    let fixtures: Vec<Fixture> =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if !(1..=MAX_FIXTURES).contains(&fixtures.len()) {
        return Err(format!("Supply between 1 and {MAX_FIXTURES} fixtures."));
    }
    let mut failures = 0;
    for (index, case) in fixtures.into_iter().enumerate() {
        let trace = crate::plugin_runtime::telemetry::Recorder::default();
        let result: Result<(), String> = (|| {
            let source = manifest
                .lua_source(&case.command)
                .ok_or("Unknown Lua command.")?;
            if hedit::syntax::Language::from_id(&case.language).is_none() {
                return Err("Unknown fixture language.".into());
            }
            let settings = manifest.fixture_settings(&case.settings)?;
            let result = if manifest.sdk {
                let len = case.input.len();
                let crlf = case.input.contains("\r\n");
                let snapshot = super::options::sdk_snapshot(
                    &case.language,
                    case.input,
                    case.selections.unwrap_or_else(|| {
                        vec![crate::plugin_runtime::sdk::Selection {
                            anchor: 0,
                            head: len,
                        }]
                    }),
                    crlf,
                )
                .map_err(|e| e.to_string())?;
                let arguments = manifest.fixture_arguments(&case.command, &case.arguments)?;
                let storage_allowed = manifest.uses_storage(&case.command);
                if !storage_allowed && (case.storage.is_some() || case.expected_storage.is_some()) {
                    return Err("This command has no private storage permission.".into());
                }
                let initial_storage = storage_allowed.then(|| case.storage.unwrap_or_default());
                crate::plugin_runtime::run_sdk_fixture_traced(
                    crate::plugin_runtime::sdk::Input {
                        sdk_version: crate::plugin_runtime::sdk::VERSION,
                        modules: manifest
                            .modules
                            .iter()
                            .map(|(k, v)| (k.clone(), v.to_string()))
                            .collect(),
                        resources: manifest.resources.as_ref().clone(),
                        source: source.to_owned(),
                        snapshot,
                        settings,
                        arguments,
                        storage: initial_storage,
                    },
                    &trace,
                )
                .and_then(|prepared| {
                    if prepared.storage != case.expected_storage {
                        return Err(crate::plugin_runtime::Error::Worker.into());
                    }
                    Ok(prepared.text)
                })
                .map_err(|error| error.to_string())
            } else {
                if !case.arguments.is_empty()
                    || case.selections.is_some()
                    || case.storage.is_some()
                    || case.expected_storage.is_some()
                {
                    return Err("Structured fixture fields require API version 3.".into());
                }
                crate::plugin_runtime::run_fixture(crate::plugin_runtime::Input {
                    source: source.to_owned(),
                    selections: vec![case.input],
                    language: case.language,
                    settings,
                })
                .map_err(|error| error.to_string())
            }?;
            if result != case.expected {
                return Err("Output differs from expected text.".into());
            }
            Ok(())
        })();
        if timings {
            println!(
                "TIMING {}",
                serde_json::to_string(&trace.snapshot())
                    .map_err(|_| "Cannot encode numeric timings.")?
            );
        }
        match result {
            Ok(()) => println!("PASS {} {}", index + 1, case.command),
            Err(error) => {
                failures += 1;
                eprintln!("FAIL {} {:?} ({error})", index + 1, case.command);
            }
        }
    }
    if failures > 0 {
        return Err(format!("{failures} fixture(s) failed."));
    }
    Ok(())
}

const USAGE: &str = "Usage: hedit plugin check MANIFEST_OR_BUNDLE
       hedit plugin test MANIFEST_OR_BUNDLE FIXTURES.json [--timings]
       hedit plugin pack MANIFEST OUTPUT.hedit-plugin --version VERSION [--publisher NAME --sign-key FILE]
       hedit plugin keygen PRIVATE_KEY_FILE";

pub(crate) fn run_cli(args: &[OsString]) -> Result<(), String> {
    match args {
        [] => {
            println!("{USAGE}");
            Ok(())
        }
        [help] if help == "--help" || help == "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        [verb, path] if verb == "check" => {
            let (manifest, release) = load_cli_manifest(Path::new(path))?;
            println!(
                "{} ({} commands)",
                manifest.id,
                manifest.command_labels().len()
            );
            if let Some(release) = release {
                println!("Version {}", release.version);
                if let Some(publisher) = release.publisher {
                    println!("Signed by {} ({})", publisher.name, publisher.fingerprint);
                } else {
                    println!("Unsigned bundle");
                }
            }
            Ok(())
        }
        [verb, path, fixtures] if verb == "test" => test(Path::new(path), Path::new(fixtures)),
        [verb, path, fixtures, flag] if verb == "test" && flag == "--timings" => {
            test_traced(Path::new(path), Path::new(fixtures), true)
        }
        [verb, path] if verb == "keygen" => keygen(Path::new(path)),
        [verb, path, output, flag, release, options @ ..]
            if verb == "pack" && flag == "--version" =>
        {
            let release = release.to_str().ok_or("Use a UTF-8 release version.")?;
            let sign = match options {
                [] => None,
                [publisher_flag, name, sign_flag, key]
                    if publisher_flag == "--publisher" && sign_flag == "--sign-key" =>
                {
                    Some((
                        name.to_str().ok_or("Use a UTF-8 publisher name.")?,
                        Path::new(key),
                    ))
                }
                _ => return Err(USAGE.into()),
            };
            pack(Path::new(path), Path::new(output), release, sign)
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::extensions::catalog::{
        lua_authorization, set_lua_approved, ImportPreview, InstallChange,
    };
    use crate::app::extensions::manifest::ExtensionAction;

    const AUTHOR: &str = r#"{
        "api_version": 2,
        "id": "quote",
        "permissions": ["selection.read", "selection.replace"],
        "script_files": ["quote.lua"],
        "commands": [{"id":"transform", "title":"Quote", "action":{"kind":"lua", "source":"quote.lua"}}]
    }"#;

    #[test]
    fn inline_and_sibling_lua_sources_share_the_runtime_byte_limit() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("manifest.json");
        for extra in [0, 1] {
            let mut source = "return function(text) return text end".to_owned();
            source.push_str(&" ".repeat(MAX_SOURCE_BYTES + extra - source.len()));
            fs::write(directory.path().join("quote.lua"), &source).unwrap();
            let mut inline: serde_json::Value = serde_json::from_str(AUTHOR).unwrap();
            inline.as_object_mut().unwrap().remove("script_files");
            inline["scripts"] = serde_json::json!({"quote.lua": source});
            assert_eq!(
                manifest::parse_manifest(&inline.to_string()).is_ok(),
                extra == 0,
                "inline source with {extra} excess bytes"
            );
            assert_eq!(
                authoring_manifest(&manifest, AUTHOR).is_ok(),
                extra == 0,
                "sibling source with {extra} excess bytes"
            );
        }
    }

    #[test]
    fn pack_binds_release_publisher_and_every_script_to_the_signature() {
        let source = tempfile::tempdir().unwrap();
        let manifest = source.path().join("manifest.json");
        let keyfile = source.path().join("publisher.key");
        let bundle = source.path().join("quote.hedit-plugin");
        fs::write(&manifest, AUTHOR).unwrap();
        fs::write(
            source.path().join("quote.lua"),
            "return function(s) return s .. '!' end",
        )
        .unwrap();
        keygen(&keyfile).unwrap();
        pack(
            &manifest,
            &bundle,
            "1.2.0",
            Some(("Editor tools", &keyfile)),
        )
        .unwrap();
        let contents = fs::read_to_string(&bundle).unwrap();
        let (parsed, release) = parse_installable(&contents).unwrap();
        assert_eq!(
            parsed.lua_digest.as_deref(),
            Some(manifest::digest(&contents).as_str())
        );
        assert_eq!(
            parsed.lua_source("quote.transform"),
            Some("return function(s) return s .. '!' end")
        );
        assert_eq!(release.unwrap().publisher.unwrap().name, "Editor tools");

        for replace in ["release", "publisher", "script"] {
            let mut edited: Bundle = serde_json::from_str(&contents).unwrap();
            match replace {
                "release" => edited.release = "1.3.0".into(),
                "publisher" => edited.publisher.as_mut().unwrap().name = "Someone else".into(),
                "script" => edited.manifest = edited.manifest.replace('!', "?"),
                _ => unreachable!(),
            }
            let result = parse_installable(&serde_json::to_string(&edited).unwrap());
            assert!(result.err().unwrap().contains("signature"));
        }
        assert!(pack(&manifest, &bundle, "2.0.0", None).is_err());
        assert_eq!(fs::read_to_string(bundle).unwrap(), contents);
    }

    #[test]
    fn authoring_rejects_unsafe_paths_and_retains_duplicate_field_validation() {
        let directory = tempfile::tempdir().unwrap();
        let manifest = directory.path().join("manifest.json");
        assert!(
            authoring_manifest(&manifest, &AUTHOR.replace("quote.lua", "../outside.lua")).is_err()
        );
        fs::write(
            directory.path().join("quote.lua"),
            "return function(s) return s end",
        )
        .unwrap();
        assert!(authoring_manifest(
            &manifest,
            &AUTHOR.replace(
                "\"id\":\"transform\"",
                "\"id\":\"transform\", \"id\":\"second\""
            )
        )
        .is_err());
        assert!(authoring_manifest(
            &manifest,
            &AUTHOR.replace("\"quote.lua\"]", "\"quote.lua\", \"quote.lua\"]")
        )
        .is_err());
        assert!(authoring_manifest(
            &manifest,
            &AUTHOR.replace("\"script_files\"", "\"scripts\": {}, \"script_files\"")
        )
        .is_err());

        #[cfg(unix)]
        {
            let script = directory.path().join("quote.lua");
            let actual = directory.path().join("actual.lua");
            fs::rename(&script, &actual).unwrap();
            std::os::unix::fs::symlink(actual, script).unwrap();
            assert!(authoring_manifest(&manifest, AUTHOR).is_err());
        }
    }

    #[test]
    fn first_install_confirms_a_key_and_updates_cannot_change_it_or_reuse_grants() {
        let source = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let author = source.path().join("author.json");
        let script = source.path().join("quote.lua");
        let first_key = source.path().join("first.key");
        let second_key = source.path().join("second.key");
        let first = source.path().join("first.hedit-plugin");
        let later = source.path().join("later.hedit-plugin");
        let changed_key = source.path().join("changed-key.hedit-plugin");
        let unsigned = source.path().join("unsigned.hedit-plugin");
        fs::write(&author, AUTHOR).unwrap();
        fs::write(&script, "return function(s) return s end").unwrap();
        keygen(&first_key).unwrap();
        keygen(&second_key).unwrap();
        pack(&author, &first, "1.0.0", Some(("Tool author", &first_key))).unwrap();
        let mut review = ImportPreview::open(first.clone())
            .unwrap()
            .review(config.path())
            .unwrap();
        assert!(review.requires_publisher_confirmation());
        assert!(review.install(config.path()).is_err());
        review.publisher_confirmed = true;
        let installed = review.install(config.path()).unwrap();
        let snapshot = &installed.view.files[0].installed.as_ref().unwrap();
        assert!(installed.contributions.is_empty());
        let approved = set_lua_approved(
            config.path(),
            &snapshot.path,
            "quote",
            "quote.transform",
            &review.preview.digest,
            true,
        )
        .unwrap();
        let ExtensionAction::Lua(grant) = &approved.contributions[0].action else {
            panic!("approved Lua command");
        };
        assert!(lua_authorization(config.path(), grant).is_some());

        fs::write(&script, "return function(s) return '<' .. s .. '>' end").unwrap();
        pack(&author, &later, "2.0.0", Some(("Tool author", &first_key))).unwrap();
        pack(
            &author,
            &changed_key,
            "2.0.0",
            Some(("Tool author", &second_key)),
        )
        .unwrap();
        pack(&author, &unsigned, "2.0.0", None).unwrap();
        for candidate in [changed_key, unsigned] {
            assert!(ImportPreview::open(candidate)
                .unwrap()
                .review(config.path())
                .is_err());
        }
        let update = ImportPreview::open(later)
            .unwrap()
            .review(config.path())
            .unwrap();
        let InstallChange::Update {
            changed_scripts, ..
        } = &update.change
        else {
            panic!("expected update");
        };
        assert_eq!(changed_scripts, &["quote.lua"]);
        assert!(!update.requires_publisher_confirmation());
        assert!(update
            .install(config.path())
            .unwrap()
            .contributions
            .is_empty());
        assert!(lua_authorization(config.path(), grant).is_none());
        assert!(ImportPreview::open(first)
            .unwrap()
            .review(config.path())
            .is_err());
    }
}
