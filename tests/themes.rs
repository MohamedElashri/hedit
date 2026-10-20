use eframe::egui::{self, Color32};
use hedit::{
    syntax,
    theme::{Preset, Rgb, Role, Theme},
};

#[test]
fn presets_support_light_and_dark_controls_with_readable_text() {
    for preset in Preset::ALL {
        let theme = preset.theme();
        theme.validate().unwrap();
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
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        let visuals = ctx.style().visuals.clone();
        assert_eq!(visuals.dark_mode, theme.dark);
        assert_eq!(visuals.panel_fill, theme.panel.color());
        assert_eq!(visuals.extreme_bg_color, theme.background.color());
        assert_eq!(visuals.text_cursor.stroke.color, theme.accent.color());
        assert_eq!(visuals.selection.stroke.color, theme.selection_text.color());
    }
    assert!(!Preset::Paper.theme().dark);
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
fn bundled_themes_match_the_native_presets() {
    for preset in Preset::ALL {
        let slug = preset.name().to_lowercase().replace(' ', "-");
        let path = std::path::Path::new("themes").join(format!("{slug}.json"));
        assert_eq!(Theme::load(&path).unwrap(), preset.theme());
    }
    Theme::load(std::path::Path::new("themes/custom-example.json")).unwrap();
}
