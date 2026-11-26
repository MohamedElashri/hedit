//! Editor operations in Unicode scalar coordinates; display projections never own file bytes.
use crate::{editing, workspace::splice};
use serde::{Deserialize, Serialize};

pub const MAX_SELECTIONS: usize = 1000;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}
impl Selection {
    pub fn caret(index: usize) -> Self {
        Self {
            anchor: index,
            head: index,
        }
    }
    pub fn start(self) -> usize {
        self.anchor.min(self.head)
    }
    pub fn end(self) -> usize {
        self.anchor.max(self.head)
    }
    fn with_bounds(self, start: usize, end: usize) -> Self {
        if self.anchor <= self.head {
            Self {
                anchor: start,
                head: end,
            }
        } else {
            Self {
                anchor: end,
                head: start,
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Fold {
    pub start: usize,
    pub end: usize,
}
pub struct Batch {
    pub text: String,
    pub selections: Vec<Selection>,
}
pub fn byte(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(i, _)| i)
}
pub fn normalize(selections: &[Selection], length: usize) -> Vec<Selection> {
    let mut ranges: Vec<_> = selections
        .iter()
        .take(MAX_SELECTIONS)
        .map(|s| Selection {
            anchor: s.anchor.min(length),
            head: s.head.min(length),
        })
        .collect();
    ranges.sort_by_key(|s| (s.start(), s.end()));
    let mut result: Vec<Selection> = vec![];
    for range in ranges {
        if let Some(last) = result.last_mut() {
            if range.start() < last.end()
                || range == *last
                || range.start() == last.start()
                || (range.start() == last.end()
                    && (range.start() == range.end() || last.start() == last.end()))
            {
                let start = last.start().min(range.start());
                let end = last.end().max(range.end());
                last.head = end;
                last.anchor = start;
                continue;
            }
        }
        result.push(range);
    }
    result
}

/// Convert sorted, non-overlapping selections (as returned by `normalize`)
/// with one forward traversal, without a per-character offset table.
pub fn byte_ranges<'a>(
    text: &'a str,
    selections: &'a [Selection],
) -> impl Iterator<Item = std::ops::Range<usize>> + 'a {
    let mut chars = text.chars();
    let mut at = 0;
    let mut offset = move |target| {
        debug_assert!(
            target >= at,
            "selection ranges must be sorted and non-overlapping"
        );
        if target > at {
            let _ = chars.nth(target - at - 1);
            at = target;
        }
        text.len() - chars.as_str().len()
    };
    selections
        .iter()
        .map(move |range| offset(range.start())..offset(range.end()))
}

pub fn replace(text: &str, selections: &[Selection], replacement: &str) -> Batch {
    let ranges = normalize(selections, text.chars().count());
    let mut output = String::with_capacity(text.len());
    let mut previous = 0;
    let mut previous_char = 0;
    let mut cursors = Vec::with_capacity(ranges.len());
    let mut count = 0;
    let replacement_chars = replacement.chars().count();
    for (selection, range) in ranges.iter().zip(byte_ranges(text, &ranges)) {
        output.push_str(&text[previous..range.start]);
        output.push_str(replacement);
        count += selection.start() - previous_char + replacement_chars;
        cursors.push(Selection::caret(count));
        previous = range.end;
        previous_char = selection.end();
    }
    output.push_str(&text[previous..]);
    Batch {
        text: output,
        selections: cursors,
    }
}
fn word_character(ch: char) -> bool {
    crate::completion::is_word_char(ch)
}

pub fn word(text: &str, cursor: usize) -> Selection {
    let chars: Vec<_> = text.chars().collect();
    let mut a = cursor.min(chars.len());
    let mut b = a;
    if a == chars.len() || !word_character(chars[a]) {
        if a == 0 || !word_character(chars[a - 1]) {
            return Selection::caret(a);
        }
        a -= 1;
        b = a;
    }
    while a > 0 && word_character(chars[a - 1]) {
        a -= 1;
    }
    while b < chars.len() && word_character(chars[b]) {
        b += 1;
    }
    Selection { anchor: a, head: b }
}
pub fn occurrences(text: &str, selections: &[Selection], all: bool) -> Vec<Selection> {
    let length = text.chars().count();
    let mut ranges = normalize(selections, length);
    if ranges.is_empty() {
        ranges.push(Selection::caret(0));
    }
    let from_caret = ranges[0].start() == ranges[0].end();
    if from_caret {
        let at = word(text, ranges[0].head);
        ranges[0] = at;
        if !all || at.start() == at.end() {
            return normalize(&ranges, length);
        }
    }
    let needle = &text[byte(text, ranges[0].start())..byte(text, ranges[0].end())];
    if needle.is_empty() {
        return ranges;
    }
    if all {
        let mut previous_byte = 0;
        let mut previous_char = 0;
        let needle_len = needle.chars().count();
        return text
            .match_indices(needle)
            .filter(|&(start, value)| {
                !from_caret
                    || (!text[..start]
                        .chars()
                        .next_back()
                        .is_some_and(word_character)
                        && !text[start + value.len()..]
                            .chars()
                            .next()
                            .is_some_and(word_character))
            })
            .take(MAX_SELECTIONS)
            .map(|(at, _)| {
                let start = previous_char + text[previous_byte..at].chars().count();
                previous_byte = at;
                previous_char = start;
                Selection {
                    anchor: start,
                    head: start + needle_len,
                }
            })
            .collect();
    }
    if ranges.len() < MAX_SELECTIONS {
        if let Some(next) = next_unselected(text, needle, &ranges) {
            ranges.push(next);
        }
    }
    normalize(&ranges, length)
}

/// Search the free text between non-overlapping selections. Start after the
/// last selection so a match can begin even when a global scan would skip it.
fn next_unselected(text: &str, needle: &str, ranges: &[Selection]) -> Option<Selection> {
    let mut from_byte = 0;
    let mut from_char = 0;
    let needle_chars = needle.chars().count();
    let mut wrapped = None;
    for selected in ranges {
        let start = from_byte + byte(&text[from_byte..], selected.start() - from_char);
        if wrapped.is_none() {
            if let Some(hit) = text[from_byte..start].find(needle) {
                let anchor = from_char + text[from_byte..from_byte + hit].chars().count();
                wrapped = Some(Selection {
                    anchor,
                    head: anchor + needle_chars,
                });
            }
        }
        from_byte = start + byte(&text[start..], selected.end() - selected.start());
        from_char = selected.end();
    }
    text[from_byte..]
        .find(needle)
        .map(|hit| {
            let anchor = from_char + text[from_byte..from_byte + hit].chars().count();
            Selection {
                anchor,
                head: anchor + needle_chars,
            }
        })
        .or(wrapped)
}
pub fn word_boundary(text: &str, position: usize, forward: bool) -> usize {
    let chars: Vec<_> = text.chars().collect();
    let mut n = position.min(chars.len());
    let word = |c: char| c.is_alphanumeric() || c == '_';
    if forward {
        while n < chars.len() && word(chars[n]) {
            n += 1;
        }
        while n < chars.len() && !word(chars[n]) {
            n += 1;
        }
    } else {
        while n > 0 && !word(chars[n - 1]) {
            n -= 1;
        }
        while n > 0 && word(chars[n - 1]) {
            n -= 1;
        }
    }
    n
}
pub fn line_column(text: &str, cursor: usize, width: usize) -> (usize, usize) {
    text.chars().take(cursor).fold((0, 0), |(line, col), ch| {
        if ch == '\n' {
            (line + 1, 0)
        } else if ch == '\r' {
            (line, col)
        } else {
            (line, col + if ch == '\t' { width - col % width } else { 1 })
        }
    })
}
pub fn at_column(text: &str, line: usize, column: usize, width: usize) -> usize {
    let mut row = 0;
    let mut col = 0;
    let mut offset = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if row == line {
            if ch == '\n' || (ch == '\r' && chars.peek() == Some(&'\n')) || col >= column {
                return offset;
            }
            col += match ch {
                '\r' => 0,
                '\t' => width - col % width,
                _ => 1,
            };
        }
        if ch == '\n' {
            row += 1;
        }
        offset += 1;
    }
    offset
}
pub fn rectangle(text: &str, anchor: usize, head: usize, width: usize) -> Vec<Selection> {
    let (a, x) = line_column(text, anchor, width);
    let (b, y) = line_column(text, head, width);
    (a.min(b)..=a.max(b))
        .take(MAX_SELECTIONS)
        .map(|line| Selection {
            anchor: at_column(text, line, x, width),
            head: at_column(text, line, y, width),
        })
        .collect()
}
pub fn detect_indentation(text: &str) -> (bool, u8) {
    let mut tabs = 0;
    let mut spaces = 0;
    let mut differences = [0usize; 9];
    let mut previous = 0;
    for line in text
        .lines()
        .take(10000)
        .filter(|line| !line.trim().is_empty())
    {
        let prefix: String = line
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .collect();
        if prefix.contains('\t') {
            tabs += 1;
            previous = 0;
            continue;
        }
        let n = prefix.len();
        if n > 0 {
            spaces += 1;
        }
        let delta = n.abs_diff(previous);
        if (1..=8).contains(&delta) {
            differences[delta] += 1;
        }
        previous = n;
    }
    let width = (1..=8)
        .max_by_key(|n| (differences[*n], usize::from(*n == 4)))
        .unwrap();
    (tabs > spaces, width as u8)
}
/// Indentation found in the text: tabs or spaces, and the space width. `None`
/// when no line is indented, so settings decide instead.
pub fn found_indentation(text: &str) -> Option<(bool, Option<u8>)> {
    let indented = text
        .lines()
        .take(10000)
        .any(|line| line.starts_with([' ', '\t']) && !line.trim().is_empty());
    indented.then(|| {
        let (tabs, width) = detect_indentation(text);
        (tabs, (!tabs).then_some(width))
    })
}

