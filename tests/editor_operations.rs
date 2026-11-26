use hedit::{advanced, completion, editing, intelligence, text_commands};
use std::path::Path;

#[test]
fn rust_foreign_abi_function_appears_in_symbol_index() {
    let text = "pub extern \"C\" fn sample() {}\nfn control() {}\n";
    let names: Vec<_> = intelligence::symbols(text, Path::new("lib.rs"))
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, ["sample", "control"]);
}

#[test]
fn automatic_indentation_recognizes_three_spaces() {
    let source = "outer\n   inner\n      nested\n   inner\nouter\n";
    assert_eq!(advanced::found_indentation(source), Some((false, Some(3))));
}

#[test]
fn completion_does_not_index_half_a_word_at_the_byte_cap() {
    let text = format!(
        "{}{}",
        " ".repeat(completion::MAX_TEXT - 3),
        "long_identifier"
    );
    let indexed = completion::index_cancellable(&[text], || false).unwrap();
    assert!(indexed.limited);
    assert!(indexed.words.is_empty(), "{:?}", indexed.words);
}

#[test]
fn toggling_block_comment_twice_restores_a_selection() {
    let initial = "value";
    let selection = advanced::Selection {
        anchor: 0,
        head: initial.len(),
    };
    let commented = text_commands::toggle_block_comment(initial, &[selection], "/*", "*/");
    assert_eq!(commented.text, "/*value*/");
    let restored =
        text_commands::toggle_block_comment(&commented.text, &commented.selections, "/*", "*/");
    assert_eq!(restored.text, initial);
}

#[test]
fn an_escaped_quote_does_not_pair_automatically() {
    let source = "\"a\\";
    let caret = advanced::Selection::caret(source.chars().count());
    assert!(advanced::pair(source, &[caret], '"', true).is_none());
}

#[test]
fn duplicating_multiple_caret_lines_keeps_a_separator_at_eof() {
    let source = "first\nlast";
    let result = advanced::transform(
        source,
        &[advanced::Selection::caret(1), advanced::Selection::caret(7)],
        editing::Operation::Duplicate,
        4,
        false,
        None,
    );
    assert_eq!(result.text, "first\nfirst\nlast\nlast");
}
