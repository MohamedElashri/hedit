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
