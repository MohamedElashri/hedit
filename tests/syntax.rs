use eframe::egui::{text::LayoutJob, Color32};
use hedit::{
    document::{Document, PAGE_SIZE, SMALL_LIMIT},
    syntax::{highlight_language, Language},
    theme::Theme,
    workspace::Workspace,
};
use std::fs;

fn color_at(job: &LayoutJob, needle: &str) -> Color32 {
    let at = job.text.find(needle).unwrap();
    job.sections
        .iter()
        .find(|section| section.byte_range.contains(&at))
        .unwrap()
        .format
        .color
}

#[test]
fn filenames_detect_supported_languages_and_unknown_files_stay_plain() {
    use Language::*;
    for (name, expected) in [
        ("notes.txt", PlainText),
        ("extensionless", PlainText),
        ("cu", PlainText),
        ("draft.unknown", PlainText),
        ("archive.cu.bak", PlainText),
        ("/tmp/a.b/README", PlainText),
        ("/tmp/a.b/main.CPP", Cpp),
        ("header.H", Cpp),
        ("kernel.CUH", Cuda),
        ("tool.go", Go),
        ("app.mjs", JavaScript),
        ("component.jsx", JavaScript),
        ("settings.json", Json),
        ("events.ndjson", Json),
        ("events.jsonl", Json),
        ("output.LOG", Log),
        ("README.MD", Markdown),
        ("notes.markdown", Markdown),
        ("script.PY", Python),
        ("typing.pyi", Python),
        ("main.rs", Rust),
        ("setup.sh", Shell),
        ("/home/user/.bashrc", Shell),
        (".zshrc", Shell),
        ("Cargo.toml", Toml),
        ("Cargo.lock", Toml),
        ("other.lock", PlainText),
        ("main.ts", TypeScript),
        (r"C:\src\component.TSX", TypeScript),
        ("ci.YML", Yaml),
    ] {
        assert_eq!(Language::detect(name), expected, "{name}");
    }
    for language in Language::ALL {
        assert_eq!(Language::from_id(language.id()), Some(language));
        for extension in language.extensions() {
            assert_eq!(Language::detect(&format!("example.{extension}")), language);
        }
        for filename in language.filenames() {
            assert_eq!(Language::detect(filename), language);
        }
    }
    assert_eq!(Language::from_id("future-grammar"), None);
}

#[test]
fn c_cpp_and_cuda_have_distinct_associations_labels_and_keywords() {
    use Language::*;
    for (name, expected) in [
        ("file.c", C),
        ("header.h", C),
        ("file.C", Cpp),
        ("header.H", Cpp),
        ("file.cpp", Cpp),
        ("header.hpp", Cpp),
        ("kernel.cu", Cuda),
    ] {
        assert_eq!(Language::detect(name), expected);
    }
    assert_eq!((C.label(), Cpp.label(), Cuda.label()), ("C", "C++", "CUDA"));
    let theme = Theme::default();
    let source = "_Atomic int value; class Example {}; __global__ void kernel();";
    let c = highlight_language(source, 14.0, 800.0, C, &theme);
    let cpp = highlight_language(source, 14.0, 800.0, Cpp, &theme);
    let cuda = highlight_language(source, 14.0, 800.0, Cuda, &theme);
    assert_eq!(color_at(&c, "_Atomic"), theme.keyword.color());
    assert_eq!(color_at(&c, "class"), theme.text.color());
    assert_eq!(color_at(&cpp, "class"), theme.keyword.color());
    assert_eq!(color_at(&cpp, "__global__"), theme.text.color());
    assert_eq!(color_at(&cuda, "__global__"), theme.cuda.color());
    assert_eq!(Language::from_id("cpp"), Some(Cpp));
    assert_eq!(Language::from_id("cuda"), Some(Cuda));
}

