use eframe::egui::{self, Color32};
use hedit::{
    syntax,
    theme::{self as catalog, Rgb, Role, Theme},
};

#[test]
fn builtin_themes_are_readable_and_drive_native_controls() {
    let themes = catalog::builtins();
    assert!(themes.len() >= 16);
    let mut names = std::collections::HashSet::new();
    for theme in themes {
        assert!(
            names.insert(theme.name.to_lowercase()),
            "duplicate {}",
            theme.name
        );
        theme.validate().unwrap();
        assert_eq!(catalog::builtin(&theme.name).as_ref(), Some(theme));
        assert!(
            Theme::contrast(theme.text, theme.background) >= 4.5,
            "{} text",
            theme.name
        );
        assert!(
            Theme::contrast(theme.selection_text, theme.selection) >= 4.5,
            "{} selection",
            theme.name
        );
        assert!(
            Theme::contrast(theme.muted, theme.panel) >= 3.0,
            "{} secondary text",
            theme.name
        );
        // The dark flag follows the editor background.
        assert_eq!(
            theme.dark,
            Theme::contrast(theme.background, Rgb::new(0, 0, 0))
                < Theme::contrast(theme.background, Rgb::new(255, 255, 255)),
            "{} dark flag",
            theme.name
        );
        // Hovered and open menu entries stand out from menus and the menu bar.
        let hover = theme.hover();
        for (name, fill) in [("surface", theme.surface), ("panel", theme.panel)] {
            assert!(
                Theme::contrast(hover, fill) >= 1.2,
                "{} hover on {name}",
                theme.name
            );
        }
        assert!(
            Theme::contrast(theme.text, hover) >= 3.0,
            "{} hover text",
            theme.name
        );
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let visuals = ctx.style().visuals.clone();
        assert_eq!(visuals.widgets.hovered.weak_bg_fill, hover.color());
        assert_eq!(visuals.widgets.open.weak_bg_fill, hover.color());
        assert_eq!(visuals.dark_mode, theme.dark);
        assert_eq!(visuals.panel_fill, theme.panel.color());
        assert_eq!(visuals.extreme_bg_color, theme.background.color());
        assert_eq!(visuals.text_cursor.stroke.color, theme.accent.color());
        assert_eq!(visuals.selection.stroke.color, theme.selection_text.color());
    }
    // Dark themes come first, and both defaults are present.
    assert!(themes.windows(2).all(|pair| pair[0].dark || !pair[1].dark));
    assert_eq!(Theme::default().name, catalog::INK);
    assert!(!Theme::paper().dark);
}

#[test]
fn applied_theme_survives_a_system_appearance_change() {
    let ctx = egui::Context::default();
    let theme = catalog::builtin("Dracula").unwrap();
    theme.apply(&ctx);
    let input = egui::RawInput {
        system_theme: Some(egui::Theme::Light),
        ..Default::default()
    };
    let _ = ctx.run(input, |_| {});
    assert_eq!(ctx.style().visuals.panel_fill, theme.panel.color());
    assert!(ctx.style().visuals.dark_mode);
}

