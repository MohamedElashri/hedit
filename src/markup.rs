//! Balanced HTML/XML element selection in Unicode scalar coordinates.
//! This is a structural scanner, not an HTML error-recovery or template parser.
use crate::advanced::{self, Selection};
use std::sync::atomic::{AtomicBool, Ordering};

const LIMIT: usize = 8 * 1024 * 1024;

struct Tag {
    start: usize,
    end: usize,
    name: String,
    close: bool,
    empty: bool,
}
struct Element {
    start: usize,
    end: usize,
    inner: Option<(usize, usize)>,
}

// XML 1.0 (Fifth Edition), productions 4 and 4a:
// https://www.w3.org/TR/xml/#NT-NameStartChar
fn name_start(ch: char) -> bool {
    matches!(ch, ':' | '_' | 'A'..='Z' | 'a'..='z'
        | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}'
        | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}'
        | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}'
        | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}

fn name_char(ch: char) -> bool {
    name_start(ch)
        || matches!(ch, '-' | '.' | '0'..='9' | '\u{b7}'
        | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
}

fn prefix(chars: &[char], at: usize, value: &str) -> bool {
    chars[at..]
        .iter()
        .copied()
        .take(value.len())
        .eq(value.chars())
}

fn after(chars: &[char], start: usize, ending: &str) -> usize {
    (start..chars.len())
        .find(|&i| prefix(chars, i, ending))
        .map_or(chars.len(), |i| i + ending.len())
}

fn tag(chars: &[char], start: usize, html: bool) -> Option<Tag> {
    let mut at = start + 1;
    let close = chars.get(at) == Some(&'/');
    at += usize::from(close);
    let first = *chars.get(at)?;
    if !name_start(first) {
        return None;
    }
    let name_start = at;
    while chars.get(at).is_some_and(|c| name_char(*c)) {
        at += 1;
    }
    if !chars
        .get(at)
        .is_some_and(|c| c.is_whitespace() || matches!(c, '/' | '>'))
    {
        return None;
    }
    let mut name: String = chars[name_start..at].iter().collect();
    if html {
        name.make_ascii_lowercase();
    }
    if close {
        while chars.get(at).is_some_and(|c| c.is_whitespace()) {
            at += 1;
        }
        return (chars.get(at) == Some(&'>')).then_some(Tag {
            start,
            end: at + 1,
            name,
            close: true,
            empty: false,
        });
    }
    let tail = at;
    let mut quote = None;
    let mut before_value = false;
    let mut unquoted = false;
    while let Some(&ch) = chars.get(at) {
        match (quote, ch) {
            (Some(q), c) if q == c => quote = None,
            (None, '\'' | '"') => {
                quote = Some(ch);
                before_value = false;
                unquoted = false;
            }
            (None, '>') => {
                let empty = !unquoted
                    && chars[tail..at].iter().rev().find(|c| !c.is_whitespace()) == Some(&'/');
                return Some(Tag {
                    start,
                    end: at + 1,
                    name,
                    close,
                    empty,
                });
            }
            (None, '<') => return None,
            (None, '=') if !unquoted => before_value = true,
            (None, c) if c.is_whitespace() => unquoted = false,
            (None, _) if before_value => {
                before_value = false;
                unquoted = true;
            }
            _ => {}
        }
        at += 1;
    }
    None
}

fn declaration_end(chars: &[char], mut at: usize) -> usize {
    let mut quote = None;
    let mut subset: usize = 0;
    while let Some(&ch) = chars.get(at) {
        match (quote, ch) {
            (Some(q), c) if q == c => quote = None,
            (None, '\'' | '"') => quote = Some(ch),
            (None, '[') => subset += 1,
            (None, ']') => subset = subset.saturating_sub(1),
            (None, '>') if subset == 0 => return at + 1,
            _ => {}
        }
        at += 1;
    }
    at
}

fn elements(chars: &[char], html: bool, cancel: &AtomicBool) -> Result<Vec<Element>, String> {
    let mut stack: Vec<Tag> = Vec::new();
    let mut found = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Selection cancelled.".into());
        }
        if chars[at] != '<' {
            at += 1;
            continue;
        }
        // HTML raw-text content must not contribute fake nested tags.
        if html
            && stack.last().is_some_and(|t| {
                matches!(t.name.as_str(), "script" | "style" | "textarea" | "title")
            })
        {
            let name = &stack.last().unwrap().name;
            let name_start = at + 2;
            if chars.get(at + 1) != Some(&'/')
                || !chars
                    .get(name_start..name_start + name.len())
                    .is_some_and(|candidate| {
                        candidate
                            .iter()
                            .zip(name.chars())
                            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
                    })
            {
                at += 1;
                continue;
            }
            if let Some(closing) =
                tag(chars, at, html).filter(|t| t.close && t.name == stack.last().unwrap().name)
            {
                let opening = stack.pop().unwrap();
                found.push(Element {
                    start: opening.start,
                    end: closing.end,
                    inner: Some((opening.end, closing.start)),
                });
                at = closing.end;
            } else {
                at += 1;
            }
            continue;
        }
        if prefix(chars, at, "<!--") {
            at = after(chars, at + 4, "-->");
            continue;
        }
        if prefix(chars, at, "<![CDATA[") {
            at = after(chars, at + 9, "]]>");
            continue;
        }
        if prefix(chars, at, "<?") {
            at = after(chars, at + 2, "?>");
            continue;
        }
        if prefix(chars, at, "<!") {
            at = declaration_end(chars, at + 2);
            continue;
        }
        let Some(current) = tag(chars, at, html) else {
            // An unfinished attribute or tag must not expose fake tags inside it.
            if chars
                .get(at + 1)
                .is_some_and(|c| name_start(*c) || *c == '/')
            {
                break;
            }
            at += 1;
            continue;
        };
        at = current.end;
        if current.close {
            if stack.last().is_some_and(|t| t.name == current.name) {
                let opening = stack.pop().unwrap();
                found.push(Element {
                    start: opening.start,
                    end: current.end,
                    inner: Some((opening.end, current.start)),
                });
            } else {
                // Do not invent a containing element across mismatched tags.
                stack.clear();
            }
        } else if current.empty
            || (html
                && matches!(
                    current.name.as_str(),
                    "area"
                        | "base"
                        | "br"
                        | "col"
                        | "embed"
                        | "hr"
                        | "img"
                        | "input"
                        | "link"
                        | "meta"
                        | "param"
                        | "source"
                        | "track"
                        | "wbr"
                ))
        {
            found.push(Element {
                start: current.start,
                end: current.end,
                inner: None,
            });
        } else {
            stack.push(current);
        }
    }
    Ok(found)
}

