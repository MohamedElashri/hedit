use hedit::{advanced::Selection as S, text_commands as t};

#[test]
fn text_commands_keep_crlf_intact_at_either_end_key_position() {
    for at in [2, 3] {
        let cursor = [S::caret(at)];
        assert_eq!(t::transpose("ab\r\ncd", &cursor).text, "ba\r\ncd");
        assert_eq!(
            t::delete_to_line_edge("ab\r\ncd", &cursor, true).text,
            "abcd"
        );
        assert_eq!(
            t::delete_to_line_edge("ab\r\ncd", &cursor, false).text,
            "\r\ncd"
        );
    }
    assert_eq!(t::transpose("a\r\n", &[S::caret(2)]).text, "a\r\n");
    assert_eq!(
        t::toggle_block_comment("\r\n", &[S::caret(1)], "/*", "*/").text,
        "/**/\r\n"
    );
}
