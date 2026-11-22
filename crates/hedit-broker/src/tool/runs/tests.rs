use super::*;
use crate::{
    declaration::ToolDefinition,
    tool::{Binding, Source, Store},
};
fn fixture() -> (tempfile::TempDir, Arc<Runs>, Store) {
    let root = tempfile::tempdir().unwrap();
    let parent = Dir::open_ambient_dir(root.path(), cap_std::ambient_authority()).unwrap();
    let protected = Protected::new([]).unwrap();
    let runs = Runs::open(
        native::private_directory(&parent, "runs").unwrap(),
        &root.path().join("runs"),
        &protected,
    )
    .unwrap();
    let images = Store::open(
        native::private_directory(&parent, "images").unwrap(),
        &root.path().join("images"),
        &protected,
    )
    .unwrap();
    (root, Arc::new(runs), images)
}
fn image(store: &Store, path: &Path) -> Image {
    let definition: ToolDefinition = serde_json::from_value(
        serde_json::json!({"protocol":"filter", "workspace":false, "arguments":[]}),
    )
    .unwrap();
    let binding = Binding::new(
        "sample.format",
        Handle::fresh().unwrap(),
        "formatter",
        &definition,
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    let id = store
        .import(Source::select(path).unwrap(), binding.clone(), &cancel)
        .unwrap();
    store.resolve(id, &binding, &cancel).unwrap()
}
#[test]
fn bundles_have_distinct_images_and_live_leases_prevent_reaping() {
    let (root, runs, images) = fixture();
    let (source, _) = super::super::source::tests::fixture(root.path());
    let cancel = AtomicBool::new(false);
    let run = runs
        .prepare(
            image(&images, &source),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .unwrap();
    assert_ne!(run.launch.executable, source);
    assert!(run.launch.snapshot.is_dir());
    assert_eq!(runs.records().unwrap().len(), 1);
    runs.reap().unwrap();
    assert!(run.launch.executable.is_file());
    let path = run.launch.executable.clone();
    drop(run);
    assert!(!path.exists());
    assert!(runs.records().unwrap().is_empty());
    assert!(source.is_file());
}
#[test]
fn orphaned_and_mid_retirement_bundles_recover_without_losing_admission() {
    let (root, runs, images) = fixture();
    let (source, _) = super::super::source::tests::fixture(root.path());
    let cancel = AtomicBool::new(false);
    let mut run = runs
        .prepare(
            image(&images, &source),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .unwrap();
    run.lease.take();
    let path = run.launch.executable.clone();
    runs.reap().unwrap();
    assert!(!path.exists());
    drop(run);
    let mut run = runs
        .prepare(
            image(&images, &source),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .unwrap();
    let id = run.record.id;
    run.lease.take();
    runs.directory
        .rename(id.to_string(), &runs.directory, format!(".removed-{id}"))
        .unwrap();
    let child = runs
        .directory
        .open_dir_nofollow(format!(".removed-{id}"))
        .unwrap();
    child.remove_file("image.exe").unwrap();
    drop(child);
    drop(run);
    runs.reap().unwrap();
    assert!(runs.records().unwrap().is_empty());
}
#[test]
fn run_metadata_rejects_duplicate_unsafe_and_conflicting_paths() {
    let root = Identity { volume: 1, file: 2 };
    let id = Handle::fresh().unwrap();
    let prefix = format!(
        r#"{{"version":1,"id":"{id}","root":{{"volume":1,"file":"00000000000000000000000000000002"}},"image":256,"files":"#
    );
    for suffix in [
        r#"{"same":1,"same":2}}"#,
        r#"{"../escape":1}}"#,
        r#"{".secret":1}}"#,
    ] {
        assert!(serde_json::from_str::<Record>(&format!("{prefix}{suffix}")).is_err());
    }
    let record = Record {
        version: 1,
        id,
        root,
        image: 256,
        files: BTreeMap::from([("parent".into(), 1), ("parent/child".into(), 1)]),
    };
    assert!(record.reserved().is_err());
}

#[test]
fn active_runs_reserve_capacity_and_unknown_payloads_are_preserved() {
    let (root, runs, images) = fixture();
    let (source, _) = super::super::source::tests::fixture(root.path());
    let cancel = AtomicBool::new(false);
    let mut active: Vec<_> = (0..MAX_RUNS)
        .map(|_| {
            runs.prepare(
                image(&images, &source),
                Projection::empty(),
                vec![],
                &cancel,
            )
            .unwrap()
        })
        .collect();
    let error = runs
        .prepare(
            image(&images, &source),
            Projection::empty(),
            vec![],
            &cancel,
        )
        .err()
        .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(runs.records().unwrap().len(), MAX_RUNS);
    let mut run = active.pop().unwrap();
    let foreign = runs
        .path
        .join(run.record.id.to_string())
        .join("foreign.txt");
    std::fs::write(&foreign, "preserve this").unwrap();
    run.lease.take();
    assert!(runs.reap().is_err());
    assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "preserve this");
    drop(run);
    std::fs::remove_file(foreign).unwrap();
    runs.reap().unwrap();
    drop(active);
    assert!(runs.records().unwrap().is_empty());
}

#[test]
fn workspace_copies_are_immutable_and_independent_of_live_project_files() {
    use crate::{
        resources::Resources,
        workspace::{Budget, Policy, Right, Root},
    };
    let (root, runs, images) = fixture();
    let (source, _) = super::super::source::tests::fixture(root.path());
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(project.join("nested")).unwrap();
    std::fs::write(project.join("nested/document.txt"), "café / 東京").unwrap();
    let resource = Resources::new(
        Root::open_selected(
            &project,
            Policy::new([Right::Read, Right::Discover]).unwrap(),
            Protected::new([]).unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    let projection = Projection::workspace(
        &resource,
        &mut Budget::new(
            &cancel,
            Duration::from_secs(10),
            projection::MAX_PROJECT_BYTES,
        ),
    )
    .unwrap();
    let run = runs
        .prepare(image(&images, &source), projection, vec![], &cancel)
        .unwrap();
    std::fs::write(project.join("nested/document.txt"), "later live revision").unwrap();
    assert_eq!(
        std::fs::read_to_string(run.launch.snapshot.join("nested/document.txt")).unwrap(),
        "café / 東京"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::metadata(run.launch.snapshot.join("nested/document.txt"))
                .unwrap()
                .mode()
                & 0o777,
            0o400
        );
    }
    drop(run);
    assert_eq!(
        std::fs::read_to_string(project.join("nested/document.txt")).unwrap(),
        "later live revision"
    );
    assert!(runs.records().unwrap().is_empty());
}
