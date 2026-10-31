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
fn fold_levels_keep_parents_and_count_irregular_indent_as_one_step() {
    let text = "outer:\n   middle:\n        inner:\n            α()\n   tail()\nnext:\n   β()\n";
    let levels: Vec<_> = (1..=4)
        .map(|level| a::indentation_folds_at_level(text, 4, level))
        .collect();
    assert_eq!(
        levels.iter().map(Vec::len).collect::<Vec<_>>(),
        [2, 1, 1, 0]
    );
    assert_eq!(
        Projection::new(text, 4, &levels[1]).text,
        "outer:\n   middle:\n⋯\n   tail()\nnext:\n   β()\n"
    );
    assert_eq!(
        Projection::new(text, 4, &levels[2]).text,
        "outer:\n   middle:\n        inner:\n⋯\n   tail()\nnext:\n   β()\n"
    );
    assert!(a::indentation_folds_at_level(text, 4, 0).is_empty());
}

#[test]
fn fold_levels_handle_tabs_crlf_blank_lines_and_no_final_newline() {
    let text = "root:\r\n\tchild:\r\n\r\n\t\tλ()\r\n\tlast()\r\nend";
    let outer = a::indentation_folds_at_level(text, 4, 1);
    let inner = a::indentation_folds_at_level(text, 4, 2);
    assert_eq!((outer.len(), inner.len()), (1, 1));
    assert!(outer[0].start < inner[0].start && inner[0].end < outer[0].end);
    for width in [0, 1, 2, 8] {
        assert_eq!(a::indentation_folds_at_level(text, width, 2), inner);
    }
    for text in ["", "flat\ntext", "\r\n\t\r\n"] {
        assert!(a::indentation_folds_at_level(text, 0, 1).is_empty());
    }
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

#[test]
fn typing_brackets_and_quotes_pairs_wraps_and_steps_over() {
    use hedit::advanced::{inside_empty_pair, pair, Selection};
    let typed = |text: &str, at: usize, c: char| {
        pair(text, &[Selection::caret(at)], c, true).map(|b| (b.text, b.selections[0].head))
    };
    assert_eq!(typed("f", 1, '('), Some(("f()".into(), 2)));
    assert_eq!(typed("x = ;", 4, '['), Some(("x = [];".into(), 5)));
    // Next to a word, an opening bracket is inserted on its own.
    assert_eq!(typed("word", 0, '('), None);
    // A closer that is already there is stepped over.
    assert_eq!(typed("f()", 2, ')'), Some(("f()".into(), 3)));
    assert_eq!(typed("\"\"", 1, '"'), Some(("\"\"".into(), 2)));
    // Apostrophes after letters are not quotes.
    assert_eq!(typed("don", 3, '\''), None);
    assert_eq!(
        pair("it", &[Selection::caret(2)], '\'', false).map(|b| b.text),
        None
    );
    // A selection is wrapped and stays selected.
    let wrapped = pair("a b", &[Selection { anchor: 0, head: 1 }], '"', true).unwrap();
    assert_eq!(wrapped.text, "\"a\" b");
    assert_eq!(
        (wrapped.selections[0].anchor, wrapped.selections[0].head),
        (1, 2)
    );
    // Every cursor gets the same treatment.
    let many = pair(
        "a\nb\n",
        &[Selection::caret(1), Selection::caret(3)],
        '{',
        true,
    )
    .unwrap();
    assert_eq!(many.text, "a{}\nb{}\n");
    assert!(inside_empty_pair("f()", 2));
    assert!(!inside_empty_pair("f(x)", 2));
    assert!(!inside_empty_pair("()", 0));
}

#[test]
fn matching_brackets_skip_nested_pairs_and_other_kinds() {
    use hedit::advanced::matching_bracket;
    let text = "f(a[1], (b)) {}";
    assert_eq!(matching_bracket(text, 2, 1000), Some((1, 11)));
    assert_eq!(matching_bracket(text, 12, 1000), Some((11, 1)));
    assert_eq!(matching_bracket(text, 4, 1000), Some((3, 5)));
    assert_eq!(matching_bracket("(((", 1, 1000), None);
    assert_eq!(matching_bracket("abc", 1, 1000), None);
    assert_eq!(matching_bracket("λ(é)", 2, 1000), Some((1, 3)));
    // The scan is bounded.
    assert_eq!(matching_bracket("(aaaa)", 0, 3), None);
}

#[test]
fn found_indentation_is_none_without_indented_lines() {
    use hedit::advanced::found_indentation;
    assert_eq!(found_indentation("a\nb\n"), None);
    assert_eq!(found_indentation("a\n\tb\n"), Some((true, None)));
    assert_eq!(found_indentation("a\n  b\n    c\n"), Some((false, Some(2))));
}

#[test]
fn selection_expands_like_sublime() {
    use hedit::advanced::{expand_selection, expand_to_brackets, expand_to_indentation, Selection};
    let text = "f(a, \"hi there\", [x, y])\n";
    let range = |s: Selection| (s.start(), s.end());
    // Word, then quoted text, then quotes, then bracket contents, ...
    let mut s = Selection::caret(8);
    let mut steps = vec![];
    for _ in 0..6 {
        s = expand_selection(text, s);
        steps.push(range(s));
    }
    assert_eq!(steps, [(6, 8), (6, 14), (5, 15), (2, 23), (1, 24), (0, 24)]);
    // Brackets: inside, then including, then the next pair out.
    let s = expand_to_brackets(text, Selection::caret(18));
    assert_eq!(range(s), (18, 22));
    let s = expand_to_brackets(text, s);
    assert_eq!(range(s), (17, 23));
    let s = expand_to_brackets(text, s);
    assert_eq!(range(s), (2, 23));
    assert_eq!(
        range(expand_to_brackets("no brackets", Selection::caret(3))),
        (3, 3)
    );
    let code = "fn f() {\n    a();\n\n    b();\n}\n";
    assert_eq!(
        range(expand_to_indentation(code, Selection::caret(12))),
        (9, 27)
    );
}

#[test]
fn cursors_jump_split_and_skip() {
    use hedit::advanced::{jump_to_bracket, skip_occurrence, split_into_lines, Selection};
    let text = "if (a) { b(); }";
    let s = jump_to_bracket(text, Selection::caret(7));
    assert_eq!(s.head, 14);
    assert_eq!(jump_to_bracket(text, s).head, 8);
    let lines = split_into_lines(
        "one\ntwo\nthree",
        &[Selection {
            anchor: 1,
            head: 11,
        }],
    );
    let ranges: Vec<_> = lines.iter().map(|s| (s.start(), s.end())).collect();
    assert_eq!(ranges, [(1, 3), (4, 7), (8, 11)]);
    let text = "x y x y x";
    let skipped = skip_occurrence(
        text,
        &[
            Selection { anchor: 0, head: 1 },
            Selection { anchor: 4, head: 5 },
        ],
    );
    let ranges: Vec<_> = skipped.iter().map(|s| (s.start(), s.end())).collect();
    assert_eq!(ranges, [(0, 1), (8, 9)]);
}

#[test]
fn line_selections_add_carets_and_ranges_on_neighboring_lines() {
    let text = "abcdef\nab\nabcdef\n\tx";
    // Below a caret at column 4: line 2 is short, so the caret stops at its end.
    let below = a::add_line_selections(text, &[S::caret(4)], true, 4, None);
    assert_eq!(below, vec![S::caret(4), S::caret(9)]);
    // With the goal column kept, the next press returns to column 4.
    let again = a::add_line_selections(text, &below, true, 4, Some(4));
    assert_eq!(again, vec![S::caret(4), S::caret(9), S::caret(14)]);
    // Tabs count to the next tab stop: column 4 on "\tx" is after the tab.
    let last = a::add_line_selections(text, &[S::caret(14)], true, 4, None);
    assert_eq!(last.last(), Some(&S::caret(18)));
    // Nothing above the first line or below the last.
    assert_eq!(
        a::add_line_selections(text, &[S::caret(2)], false, 4, None),
        vec![S::caret(2)]
    );
    assert_eq!(
        a::add_line_selections(text, &[S::caret(19)], true, 4, None),
        vec![S::caret(19)]
    );
    // A selection within a line is repeated at the same columns.
    let range = S {
        anchor: 10,
        head: 12,
    };
    assert_eq!(
        a::add_line_selections(text, &[range], false, 4, None),
        vec![S { anchor: 7, head: 9 }, range]
    );
}

#[test]
fn several_cursors_each_keep_their_column_when_adding_lines() {
    let text = "abcdef\nabcdef\nabcdef\nabcdef";
    let added = a::add_line_selections(text, &[S::caret(1), S::caret(18)], true, 4, None);
    assert_eq!(
        added,
        vec![S::caret(1), S::caret(8), S::caret(18), S::caret(25)]
    );
}
