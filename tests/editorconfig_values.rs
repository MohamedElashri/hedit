use hedit::projects::{self, Defaults};

#[test]
fn tab_indentation_uses_the_cascaded_tab_width() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[*]\nindent_style = tab\nindent_size = tab\ntab_width = 2\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/.editorconfig"), "[*]\ntab_width = 8\n").unwrap();
    let mut defaults = Defaults::default();
    defaults.formatting.indent_size = Some(4);
    let format =
        projects::formatting(&dir.path().join("src/a.rs"), "rust", &defaults, None).unwrap();
    assert_eq!(format.indent_size, Some(8));
    assert_eq!(format.indent_style.as_deref(), Some("tabs"));
}

#[test]
fn unsupported_editorconfig_values_do_not_block_saving_or_override_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let mut defaults = Defaults::default();
    defaults.formatting.indent_size = Some(4);
    defaults.formatting.end_of_line = Some("lf".into());
    defaults.formatting.trim_trailing_whitespace = Some(true);
    for invalid in [
        "indent_size = 99",
        "indent_style = sideways",
        "end_of_line = cr",
        "charset = unknown-encoding",
        "trim_trailing_whitespace = perhaps",
    ] {
        std::fs::write(
            dir.path().join(".editorconfig"),
            format!("root = true\n[*]\n{invalid}\n"),
        )
        .unwrap();
        let format = projects::formatting(&dir.path().join("a.rs"), "rust", &defaults, None)
            .expect("unsupported EditorConfig values must be ignored");
        assert_eq!(format.indent_size, Some(4), "{invalid}");
        assert_eq!(format.end_of_line.as_deref(), Some("lf"), "{invalid}");
        assert_eq!(format.trim_trailing_whitespace, Some(true), "{invalid}");
    }
}
