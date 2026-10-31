use hedit::{advanced::Selection as S, text_commands::change_case};

#[test]
fn case_commands_map_cursors_across_expanding_unicode_characters() {
    for (text, at, title, expected, cursor) in [
        ("straße", 6, false, "STRASSE", 7),
        ("straße", 5, false, "STRASSE", 6),
        ("ßeta", 1, true, "SSeta", 2),
        ("İSTANBUL", 1, false, "i\u{307}stanbul", 2),
    ] {
        let batch = change_case(text, &[S::caret(at)], title);
        assert_eq!(batch.text, expected);
        assert_eq!(batch.selections, [S::caret(cursor)], "{text} at {at}");
    }
}
