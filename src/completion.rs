//! Words offered while typing: an index built away from the editor, and a
//! ranking of its words against what was typed so far.

/// The most distinct words kept, so ranking stays fast on every keystroke.
pub const MAX_WORDS: usize = 100_000;
/// Bytes read from each file; larger files are indexed only up to here.
pub const MAX_TEXT: usize = 4 * 1024 * 1024;

pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// One indexed word, with its lowercase form for matching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    lower: String,
}

/// Distinct words of 3 to 64 characters from `texts`, in the order given,
/// up to `MAX_WORDS`.
pub fn index<S: AsRef<str>>(texts: &[S]) -> Vec<Word> {
    let mut seen = std::collections::HashSet::new();
    let mut words = vec![];
    for text in texts {
        let text = text.as_ref();
        let mut end = text.len().min(MAX_TEXT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        for word in text[..end].split(|c: char| !is_word_char(c)) {
            let length = word.chars().count();
            if !(3..=64).contains(&length)
                || word.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                continue;
            }
            if seen.insert(word) {
                words.push(Word {
                    text: word.to_owned(),
                    lower: word.to_lowercase(),
                });
                if words.len() == MAX_WORDS {
                    return words;
                }
            }
        }
    }
    words
}

/// Words from several indexes, the first ones kept when they repeat, up to
/// `MAX_WORDS`.
pub fn merge(lists: &[&[Word]]) -> Vec<Word> {
    let mut seen = std::collections::HashSet::new();
    let mut words = vec![];
    for word in lists.iter().flat_map(|list| list.iter()) {
        if seen.insert(word.text.as_str()) {
            words.push(word.clone());
            if words.len() == MAX_WORDS {
                break;
            }
        }
    }
    words
}

/// How well `word` matches `typed`: lower is better, None is no match.
/// Exact-case prefixes come first, then any-case prefixes, then words that
/// contain the typed letters in order, starting with the same letter.
fn score(word: &Word, typed: &str, typed_lower: &str) -> Option<u8> {
    if word.text == typed {
        return None;
    }
    if word.text.starts_with(typed) {
        return Some(0);
    }
    if word.lower.starts_with(typed_lower) {
        return Some(1);
    }
    let mut letters = word.lower.chars();
    let mut wanted = typed_lower.chars();
    let first = wanted.next()?;
    if letters.next() != Some(first) {
        return None;
    }
    let mut next = wanted.next();
    for c in letters {
        if Some(c) == next {
            next = wanted.next();
        }
    }
    next.is_none().then_some(2)
}

/// The best `limit` words for what was typed, best first.
pub fn rank<'a>(words: &'a [Word], typed: &str, limit: usize) -> Vec<&'a str> {
    if typed.is_empty() {
        return vec![];
    }
    let lower = typed.to_lowercase();
    let mut found: Vec<(u8, usize, &str)> = words
        .iter()
        .filter_map(|w| score(w, typed, &lower).map(|s| (s, w.text.len(), w.text.as_str())))
        .collect();
    if found.len() > limit {
        found.select_nth_unstable(limit);
        found.truncate(limit);
    }
    found.sort_unstable();
    found.into_iter().map(|(_, _, w)| w).collect()
}

/// The start of the word that ends at `cursor` (a character index), and that
/// word.
pub fn typed_word(text: &str, cursor: usize) -> (usize, String) {
    let end = text
        .char_indices()
        .nth(cursor)
        .map_or(text.len(), |(i, _)| i);
    let mut word: Vec<char> = text[..end]
        .chars()
        .rev()
        .take_while(|c| is_word_char(*c))
        .collect();
    word.reverse();
    let cursor = cursor.min(text[..end].chars().count());
    (cursor - word.len(), word.into_iter().collect())
}
