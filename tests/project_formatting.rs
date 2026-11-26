use hedit::projects;
use std::{fs, path::Path};

fn configured(root: &Path, relative: &str, defaults: projects::Defaults) -> projects::Formatting {
    projects::formatting(&root.join(relative), "plain", &defaults, None).unwrap()
}

#[test]
fn editorconfig_invalid_child_value_keeps_valid_parent_rule() {
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("src");
    fs::create_dir(&src).unwrap();
    fs::write(
        d.path().join(".editorconfig"),
        "root = true\n[*]\nindent_style = space\nindent_size = 3\n",
    )
    .unwrap();
    fs::write(src.join(".editorconfig"), "[*]\nindent_size = nonsense\n").unwrap();
    assert_eq!(
        configured(d.path(), "src/notes.txt", projects::Defaults::default()).indent_size,
        Some(3)
    );
}

#[test]
fn editorconfig_can_match_escaped_literal_brackets() {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join(".editorconfig"),
        "root = true\n[file\\[1\\].txt]\nindent_size = 7\n",
    )
    .unwrap();
    assert_eq!(
        configured(d.path(), "file[1].txt", projects::Defaults::default()).indent_size,
        Some(7)
    );
}

#[test]
fn jsonc_comments_do_not_join_separate_number_tokens() {
    let text = r#"{"wrap_width":1/* comment */2}"#;
    assert!(
        hedit::settings::Settings::parse(text).is_err(),
        "invalid separate tokens became the number 12"
    );
}

#[test]
fn saveable_project_text_remains_loadable_after_json_escaping() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("work.hedit-project");
    let (workspace, _) = hedit::workspace::Workspace::restore(d.path().join("session")).unwrap();
    let view = workspace.open[0].view.clone();
    let mut second = view.clone();
    second.id += 1;
    let text = "\\".repeat(16 * 1024 * 1024);
    let state = projects::State {
        tabs: vec![
            projects::Tab {
                path: None,
                text: Some(text.clone()),
                view: view.clone(),
                encoding: "UTF-8".into(),
                bom: None,
            },
            projects::Tab {
                path: None,
                text: Some(text),
                view: second,
                encoding: "UTF-8".into(),
                bom: None,
            },
        ],
        ..Default::default()
    };
    projects::atomic_json(&path, &projects::Project::default()).unwrap();
    let companion = fs::File::create(path.with_extension("hedit-workspace")).unwrap();
    serde_json::to_writer_pretty(std::io::BufWriter::new(companion), &state).unwrap();
    assert_eq!(projects::load(&path).unwrap().1.tabs.len(), 2);
}
