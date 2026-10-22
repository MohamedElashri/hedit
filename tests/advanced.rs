use hedit::advanced::{self as a, Fold, Projection, Selection as S};
#[test]
fn word_boundaries_are_unicode_safe_and_cross_whitespace() {
    assert_eq!(a::word_boundary("αβ  name", 4, false), 0);
    assert_eq!(a::word_boundary("αβ  name", 0, true), 4);
    assert_eq!(a::word_boundary("αβ  name", 4, true), 8);
}
#[test]
fn batch_edits_merge_overlaps_and_keep_unicode_cursor_positions() {
    let edit = a::replace(
        "α foo β foo",
        &[
            S { anchor: 2, head: 5 },
            S {
                anchor: 8,
                head: 11,
            },
        ],
        "🦀",
    );
    assert_eq!(edit.text, "α 🦀 β 🦀");
    assert_eq!(edit.selections, vec![S::caret(3), S::caret(7)]);
    assert_eq!(
        a::replace(
            "abc",
            &[S { anchor: 2, head: 0 }, S { anchor: 1, head: 3 }],
            "x"
        )
        .text,
        "x"
    );
}
#[test]
fn line_commands_indent_a_shared_line_once_and_case_edits_keep_separate_ranges() {
    use hedit::editing::Operation;
    let edit = a::transform(
        "one\ntwo\n",
        &[S::caret(0), S::caret(2)],
        Operation::Indent,
        2,
        false,
        None,
    );
    assert_eq!(edit.text, "  one\ntwo\n");
    let edit = a::transform(
        "ß and λ",
        &[S { anchor: 0, head: 1 }, S { anchor: 6, head: 7 }],
        Operation::Uppercase,
        4,
        false,
        None,
    );
    assert_eq!(edit.text, "SS and Λ");
    assert_eq!(edit.selections.len(), 2);
}
#[test]
fn occurrences_wrap_skip_selected_ranges_and_support_unicode_words() {
    let first = a::occurrences("α_name α_name α_name", &[S::caret(2)], false);
    assert_eq!(first, vec![S { anchor: 0, head: 6 }]);
    let next = a::occurrences("α_name α_name α_name", &first, false);
    assert_eq!(next.len(), 2);
    assert_eq!(a::occurrences("α_name α_name α_name", &next, true).len(), 3);
}
#[test]
fn rectangular_selection_uses_tab_stops_and_clamps_short_lines() {
    let source = "  α🦀z\n\tβxyz\nx\n";
    let ranges = a::rectangle(source, 2, 11, 4);
    assert!(ranges.len() >= 2);
    for s in ranges {
        assert!(s.end() <= source.chars().count());
    }
    assert_eq!(a::at_column("\tx", 0, 4, 4), 1);
}
#[test]
fn indentation_detection_and_smart_newlines_follow_file_policy() {
    assert_eq!(
        a::detect_indentation("if x:\n  a\n  if y:\n    b\n"),
        (false, 2)
    );
    assert_eq!(a::detect_indentation("fn f() {\n\tcall();\n}\n"), (true, 4));
    let result = a::newline("{}", S::caret(1), 2, false, true, true);
    assert_eq!(result.text, "{\r\n  \r\n}");
    assert_eq!(result.selections, vec![S::caret(5)]);
}
#[test]
fn projection_expands_tab_stops_without_rewriting_original_tabs() {
    let source = "α\tβ\n\tx";
    let p = Projection::new(source, 4, &[]);
    assert_eq!(p.text, "α   β\n    x");
    assert_eq!(p.source(p.display(2)), 2);
    let edit = p.edited(source, "α   zβ\n    x", S::caret(5));
    assert_eq!(edit.text, "α\tzβ\n\tx");
    assert_eq!(edit.selections, vec![S::caret(3)]);
    let p = Projection::new("\tx", 8, &[]);
    let edit = p.edited("\tx", "       x", S::caret(0));
    assert_eq!(edit.text, "x");
}
#[test]
fn folding_retains_hidden_text_and_document_line_numbers() {
    let source = "fn f() {\n  α();\n  β();\n}\nlast\n";
    let folds = a::indentation_folds(source, 2);
    assert_eq!(folds.len(), 1);
    let p = Projection::new(source, 2, &folds);
    assert_eq!(p.text, "fn f() {\n⋯\n}\nlast\n");
    assert_eq!(p.source_lines, vec![1, 2, 4, 5, 6]);
    let display = p.text.replace("last", "LAST");
    let edit = p.edited(source, &display, S::caret(display.chars().count()));
    assert_eq!(edit.text, source.replace("last", "LAST"));
}
#[test]
fn invalid_fold_metadata_and_selections_are_bounded() {
    let p = Projection::new(
        "α\nβ",
        0,
        &[
            Fold {
                start: 99,
                end: 100,
            },
            Fold { start: 1, end: 0 },
        ],
    );
    assert_eq!(p.text, "α\nβ");
    assert_eq!(
        a::normalize(
            &[S {
                anchor: usize::MAX,
                head: usize::MAX
            }],
            3
        ),
        vec![S::caret(3)]
    );
    for source in ["", "α\t🦀\r\n", "a\nb\nc\n"] {
        let p = Projection::new(source, 8, &[]);
        for n in 0..=source.chars().count() {
            assert_eq!(p.source(p.display(n)), n);
        }
    }
}
