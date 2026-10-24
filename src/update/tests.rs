use super::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

const VERSION: &str = "9.4.0";
fn release_json(version: &str, size: u64, checksum_size: u64) -> String {
    let name = format!("hedit-{version}-linux-{}", std::env::consts::ARCH);
    json!({"tag_name": format!("v{version}"), "body": "A useful release.\nTwo changes.", "draft": false, "prerelease": false,
        "assets": [{"id":1,"name":format!("{name}.tar.gz"),"size":size}, {"id":2,"name":format!("{name}.tar.gz.sha256"),"size":checksum_size}]}).to_string()
}
fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
struct Fixture {
    _directory: tempfile::TempDir,
    root: PathBuf,
    package: PathBuf,
    archive: PathBuf,
    requests: PathBuf,
    client: Client,
}
impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("installation with spaces");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir_all(root.join("share/doc/hedit")).unwrap();
        fs::create_dir_all(root.join("share/applications")).unwrap();
        fs::create_dir_all(root.join("share/hedit/themes")).unwrap();
        executable(
            &root.join("bin/hedit"),
            "#!/bin/sh\nprintf 'old executable\\n'\n",
        );
        fs::write(root.join("share/doc/hedit/LICENSE"), "old license").unwrap();
        fs::write(root.join("share/applications/hedit.desktop"), "old desktop").unwrap();
        fs::write(root.join("share/hedit/themes/custom.json"), "keep custom").unwrap();
        fs::create_dir_all(root.join("share/hedit/session")).unwrap();
        fs::write(
            root.join("share/hedit/session/session.json"),
            "unsaved workspace",
        )
        .unwrap();
        let package = directory
            .path()
            .join(format!("hedit-{VERSION}-linux-{}", std::env::consts::ARCH));
        for name in ["assets", "themes", "examples"] {
            fs::create_dir_all(package.join(name)).unwrap();
        }
        for name in [
            "README.md",
            "USER_GUIDE.md",
            "DEVELOPER_GUIDE.md",
            "LICENSE",
            "install.sh",
            "hedit.desktop",
            "assets/hedit.svg",
            "themes/hedit-ink.json",
            "examples/example.cu",
        ] {
            fs::write(package.join(name), format!("new {name}")).unwrap();
        }
        executable(
            &package.join("hedit"),
            &format!("#!/bin/sh\nprintf 'hedit {VERSION}\\n'\n"),
        );
        let archive = directory.path().join(format!(
            "{}.tar.gz",
            package.file_name().unwrap().to_string_lossy()
        ));
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(directory.path())
            .arg(package.file_name().unwrap())
            .status()
            .unwrap();
        assert!(status.success());
        let hash: [u8; 32] = Sha256::digest(fs::read(&archive).unwrap()).into();
        let hash_hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        let checksum = format!(
            "{hash_hex}  {}\n",
            archive.file_name().unwrap().to_string_lossy()
        );
        fs::write(archive.with_extension("gz.sha256"), &checksum).unwrap();
        fs::write(
            directory.path().join("release.json"),
            release_json(
                VERSION,
                archive.metadata().unwrap().len(),
                checksum.len() as u64,
            ),
        )
        .unwrap();
        let requests = directory.path().join("requests");
        let tool = directory.path().join("fake-network");
        executable(
            &tool,
            &format!(
                r##"#!/usr/bin/env python3
import sys
from pathlib import Path
root=Path({})
with (root/'requests').open('a') as f: f.write(repr(sys.argv[1:])+'\n')
args=sys.argv[1:]
url=args[-1]
if url.endswith('/latest'): path=root/'release.json'
else: path=root/url.rsplit('/',1)[1]
sys.stdout.buffer.write(path.read_bytes())
"##,
                serde_json::to_string(directory.path().to_str().unwrap()).unwrap()
            ),
        );
        let client = Client {
            curl: tool,
            tar: "tar".into(),
        };
        Self {
            _directory: directory,
            root,
            package,
            archive,
            requests,
            client,
        }
    }
    fn installation(&self) -> Installation {
        Installation::for_executable(&self.root.join("bin/hedit")).unwrap()
    }
}