#[test]
fn version_one_themes_derive_new_syntax_roles() {
    let theme = Theme::from_json(
        r##"{"version":1,"name":"Old","text":"#010203","keyword":"#040506","number":"#070809"}"##,
    )
    .unwrap();
    assert_eq!(theme.version, catalog::VERSION);
    for (role, expected) in [
        (Role::Function, Rgb::new(1, 2, 3)),
        (Role::Type, Rgb::new(1, 2, 3)),
        (Role::Operator, Rgb::new(1, 2, 3)),
        (Role::Constant, Rgb::new(7, 8, 9)),
        (Role::Property, Rgb::new(4, 5, 6)),
        (Role::Preprocessor, Rgb::new(4, 5, 6)),
    ] {
        assert_eq!(theme.color(role), expected, "{role:?}");
    }
    // Explicit colors win over derived ones.
    let theme = Theme::from_json(r##"{"text":"#010203","function":"#AABBCC"}"##).unwrap();
    assert_eq!(theme.function, Rgb::new(170, 187, 204));
    assert_eq!(theme.type_name, Rgb::new(1, 2, 3));
}

#[test]
fn partial_themes_can_start_from_a_builtin_base() {
    let dracula = catalog::builtin("Dracula").unwrap();
    let theme =
        Theme::from_json(r##"{"name":"Night","base":"dracula","accent":"#112233"}"##).unwrap();
    assert_eq!(theme.name, "Night");
    assert_eq!(theme.accent, Rgb::new(0x11, 0x22, 0x33));
    assert_eq!(theme.background, dracula.background);
    assert_eq!(theme.function, dracula.function);
    let unnamed = Theme::from_json(r#"{"base":"GitHub Light"}"#).unwrap();
    assert_eq!(unnamed, catalog::builtin("GitHub Light").unwrap());
    assert!(Theme::from_json(r#"{"base":"Missing"}"#)
        .unwrap_err()
        .contains("unknown base"));
    // Saved files are complete, so they never depend on a base.
    assert!(!theme.to_json().unwrap().contains("base"));
}

#[test]
fn theme_library_saves_lists_and_names_themes() {
    let directory = tempfile::tempdir().unwrap();
    let library = directory.path().join("themes");
    assert_eq!(catalog::load_library(&library), (vec![], vec![]));
    let mut theme = catalog::builtin("Nord").unwrap();
    assert!(catalog::save_to_library(&library, &theme, None)
        .unwrap_err()
        .contains("built-in"));
    theme.name = "My Nord / Night".into();
    let first = catalog::save_to_library(&library, &theme, None).unwrap();
    assert_eq!(first.file_name().unwrap(), "my-nord-night.json");
    let second = catalog::save_to_library(&library, &theme, None).unwrap();
    assert_eq!(second.file_name().unwrap(), "my-nord-night-2.json");
    theme.accent = Rgb::new(1, 2, 3);
    assert_eq!(
        catalog::save_to_library(&library, &theme, Some(&first)).unwrap(),
        first
    );
    std::fs::write(library.join("broken.json"), "{").unwrap();
    std::fs::write(library.join("notes.txt"), "ignored").unwrap();
    let (themes, errors) = catalog::load_library(&library);
    assert_eq!(themes.len(), 2);
    assert!(themes.iter().any(|t| t.path == first && t.theme == theme));
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("broken.json"));
    assert_eq!(
        catalog::unique_name("Dracula", ["dracula", "Dracula 2"].into_iter()),
        "Dracula 3"
    );
    assert_eq!(
        catalog::unique_name(" Fresh ", ["Other"].into_iter()),
        "Fresh"
    );
}

#[test]
fn new_syntax_roles_color_code_tokens() {
    use syntax::{highlight_language, Language};
    let mut theme = Theme::default();
    for (i, role) in Role::ALL.into_iter().enumerate() {
        *theme.color_mut(role) = Rgb::new(i as u8 * 9, 40, 80);
    }
    let color_of = |job: &egui::text::LayoutJob, needle: &str| {
        let at = job.text.find(needle).unwrap();
        job.sections
            .iter()
            .find(|s| s.byte_range.contains(&at))
            .unwrap()
            .format
            .color
    };
    let cuda = highlight_language(
        "#include <cstdio>\n#define LIMIT 4\nstruct Point p = make(LIMIT) + 1;",
        14.0,
        800.0,
        Language::Cuda,
        &theme,
    );
    assert_eq!(color_of(&cuda, "#include"), theme.preprocessor.color());
    assert_eq!(color_of(&cuda, "<cstdio>"), theme.string.color());
    assert_eq!(color_of(&cuda, "#define"), theme.preprocessor.color());
    assert_eq!(color_of(&cuda, "Point"), theme.type_name.color());
    assert_eq!(color_of(&cuda, "make"), theme.function.color());
    assert_eq!(color_of(&cuda, "LIMIT)"), theme.constant.color());
    assert_eq!(color_of(&cuda, "+"), theme.operator.color());
    let rust = highlight_language(
        "#[derive(Debug)]\nfn main() { println!(\"x\"); }",
        14.0,
        800.0,
        Language::Rust,
        &theme,
    );
    assert_eq!(color_of(&rust, "#[derive"), theme.preprocessor.color());
    assert_eq!(color_of(&rust, "println"), theme.function.color());
    let python = highlight_language(
        "@cache\ndef f(): pass",
        14.0,
        800.0,
        Language::Python,
        &theme,
    );
    assert_eq!(color_of(&python, "@cache"), theme.preprocessor.color());
    let json = highlight_language(r#"{"key": "value"}"#, 14.0, 800.0, Language::Json, &theme);
    assert_eq!(color_of(&json, "\"key\""), theme.property.color());
    assert_eq!(color_of(&json, "\"value\""), theme.string.color());
    let html = highlight_language(r#"<a href="x">"#, 14.0, 800.0, Language::Html, &theme);
    assert_eq!(color_of(&html, "a "), theme.keyword.color());
    assert_eq!(color_of(&html, "href"), theme.property.color());
    // Data formats keep identifiers as plain text.
    let toml = highlight_language("x = Value", 14.0, 800.0, Language::Toml, &theme);
    assert_eq!(color_of(&toml, "Value"), theme.text.color());
}

#[test]
fn all_custom_colors_round_trip_and_drive_native_controls() {
    let mut theme = Theme {
        name: "My violet workspace".into(),
        ..Theme::default()
    };
    for (i, role) in Role::ALL.into_iter().enumerate() {
        *theme.color_mut(role) = Rgb::new((20 + i * 7) as u8, 91, 204);
    }
    let json = theme.to_json().unwrap();
    assert!(json.contains("#145BCC"));
    assert_eq!(Theme::from_json(&json).unwrap(), theme);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("custom.json");
    theme.export(&path).unwrap();
    assert_eq!(Theme::load(&path).unwrap(), theme);
    // Replacement is atomic and leaves a readable, complete theme.
    theme.name = "Revised".into();
    theme.export(&path).unwrap();
    assert_eq!(Theme::load(&path).unwrap(), theme);
}

#[test]
fn invalid_themes_report_errors_and_partial_themes_use_defaults() {
    for json in [
        r##"{"background":"#zzzzzz"}"##,
        r#"{"version":99}"#,
        r#"{"accent":[2,3,4]}"#,
        r##"{"typo":"#112233"}"##,
        r#"{"name":"  "}"#,
    ] {
        assert!(Theme::from_json(json).is_err(), "{json}");
    }
    let theme = Theme::from_json(r##"{"name":"Ocean","accent":"#AABBCC"}"##).unwrap();
    assert_eq!(theme.accent, Rgb::new(170, 187, 204));
    assert_eq!(theme.background, Theme::default().background);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("huge.json");
    std::fs::write(&path, vec![b' '; 65537]).unwrap();
    assert!(Theme::load(&path).unwrap_err().contains("64 KiB"));
}

#[test]
fn theme_colors_reach_plain_text_and_cuda_tokens() {
    let theme = Theme {
        text: Rgb::new(1, 2, 3),
        comment: Rgb::new(4, 5, 6),
        keyword: Rgb::new(7, 8, 9),
        string: Rgb::new(10, 11, 12),
        number: Rgb::new(13, 14, 15),
        cuda: Rgb::new(16, 17, 18),
        ..Theme::default()
    };
    let plain = syntax::highlight_with_theme("plain log text", 14.0, 800.0, false, &theme);
    assert_eq!(plain.sections[0].format.color, Color32::from_rgb(1, 2, 3));
    let source = "__global__ void f() { int n = 42; const char* s = \"hello\"; // comment\n}";
    let highlighted = syntax::highlight_with_theme(source, 14.0, 800.0, true, &theme);
    assert_eq!(highlighted.text, source);
    for color in [
        theme.text,
        theme.comment,
        theme.keyword,
        theme.string,
        theme.number,
        theme.cuda,
    ] {
        assert!(highlighted
            .sections
            .iter()
            .any(|s| s.format.color == color.color()));
    }
}

#[test]
fn color_parser_handles_bad_unicode_without_panicking() {
    for value in ["", "#123", "#12345678", "ééé", "🦀00000", "#ABCDEX"] {
        assert!(Rgb::try_from(value.to_string()).is_err());
    }
    assert_eq!(
        Rgb::try_from("aabbcc".to_string()).unwrap().hex(),
        "#AABBCC"
    );
}

#[test]
fn icon_has_transparent_corners_and_brand_colors() {
    let icon = hedit::brand::icon();
    assert_eq!((icon.width, icon.height), (128, 128));
    assert_eq!(icon.rgba[3], 0);
    assert!(icon.rgba.chunks_exact(4).any(|p| p == [100, 216, 203, 255]));
    assert!(icon.rgba.chunks_exact(4).any(|p| p == [16, 22, 31, 255]));
}

#[test]
fn bundled_themes_match_the_builtin_catalog() {
    // HEDIT_WRITE_THEMES=1 cargo test --test themes regenerates the files.
    let directory = std::path::Path::new("themes");
    let write = std::env::var_os("HEDIT_WRITE_THEMES").is_some();
    let mut expected = vec!["custom-example.json".to_owned()];
    for theme in catalog::builtins() {
        let file = format!("{}.json", catalog::slug(&theme.name.replace('é', "e")));
        let path = directory.join(&file);
        if write {
            theme.export(&path).unwrap();
        }
        assert_eq!(&Theme::load(&path).unwrap(), theme, "{file}");
        expected.push(file);
    }
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    expected.sort();
    assert_eq!(files, expected, "themes/ has stale or missing files");
    let example = Theme::load(&directory.join("custom-example.json")).unwrap();
    assert_ne!(catalog::builtin(&example.name), Some(example));
}

#[test]
fn additional_dark_and_light_palettes_are_available_by_name() {
    for (dark, names) in [
        (
            true,
            [
                "Ayu Dark",
                "Night Owl",
                "Cobalt2",
                "Everforest Dark",
                "Kanagawa Wave",
            ],
        ),
        (
            false,
            [
                "Ayu Light",
                "Light Owl",
                "Quiet Light",
                "Everforest Light",
                "Flexoki Light",
            ],
        ),
    ] {
        for name in names {
            let theme = catalog::builtin(name).unwrap_or_else(|| panic!("missing {name}"));
            assert_eq!(theme.dark, dark, "{name}");
            assert_eq!(Theme::from_json(&theme.to_json().unwrap()).unwrap(), theme);
        }
    }
}
