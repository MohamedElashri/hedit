use eframe::egui::Color32;
use hedit::syntax::grammar::{has_grammar, import_tm_theme, Grammar};
use hedit::syntax::Language;
use hedit::theme::{Rgb, Role, Theme};

/// A theme whose every role has a distinct color.
fn distinct() -> Theme {
    let mut theme = Theme::default();
    for (i, role) in Role::ALL.into_iter().enumerate() {
        *theme.color_mut(role) = Rgb::new(10 + i as u8 * 9, 100, 200);
    }
    theme
}

fn color_at(text: &str, runs: &[(usize, Color32)], needle: &str) -> Color32 {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} missing"));
    let mut offset = 0;
    for &(len, color) in runs {
        if at < offset + len {
            return color;
        }
        offset += len;
    }
    panic!("{needle} not covered")
}

#[test]
fn most_languages_use_a_grammar() {
    use Language::*;
    for language in Language::ALL {
        let lexer_only = matches!(language, PlainText | Log | Csv | PowerShell | VisualBasic);
        assert_eq!(has_grammar(language), !lexer_only, "{language:?}");
    }
}

#[test]
fn grammar_scopes_map_to_theme_roles() {
    let theme = distinct();
    let check = |language, text: &str, expected: &[(&str, Rgb)]| {
        let grammar_runs = Grammar::default()
            .highlight(text, language, &theme)
            .unwrap();
        let runs = grammar_runs.as_slice();
        assert_eq!(runs.iter().map(|(n, _)| n).sum::<usize>(), text.len());
        for (needle, rgb) in expected {
            assert_eq!(
                color_at(text, runs, needle),
                rgb.color(),
                "{language:?}: {needle}"
            );
        }
    };
    check(
        Language::Cpp,
        "#include <cstdio>\n// note\nint scale(float x) { return x * 2 + 0x1f; }\nconst char* s = \"hi\";\n",
        &[
            ("// note", theme.comment),
            ("int", theme.keyword),
            ("scale", theme.function),
            ("\"hi\"", theme.string),
            ("0x1f", theme.number),
            ("#include", theme.preprocessor),
        ],
    );
    check(
        Language::Python,
        "@cache\ndef area(r):\n    return True\n",
        &[
            ("def", theme.keyword),
            ("area", theme.function),
            ("True", theme.constant),
        ],
    );
    check(
        Language::Json,
        "{\"key\": \"value\", \"n\": 3}\n",
        &[("key", theme.property), ("value", theme.string)],
    );
    check(
        Language::Cuda,
        "__global__ void k(int* a) { a[threadIdx.x] = 0; }\n",
        &[
            ("__global__", theme.cuda),
            ("threadIdx", theme.cuda),
            ("void", theme.keyword),
        ],
    );
    // Languages without a grammar fall back to the lexer.
    assert!(Grammar::default()
        .highlight("x", Language::PlainText, &theme)
        .is_none());
    // So do very long lines.
    assert!(Grammar::default()
        .highlight(&"x".repeat(30_000), Language::Rust, &theme)
        .is_none());
}

#[test]
fn incremental_highlighting_matches_a_full_highlight() {
    let theme = Theme::default();
    let mut text = String::from(
        "/* block\n comment */\nfn main() {\n    let s = \"text\";\n    println!(\"{s}\");\n}\n\nstruct Point { x: i32 }\n",
    );
    let mut grammar = Grammar::default();
    grammar.highlight(&text, Language::Rust, &theme).unwrap();
    // A small deterministic generator keeps the test reproducible.
    let mut seed = 0x2545_f491_u64;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    let pieces = [
        "\"", "/*", "*/", "\n", "fn ", "x", " ", "}", "{", "//", "λ", "'a",
    ];
    for step in 0..300 {
        let boundaries: Vec<usize> = (0..=text.len())
            .filter(|&i| text.is_char_boundary(i))
            .collect();
        let at = boundaries[next(boundaries.len())];
        if next(3) == 0 && at < text.len() {
            let end = boundaries.iter().copied().find(|&b| b > at).unwrap();
            text.replace_range(at..end, "");
        } else {
            text.insert_str(at, pieces[next(pieces.len())]);
        }
        let incremental = grammar.highlight(&text, Language::Rust, &theme).unwrap();
        let full = Grammar::default()
            .highlight(&text, Language::Rust, &theme)
            .unwrap();
        assert_eq!(*incremental, *full, "step {step}: {text:?}");
    }
}

