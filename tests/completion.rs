use hedit::completion as c;

#[test]
fn index_keeps_distinct_words_and_rank_prefers_prefixes() {
    let words = c::index(&[
        "fn render_frame(frame: &Frame) { frame_count += 1; }",
        "Frame rendering 42abc ab",
    ]);
    let texts: Vec<_> = words.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(
        texts,
        ["render_frame", "frame", "Frame", "frame_count", "rendering"],
        "short words and words starting with digits are skipped"
    );
    assert_eq!(
        c::rank(&words, "fra", 10),
        ["frame", "frame_count", "Frame"]
    );
    // The typed word itself is not offered, and letters in order match too.
    assert_eq!(c::rank(&words, "frame", 10), ["frame_count", "Frame"]);
    assert_eq!(c::rank(&words, "rfr", 10), ["render_frame"]);
    assert_eq!(c::rank(&words, "fra", 1), ["frame"]);
    assert!(c::rank(&words, "", 10).is_empty());
}

#[test]
fn typed_word_ends_at_the_cursor_in_characters() {
    assert_eq!(c::typed_word("let λamb = x", 8), (4, "λamb".into()));
    assert_eq!(c::typed_word("a + ", 4), (4, String::new()));
    assert_eq!(c::typed_word("ab", 99), (0, "ab".into()));
}

#[test]
fn index_stops_at_its_limits() {
    let many: String = (0..c::MAX_WORDS + 10)
        .map(|i| format!("w{i:07} "))
        .collect();
    assert_eq!(c::index(&[many]).len(), c::MAX_WORDS);
    let mut large = "x".repeat(c::MAX_TEXT - 2);
    large.push_str(" é tail_word");
    let words = c::index(&[large]);
    assert!(
        words.iter().all(|w| w.text != "tail_word"),
        "text past the limit is skipped"
    );
}

#[test]
fn merging_keeps_the_first_of_repeated_words() {
    let a = c::index(&["alpha beta"]);
    let b = c::index(&["beta gamma"]);
    let merged = c::merge(&[&a, &b]);
    let texts: Vec<_> = merged.iter().map(|w| w.text.as_str()).collect();
    assert_eq!(texts, ["alpha", "beta", "gamma"]);
}

#[test]
fn bounded_ranking_matches_the_original_full_sort() {
    let text = (0..12_000)
        .map(|i| format!("value_{i} Frame{i} αβγ{i} render_frame_{i}\n"))
        .collect::<String>();
    let words = c::index(&[&text]);
    for typed in [
        "v",
        "value_1",
        "F",
        "fr",
        "rfr",
        "αβ",
        "α9",
        "",
        "value_999999999",
    ] {
        for limit in [0, 1, 12, 100] {
            let lower = typed.to_lowercase();
            let mut expected: Vec<_> = words
                .iter()
                .filter_map(|w| {
                    if typed.is_empty() || w.text == typed {
                        return None;
                    }
                    let word_lower = w.text.to_lowercase();
                    let score = if w.text.starts_with(typed) {
                        0
                    } else if word_lower.starts_with(&lower) {
                        1
                    } else {
                        let mut letters = word_lower.chars();
                        let mut wanted = lower.chars();
                        if letters.next() != wanted.next() {
                            return None;
                        }
                        for ch in wanted {
                            if !letters.any(|c| c == ch) {
                                return None;
                            }
                        }
                        2
                    };
                    Some((score, w.text.len(), w.text.as_str()))
                })
                .collect();
            expected.sort_unstable();
            expected.truncate(limit);
            assert_eq!(
                c::rank(&words, typed, limit),
                expected.into_iter().map(|(_, _, s)| s).collect::<Vec<_>>()
            );
        }
    }
}
#[test]
fn indexing_and_ranking_cancel_cooperatively_including_a_long_token() {
    use std::cell::Cell;
    let calls = Cell::new(0);
    let long = "界".repeat(c::MAX_TEXT);
    assert!(c::index_cancellable(&[long], || {
        calls.set(calls.get() + 1);
        calls.get() == 2
    })
    .is_none());
    assert_eq!(calls.get(), 2);
    let words = c::index(&[&(0..2000).map(|i| format!("value_{i} ")).collect::<String>()]);
    calls.set(0);
    assert!(c::rank_cancellable(&words, "v", 12, || {
        calls.set(calls.get() + 1);
        calls.get() == 3
    })
    .is_none());
    assert_eq!(calls.get(), 3);
}
#[test]
fn word_storage_obeys_its_byte_budget() {
    let text = (0..70_000)
        .map(|i| format!("w{i:06}{} ", "x".repeat(57)))
        .collect::<String>();
    let words = c::index(&[text]);
    let bytes: usize = words
        .iter()
        .map(|w| std::mem::size_of_val(w) + w.text.capacity() + w.text.to_lowercase().capacity())
        .sum();
    assert!(bytes <= c::MAX_INDEX_BYTES);
    assert!(words.len() < 60_000);
}
#[test]
fn bounded_prefix_matches_unicode_reference_and_rejects_overlong_tokens() {
    let text = "αβγ🦀\r\n\tשלום alpha_1";
    for (cursor, byte) in text
        .char_indices()
        .map(|(i, _)| i)
        .chain([text.len()])
        .enumerate()
    {
        assert_eq!(
            c::typed_word_at(text, byte, cursor),
            Some(c::typed_word(text, cursor))
        );
    }
    assert!(c::typed_word_at("🦀", 1, 0).is_none());
    let long = "界".repeat(1_000_000);
    assert!(c::typed_word_at(&long, long.len(), 1_000_000).is_none());
}
