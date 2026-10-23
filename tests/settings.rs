use hedit::settings::{strip_comments, FormatSettings, Settings};

#[test]
fn settings_accept_comments_trailing_commas_and_report_unknown_keys() {
    let text = r#"
        // Sublime-style comments
        {
            "font_size": 16, /* inline */
            "rulers": [80, 120,],
            "word_wrap": true,
            "url": "https://example.test/*not a comment*/ // still text",
            "tab_size": 2,
            "syntax_specific": { "python": { "translate_tabs_to_spaces": true, }, },
        }
    "#;
    let (settings, warnings) = Settings::parse(text).unwrap();
    assert_eq!(settings.font_size, 16.0);
    assert_eq!(settings.rulers, [80, 120]);
    assert!(settings.word_wrap);
    assert_eq!(settings.format.tab_size, Some(2));
    assert_eq!(
        settings.syntax_specific["python"].translate_tabs_to_spaces,
        Some(true)
    );
    assert_eq!(warnings, ["Unknown setting “url” is ignored."]);
    // Missing keys keep their defaults.
    assert!(settings.line_numbers);
    assert_eq!(settings.cuda_gdb_path, "cuda-gdb");
}

#[test]
fn comment_stripping_leaves_strings_alone() {
    let text = r#"{"a": "x // y", "b": "/* z */", "c": "q\"//", } // end"#;
    let value: serde_json::Value = serde_json::from_str(&strip_comments(text)).unwrap();
    assert_eq!(value["a"], "x // y");
    assert_eq!(value["b"], "/* z */");
    assert_eq!(value["c"], "q\"//");
}

#[test]
fn invalid_settings_are_rejected_with_the_key_name() {
    for (text, message) in [
        (r#"{"font_size": 99}"#, "font_size"),
        (r#"{"rulers": [0]}"#, "rulers"),
        (r#"{"wrap_width": 5000}"#, "wrap_width"),
        (r#"{"tab_size": 12}"#, "Indent size"),
        (r#"{"line_ending": "cr"}"#, "Line ending"),
        (
            r#"{"syntax_specific": {"rust": {"encoding": "nope"}}}"#,
            "syntax_specific.rust",
        ),
        (r#"{"word_wrap": "yes"}"#, "Invalid settings"),
        ("[]", "JSON object"),
    ] {
        let error = Settings::parse(text).unwrap_err();
        assert!(error.contains(message), "{text}: {error}");
    }
}

#[test]
fn settings_round_trip_through_a_commented_file() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("nested/settings.json");
    let mut settings = Settings {
        font_size: 18.0,
        rulers: vec![72],
        ..Settings::default()
    };
    settings.format.trim_trailing_white_space_on_save = Some(true);
    settings.save(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("// hedit settings"));
    // Unset formatting keys are omitted rather than written as null.
    assert!(!text.contains("null"));
    assert!(!text.contains("tab_size"));
    assert_eq!(Settings::load(&path).unwrap(), (settings, vec![]));
}

#[test]
fn formatting_settings_map_to_editorconfig_rules() {
    let format = FormatSettings {
        tab_size: Some(2),
        translate_tabs_to_spaces: Some(false),
        trim_trailing_white_space_on_save: Some(true),
        ensure_newline_at_eof_on_save: Some(true),
        line_ending: Some("crlf".into()),
        encoding: Some("utf-8".into()),
    };
    let rules = format.to_formatting();
    assert_eq!(rules.indent_style.as_deref(), Some("tabs"));
    assert_eq!(rules.indent_size, Some(2));
    assert_eq!(rules.end_of_line.as_deref(), Some("crlf"));
    assert_eq!(FormatSettings::from_formatting(&rules), format);
    let mut settings = Settings::default();
    settings
        .syntax_specific
        .insert("python".into(), format.clone());
    let defaults = settings.defaults();
    let mut restored = Settings::default();
    restored.set_defaults(&defaults);
    assert_eq!(restored.syntax_specific, settings.syntax_specific);
}

#[test]
fn fonts_are_found_by_family_name_preferring_the_regular_style() {
    let directory = tempfile::tempdir().unwrap();
    let nested = directory.path().join("truetype/jetbrains");
    std::fs::create_dir_all(&nested).unwrap();
    for name in [
        "JetBrainsMono-Bold.ttf",
        "JetBrainsMono-Italic.ttf",
        "JetBrainsMono-Regular.ttf",
        "JetBrainsMonoNL-Regular.ttf",
        "JetBrainsMono-Regular.txt",
    ] {
        std::fs::write(nested.join(name), "font").unwrap();
    }
    let dirs = [directory.path().to_owned()];
    let found = hedit::fonts::find("JetBrains Mono", &dirs).unwrap();
    assert_eq!(found.file_name().unwrap(), "JetBrainsMono-Regular.ttf");
    assert!(hedit::fonts::find("No Such Font", &dirs)
        .unwrap_err()
        .contains("No installed font"));
    // A file path is used as is.
    let path = nested.join("JetBrainsMono-Bold.ttf");
    assert_eq!(
        hedit::fonts::find(path.to_str().unwrap(), &dirs).unwrap(),
        path
    );
    assert!(Settings::parse(r#"{"caret_width": 9}"#)
        .unwrap_err()
        .contains("caret_width"));
    assert!(Settings::parse(r#"{"line_padding": -1}"#)
        .unwrap_err()
        .contains("line_padding"));
}

#[test]
#[ignore = "depends on fonts installed on this machine"]
fn installed_font_lookup() {
    for name in ["JetBrains Mono", "DejaVu Sans Mono", "Noto Sans Mono"] {
        println!(
            "{name}: {:?}",
            hedit::fonts::find(name, &hedit::fonts::font_dirs())
        );
    }
}
