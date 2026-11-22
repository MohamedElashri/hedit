use super::*;
use std::{sync::atomic::AtomicBool, time::Duration};
fn fixture() -> (tempfile::TempDir, State, PathBuf) {
    let outer = tempfile::tempdir().unwrap();
    let config = outer.path().join("config");
    std::fs::create_dir(&config).unwrap();
    std::fs::write(config.join("private.txt"), "never disclose configuration").unwrap();
    let project = outer.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("text.txt"), "café / 東京").unwrap();
    let state = State::open(&config, []).unwrap();
    (outer, state, project)
}
fn definition() -> Definition {
    serde_json::from_str(r#"{"capabilities":["workspace.discover","workspace.read","network.https"],"endpoints":{"translation":{"url":"https://example.com/translate","method":"POST","upload":1024,"response":1024}}}"#).unwrap()
}
fn approval(state: &State, project: &Path) -> Approval {
    Approval {
        command: "test.command".into(),
        token: Handle::fresh().unwrap(),
        scope: Some(
            SelectedScope::select(project, vec![Right::Discover, Right::Read], state).unwrap(),
        ),
        endpoints: vec!["translation".into()],
        tools: Default::default(),
    }
}
#[test]
fn resource_records_require_exact_source_tokens_declared_rights_and_explicit_endpoints() {
    let (_outer, state, project) = fixture();
    let definition = definition();
    let record = approval(&state, &project);
    assert!(state
        .resolve(&record.command, record.token, &definition)
        .is_err());
    state.approve(record.clone(), &definition).unwrap();
    assert!(state
        .resolve(&record.command, Handle::fresh().unwrap(), &definition)
        .is_err());
    assert!(state
        .resolve("other.command", record.token, &definition)
        .is_err());
    let resolved = state
        .resolve(&record.command, record.token, &definition)
        .unwrap();
    assert_eq!(resolved.endpoints.len(), 1);
    assert!(resolved.resources.is_some());
    let mut broader = record.clone();
    broader.scope.as_mut().unwrap().rights.push(Right::Edit);
    assert!(state.approve(broader, &definition).is_err());
    let mut unknown = record.clone();
    unknown.endpoints.push("unreviewed".into());
    assert!(state.approve(unknown, &definition).is_err());
    let mut narrower = record.clone();
    narrower.endpoints.clear();
    state.approve(narrower, &definition).unwrap();
    assert!(state
        .resolve(&record.command, record.token, &definition)
        .unwrap()
        .endpoints
        .is_empty());
    state.revoke(&record.command).unwrap();
    assert!(state
        .resolve(&record.command, record.token, &definition)
        .is_err());
}
#[test]
fn parent_grants_exclude_configuration_and_replaced_named_roots_need_new_review() {
    let (outer, state, project) = fixture();
    assert!(
        SelectedScope::select(&outer.path().join("config"), vec![Right::Read], &state).is_err()
    );
    let parent = SelectedScope::select(outer.path(), vec![Right::Read], &state)
        .unwrap()
        .resolve(&state)
        .unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = workspace::Budget::new(&cancel, Duration::from_secs(2), 4096);
    assert!(parent.read("config/private.txt", &mut budget).is_err());
    assert!(parent.read("project/text.txt", &mut budget).is_ok());
    let record = approval(&state, &project);
    state.approve(record.clone(), &definition()).unwrap();
    std::fs::rename(&project, outer.path().join("old-project")).unwrap();
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("text.txt"), "unreviewed directory").unwrap();
    assert!(state
        .resolve(&record.command, record.token, &definition())
        .is_err());
}
#[test]
fn corrupt_private_approval_data_fails_closed_and_explicit_reset_drops_old_authority() {
    let (_outer, state, project) = fixture();
    let record = approval(&state, &project);
    state.approve(record.clone(), &definition()).unwrap();
    state.directory.write(FILE, b"{broken").unwrap();
    assert!(state.approvals().is_err());
    assert!(state
        .resolve(&record.command, record.token, &definition())
        .is_err());
    state.reset().unwrap();
    let backups: Vec<_> = state
        .directory
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("approvals-backup-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        state.directory.read(backups[0].file_name()).unwrap(),
        b"{broken"
    );
    assert!(state.approvals().unwrap().is_empty());
    state.approve(record.clone(), &definition()).unwrap();
    state.prune(&BTreeSet::new()).unwrap();
    assert!(state.approvals().unwrap().is_empty());
    let reopened = State::open(&_outer.path().join("config"), []).unwrap();
    assert!(reopened.approvals().unwrap().is_empty());
}
#[test]
fn pruning_unchanged_approval_tokens_does_not_rewrite_private_state() {
    let (_outer, state, project) = fixture();
    let record = approval(&state, &project);
    state.approve(record.clone(), &definition()).unwrap();
    let before = workspace::identity::regular(&state.directory.open(FILE).unwrap()).unwrap();
    state.prune(&BTreeSet::from([record.token])).unwrap();
    let after = workspace::identity::regular(&state.directory.open(FILE).unwrap()).unwrap();
    assert_eq!(before, after);
}
#[test]
fn session_storage_exclusions_are_durable_even_when_inside_a_selected_workspace() {
    let (outer, _state, project) = fixture();
    let private = project.join("session-data");
    std::fs::create_dir(&private).unwrap();
    std::fs::write(private.join("buffer.txt"), "private recovered text").unwrap();
    State::open(&outer.path().join("config"), [private.clone()]).unwrap();
    let state = State::open(&outer.path().join("config"), []).unwrap();
    let scope = SelectedScope::select(&project, vec![Right::Read], &state).unwrap();
    let root = scope.resolve(&state).unwrap();
    let flag = std::sync::atomic::AtomicBool::new(false);
    let mut budget = workspace::Budget::new(&flag, Duration::from_secs(2), 4096);
    assert!(root.read("session-data/buffer.txt", &mut budget).is_err());
    std::fs::rename(&private, project.join("moved-session")).unwrap();
    assert!(root.read("moved-session/buffer.txt", &mut budget).is_err());
}

#[test]
fn duplicate_native_tool_approval_keys_are_never_reinterpreted_as_one_grant() {
    let token = Handle::fresh().unwrap();
    let image = Handle::fresh().unwrap();
    let json = format!(
        r#"{{"command":"test.command","token":"{token}","scope":null,"endpoints":[],"tools":{{"formatter":"{image}","formatter":"{image}"}}}}"#
    );
    assert!(serde_json::from_str::<Approval>(&json).is_err());
}
