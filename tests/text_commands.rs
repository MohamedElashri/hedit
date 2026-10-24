use hedit::advanced::Selection as S;
use hedit::text_commands as t;

fn range(anchor: usize, head: usize) -> S {
    S { anchor, head }
}

#[test]
fn block_comments_wrap_and_unwrap_selections_and_lines() {
    let (open, close) = t::block_comment("c").unwrap();
    // A selection is wrapped and stays selected inside the markers.
    let batch = t::toggle_block_comment("int a = 1;", &[range(4, 9)], open, close);
    assert_eq!(batch.text, "int /*a = 1*/;");
    assert_eq!(batch.selections, [range(6, 11)]);
    // Selecting the comment, markers included, removes them.
    let batch = t::toggle_block_comment(&batch.text, &[range(4, 13)], open, close);
    assert_eq!(batch.text, "int a = 1;");
    // A cursor comments its line without the indentation, and toggles back.
    let batch = t::toggle_block_comment("  x();\n", &[S::caret(3)], open, close);
    assert_eq!(batch.text, "  /*x();*/\n");
    assert_eq!(batch.selections, [S::caret(5)]);
    let batch = t::toggle_block_comment(&batch.text, &batch.selections, open, close);
    assert_eq!(batch.text, "  x();\n");
    // Inside a comment spanning lines, the cursor removes that comment.
    let text = "a /* one\ntwo */ b";
    let batch = t::toggle_block_comment(text, &[S::caret(10)], open, close);
    assert_eq!(batch.text, "a one\ntwo b");
    // Spaces just inside the markers go too.
    let batch = t::toggle_block_comment("/* x */", &[S::caret(3)], open, close);
    assert_eq!(batch.text, "x");
    // A blank line gets an empty comment with the cursor inside.
    let batch = t::toggle_block_comment("\n", &[S::caret(0)], "<!--", "-->");
    assert_eq!(batch.text, "<!---->\n");
    assert_eq!(batch.selections, [S::caret(4)]);
    assert_eq!(t::block_comment("python"), None);
}

#[test]
fn wrapping_fills_paragraphs_and_keeps_prefixes() {
    let text = "one two three four five\nsix\n\nnext paragraph\n";
    let batch = t::wrap_lines(text, &[S::caret(2)], 10, 4, &[]);
    assert_eq!(
        batch.text,
        "one two\nthree four\nfive six\n\nnext paragraph\n"
    );
    // Comment markers and indentation repeat on each line.
    let markers = t::wrap_markers("rust", Some("//"));
    let text = "    // alpha beta gamma delta\n    // epsilon\nfn x() {}\n";
    let batch = t::wrap_lines(text, &[S::caret(8)], 23, 4, &markers);
    assert_eq!(
        batch.text,
        "    // alpha beta gamma\n    // delta epsilon\nfn x() {}\n"
    );
    // List items hang under their text, and CRLF stays CRLF.
    let text = "- aaa bbb ccc\r\n";
    let batch = t::wrap_lines(text, &[S::caret(0)], 8, 4, &[]);
    assert_eq!(batch.text, "- aaa\r\n  bbb\r\n  ccc\r\n");
    // A blank line is left alone; a long word gets its own line.
    assert_eq!(
        t::wrap_lines("\nx\n", &[S::caret(0)], 5, 4, &[]).text,
        "\nx\n"
    );
    assert_eq!(
        t::wrap_lines("a abcdefgh b", &[S::caret(0)], 4, 4, &[]).text,
        "a\nabcdefgh\nb"
    );
    // Only the selected lines are wrapped.
    let text = "a b c\nd e f\ng h i\n";
    let batch = t::wrap_lines(text, &[range(6, 9)], 3, 4, &[]);
    assert_eq!(batch.text, "a b c\nd e\nf\ng h i\n");
}

#[test]
fn transpose_swaps_characters_or_rotates_selections() {
    let batch = t::transpose("abc", &[S::caret(1)]);
    assert_eq!(batch.text, "bac");
    assert_eq!(batch.selections, [S::caret(2)]);
    // At the end of a line, the two characters before the cursor swap.
    let batch = t::transpose("ab\ncd", &[S::caret(2)]);
    assert_eq!(batch.text, "ba\ncd");
    assert_eq!(batch.selections, [S::caret(2)]);
    // Nothing happens at the start of a line.
    assert_eq!(t::transpose("ab\ncd", &[S::caret(3)]).text, "ab\ncd");
    // Selections trade their text.
    let batch = t::transpose("one, two", &[range(0, 3), range(5, 8)]);
    assert_eq!(batch.text, "two, one");
    assert_eq!(batch.selections, [range(0, 3), range(5, 8)]);
}