#[test]
fn textmate_color_schemes_import_as_themes() {
    let scheme = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>name</key><string>Sample Night</string>
<key>settings</key><array>
 <dict><key>settings</key><dict>
  <key>background</key><string>#101820</string>
  <key>foreground</key><string>#E0E6F0</string>
  <key>caret</key><string>#FFCC00</string>
  <key>selection</key><string>#334455</string>
 </dict></dict>
 <dict><key>scope</key><string>comment</string><key>settings</key><dict><key>foreground</key><string>#778899</string></dict></dict>
 <dict><key>scope</key><string>keyword</string><key>settings</key><dict><key>foreground</key><string>#FF6688</string></dict></dict>
 <dict><key>scope</key><string>string</string><key>settings</key><dict><key>foreground</key><string>#88DD66</string></dict></dict>
 <dict><key>scope</key><string>entity.name.function</string><key>settings</key><dict><key>foreground</key><string>#66AAFF</string></dict></dict>
</array></dict></plist>"#;
    let theme = import_tm_theme(scheme.as_bytes(), "fallback").unwrap();
    assert_eq!(theme.name, "Sample Night");
    assert!(theme.dark);
    assert_eq!(theme.background, Rgb::new(0x10, 0x18, 0x20));
    assert_eq!(theme.text, Rgb::new(0xE0, 0xE6, 0xF0));
    assert_eq!(theme.accent, Rgb::new(0xFF, 0xCC, 0x00));
    assert_eq!(theme.comment, Rgb::new(0x77, 0x88, 0x99));
    assert_eq!(theme.keyword, Rgb::new(0xFF, 0x66, 0x88));
    assert_eq!(theme.string, Rgb::new(0x88, 0xDD, 0x66));
    assert_eq!(theme.function, Rgb::new(0x66, 0xAA, 0xFF));
    // Roles the scheme does not color use its foreground.
    assert_eq!(theme.type_name, theme.text);
    assert!(import_tm_theme(b"not a plist", "x").is_err());
}

#[test]
#[ignore = "timing; run with --release"]
fn highlighting_speed() {
    let theme = Theme::default();
    let unit = std::fs::read_to_string("examples/cuda/vector_add.cu").unwrap();
    for size in [64 * 1024, 512 * 1024, 2 * 1024 * 1024] {
        let text = unit.repeat(size / unit.len() + 1);
        let mut grammar = Grammar::default();
        let started = std::time::Instant::now();
        grammar.highlight(&text, Language::Cpp, &theme).unwrap();
        let full = started.elapsed();
        let mut edited = text.clone();
        edited.insert(text.len() / 2, 'x');
        let started = std::time::Instant::now();
        grammar.highlight(&edited, Language::Cpp, &theme).unwrap();
        println!(
            "{} KiB: full {:?}, one edit {:?}",
            size / 1024,
            full,
            started.elapsed()
        );
    }
}

#[test]
#[ignore = "timing; run with --release"]
fn raw_syntect_speed() {
    use syntect::easy::HighlightLines;
    use syntect::util::LinesWithEndings;
    let set = two_face::syntax::extra_newlines();
    let themes = syntect::highlighting::ThemeSet::load_defaults();
    let theme = &themes.themes["base16-ocean.dark"];
    let unit = std::fs::read_to_string("examples/cuda/vector_add.cu").unwrap();
    let text = unit.repeat(64 * 1024 / unit.len() + 1);
    for name in ["C++", "C", "Rust", "Python"] {
        let syntax = set.find_syntax_by_name(name).unwrap();
        let started = std::time::Instant::now();
        let mut h = HighlightLines::new(syntax, theme);
        for line in LinesWithEndings::from(&text) {
            h.highlight_line(line, &set).unwrap();
        }
        println!("{name} 64 KiB raw: {:?}", started.elapsed());
    }
}
