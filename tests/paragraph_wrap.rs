use hedit::{advanced::Selection as S, text_commands::wrap_lines};

#[test]
fn wrapping_a_selection_preserves_paragraphs_and_list_items() {
    for (text, width, expected) in [
        (
            "alpha beta gamma\n\ndelta epsilon zeta\n",
            10,
            "alpha beta\ngamma\n\ndelta\nepsilon\nzeta\n",
        ),
        (
            "- aaa bbb ccc\n- ddd eee fff\n",
            8,
            "- aaa\n  bbb\n  ccc\n- ddd\n  eee\n  fff\n",
        ),
        (
            "// aaa bbb\n# ccc ddd\n",
            7,
            "// aaa\n// bbb\n# ccc\n# ddd\n",
        ),
    ] {
        let batch = wrap_lines(
            text,
            &[S {
                anchor: 0,
                head: text.chars().count(),
            }],
            width,
            4,
            &["//", "#"],
        );
        assert_eq!(batch.text, expected);
    }
}

#[test]
fn wrapping_an_unterminated_crlf_paragraph_keeps_crlf() {
    let text = "aaa bbb\r\nccc ddd";
    assert_eq!(
        wrap_lines(text, &[S::caret(0)], 5, 4, &[]).text,
        "aaa\r\nbbb\r\nccc\r\nddd"
    );
}
