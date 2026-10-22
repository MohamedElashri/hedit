use hedit::editing::{apply, Operation::*};

#[test]
fn line_selection_excludes_the_line_starting_at_the_selection_end() {
    let edit = apply("α\nβ\nγ", 0, 2, SelectLine, 4, false, None);
    assert_eq!((edit.anchor, edit.head), (0, 2));
    assert_eq!(edit.text, "α\nβ\nγ");
}

#[test]
fn duplication_handles_unicode_selections_and_unterminated_lines() {
    let edit = apply("α🦀z", 2, 0, Duplicate, 4, false, None);
    assert_eq!(edit.text, "α🦀α🦀z");
    assert_eq!((edit.anchor, edit.head), (2, 4));
    let edit = apply("first\r\nlast", 9, 9, Duplicate, 4, false, None);
    assert_eq!(edit.text, "first\r\nlast\r\nlast");
    assert_eq!(edit.head, 15);
}

#[test]
fn move_lines_preserves_crlf_and_final_newline_policy() {
    let source = "α\r\nβ\r\nγ";
    let moved = apply(source, 3, 3, MoveUp, 4, false, None);
    assert_eq!(moved.text, "β\r\nα\r\nγ");
    assert_eq!(moved.head, 0);
    let restored = apply(
        &moved.text,
        moved.anchor,
        moved.head,
        MoveDown,
        4,
        false,
        None,
    );
    assert_eq!(restored.text, source);
    let moved = apply("a\nb", 0, 0, MoveDown, 4, false, None);
    assert_eq!(moved.text, "b\na");
    assert_eq!(moved.head, 2);
    assert_eq!(apply("a\nb", 2, 2, MoveDown, 4, false, None).text, "a\nb");
}

#[test]
fn indent_conversion_uses_tab_stops_and_keeps_nonleading_tabs() {
    let source = " \tα\tβ\r\n  z\r\n";
    let spaces = apply(source, 0, 0, ToSpaces, 4, false, None);
    assert_eq!(spaces.text, "    α\tβ\r\n  z\r\n");
    let tabs = apply(&spaces.text, 0, 0, ToTabs, 4, false, None);
    assert_eq!(tabs.text, "\tα\tβ\r\n  z\r\n");
    let indented = apply("α\nβ\n", 0, 4, Indent, 2, false, None);
    assert_eq!(indented.text, "  α\n  β\n");
    assert_eq!(
        apply(&indented.text, 0, indented.head, Unindent, 2, false, None).text,
        "α\nβ\n"
    );
}

#[test]
fn comments_round_trip_with_indentation_unicode_and_blank_lines() {
    let source = "  α();\r\n\r\n\tβ();\r\n";
    let edit = apply(
        source,
        0,
        source.chars().count(),
        ToggleComment,
        4,
        false,
        Some("//"),
    );
    assert_eq!(edit.text, "  // α();\r\n\r\n\t// β();\r\n");
    let restored = apply(
        &edit.text,
        edit.anchor,
        edit.head,
        ToggleComment,
        4,
        false,
        Some("//"),
    );
    assert_eq!(restored.text, source);
}

#[test]
fn text_operations_keep_endings_and_unicode_cursor_ranges() {
    assert_eq!(
        apply("b\r\na\n", 0, 0, SortLines, 4, false, None).text,
        "a\r\nb\n"
    );
    assert_eq!(
        apply("a\r\nb", 0, 0, ReverseLines, 4, false, None).text,
        "b\r\na"
    );
    assert_eq!(
        apply(" α  \r\nβ\t\n", 0, 0, TrimWhitespace, 4, false, None).text,
        " α\r\nβ\n"
    );
    let upper = apply("xßλ🦀", 1, 3, Uppercase, 4, false, None);
    assert_eq!(upper.text, "xSSΛ🦀");
    assert_eq!((upper.anchor, upper.head), (1, 4));
    assert_eq!(
        apply("a\r\n  b\r\nc", 0, 0, JoinLines, 4, false, None).text,
        "a b\r\nc"
    );
}

#[test]
fn commands_never_split_unicode_or_leave_a_cursor_outside_the_text() {
    for text in ["", "α", "α\r\nβ\t\n", "one\ntwo", "\n\n"] {
        for a in 0..=text.chars().count() {
            for b in 0..=text.chars().count() {
                for operation in [
                    SelectLine,
                    Duplicate,
                    DeleteLine,
                    InsertBefore,
                    InsertAfter,
                    MoveUp,
                    MoveDown,
                    JoinLines,
                    Indent,
                    Unindent,
                    Uppercase,
                    Lowercase,
                    SortLines,
                    ReverseLines,
                    UniqueLines,
                    TrimWhitespace,
                    ToSpaces,
                    ToTabs,
                    ToggleComment,
                    ToLf,
                    ToCrlf,
                ] {
                    let edit = apply(text, a, b, operation, 4, false, Some("//"));
                    assert!(edit.anchor <= edit.text.chars().count(), "{operation:?}");
                    assert!(edit.head <= edit.text.chars().count(), "{operation:?}");
                }
            }
        }
    }
}

#[test]
fn inserted_lines_inherit_indentation_and_keep_crlf_and_unicode_cursors() {
    let edit = apply("  α();\r\n  β();", 3, 3, InsertBefore, 4, false, None);
    assert_eq!(edit.text, "  \r\n  α();\r\n  β();");
    assert_eq!((edit.anchor, edit.head), (2, 2));
    let edit = apply("  α();\r\n  β();", 3, 3, InsertAfter, 4, false, None);
    assert_eq!(edit.text, "  α();\r\n  \r\n  β();");
    assert_eq!(edit.head, 10);
    let edit = apply("\tβ();", 2, 2, InsertAfter, 4, false, None);
    assert_eq!(edit.text, "\tβ();\n\t");
    assert_eq!(edit.head, 7);
}

#[test]
fn duplicate_removal_keeps_order_selection_scope_and_final_newline_policy() {
    assert_eq!(
        apply("α\r\nβ\r\nα", 0, 0, UniqueLines, 4, false, None).text,
        "α\r\nβ"
    );
    assert_eq!(
        apply("α\nβ\nα\n", 0, 0, UniqueLines, 4, false, None).text,
        "α\nβ\n"
    );
    assert_eq!(
        apply("a\na\nb\nb\n", 0, 4, UniqueLines, 4, false, None).text,
        "a\nb\nb\n"
    );
}

#[test]
fn line_ending_conversion_maps_backward_selections_and_preserves_bare_carriage_returns() {
    let source = "α\r\nβ\n🦀\r";
    let edit = apply(source, 6, 3, ToCrlf, 4, false, None);
    assert_eq!(edit.text, "α\r\nβ\r\n🦀\r");
    assert_eq!((edit.anchor, edit.head), (7, 3));
    let edit = apply(source, 6, 3, ToLf, 4, false, None);
    assert_eq!(edit.text, "α\nβ\n🦀\r");
    assert_eq!((edit.anchor, edit.head), (5, 2));
    assert_eq!(apply("\r\n", 1, 1, ToLf, 4, false, None).head, 0);
}
