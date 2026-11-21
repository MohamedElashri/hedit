use super::*;
use crate::tool::source::tests::fixture;
fn definition() -> ToolDefinition {
    serde_json::from_value(serde_json::json!({"protocol":"filter","workspace":false,"arguments":[{"literal":"--json"}]})).unwrap()
}
fn binding() -> Binding {
    Binding::new(
        "sample.format",
        Handle::fresh().unwrap(),
        "formatter",
        &definition(),
    )
    .unwrap()
}
fn open(root: &Path) -> Store {
    let parent = Dir::open_ambient_dir(root, cap_std::ambient_authority()).unwrap();
    let directory = match native::private_directory(&parent, "images") {
        Ok(dir) => dir,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            parent.open_dir_nofollow("images").unwrap()
        }
        Err(error) => panic!("{error}"),
    };
    Store::open(
        directory,
        &root.join("images"),
        &Protected::new([]).unwrap(),
    )
    .unwrap()
}
#[test]
fn image_grants_bind_command_source_profile_recipe_and_pinned_content() {
    let directory = tempfile::tempdir().unwrap();
    let (source, bytes) = fixture(directory.path());
    let store = open(directory.path());
    let owner = binding();
    let cancel = AtomicBool::new(false);
    let id = store
        .import(Source::select(&source).unwrap(), owner.clone(), &cancel)
        .unwrap();
    let mut image = store.resolve(id, &owner, &cancel).unwrap().into_file();
    let mut copied = Vec::new();
    image.read_to_end(&mut copied).unwrap();
    assert_eq!(copied, bytes);
    for field in 0..4 {
        let mut other = owner.clone();
        match field {
            0 => other.command = "sample.other".into(),
            1 => other.source = Handle::fresh().unwrap(),
            2 => other.profile = "other".into(),
            _ => other.recipe = "0".repeat(64),
        }
        assert!(store.resolve(id, &other, &cancel).is_err());
        assert!(store.remove(id, &other).is_err());
    }
    drop(image);
    drop(store);
    let store = open(directory.path());
    assert_eq!(
        store.resolve(id, &owner, &cancel).unwrap().bytes,
        bytes.len() as u64
    );
    store.remove(id, &owner).unwrap();
    assert!(store.records().unwrap().is_empty());
    assert!(store.resolve(id, &owner, &cancel).is_err());
    assert_eq!(std::fs::read(source).unwrap(), bytes);
}
#[test]
fn interrupted_and_oversized_reservations_survive_reopen_without_quota_reset() {
    let directory = tempfile::tempdir().unwrap();
    let store = open(directory.path());
    let owner = binding();
    // Model an interrupted import after admission but before image data.
    for _ in 0..3 {
        let id = Handle::fresh().unwrap();
        let child = native::private_directory(&store.directory, &id.to_string()).unwrap();
        write(
            &child,
            "reservation.json",
            &Record {
                version: 1,
                id,
                binding: owner.clone(),
                reserved: super::super::format::MAX_IMAGE_BYTES + OVERHEAD,
            },
        )
        .unwrap();
    }
    drop(store);
    let store = open(directory.path());
    assert_eq!(store.records().unwrap().len(), 3);
    let (path, _) = fixture(directory.path());
    let source = Source::select(&path).unwrap();
    // Small images still fit; aggregate reservation is tested at its boundary.
    let id = Handle::fresh().unwrap();
    let child = native::private_directory(&store.directory, &id.to_string()).unwrap();
    write(
        &child,
        "reservation.json",
        &Record {
            version: 1,
            id,
            binding: owner.clone(),
            reserved: MAX_STORE_BYTES - 3 * (super::super::format::MAX_IMAGE_BYTES + OVERHEAD),
        },
    )
    .unwrap();
    drop(child);
    assert_eq!(
        store
            .import(source, owner.clone(), &AtomicBool::new(false))
            .unwrap_err()
            .kind(),
        io::ErrorKind::OutOfMemory
    );
    for record in store.records().unwrap() {
        assert!(store
            .resolve(record.id, &owner, &AtomicBool::new(false))
            .is_err());
        store.remove(record.id, &owner).unwrap();
    }
    assert!(store.records().unwrap().is_empty());
}
#[test]
fn corrupt_receipts_and_foreign_files_are_preserved_and_never_authorize_images() {
    let directory = tempfile::tempdir().unwrap();
    let (source, _) = fixture(directory.path());
    let store = open(directory.path());
    let owner = binding();
    let cancel = AtomicBool::new(false);
    let id = store
        .import(Source::select(&source).unwrap(), owner.clone(), &cancel)
        .unwrap();
    let path = directory
        .path()
        .join("images")
        .join(id.to_string())
        .join("receipt.json");
    std::fs::write(&path, br#"{"version":1,"version":1}"#).unwrap();
    assert!(store.resolve(id, &owner, &cancel).is_err());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        br#"{"version":1,"version":1}"#
    );
    let foreign = directory.path().join("images/foreign.txt");
    std::fs::write(&foreign, "preserve me").unwrap();
    assert!(store.records().is_err());
    assert!(store.remove(id, &owner).is_err());
    assert_eq!(std::fs::read_to_string(foreign).unwrap(), "preserve me");
}
#[test]
fn import_retirement_does_not_exhaust_permanent_workspace_exclusions() {
    let outer = tempfile::tempdir().unwrap();
    let parent = Dir::open_ambient_dir(outer.path(), cap_std::ambient_authority()).unwrap();
    let state = native::private_directory(&parent, "state").unwrap();
    let protected = Protected::persistent(&state, []).unwrap();
    let images = native::private_directory(&state, "images").unwrap();
    let store = Store::open(images, &outer.path().join("state/images"), &protected).unwrap();
    let size = state.metadata("plugin-protected-identities").unwrap().len();
    let (source, _) = fixture(outer.path());
    let cancel = AtomicBool::new(false);
    let owner = binding();
    for _ in 0..70 {
        let id = store
            .import(Source::select(&source).unwrap(), owner.clone(), &cancel)
            .unwrap();
        store.remove(id, &owner).unwrap();
    }
    assert!(store.records().unwrap().is_empty());
    assert_eq!(
        state.metadata("plugin-protected-identities").unwrap().len(),
        size
    );
}
#[test]
fn crash_cleanup_reclaims_only_authenticated_retired_images() {
    let directory = tempfile::tempdir().unwrap();
    let (source, _) = fixture(directory.path());
    let store = open(directory.path());
    let owner = binding();
    let id = store
        .import(
            Source::select(&source).unwrap(),
            owner,
            &AtomicBool::new(false),
        )
        .unwrap();
    let removed = format!(".removed-{id}");
    store
        .directory
        .rename(id.to_string(), &store.directory, &removed)
        .unwrap();
    let child = store.directory.open_dir_nofollow(&removed).unwrap();
    child.remove_file("receipt.json").unwrap();
    drop(child);
    drop(store);
    let store = open(directory.path());
    assert!(store.records().unwrap().is_empty());
    assert!(!directory.path().join("images").join(removed).exists());
}