#[test]
fn syntax_search_finds_names_extensions_and_special_filenames() {
    for (language, query) in [
        (Language::CSharp, "C#"),
        (Language::Cpp, ".hpp"),
        (Language::Dockerfile, "containerfile"),
        (Language::Html, " HTML "),
        (Language::CMake, "CMakeLists"),
        (Language::Ruby, "Gemfile"),
        (Language::Sql, "sql"),
        (Language::Cuda, ".cuh"),
    ] {
        assert!(language.matches_query(query), "{language:?}: {query}");
    }
    assert!(!Language::C.matches_query("C++"));
    assert!(!Language::Java.matches_query("javascript"));
    for language in Language::ALL {
        assert!(language.matches_query(""));
    }
    assert_eq!(Language::detect("Dockerfile.prod"), Language::Dockerfile);
    assert_eq!(Language::detect(".env.local"), Language::Ini);
}

#[test]
fn common_web_science_config_and_build_files_have_working_lexers() {
    use Language::*;
    let theme = Theme::default();
    for (language, source, keyword, comment) in [
        (
            Assembly,
            "section .text\nmov eax, 42 ; note",
            "section",
            "; note",
        ),
        (
            Batch,
            "REM note\nset count=42\necho hello",
            "set",
            "REM note",
        ),
        (C, "int count = 42; // note", "int", "// note"),
        (
            CSharp,
            "public class Notes { string s = \"λ\"; } // note",
            "public",
            "// note",
        ),
        (
            CMake,
            "project(Notes)\nset(COUNT 42) # note",
            "project",
            "# note",
        ),
        (
            Css,
            "main { color: #fff; } /* note */",
            "color",
            "/* note */",
        ),
        (Dart, "final name = 'λ'; // note", "final", "// note"),
        (
            Dockerfile,
            "FROM ubuntu\nRUN echo hello\n# note",
            "FROM",
            "# note",
        ),
        (
            Elixir,
            "defmodule Notes do\nend # note",
            "defmodule",
            "# note",
        ),
        (Erlang, "-module(notes).\n% note", "module", "% note"),
        (
            Fortran,
            "PROGRAM main\nINTEGER :: n = 42\n! note\nEND PROGRAM",
            "PROGRAM",
            "! note",
        ),
        (Graphql, "query { user { name } } # note", "query", "# note"),
        (Groovy, "def name = 'λ' // note", "def", "// note"),
        (Haskell, "module Main where\n-- note", "module", "-- note"),
        (
            Hcl,
            "resource \"server\" \"main\" {} # note",
            "resource",
            "# note",
        ),
        (
            Html,
            "<main title=\"λ\">Text</main><!-- note -->",
            "main",
            "<!-- note -->",
        ),
        (
            Ini,
            "[settings]\nenabled=true ; note",
            "[settings]",
            "; note",
        ),
        (Java, "public class Notes {} // note", "public", "// note"),
        (
            Julia,
            "function f()\nreturn 42\nend # note",
            "function",
            "# note",
        ),
        (Kotlin, "val name = \"λ\" // note", "val", "// note"),
        (Latex, "\\section{Notes} % note", "\\section", "% note"),
        (
            Less,
            "@color: red; main { color: @color; } // note",
            "color",
            "// note",
        ),
        (Lua, "local name = 'λ' -- note", "local", "-- note"),
        (
            Makefile,
            "include common.mk\n# note\nall:\n\techo $(NAME)",
            "include",
            "# note",
        ),
        (
            Matlab,
            "function x = f()\nx = 42; % note\nend",
            "function",
            "% note",
        ),
        (Nix, "let x = 42; in x # note", "let", "# note"),
        (
            ObjectiveC,
            "@interface Notes : NSObject\n@end // note",
            "interface",
            "// note",
        ),
        (Perl, "my $name = 'λ'; # note", "my", "# note"),
        (
            Php,
            "<?php function f() { return 42; } // note",
            "function",
            "// note",
        ),
        (
            PowerShell,
            "function Get-Notes { return 42 } # note",
            "function",
            "# note",
        ),
        (
            Protobuf,
            "message Notes { string name = 1; } // note",
            "message",
            "// note",
        ),
        (
            R,
            "function(x) { return(TRUE) } # note",
            "function",
            "# note",
        ),
        (Ruby, "def notes\n42\nend # note", "def", "# note"),
        (
            Scala,
            "object Notes { val n = 42 } // note",
            "object",
            "// note",
        ),
        (
            Scss,
            "$color: red; main { color: $color; } // note",
            "color",
            "// note",
        ),
        (
            Sql,
            "SELECT name FROM notes WHERE n = 42; -- note",
            "SELECT",
            "-- note",
        ),
        (
            Swift,
            "func notes() -> Int { return 42 } // note",
            "func",
            "// note",
        ),
        (
            Verilog,
            "module notes; reg n; endmodule // note",
            "module",
            "// note",
        ),
        (
            Vhdl,
            "ENTITY notes IS END notes; -- note",
            "ENTITY",
            "-- note",
        ),
        (
            VisualBasic,
            "Public Class Notes\nEnd Class ' note",
            "Public",
            "' note",
        ),
        (
            Xml,
            "<notes name=\"λ\"/><!-- note -->",
            "notes",
            "<!-- note -->",
        ),
        (Zig, "const n: u32 = 42; // note", "const", "// note"),
    ] {
        let job = highlight_language(source, 14.0, 800.0, language, &theme);
        assert_eq!(job.text, source, "{language:?}");
        assert_eq!(
            color_at(&job, keyword),
            theme.keyword.color(),
            "{language:?}: {keyword}"
        );
        assert_eq!(
            color_at(&job, comment),
            theme.comment.color(),
            "{language:?}: {comment}"
        );
    }
    let diff = "--- old\n+++ new\n@@ -1 +1 @@\n-old\n+new\n\n";
    let job = highlight_language(diff, 14.0, 800.0, Diff, &theme);
    assert_eq!(job.text, diff);
    assert_eq!(color_at(&job, "-old"), theme.error.color());
    assert_eq!(color_at(&job, "+new"), theme.success.color());
    assert_eq!(color_at(&job, "@@"), theme.keyword.color());
    let csv = "name,count\nO'Brien,42\n\"λ,🦀\",7\n";
    let job = highlight_language(csv, 14.0, 800.0, Csv, &theme);
    assert_eq!(job.text, csv);
    assert_eq!(color_at(&job, ","), theme.accent.color());
    assert_eq!(color_at(&job, "42"), theme.number.color());
    assert_eq!(color_at(&job, "λ"), theme.string.color());
}