#[test]
fn version_ordering_is_numeric_stable_only_and_never_downgrades() {
    let release =
        Release::from_json(&release_json("0.10.0", 200, 100), std::env::consts::ARCH).unwrap();
    assert!(release.newer_than("0.9.9").unwrap());
    assert!(!release.newer_than("0.10.0").unwrap());
    assert!(!release.newer_than("1.0.0").unwrap());
    assert!(!release.newer_than("0.11.0-rc.1").unwrap());
    assert!(!release.newer_than("0.10.0+local").unwrap());
    assert!(Release::from_json(
        &release_json("0.11.0-rc.1", 200, 100),
        std::env::consts::ARCH
    )
    .is_err());
    let mut value: serde_json::Value =
        serde_json::from_str(&release_json(VERSION, 200, 100)).unwrap();
    value["prerelease"] = json!(true);
    assert!(Release::from_json(&value.to_string(), std::env::consts::ARCH).is_err());
    value["prerelease"] = json!(false);
    value["draft"] = json!(true);
    assert!(Release::from_json(&value.to_string(), std::env::consts::ARCH).is_err());
}
#[test]
fn malformed_missing_duplicate_or_oversized_assets_are_rejected() {
    for arch in ["x86_64", "aarch64"] {
        let json = release_json(VERSION, 200, 100);
        let result = Release::from_json(&json, arch);
        assert_eq!(result.is_ok(), arch == std::env::consts::ARCH);
    }
    for (field, value) in [("size", json!(ARCHIVE_LIMIT + 1)), ("id", json!(0))] {
        let mut json: serde_json::Value =
            serde_json::from_str(&release_json(VERSION, 200, 100)).unwrap();
        json["assets"][0][field] = value;
        assert!(Release::from_json(&json.to_string(), std::env::consts::ARCH).is_err());
    }
    let mut json: serde_json::Value =
        serde_json::from_str(&release_json(VERSION, 200, 100)).unwrap();
    let duplicate = json["assets"][0].clone();
    json["assets"].as_array_mut().unwrap().push(duplicate);
    assert!(Release::from_json(&json.to_string(), std::env::consts::ARCH).is_err());
    assert!(Release::from_json("{}", std::env::consts::ARCH).is_err());
}
#[test]
fn checks_do_not_install_or_run_on_construction() {
    let fixture = Fixture::new();
    assert!(!fixture.requests.exists());
    let release = fixture.client.check().unwrap();
    assert_eq!(release.version.to_string(), VERSION);
    assert_eq!(
        fs::read_to_string(fixture.root.join("bin/hedit")).unwrap(),
        "#!/bin/sh\nprintf 'old executable\\n'\n"
    );
    let requests = fs::read_to_string(&fixture.requests).unwrap();
    assert!(requests.contains("api.github.com/repos/MohamedElashri/hedit/releases/latest"));
}
#[test]
fn verified_install_preserves_custom_themes_and_running_inode() {
    {
        let fixture = Fixture::new();
        let installation = fixture.installation();
        let mut old = File::open(fixture.root.join("bin/hedit")).unwrap();
        let release = fixture.client.check().unwrap();
        fixture
            .client
            .install(&release, &installation, |_| ())
            .unwrap();
        assert!(fs::read_to_string(fixture.root.join("bin/hedit"))
            .unwrap()
            .contains(VERSION));
        let mut original = String::new();
        old.read_to_string(&mut original).unwrap();
        assert!(original.contains("old executable"));
        assert_eq!(
            fs::read_to_string(fixture.root.join("share/hedit/themes/custom.json")).unwrap(),
            "keep custom"
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("share/doc/hedit/LICENSE")).unwrap(),
            "new LICENSE"
        );
        assert_eq!(
            fs::read_to_string(fixture.root.join("share/applications/hedit.desktop")).unwrap(),
            "new hedit.desktop"
        );
        assert!(fixture
            .root
            .join("share/icons/hicolor/scalable/apps/hedit.svg")
            .is_file());
        assert_eq!(
            fs::read_to_string(fixture.root.join("share/hedit/session/session.json")).unwrap(),
            "unsaved workspace"
        );
        assert!(installation.unchanged().is_err());
    }
}
#[test]
fn corrupted_checksum_never_changes_the_installation() {
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
    assert!(fs::read_to_string(fixture.root.join("bin/hedit"))
        .unwrap()
        .contains("old executable"));
}
#[test]
fn install_failure_rolls_back_files_and_directories() {
    let fixture = Fixture::new();
    let installation = fixture.installation();
    let error = installation
        .replace_with_hook(&fixture.package, |index| {
            if index == 4 {
                Err("injected I/O failure".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(error.contains("previous installation restored"));
    assert!(fs::read_to_string(fixture.root.join("bin/hedit"))
        .unwrap()
        .contains("old executable"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("share/doc/hedit/LICENSE")).unwrap(),
        "old license"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("share/applications/hedit.desktop")).unwrap(),
        "old desktop"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("share/hedit/themes/custom.json")).unwrap(),
        "keep custom"
    );
    assert!(!fixture
        .root
        .join("share/hedit/themes/hedit-ink.json")
        .exists());
}
#[test]
fn portable_installation_updates_and_source_layout_is_refused() {
    let fixture = Fixture::new();
    let portable = Installation::for_executable(&fixture.package.join("hedit")).unwrap();
    let source = fixture.root.join("target/release/hedit");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    executable(&source, "#!/bin/sh\nexit 0\n");
    assert!(Installation::for_executable(&source).is_err());
    // A second extracted package with a different executable exercises directory merges.
    let other = fixture._directory.path().join("new-package");
    copy_package(&fixture.package, &other);
    executable(
        &other.join("hedit"),
        "#!/bin/sh\nprintf 'updated portable\\n'\n",
    );
    portable.replace(&other).unwrap();
    assert!(fs::read_to_string(fixture.package.join("hedit"))
        .unwrap()
        .contains("updated portable"));
}
fn copy_package(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_dir() {
            copy_package(&entry.path(), &destination.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
}
#[test]
fn install_lock_and_external_changes_are_detected() {
    let fixture = Fixture::new();
    let installation = fixture.installation();
    let lock = installation.lock().unwrap();
    assert!(installation.lock().is_err());
    drop(lock);
    // A child process forked by another test thread shares the lock until it
    // starts its program, so allow the release a moment to land.
    let started = std::time::Instant::now();
    while installation.lock().is_err() {
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    fs::write(fixture.root.join("bin/hedit"), "externally replaced").unwrap();
    assert!(installation.lock().is_err());
}
#[test]
fn symlink_destinations_and_resource_links_are_refused() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let installation = fixture.installation();
    let elsewhere = fixture._directory.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, fixture.root.join("share/icons")).unwrap();
    assert!(installation
        .replace(&fixture.package)
        .unwrap_err()
        .contains("symlink"));
    assert!(fs::read_dir(&elsewhere).unwrap().next().is_none());
    assert!(fs::read_to_string(fixture.root.join("bin/hedit"))
        .unwrap()
        .contains("old executable"));
}
#[test]
fn archive_paths_links_duplicates_and_size_bombs_are_rejected() {
    for line in [
        "-rw-r--r-- 0/0 10 2026-01-01 00:00 ../escape",
        "-rw-r--r-- 0/0 10 2026-01-01 00:00 /release/hedit",
        "lrwxrwxrwx 0/0 0 2026-01-01 00:00 release/link -> /tmp",
        "hrw-r--r-- 0/0 0 2026-01-01 00:00 release/link link to elsewhere",
        "-rw-r--r-- 0/0 10 2026-01-01 00:00 release/a\\nb",
        "-rw-r--r-- 0/0 2147483648 2026-01-01 00:00 release/bomb",
        "-rw-r--r-- 0/0 1 2026-01-01 00:00 release/a\n-rw-r--r-- 0/0 1 2026-01-01 00:00 release/a",
    ] {
        assert!(validate_listing(line, "release").is_err(), "{line}");
    }
    assert!(validate_listing("drwxr-xr-x 0/0 0 2026-01-01 00:00 release/\n-rwxr-xr-x 0/0 10 2026-01-01 00:00 release/hedit", "release").is_ok());
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("/tmp", fixture.package.join("assets/link")).unwrap();
    let archive = fixture._directory.path().join("bad.tar.gz");
    assert!(Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(fixture.package.parent().unwrap())
        .arg(fixture.package.file_name().unwrap())
        .status()
        .unwrap()
        .success());
    let destination = fixture._directory.path().join("unpack-bad");
    fs::create_dir(&destination).unwrap();
    assert!(unpack(
        Path::new("tar"),
        &archive,
        &destination,
        fixture.package.file_name().unwrap().to_str().unwrap()
    )
    .is_err());
    assert!(fs::read_dir(destination).unwrap().next().is_none());
}
#[test]
fn incompatible_or_wrong_version_binary_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let binary = directory.path().join("hedit");
    executable(&binary, "#!/bin/sh\nexit 1\n");
    assert!(install::verify_binary(&binary, VERSION).is_err());
    executable(&binary, "#!/bin/sh\nprintf 'hedit 0.0.1\\n'\n");
    assert!(install::verify_binary(&binary, VERSION).is_err());
}
#[test]
fn subprocess_timeout_output_limit_and_network_failure_are_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let tool = directory.path().join("tool");
    executable(
        &tool,
        "#!/usr/bin/env python3\nimport time\ntime.sleep(10)\n",
    );
    let output = tempfile::tempfile().unwrap();
    assert!(run_tool(
        &mut Command::new(&tool),
        &output,
        1024,
        Duration::from_millis(80)
    )
    .unwrap_err()
    .contains("timed out"));
    executable(&tool, "#!/usr/bin/env python3\nprint('x'*200000)\n");
    assert!(run_tool(
        &mut Command::new(&tool),
        &output,
        1024,
        Duration::from_secs(5)
    )
    .is_err());
    executable(&tool, "#!/bin/sh\nexit 22\n");
    let client = Client {
        curl: tool,
        tar: "tar".into(),
    };
    assert!(client
        .check()
        .unwrap_err()
        .contains("Could not check GitHub"));
}

#[test]
fn production_packager_output_is_accepted_by_updater() {
    let work = tempfile::tempdir().unwrap();
    let binary = work.path().join("fixture-hedit");
    executable(
        &binary,
        &format!(
            "#!/bin/sh\nprintf 'hedit {}\\n'\n",
            env!("CARGO_PKG_VERSION")
        ),
    );
    let status = Command::new("python3")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/package-linux.py"))
        .arg("--binary")
        .arg(&binary)
        .arg("--out-dir")
        .arg(work.path())
        .status()
        .unwrap();
    assert!(status.success());
    let package = format!(
        "hedit-{}-linux-{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::ARCH
    );
    let archive = work.path().join(format!("{package}.tar.gz"));
    verify_checksum(
        &archive,
        &fs::read_to_string(archive.with_extension("gz.sha256")).unwrap(),
        &format!("{package}.tar.gz"),
    )
    .unwrap();
    let output = work.path().join("unpacked");
    fs::create_dir(&output).unwrap();
    unpack(Path::new("tar"), &archive, &output, &package).unwrap();
    install::verify_binary(
        &output.join(package).join("hedit"),
        env!("CARGO_PKG_VERSION"),
    )
    .unwrap();
}

#[test]
fn failed_asset_download_keeps_old_binary_and_session() {
    let fixture = Fixture::new();
    let release = fixture.client.check().unwrap();
    fs::remove_file(fixture.archive.with_extension("gz.sha256")).unwrap();
    assert!(fixture
        .client
        .install(&release, &fixture.installation(), |_| ())
        .is_err());
    assert!(fs::read_to_string(fixture.root.join("bin/hedit"))
        .unwrap()
        .contains("old executable"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("share/hedit/session/session.json")).unwrap(),
        "unsaved workspace"
    );
}
#[test]
fn external_binary_replacement_during_staging_is_not_overwritten() {
    let fixture = Fixture::new();
    let installation = fixture.installation();
    let error = installation
        .replace_with_hook(&fixture.package, |index| {
            if index == 4 {
                fs::write(fixture.root.join("bin/hedit"), "external replacement").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert!(error.contains("installation changed"));
    assert_eq!(
        fs::read_to_string(fixture.root.join("bin/hedit")).unwrap(),
        "external replacement"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.join("share/applications/hedit.desktop")).unwrap(),
        "old desktop"
    );
}
