use hedit::spelling;
use hedit::syntax::{grammar::Grammar, Language};
use std::collections::HashSet;

fn wrong(text: &str, language: Language, known: &[&str]) -> Vec<String> {
    let mut grammar = Grammar::default();
    let regions = spelling::regions(text, language, &mut grammar);
    let known: HashSet<String> = known.iter().map(|w| w.to_string()).collect();
    spelling::misspelled(text, &regions, &known)
        .into_iter()
        .map(|r| text.chars().skip(r.start).take(r.end - r.start).collect())
        .collect()
}

#[test]
fn prose_is_checked_word_by_word_with_char_offsets() {
    assert_eq!(
        wrong(
            "Thsi is a tset, don't wrory — λ teh.",
            Language::PlainText,
            &[]
        ),
        ["Thsi", "tset", "wrory", "teh"]
    );
    // Known words, numbers, code-like words, abbreviations, and URLs pass.
    assert!(wrong(
        "hedit uses GPUs and parseJson in snake_case 3rd https://exmaple.com/pathz",
        Language::PlainText,
        &["hedit"]
    )
    .is_empty());
    assert!(wrong("مرحبا بالعالم and Ελληνικά", Language::PlainText, &[]).is_empty());
    let text = "λλ wrold";
    let mut grammar = Grammar::default();
    let regions = spelling::regions(text, Language::PlainText, &mut grammar);
    let found = spelling::misspelled(text, &regions, &HashSet::new());
    assert_eq!((found.len(), found[0].clone()), (1, 3..8));
    assert_eq!(spelling::suggestions("wrold")[0], "world");
}

#[test]
fn code_is_checked_only_in_comments_and_strings() {
    let code = "fn wrold() {\n    // a mispeled comment\n    let s = \"strng here\";\n}\n";
    assert_eq!(wrong(code, Language::Rust, &[]), ["mispeled", "strng"]);
    let markdown = "Some txet with `cdoe` inside.\n\n```\nmore cdoe\n```\n";
    assert_eq!(wrong(markdown, Language::Markdown, &[]), ["txet"]);
    assert!(wrong("2024-01-01 ERORR failed", Language::Log, &[]).is_empty());
    // LaTeX commands are skipped; their arguments are text.
    assert_eq!(
        wrong("\\sectoin{Introdution}", Language::Latex, &[]),
        ["Introdution"]
    );
}