const OPENERS: [char; 3] = ['(', '[', '{'];
const CLOSERS: [char; 3] = [')', ']', '}'];

fn closer(open: char) -> Option<char> {
    OPENERS
        .iter()
        .position(|&c| c == open)
        .map(|i| CLOSERS[i])
        .or(matches!(open, '"' | '\'' | '`').then_some(open))
}

/// Typing `typed` with automatic pairing, as in Sublime Text: an opening
/// bracket or quote inserts its closer, wraps a selection, and typing a closer
/// that is already next steps over it. `None` when pairing does not apply to
/// every cursor, so the character is inserted normally.
pub fn pair(
    text: &str,
    selections: &[Selection],
    typed: char,
    single_quotes: bool,
) -> Option<Batch> {
    if (typed == '\'' && !single_quotes)
        || !(OPENERS.contains(&typed)
            || CLOSERS.contains(&typed)
            || matches!(typed, '\"' | '\'' | '`'))
    {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let ranges = normalize(selections, chars.len());
    let quote = matches!(typed, '"' | '\'' | '`');
    let opening = OPENERS.contains(&typed) || quote;
    let closing = CLOSERS.contains(&typed) || quote;
    enum Action {
        Wrap(char),
        Pair(char),
        Skip,
    }
    let mut actions = Vec::with_capacity(ranges.len());
    for range in &ranges {
        let next = chars.get(range.end()).copied();
        let previous = range.start().checked_sub(1).map(|i| chars[i]);
        if quote && range.start() == range.end() {
            let escapes = chars[..range.start()]
                .iter()
                .rev()
                .take_while(|&&ch| ch == '\\')
                .count();
            if escapes % 2 != 0 {
                return None;
            }
        }
        let action = if range.start() != range.end() {
            Action::Wrap(closer(typed).filter(|_| opening)?)
        } else if closing && next == Some(typed) {
            Action::Skip
        } else if opening
            && next
                .is_none_or(|c| c.is_whitespace() || CLOSERS.contains(&c) || matches!(c, ';' | ','))
            && !(quote && previous.is_some_and(|c| c.is_alphanumeric() || c == typed))
        {
            Action::Pair(closer(typed)?)
        } else {
            return None;
        };
        actions.push(action);
    }
    let mut output = String::with_capacity(text.len() + 2 * ranges.len());
    let mut cursors = Vec::with_capacity(ranges.len());
    let mut previous = 0;
    let mut length = 0;
    for (range, action) in ranges.iter().zip(actions) {
        let kept: String = chars[previous..range.start()].iter().collect();
        length += previous.abs_diff(range.start());
        output.push_str(&kept);
        let selected: String = chars[range.start()..range.end()].iter().collect();
        match action {
            Action::Wrap(close) => {
                output.push(typed);
                output.push_str(&selected);
                output.push(close);
                cursors.push(range.with_bounds(length + 1, length + 1 + selected.chars().count()));
                length += selected.chars().count() + 2;
                previous = range.end();
            }
            Action::Pair(close) => {
                output.push(typed);
                output.push(close);
                cursors.push(Selection::caret(length + 1));
                length += 2;
                previous = range.end();
            }
            Action::Skip => {
                output.push(typed);
                cursors.push(Selection::caret(length + 1));
                length += 1;
                previous = range.end() + 1;
            }
        }
    }
    output.extend(&chars[previous..]);
    Some(Batch {
        text: output,
        selections: cursors,
    })
}

/// Whether the cursor sits inside an empty pair such as `(|)`, which
/// Backspace removes together.
pub fn inside_empty_pair(text: &str, cursor: usize) -> bool {
    let mut around = text.chars().skip(cursor.saturating_sub(1));
    cursor > 0
        && around
            .next()
            .and_then(closer)
            .is_some_and(|close| around.next() == Some(close))
}

/// The bracket next to the cursor and its partner, as character indices.
/// Scans at most `limit` characters and ignores brackets of other kinds.
pub fn matching_bracket(text: &str, cursor: usize, limit: usize) -> Option<(usize, usize)> {
    let at = byte(text, cursor);
    let candidates = [
        text[..at]
            .chars()
            .next_back()
            .map(|c| (c, cursor - 1, at - c.len_utf8())),
        text[at..].chars().next().map(|c| (c, cursor, at)),
    ];
    for (c, index, offset) in candidates.into_iter().flatten() {
        if let Some(i) = OPENERS.iter().position(|&o| o == c) {
            let mut depth = 0usize;
            for (n, current) in text[offset..].chars().take(limit).enumerate() {
                if current == c {
                    depth += 1;
                } else if current == CLOSERS[i] {
                    depth -= 1;
                    if depth == 0 {
                        return Some((index, index + n));
                    }
                }
            }
        } else if let Some(i) = CLOSERS.iter().position(|&o| o == c) {
            let mut depth = 0usize;
            let end = offset + c.len_utf8();
            for (n, current) in text[..end].chars().rev().take(limit).enumerate() {
                if current == c {
                    depth += 1;
                } else if current == OPENERS[i] {
                    depth -= 1;
                    if depth == 0 {
                        return Some((index, index - n));
                    }
                }
            }
        }
    }
    None
}
pub fn newline(
    text: &str,
    selection: Selection,
    width: usize,
    tabs: bool,
    smart: bool,
    crlf: bool,
) -> Batch {
    newline_with_colon(text, selection, width, tabs, smart, crlf, false)
}

pub fn newline_with_colon(
    text: &str,
    selection: Selection,
    width: usize,
    tabs: bool,
    smart: bool,
    crlf: bool,
    indent_after_colon: bool,
) -> Batch {
    let before = &text[..byte(text, selection.start())];
    let line = before.rsplit('\n').next().unwrap_or("");
    let mut indent: String = line
        .chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .collect();
    let after = &text[byte(text, selection.end())..];
    let unit = if tabs {
        "\t".to_owned()
    } else {
        " ".repeat(width)
    };
    let opener = line.trim_end().chars().next_back();
    let closing = opener.filter(|ch| OPENERS.contains(ch)).and_then(closer);
    let opens = closing.is_some() || (indent_after_colon && opener == Some(':'));
    let closes =
        closing.is_some_and(|close| after.trim_start_matches([' ', '\t']).starts_with(close));
    let ending = if crlf { "\r\n" } else { "\n" };
    if smart && opens {
        indent.push_str(&unit);
    }
    let insertion = if smart && opens && closes {
        let base = &indent[..indent.len() - unit.len()];
        format!("{ending}{indent}{ending}{base}")
    } else {
        format!("{ending}{indent}")
    };
    let mut result = replace(text, &[selection], &insertion);
    result.selections = vec![Selection::caret(
        selection.start() + ending.len() + indent.chars().count(),
    )];
    result
}
pub fn indentation_folds(text: &str, width: usize) -> Vec<Fold> {
    let width = width.max(1);
    let mut lines = vec![];
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let size = line.chars().count();
        let indent = line
            .chars()
            .take_while(|c| matches!(c, ' ' | '\t'))
            .fold(0, |n, ch| {
                n + if ch == '\t' { width - n % width } else { 1 }
            });
        lines.push((offset, offset + size, indent, line.trim().is_empty()));
        offset += size;
    }
    let mut stack: Vec<(usize, usize)> = vec![];
    let mut folds = vec![];
    let mut previous: Option<(usize, usize)> = None;
    for &(start, end, indent, blank) in &lines {
        if blank {
            continue;
        }
        while stack.last().is_some_and(|(_, level)| *level >= indent) {
            let (body, _) = stack.pop().unwrap();
            if start > body {
                folds.push(Fold {
                    start: body,
                    end: start,
                });
            }
        }
        if let Some((prev_end, level)) = previous {
            if indent > level {
                stack.push((prev_end, level));
            }
        }
        previous = Some((end, indent));
    }
    for (start, _) in stack {
        if offset > start {
            folds.push(Fold { start, end: offset });
        }
    }
    folds.sort_by_key(|f| (f.start, f.end));
    folds
}