#[test]
fn each_lexer_highlights_using_theme_roles_without_altering_text() {
    use Language::*;
    let theme = Theme::default();
    for (language, source, keyword, string, number, comment) in [
        (
            Cpp,
            "const char* s = \"λ\"; int n = 42; // note\n",
            "const",
            "\"λ\"",
            "42",
            "// note",
        ),
        (
            Cuda,
            "__global__ void f() { int n = 42; const char* s = \"λ\"; /* note */ }",
            "void",
            "\"λ\"",
            "42",
            "/* note */",
        ),
        (
            Go,
            "package main\nvar n = 42; var s = `λ`; // note\n",
            "package",
            "`λ`",
            "42",
            "// note",
        ),
        (
            JavaScript,
            "const s = `λ`; let n = 42; // note\n",
            "const",
            "`λ`",
            "42",
            "// note",
        ),
        (
            Json,
            "{\"flag\": true, \"count\": 42, \"name\": \"λ\"}",
            "true",
            "\"λ\"",
            "42",
            "",
        ),
        (Log, "INFO count=42 name=\"λ\"\n", "", "\"λ\"", "42", ""),
        (
            Markdown,
            "# Heading\nUse `λ` here.\n> note\n",
            "# Heading",
            "`λ`",
            "",
            "> note",
        ),
        (
            Python,
            "def f():\n    return \"λ\", 42 # note\n",
            "def",
            "\"λ\"",
            "42",
            "# note",
        ),
        (
            Rust,
            "let n = 42; let s = \"λ\"; // note\n",
            "let",
            "\"λ\"",
            "42",
            "// note",
        ),
        (
            Shell,
            "if true; then echo \"λ\" 42; fi # note\n",
            "if",
            "\"λ\"",
            "42",
            "# note",
        ),
        (
            Toml,
            "enabled = true\nname = \"λ\"\ncount = 42 # note\n",
            "true",
            "\"λ\"",
            "42",
            "# note",
        ),
        (
            TypeScript,
            "interface T { x: string }; const s = 'λ'; let n = 42; // note\n",
            "interface",
            "'λ'",
            "42",
            "// note",
        ),
        (
            Yaml,
            "enabled: true\nname: 'λ'\ncount: 42 # note\n",
            "true",
            "'λ'",
            "42",
            "# note",
        ),
    ] {
        let job = highlight_language(source, 15.0, 320.0, language, &theme);
        assert_eq!(job.text, source, "{language:?}");
        assert_eq!(job.wrap.max_width, 320.0);
        for (needle, expected) in [
            (keyword, theme.keyword),
            (string, theme.string),
            (number, theme.number),
            (comment, theme.comment),
        ] {
            if !needle.is_empty() {
                assert_eq!(
                    color_at(&job, needle),
                    expected.color(),
                    "{language:?}: {needle}"
                );
            }
        }
    }
    let job = highlight_language("INFO WARN ERROR DEBUG", 14.0, 800.0, Log, &theme);
    for (word, color) in [
        ("INFO", theme.success),
        ("WARN", theme.warning),
        ("ERROR", theme.error),
        ("DEBUG", theme.comment),
    ] {
        assert_eq!(color_at(&job, word), color.color());
    }
    let source = "__global__ threadIdx.x";
    let job = highlight_language(source, 14.0, 800.0, Cuda, &theme);
    assert_eq!(color_at(&job, "__global__"), theme.cuda.color());
    let plain = highlight_language(source, 14.0, 800.0, PlainText, &theme);
    assert_eq!(plain.sections.len(), 1);
    assert_eq!(plain.sections[0].format.color, theme.text.color());
}

