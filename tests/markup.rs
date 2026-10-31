use hedit::{
    advanced::{self, Selection as S},
    markup,
};
use std::sync::atomic::AtomicBool;

fn span(text: &str, needle: &str) -> S {
    let begin = text.find(needle).unwrap();
    let anchor = text[..begin].chars().count();
    S {
        anchor,
        head: anchor + needle.chars().count(),
    }
}
fn expand(text: &str, selection: S, html: bool) -> S {
    markup::expand(text, &[selection], html, &AtomicBool::new(false)).unwrap()[0]
}
fn selected(text: &str, selection: S) -> &str {
    &text[advanced::byte(text, selection.start())..advanced::byte(text, selection.end())]
}

#[test]
fn markup_expands_contents_element_parent_contents_and_parent_element() {
    let text = "<outer> before <inner>مرحبا 🦀</inner> after </outer>";
    let mut selection = S::caret(span(text, "مرحبا").anchor + 2);
    for expected in [
        "مرحبا 🦀",
        "<inner>مرحبا 🦀</inner>",
        " before <inner>مرحبا 🦀</inner> after ",
        text,
    ] {
        selection = expand(text, selection, false);
        assert_eq!(selected(text, selection), expected);
    }
    assert_eq!(expand(text, selection, false), selection);
}

#[test]
fn markup_handles_nested_same_names_reversed_ranges_empty_and_sibling_elements() {
    let text = "<x><x>α</x><x>β</x><empty></empty></x>";
    let reversed = S {
        anchor: span(text, "α").head,
        head: span(text, "α").anchor,
    };
    let result = expand(text, reversed, false);
    assert!(result.anchor > result.head);
    assert_eq!(selected(text, result), "<x>α</x>");
    let both = S {
        anchor: span(text, "α").anchor,
        head: span(text, "β").head,
    };
    assert_eq!(
        selected(text, expand(text, both, false)),
        "<x>α</x><x>β</x><empty></empty>"
    );
    let empty = S::caret(span(text, "</empty>").anchor);
    assert_eq!(
        selected(text, expand(text, empty, false)),
        "<empty></empty>"
    );
    // The boundary after one element belongs to the next one.
    let next = S::caret(span(text, "<x>β</x>").anchor);
    assert_eq!(selected(text, expand(text, next, false)), "<x>β</x>");
}

#[test]
fn markup_respects_attribute_quotes_and_xml_namespaces() {
    let text = "<ns:root a='x > y <fake>'>\r\n<ns:item data=\"<x>\"/>\r\n</ns:root>";
    let attr = S::caret(span(text, "fake").anchor);
    assert_eq!(selected(text, expand(text, attr, false)), text);
    let leaf = S::caret(span(text, "ns:item").anchor + 3);
    assert_eq!(
        selected(text, expand(text, leaf, false)),
        "<ns:item data=\"<x>\"/>"
    );
}

#[test]
fn markup_skips_comments_cdata_processing_instructions_and_doctype_subsets() {
    let text = "<?xml version='1.0'?><!DOCTYPE root [<!ENTITY test '<fake/>'>]><root><!-- <fake> --> <![CDATA[<fake>]]><real>ok</real></root>";
    let real = S::caret(span(text, "ok").anchor);
    assert_eq!(selected(text, expand(text, real, false)), "ok");
    let comment = S::caret(text[..text.find("<!--").unwrap() + 7].chars().count());
    assert_eq!(
        selected(text, expand(text, comment, false)),
        "<!-- <fake> --> <![CDATA[<fake>]]><real>ok</real>"
    );
}

#[test]
fn markup_html_handles_void_elements_case_and_raw_text() {
    let text = "<DIV><BR><SPAN>x</span><img src='a'></DIV>";
    for (needle, expected) in [
        ("BR", "<BR>"),
        ("SPAN", "<SPAN>x</span>"),
        ("img", "<img src='a'>"),
    ] {
        assert_eq!(
            selected(
                text,
                expand(text, S::caret(span(text, needle).anchor), true)
            ),
            expected
        );
    }
    for name in ["script", "style", "textarea", "title"] {
        let text = format!("<div><{name}>\"<fake>inside</fake>\"</{name}></div>");
        let result = expand(&text, S::caret(span(&text, "inside").anchor), true);
        assert_eq!(selected(&text, result), "\"<fake>inside</fake>\"");
    }
    let xml = "<Root>value</root>";
    let selection = S::caret(span(xml, "value").anchor);
    assert_eq!(expand(xml, selection, false), selection);
}

#[test]
fn markup_raw_text_skips_long_runs_of_incomplete_fake_tags() {
    let contents = "<fake attr=' </other bad=' ".repeat(2000);
    let text = format!("<script>{contents}</SCRIPT>");
    let result = expand(&text, S::caret(20), true);
    assert_eq!(selected(&text, result), contents);
}