/// Collapse blocks at a 1-based nesting level, leaving their parents visible.
/// Irregular space widths still count as one nesting step, as do tab indents.
pub fn indentation_folds_at_level(text: &str, width: usize, level: usize) -> Vec<Fold> {
    if level == 0 {
        return vec![];
    }
    let mut parents = Vec::new();
    let mut result = Vec::new();
    for fold in indentation_folds(text, width) {
        while parents.last().is_some_and(|end| *end <= fold.start) {
            parents.pop();
        }
        if parents.len() + 1 == level {
            result.push(fold);
        }
        parents.push(fold.end);
    }
    result
}

#[derive(Clone, Copy)]
enum SegmentKind {
    Text,
    Tab(usize),
    Fold,
}
struct Segment {
    display: usize,
    length: usize,
    start: usize,
    end: usize,
    kind: SegmentKind,
}
pub struct Projection {
    pub text: String,
    pub source_lines: Vec<usize>,
    segments: Vec<Segment>,
    length: usize,
    source_length: usize,
}
impl Projection {
    pub fn new(source: &str, width: usize, folds: &[Fold]) -> Self {
        let width = width.clamp(1, 8);
        if folds.is_empty() && !source.as_bytes().contains(&b'\t') {
            let count = source.chars().count();
            return Self {
                text: source.to_owned(),
                source_lines: (1..=memchr::memchr_iter(b'\n', source.as_bytes()).count() + 1)
                    .collect(),
                segments: vec![Segment {
                    display: 0,
                    length: count,
                    start: 0,
                    end: count,
                    kind: SegmentKind::Text,
                }],
                length: count,
                source_length: count,
            };
        }
        let chars: Vec<_> = source.chars().collect();
        let mut result = Self {
            text: String::new(),
            source_lines: vec![1],
            segments: vec![],
            length: 0,
            source_length: chars.len(),
        };
        let mut folds = folds.to_vec();
        folds.retain(|f| f.start < f.end && f.end <= chars.len());
        folds.sort_by_key(|f| (f.start, std::cmp::Reverse(f.end)));
        let mut fold_index = 0;
        let mut i = 0;
        let mut line = 1;
        let mut column = 0;
        while i < chars.len() {
            while fold_index < folds.len() && folds[fold_index].start < i {
                fold_index += 1;
            }
            if let Some(fold) = folds.get(fold_index).filter(|f| f.start == i) {
                result.text.push('⋯');
                let mut length = 1;
                line += chars[i..fold.end].iter().filter(|c| **c == '\n').count();
                if chars[fold.end - 1] == '\n' {
                    result.text.push('\n');
                    length += 1;
                    result.source_lines.push(line);
                    column = 0;
                }
                result.segments.push(Segment {
                    display: result.length,
                    length,
                    start: i,
                    end: fold.end,
                    kind: SegmentKind::Fold,
                });
                result.length += length;
                i = fold.end;
                continue;
            }
            let ch = chars[i];
            let length = if ch == '\t' {
                width - column % width
            } else {
                1
            };
            if ch == '\t' {
                result.text.extend(std::iter::repeat_n(' ', length));
            } else {
                result.text.push(ch);
            }
            let kind = if ch == '\t' {
                SegmentKind::Tab(length)
            } else {
                SegmentKind::Text
            };
            let merge = result.segments.last().is_some_and(|s| {
                s.end == i
                    && match (s.kind, kind) {
                        (SegmentKind::Text, SegmentKind::Text) => true,
                        (SegmentKind::Tab(a), SegmentKind::Tab(b)) => a == b,
                        _ => false,
                    }
            });
            if merge {
                let last = result.segments.last_mut().unwrap();
                last.length += length;
                last.end = i + 1;
            } else {
                result.segments.push(Segment {
                    display: result.length,
                    length,
                    start: i,
                    end: i + 1,
                    kind,
                });
            }
            result.length += length;
            if ch == '\n' {
                line += 1;
                result.source_lines.push(line);
                column = 0;
            } else if ch != '\r' {
                column += length;
            }
            i += 1;
        }
        result
    }
    fn segment(&self, position: usize) -> Option<&Segment> {
        self.segments.get(
            self.segments
                .partition_point(|s| s.display + s.length <= position),
        )
    }
    pub fn source(&self, position: usize) -> usize {
        let Some(s) = self.segment(position) else {
            return self.source_length;
        };
        match s.kind {
            SegmentKind::Text => s.start + position - s.display,
            SegmentKind::Tab(width) => s.start + (position - s.display) / width,
            SegmentKind::Fold => {
                if position == s.display {
                    s.start
                } else {
                    s.end
                }
            }
        }
    }
    pub fn display(&self, position: usize) -> usize {
        let Some(s) = self
            .segments
            .get(self.segments.partition_point(|s| s.end <= position))
        else {
            return self.length;
        };
        match s.kind {
            SegmentKind::Text => s.display + position.saturating_sub(s.start),
            SegmentKind::Tab(width) => s.display + position.saturating_sub(s.start) * width,
            SegmentKind::Fold => {
                if position <= s.start {
                    s.display
                } else {
                    s.display + s.length
                }
            }
        }
    }
    pub fn span(&self, position: usize) -> Selection {
        let start = self.source(position);
        let end = match self.segment(position).map(|s| s.kind) {
            Some(SegmentKind::Tab(_)) => start + 1,
            _ => self.source(position + 1),
        };
        Selection {
            anchor: start,
            head: end,
        }
    }
    pub fn touches_fold(&self, start: usize, end: usize) -> bool {
        self.segments.iter().any(|s| {
            matches!(s.kind, SegmentKind::Fold) && start < s.display + s.length && end > s.display
        })
    }
    pub fn is_identity(&self) -> bool {
        self.segments
            .iter()
            .all(|s| matches!(s.kind, SegmentKind::Text))
    }
    pub fn edited(&self, source: &str, display: &str, selection: Selection) -> Batch {
        if self.is_identity() {
            let count = display.chars().count();
            return Batch {
                text: display.to_owned(),
                selections: vec![Selection {
                    anchor: selection.anchor.min(count),
                    head: selection.head.min(count),
                }],
            };
        }
        let (a, b, insertion) = splice(&self.text, display);
        let start = self.text[..a].chars().count();
        let end = self.text[..b].chars().count();
        let source_start = self.source(start);
        let source_end = if end > start {
            self.span(end - 1).end().max(source_start)
        } else {
            source_start
        };
        let mut text = source.to_owned();
        text.replace_range(
            byte(source, source_start)..byte(source, source_end),
            insertion,
        );
        let added = insertion.chars().count();
        let old_len = source_end - source_start;
        let cursor = |n: usize| {
            if n <= start {
                self.source(n)
            } else if n <= start + added {
                source_start + n - start
            } else {
                self.source(n - added + (end - start))
                    .saturating_sub(old_len)
                    + added
            }
        };
        Batch {
            text,
            selections: vec![Selection {
                anchor: cursor(selection.anchor),
                head: cursor(selection.head),
            }],
        }
    }
}

