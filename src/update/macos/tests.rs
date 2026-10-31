use super::*;
use serde_json::json;
use std::io::Write;

const VERSION: &str = "9.4.0";

fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn app(path: &Path, version: &str) {
    fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(path.join("Contents/Resources")).unwrap();
    let info = plist::Dictionary::from_iter(
        [
            ("CFBundleIdentifier", "hedit.hedit"),
            ("CFBundleExecutable", "hedit"),
            ("CFBundlePackageType", "APPL"),
            ("CFBundleVersion", version),
            ("CFBundleShortVersionString", version),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), plist::Value::String(value.to_owned()))),
    );
    plist::Value::Dictionary(info)
        .to_file_xml(path.join("Contents/Info.plist"))
        .unwrap();
    let source = path.parent().unwrap().join("fixture.c");
    fs::write(
        &source,
        format!("#include <stdio.h>\nint main(void) {{ puts(\"hedit {version}\"); return 0; }}\n"),
    )
    .unwrap();
    run(Command::new("/usr/bin/clang")
        .arg(&source)
        .arg("-o")
        .arg(path.join("Contents/MacOS/hedit")));
    fs::write(path.join("Contents/Resources/resource"), version).unwrap();
    sign(path);
}

fn sign(path: &Path) {
    run(Command::new("/usr/bin/codesign")
        .args(["--force", "--sign", "-"])
        .arg(path));
}