#[test]
fn strings_comments_lifetimes_and_incomplete_utf8_pages_keep_token_boundaries() {
    let theme = Theme::default();
    let rust = "fn f<'a>(s: &'a str) { /* outer /* λ */ inner */ let c = 'λ'; }";
    let job = highlight_language(rust, 14.0, 800.0, Language::Rust, &theme);
    assert_eq!(color_at(&job, "inner"), theme.comment.color());
    assert_eq!(color_at(&job, "str"), theme.keyword.color());
    assert_eq!(color_at(&job, "'λ'"), theme.string.color());
    let python = "x = '''first\nλ # still string\nlast'''\ndef f(): pass";
    let job = highlight_language(python, 14.0, 800.0, Language::Python, &theme);
    assert_eq!(color_at(&job, "# still"), theme.string.color());
    assert_eq!(color_at(&job, "def"), theme.keyword.color());
    let json = r#"{"url": "https://example.test/#a", "escaped": "\"x\"", "ok": true}"#;
    let job = highlight_language(json, 14.0, 800.0, Language::Json, &theme);
    assert_eq!(color_at(&job, "//example"), theme.string.color());
    assert_eq!(color_at(&job, "true"), theme.keyword.color());
    let shell = "echo https://example.test/#fragment # comment";
    let job = highlight_language(shell, 14.0, 800.0, Language::Shell, &theme);
    assert_eq!(color_at(&job, "#fragment"), theme.text.color());
    assert_eq!(color_at(&job, "# comment"), theme.comment.color());

    // Page ends, unfinished snippets, and empty buffers must not drop/reorder a
    // byte or return ranges that split a Unicode character.
    let source = "# λ🦀\r\n/* nested /* */ x */ \"escape\\\" x\" 'a '\\n' '''multi\nline''' ```code``` 1.2e-3 _x";
    for language in Language::ALL {
        for end in (0..=source.len()).filter(|&end| source.is_char_boundary(end)) {
            let source = &source[..end];
            let job = highlight_language(source, 14.0, f32::INFINITY, language, &theme);
            assert_eq!(job.text, source);
            let reconstructed: String = job
                .sections
                .iter()
                .map(|s| &job.text[s.byte_range.clone()])
                .collect();
            assert_eq!(reconstructed, source, "{language:?}");
        }
    }
}

#[test]
fn large_plain_runs_do_not_create_a_layout_section_per_character() {
    let text = "  λ🦀 ;\n".repeat(32768);
    let job = highlight_language(&text, 14.0, 800.0, Language::Cpp, &Theme::default());
    assert_eq!(job.text, text);
    assert_eq!(job.sections.len(), 1);
}