pub fn transform(
    text: &str,
    selections: &[Selection],
    operation: editing::Operation,
    width: usize,
    tabs: bool,
    comment: Option<&str>,
) -> Batch {
    let mut ranges = normalize(selections, text.chars().count());
    if ranges.is_empty() {
        ranges.push(Selection::caret(0));
    }
    let global = matches!(
        operation,
        editing::Operation::SortLines
            | editing::Operation::SortLinesIgnoreCase
            | editing::Operation::ReverseLines
            | editing::Operation::UniqueLines
            | editing::Operation::TrimWhitespace
            | editing::Operation::ToSpaces
            | editing::Operation::ToTabs
    );
    let only_carets = ranges.iter().all(|s| s.start() == s.end());
    if global && only_carets {
        let caret = ranges[0];
        let edit = editing::apply(
            text,
            caret.anchor,
            caret.head,
            operation,
            width,
            tabs,
            comment,
        );
        return Batch {
            text: edit.text,
            selections: vec![Selection {
                anchor: edit.anchor,
                head: edit.head,
            }],
        };
    }
    let indent_carets = (only_carets
        && matches!(
            operation,
            editing::Operation::Indent | editing::Operation::Unindent
        ))
    .then(|| ranges.clone());
    let mut duplicate_carets = std::collections::HashMap::new();
    if !matches!(
        operation,
        editing::Operation::Uppercase | editing::Operation::Lowercase
    ) {
        let bytes: Vec<_> = byte_ranges(text, &ranges).collect();
        let (mut start_byte, mut start_char) = (0, 0);
        let (mut end_byte, mut end_char) = (0, 0);
        for (s, span) in ranges.iter_mut().zip(bytes) {
            if operation == editing::Operation::Duplicate && s.start() != s.end() {
                continue;
            }
            let original = *s;
            let (a, b) = editing::line_range(text, span.start, span.end);
            start_char += text[start_byte..a].chars().count();
            end_char += text[end_byte..b].chars().count();
            (start_byte, end_byte) = (a, b);
            s.anchor = start_char;
            s.head = end_char;
            if operation == editing::Operation::JoinLines && span.is_empty() && b < text.len() {
                let (_, next_end) = editing::line_range(text, b, b);
                s.head += text[b..next_end].chars().count();
            }
            if operation == editing::Operation::Duplicate {
                duplicate_carets
                    .entry((s.start(), s.end()))
                    .or_insert(original);
            }
        }
        let length = text.chars().count();
        let indent_eof =
            indent_carets.is_some() && ranges.last() == Some(&Selection::caret(length));
        ranges = normalize(&ranges, length);
        if indent_eof && ranges.last() != Some(&Selection::caret(length)) {
            ranges.push(Selection::caret(length));
        }
        if matches!(
            operation,
            editing::Operation::MoveUp | editing::Operation::MoveDown
        ) {
            let mut blocks: Vec<Selection> = Vec::new();
            for range in ranges {
                if let Some(previous) = blocks.last_mut().filter(|s| s.end() == range.start()) {
                    previous.head = range.end();
                } else {
                    blocks.push(range);
                }
            }
            ranges = blocks;
        }
    }
    // An empty range can make a global command operate on the entire buffer.
    // Other block-local edits can be assembled against the original text.
    let local = !global || ranges.iter().all(|range| range.start() < range.end());
    if local
        && matches!(
            operation,
            editing::Operation::SelectLine
                | editing::Operation::JoinLines
                | editing::Operation::Indent
                | editing::Operation::Unindent
                | editing::Operation::Uppercase
                | editing::Operation::Lowercase
                | editing::Operation::SortLines
                | editing::Operation::SortLinesIgnoreCase
                | editing::Operation::ReverseLines
                | editing::Operation::UniqueLines
                | editing::Operation::TrimWhitespace
                | editing::Operation::ToSpaces
                | editing::Operation::ToTabs
                | editing::Operation::ToggleComment
        )
    {
        let mut output = String::with_capacity(text.len());
        let mut selections = Vec::with_capacity(ranges.len());
        let mut previous = 0;
        let mut shift = 0isize;
        for (s, bytes) in ranges.iter().zip(byte_ranges(text, &ranges)) {
            output.push_str(&text[previous..bytes.start]);
            let start = s.start();
            let edit = editing::apply(
                &text[bytes.clone()],
                s.anchor - start,
                s.head - start,
                operation,
                width,
                tabs,
                comment,
            );
            let base = start.saturating_add_signed(shift);
            let delta = edit.text.chars().count() as isize - (s.end() - start) as isize;
            if let Some(carets) = &indent_carets {
                selections.extend(
                    indent_positions(carets, *s, delta)
                        .map(|caret| Selection::caret(caret.head.saturating_add_signed(shift))),
                );
            } else {
                selections.push(Selection {
                    anchor: base + edit.anchor,
                    head: base + edit.head,
                });
            }
            shift += delta;
            output.push_str(&edit.text);
            previous = bytes.end;
        }
        output.push_str(&text[previous..]);
        let selections = normalize(&selections, output.chars().count());
        return Batch {
            text: output,
            selections,
        };
    }
    let mut output = text.to_owned();
    let mut final_ranges: Vec<Selection> = vec![];
    for s in ranges.into_iter().rev() {
        let s = duplicate_carets
            .get(&(s.start(), s.end()))
            .copied()
            .unwrap_or(s);
        let edit = editing::apply(&output, s.anchor, s.head, operation, width, tabs, comment);
        let delta = edit.text.chars().count() as isize - output.chars().count() as isize;
        for r in &mut final_ranges {
            r.anchor = r.anchor.saturating_add_signed(delta);
            r.head = r.head.saturating_add_signed(delta);
        }
        if let Some(carets) = &indent_carets {
            final_ranges.extend(indent_positions(carets, s, delta));
        } else {
            final_ranges.push(Selection {
                anchor: edit.anchor,
                head: edit.head,
            });
        }
        output = edit.text;
    }
    let selections = normalize(&final_ranges, output.chars().count());
    Batch {
        text: output,
        selections,
    }
}