#[test]
fn markup_malformed_or_unmatched_tags_do_not_invent_elements() {
    for text in [
        "",
        "plain α text",
        "<root>unclosed",
        "<a><b></a>",
        "<root attr=\"<fake>x</fake>",
        "<!-- <x>bad</x>",
        "<![CDATA[<x>bad</x>",
        "<?instruction <x>bad</x>",
        "<a></b>",
    ] {
        for index in 0..=text.chars().count() {
            let selection = S::caret(index);
            assert_eq!(
                expand(text, selection, false),
                selection,
                "{text:?} at {index}"
            );
        }
    }
}

#[test]
fn markup_multiple_cursors_preserve_unicode_ranges_and_merge_overlaps() {
    let text = "<r><a>سَلَام</a>\r\n<b>e\u{301}🦀</b></r>";
    let selections = [
        S::caret(span(text, "سَلَام").anchor + 2),
        S::caret(span(text, "e\u{301}").anchor + 1),
    ];
    let result = markup::expand(text, &selections, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(selected(text, result[0]), "سَلَام");
    assert_eq!(selected(text, result[1]), "e\u{301}🦀");
    let merged = markup::expand(
        text,
        &[S::caret(0), selections[0]],
        false,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(merged, [span(text, text)]);
}

#[test]
fn markup_limits_cancellation_and_out_of_range_selections_are_safe() {
    assert!(markup::expand(
        "<x>value</x>",
        &[S::caret(4)],
        false,
        &AtomicBool::new(true)
    )
    .is_err());
    assert!(markup::expand(
        &"a".repeat(8 * 1024 * 1024 + 1),
        &[],
        false,
        &AtomicBool::new(false)
    )
    .is_err());
    assert_eq!(expand("<x/>", S::caret(usize::MAX), false), S::caret(4));
}

#[test]
fn markup_generated_malformed_inputs_always_return_bounded_containing_ranges() {
    let alphabet: Vec<_> = "<>/'\"=!?-[]:ab α\n🦀".chars().collect();
    let mut seed = 17u64;
    for _ in 0..300 {
        let mut text = String::new();
        for _ in 0..70 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            text.push(alphabet[(seed >> 32) as usize % alphabet.len()]);
        }
        let length = text.chars().count();
        for html in [false, true] {
            for at in [0, 10, 35, length] {
                let result = expand(&text, S::caret(at), html);
                assert!(result.start() <= at && at <= result.end() && result.end() <= length);
            }
        }
    }
}

#[test]
fn markup_xml_names_allow_combining_marks_middle_dots_and_supplementary_characters() {
    for name in ["e\u{301}", "مَلَف", "a\u{b7}b", "a\u{203f}b", "\u{10000}"] {
        let text = format!("<{name}>value</{name}>");
        let selection = S::caret(span(&text, "value").anchor + 1);
        assert_eq!(
            selected(&text, expand(&text, selection, false)),
            "value",
            "{name}"
        );
    }
}

#[test]
fn markup_html_trailing_slash_in_unquoted_value_does_not_end_the_element() {
    let text = "<div data=assets/>value</div>";
    let selection = S::caret(span(text, "value").anchor + 1);
    assert_eq!(selected(text, expand(text, selection, true)), "value");
}

#[test]
fn markup_attribute_state_distinguishes_values_from_self_closing_slashes() {
    for attributes in [
        "data=assets/",
        "data= assets/",
        "data = assets/",
        "data=assets/ other=more/",
        "data='assets/'",
        "data=\"a>b/\"",
    ] {
        let text = format!("<node {attributes}>value</node>");
        let at = S::caret(span(&text, "value").anchor + 2);
        assert_eq!(selected(&text, expand(&text, at, true)), "value", "{text}");
    }
    for text in [
        "<node />",
        "<node data='assets/'/>",
        "<node data=\"a>b/\" />",
    ] {
        let at = S::caret(2);
        assert_eq!(selected(text, expand(text, at, false)), text);
    }
}

#[test]
fn markup_expands_one_thousand_separate_cursors_without_losing_any() {
    let mut text = String::from("<root>");
    let mut cursors = Vec::new();
    for index in 0..advanced::MAX_SELECTIONS {
        text.push_str("<item>");
        cursors.push(S::caret(text.chars().count() + 1));
        text.push_str(&format!("値{index}</item>"));
    }
    text.push_str("</root>");
    let expanded = markup::expand(&text, &cursors, false, &AtomicBool::new(false)).unwrap();
    assert_eq!(expanded.len(), advanced::MAX_SELECTIONS);
    for (index, selection) in expanded.into_iter().enumerate() {
        assert_eq!(selected(&text, selection), format!("値{index}"));
    }
}