#[test]
fn syntax_is_per_buffer_and_survives_recovery_shelf_and_restart() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().into()).unwrap();
    workspace.open[0].page.text = "print('unsaved λ')".into();
    workspace.edit(0, "").unwrap();
    let original = (
        workspace.open[0].page.info.dirty,
        workspace.open[0].page.info.size,
    );
    let cursor = (workspace.open[0].view.anchor, workspace.open[0].view.head);
    assert!(workspace.open[0].set_language(Some(Language::Python)));
    assert!(!workspace.open[0].set_language(Some(Language::Python)));
    assert_eq!(workspace.open[0].page.info.dirty, original.0);
    assert_eq!(workspace.open[0].page.info.size, original.1);
    assert_eq!(
        (workspace.open[0].view.anchor, workspace.open[0].view.head),
        cursor
    );
    workspace.new_buffer().unwrap();
    assert_eq!(workspace.open[1].language(), Language::PlainText);
    workspace.open[1].set_language(Some(Language::Json));
    workspace.close(0).unwrap();
    workspace.checkpoint().unwrap();
    drop(workspace);

    let (mut workspace, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(workspace.open[0].language(), Language::Json);
    assert_eq!(
        workspace.closed[0].language_override.as_deref(),
        Some("python")
    );
    workspace.reopen_at(0).unwrap();
    let active = workspace.active;
    assert_eq!(workspace.open[active].language(), Language::Python);
    assert_eq!(workspace.open[active].page.text, "print('unsaved λ')");
    assert!(workspace.open[active].page.info.path.is_none());
    workspace.open[active].set_language(None);
    workspace.checkpoint().unwrap();
    drop(workspace);
    let (workspace, errors) = Workspace::restore(directory.path().into()).unwrap();
    assert!(errors.is_empty());
    assert_eq!(
        workspace.open[workspace.active].language(),
        Language::PlainText
    );
    assert!(workspace.open[workspace.active]
        .view
        .language_override
        .is_none());
}

#[test]
fn save_as_redetects_automatically_but_respects_explicit_choices() {
    let directory = tempfile::tempdir().unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let doc = workspace
        .editor
        .get(workspace.open[0].page.info.id)
        .unwrap();
    workspace.open[0].page.info = doc
        .lock()
        .unwrap()
        .save(Some(directory.path().join("script.py")))
        .unwrap();
    assert_eq!(workspace.open[0].language(), Language::Python);
    workspace.open[0].set_language(Some(Language::PlainText));
    workspace.open[0].page.info = doc
        .lock()
        .unwrap()
        .save(Some(directory.path().join("main.rs")))
        .unwrap();
    assert_eq!(workspace.open[0].language(), Language::PlainText);
    workspace.open[0].set_language(None);
    assert_eq!(workspace.open[0].language(), Language::Rust);
    workspace.open[0].view.language_override = Some("unknown-legacy-grammar".into());
    assert_eq!(workspace.open[0].language(), Language::Rust);
}

#[test]
fn paged_files_keep_their_language_and_override_when_navigating() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("large.py");
    fs::write(&path, "print('λ')\n".repeat(SMALL_LIMIT as usize / 10)).unwrap();
    let (mut workspace, _) = Workspace::restore(directory.path().join("session")).unwrap();
    let mut doc = Document::open(workspace.editor.id(), &path).unwrap();
    let first = doc.page(0).unwrap();
    assert!(first.info.paged);
    workspace.add(doc, first).unwrap();
    let active = workspace.active;
    assert_eq!(workspace.open[active].language(), Language::Python);
    workspace.open[active].set_language(Some(Language::Json));
    let doc = workspace
        .editor
        .get(workspace.open[active].page.info.id)
        .unwrap();
    let next = doc.lock().unwrap().page(PAGE_SIZE).unwrap();
    assert!(next.start > 0);
    workspace.open[active].set_page(next);
    assert_eq!(workspace.open[active].language(), Language::Json);
    workspace.open[active].set_language(None);
    assert_eq!(workspace.open[active].language(), Language::Python);
}