fn indent_positions(
    carets: &[Selection],
    range: Selection,
    delta: isize,
) -> impl Iterator<Item = Selection> + '_ {
    let start = carets.partition_point(|caret| caret.head < range.start());
    let end = carets
        .partition_point(|caret| caret.head < range.end())
        .max(start + usize::from(range.start() == range.end()));
    carets[start..end].iter().map(move |caret| {
        Selection::caret(caret.head.saturating_add_signed(delta).max(range.start()))
    })
}

/// Character indices of line starts around `selection`: the start of its first
/// line and the end of its last line (before the newline).
fn line_bounds(chars: &[char], selection: Selection) -> (usize, usize) {
    let mut start = selection.start();
    while start > 0 && chars[start - 1] != '\n' {
        start -= 1;
    }
    let mut end = selection.end();
    while end < chars.len() && chars[end] != '\n' {
        end += 1;
    }
    (start, end)
}

/// The innermost bracket pair around `selection`, as (opener, closer) indices.
fn enclosing_brackets(
    chars: &[char],
    selection: Selection,
    limit: usize,
) -> Option<(usize, usize)> {
    let mut depth = [0usize; 3];
    let mut at = selection.start();
    let mut forward_budget = limit;
    for _ in 0..limit {
        at = at.checked_sub(1)?;
        let c = chars[at];
        if let Some(i) = CLOSERS.iter().position(|&x| x == c) {
            depth[i] += 1;
        } else if let Some(i) = OPENERS.iter().position(|&x| x == c) {
            if depth[i] > 0 {
                depth[i] -= 1;
                continue;
            }
            // The matching closer must lie at or after the selection's end.
            let mut nested = 0usize;
            for (offset, &d) in chars[at + 1..].iter().enumerate().take(forward_budget) {
                forward_budget -= 1;
                if d == OPENERS[i] {
                    nested += 1;
                } else if d == CLOSERS[i] {
                    if nested == 0 {
                        let close = at + 1 + offset;
                        if close >= selection.end() {
                            return Some((at, close));
                        }
                        break;
                    }
                    nested -= 1;
                }
            }
            if forward_budget == 0 {
                return None;
            }
        }
    }
    None
}

