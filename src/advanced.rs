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
pub fn replace(text: &str, selections: &[Selection], replacement: &str) -> Batch {
    let ranges = normalize(selections, text.chars().count());
    let mut offsets: Vec<_> = text.char_indices().map(|(i, _)| i).collect();
    offsets.push(text.len());
    let mut output = String::with_capacity(text.len());
    let mut previous = 0;
    let mut cursors = vec![];
    let mut count = 0;
    for range in ranges {
        let a = offsets[range.start()];
        let b = offsets[range.end()];
        output.push_str(&text[previous..a]);
        count += text[previous..a].chars().count();
        output.push_str(replacement);
        count += replacement.chars().count();
        cursors.push(Selection::caret(count));
        previous = b;
    }
    output.push_str(&text[previous..]);
    Batch {
        text: output,
        selections: cursors,
    }
}
pub fn word(text: &str, cursor: usize) -> Selection {
    let chars: Vec<_> = text.chars().collect();
    let mut a = cursor.min(chars.len());
    let mut b = a;
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    if a == chars.len() || !is_word(chars[a]) {
        if a == 0 || !is_word(chars[a - 1]) {
            return Selection::caret(a);
        }
        a -= 1;
        b = a;
    }
    while a > 0 && is_word(chars[a - 1]) {
        a -= 1;
    }
    while b < chars.len() && is_word(chars[b]) {
        b += 1;
    }
    Selection { anchor: a, head: b }
}
pub fn occurrences(text: &str, selections: &[Selection], all: bool) -> Vec<Selection> {
    let mut ranges = normalize(selections, text.chars().count());
    if ranges.is_empty() {
        ranges.push(Selection::caret(0));
    }
    if ranges[0].start() == ranges[0].end() {
        return vec![word(text, ranges[0].head)];
    }
    let needle = &text[byte(text, ranges[0].start())..byte(text, ranges[0].end())];
    if needle.is_empty() {
        return ranges;
    }
    let mut previous_byte = 0;
    let mut previous_char = 0;
    let matches: Vec<_> = text
        .match_indices(needle)
        .map(|(i, s)| {
            let a = previous_char + text[previous_byte..i].chars().count();
            previous_byte = i;
            previous_char = a;
            Selection {
                anchor: a,
                head: a + s.chars().count(),
            }
        })
        .collect();
    if all {
        return matches.into_iter().take(MAX_SELECTIONS).collect();
    }
    let end = ranges.last().unwrap().end();
    for m in matches
        .iter()
        .filter(|m| m.start() >= end)
        .chain(matches.iter().filter(|m| m.start() < end))
    {
        if !ranges
            .iter()
            .any(|s| s.start() < m.end() && m.start() < s.end())
            && ranges.len() < MAX_SELECTIONS
        {
            ranges.push(*m);
            break;
        }
    }
    normalize(&ranges, text.chars().count())
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
    for ch in text.chars() {
        if row == line {
            if ch == '\n' || ch == '\r' || col >= column {
                return offset;
            }
            col += if ch == '\t' { width - col % width } else { 1 };
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
        if [2, 4, 8].contains(&delta) {
            differences[delta] += 1;
        }
        previous = n;
    }
    let width = [4, 2, 8]
        .into_iter()
        .max_by_key(|n| (differences[*n], usize::from(*n == 4)))
        .unwrap();
    (tabs > spaces, width as u8)
}
pub fn newline(
    text: &str,
    selection: Selection,
    width: usize,
    tabs: bool,
    smart: bool,
    crlf: bool,
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
    let opens = line.trim_end().ends_with(['{', '[', '(']);
    let closes = after.trim_start().starts_with(['}', ']', ')']);
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
    pub fn edited(&self, source: &str, display: &str, selection: Selection) -> Batch {
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
            | editing::Operation::ReverseLines
            | editing::Operation::UniqueLines
            | editing::Operation::TrimWhitespace
            | editing::Operation::ToSpaces
            | editing::Operation::ToTabs
    );
    if global && ranges.iter().all(|s| s.start() == s.end()) {
        ranges.truncate(1);
    }
    if !matches!(
        operation,
        editing::Operation::Uppercase | editing::Operation::Lowercase
    ) {
        for s in &mut ranges {
            if operation == editing::Operation::Duplicate && s.start() != s.end() {
                continue;
            }
            let start = byte(text, s.start());
            let end = byte(text, s.end());
            let a = text[..start].rfind('\n').map_or(0, |n| n + 1);
            let b = if start != end && text[..end].ends_with('\n') {
                end
            } else {
                text[end..].find('\n').map_or(text.len(), |n| end + n + 1)
            };
            s.anchor = text[..a].chars().count();
            s.head = text[..b].chars().count();
        }
        ranges = normalize(&ranges, text.chars().count());
    }
    let mut output = text.to_owned();
    let mut final_ranges: Vec<Selection> = vec![];
    for s in ranges.into_iter().rev() {
        let edit = editing::apply(&output, s.anchor, s.head, operation, width, tabs, comment);
        let delta = edit.text.chars().count() as isize - output.chars().count() as isize;
        for r in &mut final_ranges {
            r.anchor = r.anchor.saturating_add_signed(delta);
            r.head = r.head.saturating_add_signed(delta);
        }
        final_ranges.push(Selection {
            anchor: edit.anchor,
            head: edit.head,
        });
        output = edit.text;
    }
    Batch {
        text: output.clone(),
        selections: normalize(&final_ranges, output.chars().count()),
    }
}
