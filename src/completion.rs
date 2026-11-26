//! Words offered while typing: an index built away from the editor, and a
//! ranking of its words against what was typed so far.

/// The most distinct words kept, so ranking stays fast on every keystroke.
pub const MAX_WORDS: usize = 100_000;
/// Bytes read from each file; larger files are indexed only up to here.
pub const MAX_TEXT: usize = 4 * 1024 * 1024;
/// Accounted word storage, including both strings and word records.
pub const MAX_INDEX_BYTES: usize = 8 * 1024 * 1024;

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
    index_cancellable(texts, || false)
        .map(|index| index.words)
        .unwrap_or_default()
}

pub struct Indexed {
    pub words: Vec<Word>,
    pub limited: bool,
}

/// Like `index`, with cooperative cancellation even inside a very long token.
/// Only bounded word records survive; the input snapshots belong to the caller.
pub fn index_cancellable<S: AsRef<str>>(
    texts: &[S],
    cancelled: impl Fn() -> bool,
) -> Option<Indexed> {
    let mut seen = std::collections::HashSet::new();
    let mut words = vec![];
    let mut bytes = 0;
    let mut limited = false;
    for text in texts {
        let text = text.as_ref();
        let mut end = text.len().min(MAX_TEXT);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        limited |= end < text.len();
        let boundary = text[end..]
            .chars()
            .next()
            .is_none_or(|ch| !is_word_char(ch));
        let text = &text[..end];
        let mut start = 0;
        let mut length = 0;
        for (n, (at, ch)) in text
            .char_indices()
            .chain(boundary.then_some((text.len(), ' ')))
            .enumerate()
        {
            if n % 2048 == 0 && cancelled() {
                return None;
            }
            if is_word_char(ch) {
                if length == 0 {
                    start = at;
                }
                length = (length + 1).min(65);
                continue;
            }
            if (3..=64).contains(&length) {
                let word = &text[start..at];
                if !word.as_bytes()[0].is_ascii_digit() && seen.insert(word) {
                    let word = Word {
                        text: word.to_owned(),
                        lower: word.to_lowercase(),
                    };
                    let cost =
                        std::mem::size_of::<Word>() + word.text.capacity() + word.lower.capacity();
                    if bytes + cost > MAX_INDEX_BYTES {
                        words.shrink_to_fit();
                        return Some(Indexed {
                            words,
                            limited: true,
                        });
                    }
                    bytes += cost;
                    words.push(word);
                    if words.len() == MAX_WORDS {
                        words.shrink_to_fit();
                        return Some(Indexed {
                            words,
                            limited: true,
                        });
                    }
                }
            }
            length = 0;
        }
    }
    if cancelled() {
        return None;
    }
    words.shrink_to_fit();
    Some(Indexed { words, limited })
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
    rank_cancellable(words, typed, limit, || false).unwrap_or_default()
}

/// Keep at most `limit` candidates while scanning and stop superseded queries.
pub fn rank_cancellable<'a>(
    words: &'a [Word],
    typed: &str,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Option<Vec<&'a str>> {
    if cancelled() {
        return None;
    }
    if typed.is_empty() || limit == 0 {
        return Some(vec![]);
    }
    let lower = typed.to_lowercase();
    let mut found = std::collections::BinaryHeap::new();
    for (n, word) in words.iter().enumerate() {
        if n % 256 == 0 && cancelled() {
            return None;
        }
        if let Some(score) = score(word, typed, &lower) {
            let candidate = (score, word.text.len(), word.text.as_str());
            if found.len() < limit {
                found.push(candidate);
            } else if found.peek().is_some_and(|worst| candidate < *worst) {
                *found.peek_mut().unwrap() = candidate;
            }
        }
    }
    if cancelled() {
        return None;
    }
    let mut found = found.into_vec();
    found.sort_unstable();
    Some(found.into_iter().map(|(_, _, word)| word).collect())
}

/// Word prefix at a known UTF-8 boundary. Reject overlong tokens without
/// allocating or walking their entire prefix. `cursor` counts Unicode scalars.
pub fn typed_word_at(text: &str, end: usize, cursor: usize) -> Option<(usize, String)> {
    let before = text.get(..end)?;
    let mut length = 0;
    let mut start = end;
    for ch in before.chars().rev().take_while(|c| is_word_char(*c)) {
        length += 1;
        if length > 64 {
            return None;
        }
        start -= ch.len_utf8();
    }
    Some((cursor.checked_sub(length)?, text[start..end].to_owned()))
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