/// The text inside the nearest same-line quotes around `selection`.
fn enclosing_quotes(chars: &[char], selection: Selection) -> Option<(usize, usize)> {
    let (line_start, line_end) = line_bounds(chars, selection);
    ['"', '\'', '`']
        .into_iter()
        .filter_map(|quote| {
            let positions = (line_start..line_end).filter(|&i| {
                chars[i] == quote
                    && chars[line_start..i]
                        .iter()
                        .rev()
                        .take_while(|&&c| c == '\\')
                        .count()
                        % 2
                        == 0
            });
            let mut positions = positions;
            while let (Some(open), Some(close)) = (positions.next(), positions.next()) {
                if open < selection.start() && close >= selection.end() {
                    return Some((open, close));
                }
            }
            None
        })
        .min_by_key(|(open, close)| close - open)
}

/// Like Sublime's Expand Selection to Brackets: select inside the nearest
/// brackets, then the brackets too, then the next pair out.
pub fn expand_to_brackets(text: &str, selection: Selection) -> Selection {
    let chars: Vec<char> = text.chars().collect();
    let mut current = selection;
    loop {
        let Some((open, close)) = enclosing_brackets(&chars, current, 1_000_000) else {
            return selection;
        };
        let inside = Selection {
            anchor: open + 1,
            head: close,
        };
        if (inside.start(), inside.end()) != (selection.start(), selection.end())
            && inside.start() <= selection.start()
            && inside.end() >= selection.end()
        {
            return inside;
        }
        let whole = Selection {
            anchor: open,
            head: close + 1,
        };
        if (whole.start(), whole.end()) != (selection.start(), selection.end()) {
            return whole;
        }
        current = whole;
    }
}