struct Fixture {
    directory: tempfile::TempDir,
    installed: PathBuf,
    package: PathBuf,
    archive: PathBuf,
    client: Client,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let applications = directory.path().join("Applications with spaces");
        fs::create_dir(&applications).unwrap();
        // Users may rename the app; preserve its installed name.
        let installed = applications.join("My hedit.app");
        app(&installed, "0.1.0");
        let package = directory.path().join("hedit.app");
        app(&package, VERSION);
        let archive = directory
            .path()
            .join(format!("hedit-{VERSION}-macos-universal.zip"));
        run(Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--sequesterRsrc", "--keepParent"])
            .arg(&package)
            .arg(&archive));
        let digest = Sha256::digest(fs::read(&archive).unwrap());
        let digest: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        let checksum = format!(
            "{digest}  {}\n",
            archive.file_name().unwrap().to_str().unwrap()
        );
        fs::write(archive.with_extension("zip.sha256"), &checksum).unwrap();
        let name = archive.file_name().unwrap().to_str().unwrap();
        let release = json!({"tag_name":format!("v{VERSION}"),"draft":false,"prerelease":false,
            "assets":[{"id":1,"name":name,"size":archive.metadata().unwrap().len()},
                {"id":2,"name":format!("{name}.sha256"),"size":checksum.len()}]});
        fs::write(directory.path().join("release.json"), release.to_string()).unwrap();
        let network = directory.path().join("network");
        // No network requests: emulate curl's stdout using the local release assets.
        fs::write(
            &network,
            format!(
                r#"#!/usr/bin/env python3
import sys
from pathlib import Path
root=Path({})
url=sys.argv[-1]
path=root/('release.json' if url.endswith('/latest') else url.rsplit('/',1)[1])
sys.stdout.buffer.write(path.read_bytes())
"#,
                serde_json::to_string(directory.path().to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        fs::set_permissions(&network, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            directory,
            installed,
            package,
            archive,
            client: Client { curl: network },
        }
    }

    fn installation(&self) -> Installation {
        Installation::for_executable(&self.installed.join("Contents/MacOS/hedit")).unwrap()
    }

    fn resource(&self) -> String {
        fs::read_to_string(self.installed.join("Contents/Resources/resource")).unwrap()
    }
}

#[test]
fn universal_assets_support_both_architectures_and_reject_bad_metadata() {
    let fixture = Fixture::new();
    let json = fs::read_to_string(fixture.directory.path().join("release.json")).unwrap();
    for arch in ["aarch64", "x86_64"] {
        let release = Release::from_json(&json, arch).unwrap();
        assert_eq!(release.package, format!("hedit-{VERSION}-macos-universal"));
        assert!(release.newer_than("0.1.0").unwrap());
        assert!(!release.newer_than(VERSION).unwrap());
        assert!(!release.newer_than("10.0.0").unwrap());
    }
    assert!(Release::from_json(&json, "riscv64").is_err());
    let mut json: serde_json::Value = serde_json::from_str(&json).unwrap();
    for key in ["draft", "prerelease"] {
        json[key] = json!(true);
        assert!(Release::from_json(&json.to_string(), "aarch64").is_err());
        json[key] = json!(false);
    }
    json["assets"][0]["name"] = json!("hedit-9.4.0-linux-aarch64.tar.gz");
    assert!(Release::from_json(&json.to_string(), "aarch64").is_err());
}

#[test]
fn verified_zip_install_keeps_running_inode_and_replaces_entire_signed_bundle() {
    let fixture = Fixture::new();
    let installation = fixture.installation();
    let mut original = File::open(fixture.installed.join("Contents/MacOS/hedit")).unwrap();
    let original_bytes = fs::read(fixture.installed.join("Contents/MacOS/hedit")).unwrap();
    fs::write(
        fixture.installed.join("Contents/Resources/obsolete"),
        "remove with old bundle",
    )
    .unwrap();
    let session = fixture.directory.path().join("session.json");
    fs::write(&session, "unsaved workspace").unwrap();
    let release = fixture.client.check().unwrap();
    assert_eq!(fixture.resource(), "0.1.0");
    fixture
        .client
        .install(&release, &installation, |_| ())
        .unwrap();
    assert_eq!(fixture.resource(), VERSION);
    assert!(!fixture
        .installed
        .join("Contents/Resources/obsolete")
        .exists());
    verify_bundle(&fixture.installed, VERSION).unwrap();
    let mut bytes = Vec::new();
    original.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, original_bytes);
    assert_eq!(fs::read_to_string(session).unwrap(), "unsaved workspace");
    assert!(installation.unchanged().is_err());
    assert_eq!(
        installation.location(),
        fs::canonicalize(fixture.installed.parent().unwrap()).unwrap()
    );
}

#[test]
fn checksum_failure_does_not_replace_the_app() {
    let fixture = Fixture::new();
    let release = fixture.client.check().unwrap();
    let mut bytes = fs::read(&fixture.archive).unwrap();
    bytes[20] ^= 1;
    fs::write(&fixture.archive, bytes).unwrap();
    let error = fixture
        .client
        .install(&release, &fixture.installation(), |_| ())
        .unwrap_err();
    assert!(error.contains("Checksum mismatch"), "{error}");
    assert_eq!(fixture.resource(), "0.1.0");
}

#[test]
fn signature_and_bundle_version_are_verified_before_execution() {
    let fixture = Fixture::new();
    assert!(verify_bundle(&fixture.package, "1.0.0")
        .unwrap_err()
        .contains("version"));
    fs::write(
        fixture.package.join("Contents/Resources/resource"),
        "tampered",
    )
    .unwrap();
    assert!(verify_bundle(&fixture.package, VERSION)
        .unwrap_err()
        .contains("signature"));
    assert!(fixture.installation().replace(&fixture.package).is_err());
    assert_eq!(fixture.resource(), "0.1.0");
}

#[test]
fn failed_bundle_swap_restores_previous_app() {
    let fixture = Fixture::new();
    let error = fixture
        .installation()
        .replace_with_hook(&fixture.package, |index| {
            if index == 1 {
                Err("injected I/O failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(error.contains("previous installation restored"), "{error}");
    assert_eq!(fixture.resource(), "0.1.0");
    verify_bundle(&fixture.installed, "0.1.0").unwrap();
}

#[test]
fn source_builds_external_changes_and_concurrent_installations_are_refused() {
    let fixture = Fixture::new();
    let source = fixture.directory.path().join("hedit");
    fs::write(&source, "source build").unwrap();
    assert!(Installation::for_executable(&source).is_err());
    let installation = fixture.installation();
    let lock = installation.lock().unwrap();
    assert!(installation.lock().is_err());
    drop(lock);
    fs::write(
        fixture.installed.join("Contents/MacOS/hedit"),
        "external replacement",
    )
    .unwrap();
    assert!(installation.lock().is_err());
    assert!(installation.replace(&fixture.package).is_err());
}

#[test]
fn translocated_and_read_only_apps_use_the_manual_fallback() {
    let fixture = Fixture::new();
    let translocated = fixture
        .directory
        .path()
        .join("AppTranslocation/token/d/hedit.app");
    fs::create_dir_all(translocated.parent().unwrap()).unwrap();
    fs::rename(&fixture.installed, &translocated).unwrap();
    let error =
        Installation::for_executable(&translocated.join("Contents/MacOS/hedit")).unwrap_err();
    assert!(error.contains("Applications"), "{error}");
    fs::rename(translocated, &fixture.installed).unwrap();
    if rustix::process::geteuid().as_raw() != 0 {
        let parent = fixture.installed.parent().unwrap();
        fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
        let result = Installation::for_executable(&fixture.installed.join("Contents/MacOS/hedit"));
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.unwrap_err().contains("not writable"));
    }
}

#[test]
fn resource_links_and_changes_during_staging_cannot_overwrite_the_app() {
    let fixture = Fixture::new();
    let link = fixture.package.join("Contents/Resources/link");
    std::os::unix::fs::symlink("/tmp", &link).unwrap();
    assert!(fixture
        .installation()
        .replace(&fixture.package)
        .unwrap_err()
        .contains("symlink"));
    fs::remove_file(link).unwrap();
    let error = fixture
        .installation()
        .replace_with_hook(&fixture.package, |index| {
            if index == 0 {
                fs::write(
                    fixture.installed.join("Contents/MacOS/hedit"),
                    "external replacement",
                )
                .unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert!(error.contains("installation changed"), "{error}");
    assert_eq!(fixture.resource(), "0.1.0");
    assert_eq!(
        fs::read_to_string(fixture.installed.join("Contents/MacOS/hedit")).unwrap(),
        "external replacement"
    );
}

#[test]
fn unsafe_zip_entries_are_refused_before_extraction() {
    for (name, link) in [
        ("../escape", false),
        ("/hedit.app/escape", false),
        ("hedit.app/Contents/../escape", false),
        ("hedit.app/Contents/link", true),
        ("hedit.app/Contents/escape\\name", false),
        ("another.app/file", false),
    ] {
        let work = tempfile::tempdir().unwrap();
        let archive = work.path().join("bad.zip");
        let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("hedit.app/valid", options).unwrap();
        zip.write_all(b"valid").unwrap();
        if link {
            zip.add_symlink(name, "/tmp", options).unwrap();
        } else {
            zip.start_file(name, options).unwrap();
            zip.write_all(b"bad").unwrap();
        }
        zip.finish().unwrap();
        let destination = work.path().join("unpacked");
        fs::create_dir(&destination).unwrap();
        assert!(unpack(&archive, &destination).is_err(), "{name}");
        assert!(fs::read_dir(destination).unwrap().next().is_none());
    }
}

#[test]
fn zip_case_aliases_and_size_bombs_are_refused() {
    let work = tempfile::tempdir().unwrap();
    let archive = work.path().join("bad.zip");
    let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
    let options = zip::write::SimpleFileOptions::default();
    for name in ["hedit.app/Contents/file", "hedit.app/Contents/FILE"] {
        zip.start_file(name, options).unwrap();
        zip.write_all(b"fixture").unwrap();
    }
    zip.finish().unwrap();
    let destination = work.path().join("unpacked");
    fs::create_dir(&destination).unwrap();
    assert!(unpack(&archive, &destination).is_err());
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    // Patch the central directory's uncompressed size without allocating a bomb.
    let mut bytes = fs::read(&archive).unwrap();
    let offset = bytes
        .windows(4)
        .position(|part| part == b"PK\x01\x02")
        .unwrap();
    bytes[offset + 24..offset + 28].copy_from_slice(&(2u32 * 1024 * 1024 * 1024).to_le_bytes());
    fs::write(&archive, bytes).unwrap();
    assert!(unpack(&archive, &destination)
        .unwrap_err()
        .contains("1 GiB"));
}

#[test]
fn production_macos_package_is_accepted_by_updater() {
    let work = tempfile::tempdir().unwrap();
    let binary = work.path().join("universal");
    let source = work.path().join("fixture.c");
    fs::write(
        &source,
        format!(
            "#include <stdio.h>\nint main(void) {{ puts(\"hedit {}\"); return 0; }}\n",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    run(Command::new("/usr/bin/clang")
        .args(["-arch", "arm64", "-arch", "x86_64"])
        .arg(source)
        .arg("-o")
        .arg(&binary));
    let arm = work.path().join("arm64");
    let intel = work.path().join("intel");
    for (arch, target) in [("arm64", &arm), ("x86_64", &intel)] {
        run(Command::new("/usr/bin/lipo")
            .arg(&binary)
            .args(["-thin", arch, "-output"])
            .arg(target));
    }
    run(Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package-macos.py"))
        .arg("--arm64")
        .arg(arm)
        .arg("--intel")
        .arg(intel)
        .arg("--out-dir")
        .arg(work.path()));
    let name = format!("hedit-{}-macos-universal.zip", env!("CARGO_PKG_VERSION"));
    let archive = work.path().join(&name);
    verify_checksum(
        &archive,
        &fs::read_to_string(archive.with_extension("zip.sha256")).unwrap(),
        &name,
    )
    .unwrap();
    let destination = work.path().join("unpacked");
    fs::create_dir(&destination).unwrap();
    unpack(&archive, &destination).unwrap();
    let app = destination.join("hedit.app");
    verify_bundle(&app, env!("CARGO_PKG_VERSION")).unwrap();
    install::verify_binary(&app.join("Contents/MacOS/hedit"), env!("CARGO_PKG_VERSION")).unwrap();
}
