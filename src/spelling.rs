//! Spell checking with the bundled en_US Hunspell dictionary (SCOWL; see
//! `assets/dictionaries/en_US-LICENSE.txt`): which parts of a file are prose,
//! and which words in them are misspelled.
use crate::syntax::{grammar::Grammar, Language};
use crate::theme::{Rgb, Theme};
use std::{collections::HashSet, ops::Range, sync::OnceLock};

const AFF: &str = include_str!("../assets/dictionaries/en_US.aff");
const DIC: &str = include_str!("../assets/dictionaries/en_US.dic");

/// The dictionary, parsed on first use.
pub fn dictionary() -> &'static spellbook::Dictionary {
    static DICTIONARY: OnceLock<spellbook::Dictionary> = OnceLock::new();
    DICTIONARY.get_or_init(|| spellbook::Dictionary::new(AFF, DIC).expect("bundled dictionary"))
}

/// Colors that mark comments and strings when highlighting for spelling.
const COMMENT: Rgb = Rgb::new(1, 2, 3);
const STRING: Rgb = Rgb::new(3, 2, 1);

fn marking_theme() -> Theme {
    Theme {
        comment: COMMENT,
        string: STRING,
        ..Theme::default()
    }
}

/// Which text a syntax wants checked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prose {
    /// All of it.
    All,
    /// All but code spans and blocks.
    NotCode,
    /// Comments and strings.
    Comments,
    /// None, as in logs.
    Nothing,
}

fn prose(language: Language) -> Prose {
    match language {
        Language::PlainText | Language::Latex => Prose::All,
        Language::Markdown => Prose::NotCode,
        Language::Log | Language::Csv | Language::Diff => Prose::Nothing,
        _ => Prose::Comments,
    }
}

/// Byte ranges of `text` to check, using `grammar` to find comments and
/// strings (its state is kept, so later calls only re-highlight changes).
pub fn regions(text: &str, language: Language, grammar: &mut Grammar) -> Vec<Range<usize>> {
    let kind = prose(language);
    match kind {
        Prose::All => return std::iter::once(0..text.len()).collect(),
        Prose::Nothing => return vec![],
        Prose::NotCode | Prose::Comments => {}
    }
    let theme = marking_theme();
    let runs: Vec<(usize, eframe::egui::Color32)> = match grammar.highlight(text, language, &theme)
    {
        Some(runs) => runs.to_vec(),
        None => crate::syntax::highlight_language(text, 12.0, f32::INFINITY, language, &theme)
            .sections
            .iter()
            .map(|s| (s.byte_range.len(), s.format.color))
            .collect(),
    };
    let comment = eframe::egui::Color32::from_rgb(1, 2, 3);
    let string = eframe::egui::Color32::from_rgb(3, 2, 1);
    let mut regions: Vec<Range<usize>> = vec![];
    let mut at = 0;
    for (length, color) in runs {
        let wanted = match kind {
            Prose::NotCode => color != string,
            _ => color == comment || color == string,
        };
        let end = (at + length).min(text.len());
        if wanted {
            match regions.last_mut() {
                Some(last) if last.end == at => last.end = end,
                _ => regions.push(at..end),
            }
        }
        at = end;
    }
    regions
}

/// Whether a word is worth checking: not code-like, not a number, not an
/// abbreviation in capitals.
fn checkable(word: &str) -> bool {
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 2 || word.chars().any(|c| c.is_ascii_digit() || c == '_') {
        return false;
    }
    // The dictionary is English: words in other scripts, such as Arabic or
    // Greek, are left alone.
    let latin = |c: char| c.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&c);
    if word.chars().any(|c| c.is_alphabetic() && !latin(c)) {
        return false;
    }
    let mut chars = word.chars();
    let first = chars.next().unwrap_or(' ');
    let rest: Vec<char> = chars.collect();
    // camelCase, PascalCase with more capitals, and ALLCAPS look like code.
    if rest.iter().any(|c| c.is_uppercase()) {
        return false;
    }
    first.is_alphabetic()
}

/// Character ranges of misspelled words inside `regions` (byte ranges).
/// `known` words are accepted as spelled, ignoring case.
pub fn misspelled(
    text: &str,
    regions: &[Range<usize>],
    known: &HashSet<String>,
) -> Vec<Range<usize>> {
    let dictionary = dictionary();
    let mut found = vec![];
    // Walk the text once, counting characters, so offsets come out in chars.
    let mut chars = 0;
    let mut position = 0;
    for region in regions {
        let mut region = region.clone();
        region.end = region.end.min(text.len());
        if region.start < position {
            region.start = position;
        }
        if region.start >= region.end || !text.is_char_boundary(region.start) {
            continue;
        }
        chars += text[position..region.start].chars().count();
        let part = &text[region.clone()];
        let mut word_start: Option<(usize, usize)> = None;
        let mut char_index = chars;
        let mut iter = part.char_indices().peekable();
        while let Some((i, c)) = iter.next() {
            let inside = c.is_alphanumeric()
                || c == '_'
                || ((c == '\'' || c == '’')
                    && word_start.is_some()
                    && iter.peek().is_some_and(|(_, n)| n.is_alphabetic()));
            match (inside, word_start) {
                (true, None) => word_start = Some((i, char_index)),
                (false, Some((start, start_char))) => {
                    check(
                        part,
                        start..i,
                        start_char,
                        char_index,
                        dictionary,
                        known,
                        &mut found,
                    );
                    word_start = None;
                }
                _ => {}
            }
            char_index += 1;
        }
        if let Some((start, start_char)) = word_start {
            check(
                part,
                start..part.len(),
                start_char,
                char_index,
                dictionary,
                known,
                &mut found,
            );
        }
        chars = char_index;
        position = region.end;
    }
    found
}

fn check(
    part: &str,
    bytes: Range<usize>,
    start: usize,
    end: usize,
    dictionary: &spellbook::Dictionary,
    known: &HashSet<String>,
    found: &mut Vec<Range<usize>>,
) {
    let word = &part[bytes.clone()];
    // Skip words glued to URLs, paths, and escapes, such as \n or a/b.
    let before = part[..bytes.start].chars().last();
    let after = part[bytes.end..].chars().next();
    if matches!(before, Some('\\' | '/' | '.' | '@' | '#' | '$' | '%' | '&'))
        || matches!(after, Some('/' | '@'))
        || part[bytes.end..].starts_with("://")
    {
        return;
    }
    let word = word.trim_end_matches(['\'', '’']);
    if !checkable(word) || known.contains(&word.to_lowercase()) || dictionary.check(word) {
        return;
    }
    found.push(start..end);
}

/// Suggested spellings for a word, best first.
pub fn suggestions(word: &str) -> Vec<String> {
    let mut out = vec![];
    dictionary().suggest(word, &mut out);
    out.truncate(8);
    out
}