/// Like Sublime's Expand Selection to Indentation: the surrounding lines
/// indented at least as deeply as the current one.
pub fn expand_to_indentation(text: &str, selection: Selection) -> Selection {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for line in &lines {
        starts.push(at);
        at += line.chars().count() + 1;
    }
    let line_of = |index: usize| starts.partition_point(|&s| s <= index) - 1;
    let indent = |line: &str| line.chars().take_while(|c| matches!(c, ' ' | '\t')).count();
    let (first, last) = (
        line_of(selection.start()),
        line_of(selection.end().saturating_sub(1).max(selection.start())),
    );
    let level = (first..=last)
        .filter(|&l| !lines[l].trim().is_empty())
        .map(|l| indent(lines[l]))
        .min()
        .unwrap_or(0);
    let inside = |l: usize| lines[l].trim().is_empty() || indent(lines[l]) >= level;
    let mut top = first;
    while top > 0 && inside(top - 1) {
        top -= 1;
    }
    let mut bottom = last;
    while bottom + 1 < lines.len() && inside(bottom + 1) {
        bottom += 1;
    }
    Selection {
        anchor: starts[top],
        head: (starts[bottom] + lines[bottom].chars().count()).max(selection.end()),
    }
}

/// Like Sublime's Expand Selection: the smallest of the word, quoted text,
/// bracket contents, brackets, line, or whole text that is larger than now.
pub fn expand_selection(text: &str, selection: Selection) -> Selection {
    let chars: Vec<char> = text.chars().collect();
    let size = |s: (usize, usize)| s.1 - s.0;
    let current = (selection.start(), selection.end());
    let mut candidates = vec![];
    let w = word(text, selection.start());
    if selection.start() == selection.end() && w.start() != w.end() {
        candidates.push((w.start(), w.end()));
    }
    if let Some((open, close)) = enclosing_quotes(&chars, selection) {
        candidates.push((open + 1, close));
        candidates.push((open, close + 1));
    }
    if let Some((open, close)) = enclosing_brackets(&chars, selection, 1_000_000) {
        candidates.push((open + 1, close));
        candidates.push((open, close + 1));
    }
    let (start, end) = line_bounds(&chars, selection);
    candidates.push((start, end));
    candidates.push((0, chars.len()));
    candidates
        .into_iter()
        .filter(|&c| c.0 <= current.0 && c.1 >= current.1 && size(c) > size(current))
        .min_by_key(|&c| size(c))
        .map_or(selection, |(a, b)| Selection { anchor: a, head: b })
}

/// Move the cursor to the bracket matching the one beside it: just inside the
/// closer, or just after the opener, so pressing again returns.
pub fn jump_to_bracket(text: &str, selection: Selection) -> Selection {
    match matching_bracket(text, selection.head, 1_000_000) {
        Some((at, other)) if other > at => Selection::caret(other),
        Some((_, other)) => Selection::caret(other + 1),
        None => selection,
    }
}

