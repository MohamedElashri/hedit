use hedit::folder::fuzzy_match;

#[test]
fn fuzzy_matching_keeps_character_positions_when_lowercase_expands() {
    for (text, query, positions) in [
        ("İ", "İ", vec![0]),
        ("ΟΣ", "ΟΣ", vec![0, 1]),
        ("İşlem", "İş", vec![0, 1]),
        ("Open İşlem", "İş", vec![5, 6]),
        ("İşlem", "i\u{307}ş", vec![0, 1]),
        ("İşlem", "i", vec![0]),
    ] {
        let (_, found) = fuzzy_match(text, query).expect("a word must match its own spelling");
        assert_eq!(found, positions, "{text}: {query}");
    }
    assert!(fuzzy_match("İşlem", "İx").is_none());
}