/// Select contents, then the complete element, then enclosing elements.
/// Reversed selections keep their direction; unmatched markup stays selected as-is.
pub fn expand(
    text: &str,
    selections: &[Selection],
    html: bool,
    cancel: &AtomicBool,
) -> Result<Vec<Selection>, String> {
    if text.len() > LIMIT {
        return Err("Tag selection is limited to 8 MiB.".into());
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Selection cancelled.".into());
    }
    let chars: Vec<_> = text.chars().collect();
    let elements = elements(&chars, html, cancel)?;
    let mut result = Vec::new();
    for selection in advanced::normalize(selections, chars.len()) {
        let a = selection.start();
        let z = selection.end();
        let mut best = None;
        for element in &elements {
            if cancel.load(Ordering::Relaxed) {
                return Err("Selection cancelled.".into());
            }
            for (start, end) in element
                .inner
                .into_iter()
                .chain(std::iter::once((element.start, element.end)))
            {
                if start <= a
                    && z <= end
                    && (a != z || a < end)
                    && (start != a || end != z)
                    && best.is_none_or(|(left, right)| end - start < right - left)
                {
                    best = Some((start, end));
                }
            }
        }
        let expanded = best.map_or(selection, |(start, end)| {
            if selection.anchor > selection.head {
                Selection {
                    anchor: end,
                    head: start,
                }
            } else {
                Selection {
                    anchor: start,
                    head: end,
                }
            }
        });
        result.push(expanded);
    }
    Ok(advanced::normalize(&result, chars.len()))
}