/// One cursor per line of each selection, like Sublime's Split into Lines.
pub fn split_into_lines(text: &str, selections: &[Selection]) -> Vec<Selection> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    for s in normalize(selections, chars.len()) {
        if s.start() == s.end() {
            out.push(s);
            continue;
        }
        let mut start = s.start();
        for (i, &c) in chars.iter().enumerate().take(s.end()).skip(s.start()) {
            if c == '\n' {
                let end = if i > start && chars[i - 1] == '\r' {
                    i - 1
                } else {
                    i
                };
                out.push(s.with_bounds(start, end));
                start = i + 1;
            }
        }
        if start < s.end() {
            out.push(s.with_bounds(start, s.end()));
        }
    }
    normalize(&out, chars.len())
}

/// Like Sublime's Quick Skip Next: drop the last selection and select the
/// next occurrence of its text after it instead.
pub fn skip_occurrence(text: &str, selections: &[Selection]) -> Vec<Selection> {
    let length = text.chars().count();
    let mut ranges = normalize(selections, length);
    let Some(last) = ranges.last().copied() else {
        return ranges;
    };
    if last.start() == last.end() {
        return ranges;
    }
    let start = byte(text, last.start());
    let end = start + byte(&text[start..], last.end() - last.start());
    if let Some(next) = next_unselected(text, &text[start..end], &ranges) {
        *ranges.last_mut().unwrap() = next;
    }
    normalize(&ranges, length)
}

/// Add a selection on the line above or below each one, at the same columns.
/// A caret uses `goal` when given, so a column survives passing a short line.
pub fn add_line_selections(
    text: &str,
    selections: &[Selection],
    below: bool,
    width: usize,
    goal: Option<usize>,
) -> Vec<Selection> {
    let chars: Vec<char> = text.chars().collect();
    let width = width.max(1);
    let mut starts = vec![0];
    starts.extend(
        chars
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i + 1),
    );
    let line_of = |at: usize| starts.partition_point(|&s| s <= at) - 1;
    let column = |line: usize, at: usize| {
        chars[starts[line]..at].iter().fold(0, |col, ch| match ch {
            '\r' => col,
            '\t' => col + width - col % width,
            _ => col + 1,
        })
    };
    let at_column = |line: usize, target: usize| {
        let mut col = 0;
        let mut at = starts[line];
        while let Some(&ch) = chars.get(at) {
            if ch == '\n' || (ch == '\r' && chars.get(at + 1) == Some(&'\n')) || col >= target {
                break;
            }
            col += match ch {
                '\r' => 0,
                '\t' => width - col % width,
                _ => 1,
            };
            at += 1;
        }
        at
    };
    let mut out = selections.to_vec();
    for s in normalize(selections, chars.len()) {
        let line = line_of(s.head);
        let Some(next) = (if below {
            Some(line + 1).filter(|&l| l < starts.len())
        } else {
            line.checked_sub(1)
        }) else {
            continue;
        };
        let head_column = column(line, s.head);
        let added = if s.anchor == s.head || line_of(s.anchor) != line {
            Selection::caret(at_column(next, goal.unwrap_or(head_column)))
        } else {
            Selection {
                anchor: at_column(next, column(line, s.anchor)),
                head: at_column(next, head_column),
            }
        };
        out.push(added);
    }
    normalize(&out, chars.len())
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn range(anchor: usize, head: usize) -> Selection {
        Selection { anchor, head }
    }

    #[test]
    fn regression_select_next_occurrence_keeps_other_carets() {
        let selected = occurrences(
            "first second",
            &[Selection::caret(1), Selection::caret(7)],
            false,
        );
        assert_eq!(selected, [range(0, 5), Selection::caret(7)]);
    }

    #[test]
    fn regression_select_all_from_a_caret_matches_the_whole_word() {
        let text = "cat bobcat catapult cat";
        assert_eq!(
            occurrences(text, &[Selection::caret(1)], true),
            [range(0, 3), range(20, 23)]
        );
        assert_eq!(
            occurrences(text, &[range(0, 3)], true),
            [range(0, 3), range(7, 10), range(11, 14), range(20, 23)]
        );
    }

    #[test]
    fn regression_column_navigation_continues_past_a_literal_carriage_return() {
        assert_eq!(at_column("a\rb\nz", 0, 2, 4), 3);
        assert_eq!(at_column("a\r\nz", 0, 2, 4), 1);
        let added = add_line_selections("xx\na\rb", &[Selection::caret(2)], true, 4, None);
        assert_eq!(added, [Selection::caret(2), Selection::caret(6)]);
    }

    #[test]
    fn regression_expand_to_indentation_uses_a_half_open_selection() {
        let text = "root\n  first\n  second\nnext";
        assert_eq!(expand_to_indentation(text, range(7, 22)), range(5, 22));
    }

    #[test]
    fn regression_expand_to_brackets_finds_the_parent_of_two_siblings() {
        assert_eq!(
            expand_to_brackets("((one) (two))", range(2, 11)),
            range(1, 12)
        );
    }

    #[test]
    fn regression_expand_selection_picks_the_nearest_quotes() {
        let text = "\"prefix 'x y' suffix\"";
        assert_eq!(expand_selection(text, range(9, 12)), range(8, 13));
    }
}
