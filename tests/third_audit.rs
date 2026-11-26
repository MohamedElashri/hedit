use hedit::{
    advanced::{self, Selection as S},
    counterpart, file_reference, intelligence, text_commands,
};
use std::{fs, path::Path, sync::atomic::AtomicBool};

fn range(anchor: usize, head: usize) -> S {
    S { anchor, head }
}

#[test]
fn finds_angle_delimited_include_paths_with_spaces() {
    let line = "#include <shared headers/lib.hpp>";
    assert_eq!(
        file_reference::at(line, S::caret(26)).as_deref(),
        Some("shared headers/lib.hpp")
    );
}

#[test]
fn finds_quoted_reference_from_its_opening_delimiter() {
    let line = "open \"notes today.md\"";
    assert_eq!(
        file_reference::at(line, S::caret(5)).as_deref(),
        Some("notes today.md")
    );
}

#[test]
fn ignored_sibling_does_not_hide_project_counterpart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("include")).unwrap();
    fs::write(root.join(".gitignore"), "/solver.h\n").unwrap();
    fs::write(root.join("solver.h"), "").unwrap();
    let header = root.join("include/solver.hpp");
    fs::write(&header, "").unwrap();
    let source = root.join("solver.cpp");
    fs::write(&source, "").unwrap();
    assert_eq!(
        counterpart::find(&source, &[root.to_owned()], &AtomicBool::new(false)).unwrap(),
        [header.canonicalize().unwrap()]
    );
}

#[test]
fn selecting_next_occurrence_respects_the_selected_match_alignment() {
    // The initial selection starts between the global non-overlapping matches.
    assert_eq!(
        advanced::occurrences("aaaaa", &[range(1, 3)], false),
        [range(1, 3), range(3, 5)]
    );
}

#[test]
fn skipping_occurrence_can_find_a_free_match_after_an_overlapping_match() {
    assert_eq!(
        advanced::skip_occurrence("aaaaa", &[range(1, 3)]),
        [range(3, 5)]
    );
}

#[test]
fn split_into_lines_preserves_a_reversed_selection() {
    assert_eq!(
        advanced::split_into_lines("ab\r\ncd\n", &[range(6, 0)]),
        [range(2, 0), range(6, 4)]
    );
}

#[test]
fn wrapping_in_quotes_keeps_the_caret_at_the_start_of_a_reversed_selection() {
    let batch = advanced::pair("word", &[range(4, 0)], '"', true).unwrap();
    assert_eq!(batch.text, "\"word\"");
    assert_eq!(batch.selections, [range(5, 1)]);
}

#[test]
fn expanding_to_quotes_keeps_escaped_quotes_inside_the_selection() {
    let text = "let label = \"left \\\"quoted\\\" right\";";
    let at = text.find("right").unwrap() + 1;
    assert_eq!(
        advanced::expand_selection(text, range(at - 1, at + 4)),
        range(text.find('"').unwrap() + 1, text.rfind('"').unwrap())
    );
}

#[test]
fn reindent_keeps_whitespace_inside_multiline_template_literals() {
    let code = "{\nconst message = `line one\nraw { content\nline three`;\nrun();\n}\n";
    assert_eq!(
        text_commands::reindent(code, &[range(0, code.len())], 2, false).text,
        "{\n  const message = `line one\nraw { content\nline three`;\n  run();\n}\n"
    );
}

#[test]
fn definition_location_skips_the_same_name_inside_the_return_type() {
    let cpp = "std::string string() {";
    let found = intelligence::definitions(cpp, "string", Path::new("example.cpp"));
    assert_eq!(
        found.first().map(|p| p.column),
        Some(cpp.rfind("string").unwrap() + 1),
    );
}

#[test]
fn snippet_number_inside_a_discarded_default_does_not_create_a_field() {
    let expansion = intelligence::expand_fields("${1:real} ${1:${2:discarded}} $2", "");
    assert_eq!(expansion.text, "real real ");
    assert_eq!(
        expansion.fields,
        vec![vec![(0, 4), (5, 9)], vec![(10, 10)], vec![(10, 10)]]
    );
}

#[test]
fn whole_word_search_can_match_a_token_with_trailing_punctuation() {
    let text = "make! make!target remake!";
    let options = hedit::search::Options {
        whole_word: true,
        ..Default::default()
    };
    assert_eq!(
        hedit::search::matches(text, "make!", &options, 0..text.len()).unwrap(),
        std::slice::from_ref(&(0..5))
    );
}

#[test]
fn hunk_on_the_first_line_after_a_change_is_not_the_previous_hunk() {
    let patch = "@@ -1 +1 @@\n-old one\n+new one\n@@ -2 +2 @@\n-old two\n+new two\n";
    assert_eq!(
        intelligence::hunk_at(patch, 2),
        "@@ -2 +2 @@\n-old two\n+new two\n"
    );
}