#[test]
fn case_changes_use_the_word_at_the_cursor() {
    let batch = t::change_case("hello WORLD it's", &[range(0, 16)], true);
    assert_eq!(batch.text, "Hello World It's");
    let batch = t::change_case("hello World", &[S::caret(8)], false);
    assert_eq!(batch.text, "hello wORLD");
    assert_eq!(batch.selections, [S::caret(8)]);
    let batch = t::change_case("ünïcode", &[S::caret(0)], true);
    assert_eq!(batch.text, "Ünïcode");
}

#[test]
fn deleting_to_a_line_edge_joins_lines_at_the_edge() {
    let text = "one two\nthree\r\nfour";
    assert_eq!(
        t::delete_to_line_edge(text, &[S::caret(4)], true).text,
        "one \nthree\r\nfour"
    );
    assert_eq!(
        t::delete_to_line_edge(text, &[S::caret(4)], false).text,
        "two\nthree\r\nfour"
    );
    // At the end of a CRLF line, both characters of the break go.
    assert_eq!(
        t::delete_to_line_edge(text, &[S::caret(13)], true).text,
        "one two\nthreefour"
    );
    assert_eq!(
        t::delete_to_line_edge(text, &[S::caret(15)], false).text,
        "one two\nthreefour"
    );
    // Each cursor deletes on its own line.
    let batch = t::delete_to_line_edge("ab\ncd", &[S::caret(1), S::caret(4)], true);
    assert_eq!(batch.text, "a\nc");
    assert_eq!(batch.selections, [S::caret(1), S::caret(3)]);
}

#[test]
fn reindent_follows_brackets_and_ignores_strings_and_comments() {
    let text = "fn main() {\nif x {\ny(\"}\");\n} else {\n// {\nz(a,\nb);\n}\n}\n";
    let all = range(0, text.chars().count());
    let batch = t::reindent(text, &[all], 4, false);
    assert_eq!(
        batch.text,
        "fn main() {\n    if x {\n        y(\"}\");\n    } else {\n        // {\n        z(a,\n            b);\n    }\n}\n"
    );
    // One line follows the line above it and keeps the cursor in its text.
    let text = "  if (a) {\nb;\n  }";
    let batch = t::reindent(text, &[S::caret(12)], 2, false);
    assert_eq!(batch.text, "  if (a) {\n    b;\n  }");
    assert_eq!(batch.selections, [S::caret(16)]);
    // Tabs, and blank lines lose their whitespace.
    let batch = t::reindent("{\n  \nx\n}", &[range(0, 9)], 4, true);
    assert_eq!(batch.text, "{\n\n\tx\n}");
    assert!(t::reindents("rust"));
    assert!(!t::reindents("python"));
}

#[test]
fn reindent_survives_lifetimes_unclosed_quotes_and_line_continuations() {
    let text = "fn f<'a>(x: &'a str) -> &'a str {\nx\n}\n";
    let end = text.chars().count();
    assert_eq!(
        t::reindent(text, &[range(0, end)], 4, false).text,
        "fn f<'a>(x: &'a str) -> &'a str {\n    x\n}\n"
    );
    let batch = t::reindent(text, &[S::caret(end)], 4, false);
    assert_eq!(
        batch.text, text,
        "a cursor on the empty last line changes nothing"
    );
    let text = "s = \"a\\\nb\";\n{\nx\n}";
    let end = text.chars().count();
    assert!(t::reindent(text, &[range(0, end)], 2, false)
        .text
        .ends_with("{\n  x\n}"));
    let text = "let c = '{';\nif x {\ny\n}";
    let end = text.chars().count();
    assert_eq!(
        t::reindent(text, &[range(0, end)], 2, false).text,
        "let c = '{';\nif x {\n  y\n}"
    );
}

#[test]
fn block_comment_markers_inside_strings_and_line_comments_are_text() {
    let text = "let g = \"src/*.rs\";\nfoo(); // a /* b\nbar();\n/* note */\n";
    let batch = t::toggle_block_comment(text, &[S::caret(38)], "/*", "*/");
    assert_eq!(
        batch.text,
        "let g = \"src/*.rs\";\nfoo(); // a /* b\n/*bar();*/\n/* note */\n"
    );
}

#[test]
fn cursors_that_change_nothing_are_kept() {
    let batch = t::delete_to_line_edge("ab\ncd", &[S::caret(1), S::caret(5)], true);
    assert_eq!(batch.text, "a\ncd");
    assert_eq!(batch.selections, [S::caret(1), S::caret(4)]);
    let batch = t::transpose("abc\ncd", &[S::caret(4), S::caret(2)]);
    assert_eq!(batch.text, "acb\ncd");
    assert_eq!(batch.selections, [S::caret(3), S::caret(4)]);
}

#[test]
fn list_items_wrap_as_separate_paragraphs() {
    assert_eq!(
        t::wrap_lines("- a\n- b\n", &[S::caret(1)], 80, 4, &[]).text,
        "- a\n- b\n"
    );
    assert_eq!(
        t::wrap_lines("- a b\n  c\n- d\n", &[S::caret(1)], 80, 4, &[]).text,
        "- a b c\n- d\n"
    );
}