#[test]
fn debugger_console_decodes_c_whitespace_escapes() {
    assert_eq!(
        hedit::debugger::decode_stream(r#""clear\fback\bup\v""#).as_deref(),
        Some("clear\x0cback\x08up\x0b")
    );
}

#[test]
fn sorting_with_multiple_carets_and_no_selection_sorts_the_document() {
    let text = "c\nb\na\n";
    let edited = advanced::transform(
        text,
        &[S::caret(0), S::caret(2)],
        hedit::editing::Operation::SortLines,
        4,
        false,
        None,
    );
    assert_eq!(edited.text, "a\nb\nc\n");
}

#[test]
fn joining_two_separate_lines_from_carets_joins_each_to_its_successor() {
    let text = "first\nsecond\nthird\nfourth\n";
    let edited = advanced::transform(
        text,
        &[S::caret(0), S::caret(13)],
        hedit::editing::Operation::JoinLines,
        4,
        false,
        None,
    );
    assert_eq!(edited.text, "first second\nthird fourth\n");
}

#[test]
fn moving_adjacent_lines_up_keeps_their_order() {
    let text = "a\nb\nc\nd\n";
    let edited = advanced::transform(
        text,
        &[S::caret(2), S::caret(4)],
        hedit::editing::Operation::MoveUp,
        4,
        false,
        None,
    );
    assert_eq!(edited.text, "b\nc\na\nd\n");
}

#[test]
fn indenting_two_carets_keeps_carets_instead_of_selecting_both_lines() {
    let text = "one\ntwo\n";
    let edited = advanced::transform(
        text,
        &[S::caret(1), S::caret(5)],
        hedit::editing::Operation::Indent,
        2,
        false,
        None,
    );
    assert_eq!(edited.text, "  one\n  two\n");
    assert_eq!(edited.selections, [S::caret(3), S::caret(9)]);
}

#[test]
fn selecting_a_word_keeps_combining_marks_in_decomposed_text() {
    assert_eq!(advanced::word("cafe\u{301}teria", 7), range(0, 10));
}

#[test]
fn completion_indexes_decomposed_accents_as_one_word() {
    let words = hedit::completion::index(&["re\u{301}sume\u{301}"]);
    assert_eq!(
        words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>(),
        ["re\u{301}sume\u{301}"]
    );
}

#[cfg(unix)]
#[test]
fn editorconfig_treats_a_backslash_in_a_unix_filename_as_literal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(r"file\draft.rs");
    fs::write(&path, "").unwrap();
    fs::write(
        dir.path().join(".editorconfig"),
        "root = true\n[file\\\\draft.rs]\nindent_size = 2\n",
    )
    .unwrap();
    let defaults = hedit::projects::Defaults::default();
    assert_eq!(
        hedit::projects::formatting(&path, "rust", &defaults, None)
            .unwrap()
            .indent_size,
        Some(2)
    );
}

#[test]
fn folder_replace_applies_to_utf16_files_that_decode_to_more_than_the_disk_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    let text = "needle\n".to_owned() + &"字".repeat(6_000_000);
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    assert!(bytes.len() < hedit::search::FILE_LIMIT as usize);
    assert!(text.len() > hedit::search::FILE_LIMIT as usize);
    fs::write(&path, bytes).unwrap();
    let result = hedit::search::folders(
        &[dir.path().to_owned()],
        "",
        "needle",
        Some("updated"),
        &Default::default(),
        &Default::default(),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result.plans.len(), 1);
    hedit::search::apply(&result.plans[0]).unwrap();
    assert!(fs::read(path)
        .unwrap()
        .starts_with(&[0xff, 0xfe, b'u', 0, b'p', 0]));
}

#[test]
fn git_scope_in_an_empty_repository_returns_empty_history() {
    let dir = tempfile::tempdir().unwrap();
    let git = hedit::git::Git::default();
    git.command(dir.path(), &["init", "-b", "main"]).unwrap();
    let scope = hedit::git::HistoryScope {
        path: Some("notes.txt".into()),
        reference: None,
    };
    assert!(git
        .scoped_history(dir.path(), 20, &scope)
        .unwrap()
        .is_empty());
}

#[test]
fn nested_annotated_tags_resolve_to_the_underlying_commit() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let git = hedit::git::Git::default();
    git.command(root, &["init", "-b", "main"]).unwrap();
    git.command(
        root,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--no-gpg-sign",
            "--allow-empty",
            "-m",
            "start",
        ],
    )
    .unwrap();
    for tag in [["inner", "HEAD"], ["outer", "inner"]] {
        git.command(
            root,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "tag",
                "-a",
                "--no-sign",
                "-m",
                "release tag",
                tag[0],
                tag[1],
            ],
        )
        .unwrap();
    }
    let commit = git.resolve_commit(root, "HEAD").unwrap();
    let tags = git.tags(root).unwrap();
    let outer = tags.iter().find(|tag| tag.name == "outer").unwrap();
    assert_eq!(outer.commit.as_deref(), Some(commit.as_str()));
}
