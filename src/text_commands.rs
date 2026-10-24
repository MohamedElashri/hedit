//! Text commands on selections: block comments, paragraph wrapping,
//! transposing, case changes, deleting to a line edge, and reindenting.
//! Positions are Unicode scalar indices, as in `advanced`.
use crate::advanced::{normalize, word, Batch, Selection};

/// One replacement of `chars[start..end]`, and the selection to leave behind,
/// relative to where the replacement starts in the new text.
struct Change {
    /// The selection this change replaces; selections without one stay put.
    from: Selection,
    start: usize,
    end: usize,
    text: String,
    anchor: usize,
    head: usize,
}

/// Apply changes from left to right; overlapping ones after the first are
/// dropped. Selections that no change replaced move with the text.
fn apply(chars: &[char], mut changes: Vec<Change>, selections: &[Selection]) -> Batch {
    let originals = normalize(selections, chars.len());
    changes.sort_by_key(|c| (c.start, c.end));
    let mut text = String::with_capacity(chars.len());
    let mut count = 0;
    let mut previous = 0;
    let mut out = vec![];
    let mut applied: Vec<(usize, usize, isize)> = vec![];
    let mut replaced = vec![];
    for change in changes {
        if change.start < previous {
            continue;
        }
        text.extend(&chars[previous..change.start]);
        count += change.start - previous;
        out.push(Selection {
            anchor: count + change.anchor,
            head: count + change.head,
        });
        let length = change.text.chars().count();
        count += length;
        text.push_str(&change.text);
        previous = change.end;
        applied.push((
            change.start,
            change.end,
            length as isize - (change.end - change.start) as isize,
        ));
        replaced.push(change.from);
    }
    text.extend(&chars[previous..]);
    let moved = |p: usize| {
        let shift: isize = applied
            .iter()
            .filter(|(_, end, _)| *end <= p)
            .map(|(_, _, delta)| delta)
            .sum();
        p.saturating_add_signed(shift)
    };
    for s in originals {
        let covered = replaced.contains(&s)
            || applied
                .iter()
                .any(|&(a, b, _)| a <= s.start() && s.end() <= b);
        if !covered {
            out.push(Selection {
                anchor: moved(s.anchor),
                head: moved(s.head),
            });
        }
    }
    let length = count + chars.len() - previous;
    Batch {
        text,
        selections: normalize(&out, length),
    }
}

fn line_start(chars: &[char], at: usize) -> usize {
    chars[..at]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |n| n + 1)
}

/// The end of the line's text, before `\n` or `\r\n`.
fn line_end(chars: &[char], at: usize) -> usize {
    let end = chars[at..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |n| at + n);
    if end > at && chars[end - 1] == '\r' && end < chars.len() {
        end - 1
    } else {
        end
    }
}

fn starts_with(chars: &[char], at: usize, pattern: &[char]) -> bool {
    chars.len() >= at + pattern.len() && chars[at..at + pattern.len()] == *pattern
}

fn columns(text: &[char], width: usize) -> usize {
    text.iter().fold(0, |col, &c| {
        if c == '\t' {
            col + width - col % width
        } else {
            col + 1
        }
    })
}

/// Block comment delimiters for a syntax, when it has them.
pub fn block_comment(language: &str) -> Option<(&'static str, &'static str)> {
    match language {
        "c" | "cpp" | "cuda" | "csharp" | "css" | "dart" | "go" | "groovy" | "hcl" | "java"
        | "javascript" | "kotlin" | "less" | "nix" | "objective-c" | "php" | "protobuf"
        | "rust" | "scala" | "scss" | "sql" | "swift" | "typescript" | "verilog" => {
            Some(("/*", "*/"))
        }
        "html" | "xml" | "markdown" => Some(("<!--", "-->")),
        "haskell" => Some(("{-", "-}")),
        "lua" => Some(("--[[", "]]")),
        "julia" => Some(("#=", "=#")),
        "powershell" => Some(("<#", "#>")),
        _ => None,
    }
}

/// Wrap each selection in a block comment, or remove the comment around it.
/// An empty selection comments its line, or uncomments the comment it is in.
pub fn toggle_block_comment(
    text: &str,
    selections: &[Selection],
    open: &str,
    close: &str,
) -> Batch {
    let chars: Vec<char> = text.chars().collect();
    let open: Vec<char> = open.chars().collect();
    let close: Vec<char> = close.chars().collect();
    let mut changes = vec![];
    for s in normalize(selections, chars.len()) {
        let empty = s.start() == s.end();
        let (mut a, mut b) = if empty {
            (line_start(&chars, s.head), line_end(&chars, s.head))
        } else {
            (s.start(), s.end())
        };
        while a < b && chars[a].is_whitespace() {
            a += 1;
        }
        while b > a && chars[b - 1].is_whitespace() {
            b -= 1;
        }
        let wrapped = b - a >= open.len() + close.len()
            && starts_with(&chars, a, &open)
            && starts_with(&chars, b - close.len(), &close);
        let around = if wrapped {
            Some((a, b))
        } else if empty {
            enclosing_comment(&chars, s.head, &open, &close)
        } else {
            None
        };
        if let Some((a, b)) = around {
            let mut inner = (a + open.len(), b - close.len());
            // `/* text */` loses its inner spaces too.
            if inner.1 > inner.0 + 1 && chars[inner.0] == ' ' && chars[inner.1 - 1] == ' ' {
                inner = (inner.0 + 1, inner.1 - 1);
            }
            let length = inner.1 - inner.0;
            let (anchor, head) = if empty {
                let caret = s.head.saturating_sub(inner.0).min(length);
                (caret, caret)
            } else if s.anchor <= s.head {
                (0, length)
            } else {
                (length, 0)
            };
            changes.push(Change {
                from: s,
                start: a,
                end: b,
                text: chars[inner.0..inner.1].iter().collect(),
                anchor,
                head,
            });
        } else if a == b {
            // A blank line gets an empty comment with the caret inside.
            changes.push(Change {
                from: s,
                start: s.head,
                end: s.head,
                text: open.iter().chain(&close).collect(),
                anchor: open.len(),
                head: open.len(),
            });
        } else {
            let length = b - a;
            let (anchor, head) = if empty {
                let caret = open.len() + s.head.clamp(a, b) - a;
                (caret, caret)
            } else if s.anchor <= s.head {
                (open.len(), open.len() + length)
            } else {
                (open.len() + length, open.len())
            };
            changes.push(Change {
                from: s,
                start: a,
                end: b,
                text: open.iter().chain(&chars[a..b]).chain(&close).collect(),
                anchor,
                head,
            });
        }
    }
    apply(&chars, changes, selections)
}

/// The block comment containing `at`, from its opening to its closing marker.
fn enclosing_comment(
    chars: &[char],
    at: usize,
    open: &[char],
    close: &[char],
) -> Option<(usize, usize)> {
    let start = (0..at.min(chars.len()))
        .rev()
        .find(|&i| starts_with(chars, i, open) && in_code(chars, i))?;
    // A closing marker between the opening and the caret ends that comment.
    let closed_before =
        (start + open.len()..at).any(|i| i + close.len() <= at && starts_with(chars, i, close));
    if closed_before {
        return None;
    }
    let end = (start + open.len()..chars.len()).find(|&i| starts_with(chars, i, close))?;
    Some((start, end + close.len()))
}

/// Whether `at` is outside strings and line comments on its line, so a
/// comment marker there is real.
fn in_code(chars: &[char], at: usize) -> bool {
    let mut quote: Option<char> = None;
    let mut i = line_start(chars, at);
    while i < at {
        let c = chars[i];
        match quote {
            Some(_) if c == '\\' => i += 1,
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '`' => quote = Some(c),
            None if starts_with(chars, i, &['/', '/']) => return false,
            None => {}
        }
        i += 1;
    }
    quote.is_none()
}

/// Line prefixes kept when wrapping: comment markers and quotes.
pub fn wrap_markers(language: &str, comment: Option<&'static str>) -> Vec<&'static str> {
    let mut markers: Vec<&'static str> = comment.into_iter().collect();
    if comment == Some("//") {
        markers.insert(0, "///");
        markers.push("*");
    }
    if matches!(language, "markdown" | "plain-text") {
        markers.push(">");
    }
    markers
}

/// A line's indentation and comment marker, with the spaces after it.
fn wrap_prefix(line: &[char], markers: &[&str]) -> usize {
    let mut at = line.iter().take_while(|c| c.is_whitespace()).count();
    for marker in markers {
        let marker: Vec<char> = marker.chars().collect();
        if starts_with(line, at, &marker) {
            at += marker.len();
            at += line[at..].iter().take_while(|&&c| c == ' ').count();
            break;
        }
    }
    at
}

/// The width of a list bullet at the start of `text`, such as `- ` or `12. `.
fn bullet(text: &[char]) -> usize {
    match text {
        ['-' | '*' | '+', ' ', ..] => 2,
        _ => {
            let digits = text.iter().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && text[digits..].starts_with(&['.', ' ']) {
                digits + 2
            } else {
                0
            }
        }
    }
}

/// Rewrap the selected lines, or the paragraph at the cursor, to `width`
/// columns, keeping indentation, comment markers, and list bullets.
pub fn wrap_lines(
    text: &str,
    selections: &[Selection],
    width: usize,
    tab_width: usize,
    markers: &[&str],
) -> Batch {
    let chars: Vec<char> = text.chars().collect();
    let tab_width = tab_width.max(1);
    let blank = |start: usize| {
        let end = line_end(&chars, start);
        let line = &chars[start..end];
        wrap_prefix(line, markers) == line.len()
    };
    // The comment marker or quote a line starts with, if any.
    let marker = |start: usize| -> String {
        let line = &chars[start..line_end(&chars, start)];
        line[..wrap_prefix(line, markers)]
            .iter()
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    let mut changes = vec![];
    for s in normalize(selections, chars.len()) {
        let mut first = line_start(&chars, s.start());
        let last_at = if s.end() > s.start() && s.end() == line_start(&chars, s.end()) {
            s.end() - 1
        } else {
            s.end()
        };
        let mut last = line_start(&chars, last_at);
        if s.start() == s.end() {
            if blank(first) {
                continue;
            }
            // A paragraph ends at a blank line or where the marker changes.
            let kind = marker(first);
            let same = |start: usize| !blank(start) && marker(start) == kind;
            // A list item starts a paragraph of its own.
            let item = |start: usize| {
                let line = &chars[start..line_end(&chars, start)];
                bullet(&line[wrap_prefix(line, markers)..]) > 0
            };
            while first > 0 && !item(first) && same(line_start(&chars, first - 1)) {
                first = line_start(&chars, first - 1);
            }
            loop {
                let next = chars[last..]
                    .iter()
                    .position(|&c| c == '\n')
                    .map(|n| last + n + 1);
                match next {
                    Some(next) if next < chars.len() && same(next) && !item(next) => last = next,
                    _ => break,
                }
            }
        }
        let end = line_end(&chars, last);
        let crlf = chars[end..].starts_with(&['\r', '\n']);
        let first_line = &chars[first..line_end(&chars, first)];
        let prefix_length = wrap_prefix(first_line, markers);
        let prefix: String = first_line[..prefix_length].iter().collect();
        let hanging = bullet(&first_line[prefix_length..]);
        let continuation = format!("{prefix}{}", " ".repeat(hanging));
        let mut words: Vec<String> = vec![];
        let mut at = first;
        while at <= end {
            let stop = line_end(&chars, at);
            let line = &chars[at..stop];
            let content = &line[wrap_prefix(line, markers)..];
            words.extend(
                content
                    .iter()
                    .collect::<String>()
                    .split_whitespace()
                    .map(str::to_owned),
            );
            at = chars[at..]
                .iter()
                .position(|&c| c == '\n')
                .map_or(usize::MAX, |n| at + n + 1);
        }
        if words.is_empty() {
            continue;
        }
        let eol = if crlf { "\r\n" } else { "\n" };
        let mut output = prefix.clone();
        let mut column = columns(&prefix.chars().collect::<Vec<_>>(), tab_width);
        let continuation_columns = columns(&continuation.chars().collect::<Vec<_>>(), tab_width);
        let mut line_has_word = false;
        for word in words {
            let length = word.chars().count();
            if line_has_word && column + 1 + length > width {
                output.push_str(eol);
                output.push_str(&continuation);
                column = continuation_columns;
                line_has_word = false;
            }
            if line_has_word {
                output.push(' ');
                column += 1;
            }
            output.push_str(&word);
            column += length;
            line_has_word = true;
        }
        let length = output.chars().count();
        changes.push(Change {
            from: s,
            start: first,
            end,
            text: output,
            anchor: length,
            head: length,
        });
    }
    apply(&chars, changes, selections)
}

/// Swap the characters around each cursor, or rotate the text of several
/// selections.
pub fn transpose(text: &str, selections: &[Selection]) -> Batch {
    let chars: Vec<char> = text.chars().collect();
    let ranges = normalize(selections, chars.len());
    let mut changes = vec![];
    if ranges.len() > 1 && ranges.iter().all(|s| s.start() != s.end()) {
        let pieces: Vec<String> = ranges
            .iter()
            .map(|s| chars[s.start()..s.end()].iter().collect())
            .collect();
        for (i, s) in ranges.iter().enumerate() {
            let text = pieces[(i + pieces.len() - 1) % pieces.len()].clone();
            let length = text.chars().count();
            changes.push(Change {
                from: *s,
                start: s.start(),
                end: s.end(),
                text,
                anchor: 0,
                head: length,
            });
        }
        return apply(&chars, changes, selections);
    }
    for s in ranges {
        let h = s.head;
        if s.start() != s.end() || h == 0 || chars[h - 1] == '\n' {
            continue;
        }
        let at_end = h == chars.len() || chars[h] == '\n' || chars[h] == '\r';
        let (start, caret) = if at_end {
            if h < 2 || chars[h - 2] == '\n' {
                continue;
            }
            (h - 2, 2)
        } else {
            (h - 1, 2)
        };
        changes.push(Change {
            from: s,
            start,
            end: start + 2,
            text: [chars[start + 1], chars[start]].iter().collect(),
            anchor: caret,
            head: caret,
        });
    }
    apply(&chars, changes, selections)
}

/// Title Case or sWAP cASE for each selection, or the word at each cursor.
pub fn change_case(text: &str, selections: &[Selection], title: bool) -> Batch {
    let chars: Vec<char> = text.chars().collect();
    let mut changes = vec![];
    for s in normalize(selections, chars.len()) {
        let range = if s.start() == s.end() {
            word(text, s.head)
        } else {
            s
        };
        let mut previous_letter = range.start() > 0 && is_word(chars[range.start() - 1]);
        let mut changed = String::new();
        for &c in &chars[range.start()..range.end()] {
            if title {
                if previous_letter {
                    changed.extend(c.to_lowercase());
                } else {
                    changed.extend(c.to_uppercase());
                }
            } else if c.is_uppercase() {
                changed.extend(c.to_lowercase());
            } else {
                changed.extend(c.to_uppercase());
            }
            previous_letter = is_word(c);
        }
        let length = changed.chars().count();
        let (anchor, head) = if s.start() == s.end() {
            let caret = (s.head - range.start()).min(length);
            (caret, caret)
        } else if s.anchor <= s.head {
            (0, length)
        } else {
            (length, 0)
        };
        changes.push(Change {
            from: s,
            start: range.start(),
            end: range.end(),
            text: changed,
            anchor,
            head,
        });
    }
    apply(&chars, changes, selections)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '\''
}

/// Delete from each cursor to the start or end of its line. At the edge of a
/// line, the line break goes instead, joining the lines.
pub fn delete_to_line_edge(text: &str, selections: &[Selection], end: bool) -> Batch {
    let chars: Vec<char> = text.chars().collect();
    let mut changes = vec![];
    for s in normalize(selections, chars.len()) {
        let h = s.head;
        let (start, stop) = if s.start() != s.end() {
            (s.start(), s.end())
        } else if end {
            let edge = line_end(&chars, h);
            if edge > h {
                (h, edge)
            } else if chars[h..].starts_with(&['\r', '\n']) {
                (h, h + 2)
            } else {
                (h, (h + 1).min(chars.len()))
            }
        } else {
            let edge = line_start(&chars, h);
            if edge < h {
                (edge, h)
            } else if h >= 2 && chars[h - 2..h] == ['\r', '\n'] {
                (h - 2, h)
            } else {
                (h.saturating_sub(1), h)
            }
        };
        if start < stop {
            changes.push(Change {
                from: s,
                start,
                end: stop,
                text: String::new(),
                anchor: 0,
                head: 0,
            });
        }
    }
    apply(&chars, changes, selections)
}

/// Syntaxes whose blocks are marked by brackets, which Reindent understands.
pub fn reindents(language: &str) -> bool {
    matches!(
        language,
        "c" | "cpp"
            | "cuda"
            | "csharp"
            | "css"
            | "dart"
            | "go"
            | "groovy"
            | "hcl"
            | "java"
            | "javascript"
            | "json"
            | "kotlin"
            | "less"
            | "nix"
            | "objective-c"
            | "php"
            | "protobuf"
            | "rust"
            | "scala"
            | "scss"
            | "swift"
            | "typescript"
            | "zig"
    )
}

/// What a line does to the bracket depth.
#[derive(Default, Clone, Copy)]
struct Brackets {
    /// Closing brackets before any other text on the line.
    leading_closers: isize,
    /// Opening minus closing brackets.
    net: isize,
    /// The line starts inside a block comment.
    in_comment: bool,
}

/// Bracket counts for each line, skipping strings and comments.
fn scan_brackets(chars: &[char]) -> Vec<Brackets> {
    let mut lines = vec![Brackets::default()];
    let mut in_comment = false;
    let mut leading = true;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let line = lines.last_mut().unwrap();
        if c == '\n' {
            lines.push(Brackets {
                in_comment,
                ..Default::default()
            });
            leading = true;
            i += 1;
            continue;
        }
        if in_comment {
            if starts_with(chars, i, &['*', '/']) {
                in_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if starts_with(chars, i, &['/', '/']) {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if starts_with(chars, i, &['/', '*']) {
            in_comment = true;
            leading = false;
            i += 2;
            continue;
        }
        match c {
            // A quote that does not close soon is a Rust lifetime or an
            // apostrophe, not a character literal.
            '\'' if !char_literal(chars, i) => leading = false,
            '"' | '\'' | '`' => {
                leading = false;
                i += 1;
                // A string ends at its closing quote or, unclosed, at the end
                // of the line; the newline itself is left for the line count.
                while i < chars.len() && chars[i] != c && chars[i] != '\n' {
                    let escaped = chars[i] == '\\' && chars.get(i + 1).is_some_and(|&n| n != '\n');
                    i += if escaped { 2 } else { 1 };
                }
                if chars.get(i) == Some(&c) {
                    i += 1;
                }
                continue;
            }
            '{' | '[' | '(' => {
                leading = false;
                line.net += 1;
            }
            '}' | ']' | ')' => {
                if leading {
                    line.leading_closers += 1;
                }
                line.net -= 1;
            }
            c if c.is_whitespace() => {}
            _ => leading = false,
        }
        i += 1;
    }
    lines
}

/// Whether the `'` at `at` starts a character literal such as `'x'` or `'\\n'`.
fn char_literal(chars: &[char], at: usize) -> bool {
    match chars.get(at + 1) {
        Some('\\') => chars[at + 2..].iter().take(10).any(|&c| c == '\''),
        Some(_) => chars.get(at + 2) == Some(&'\''),
        None => false,
    }
}

/// Indent the selected lines, or the line at each cursor, by bracket depth,
/// starting from the line above.
pub fn reindent(text: &str, selections: &[Selection], tab_width: usize, tabs: bool) -> Batch {
    let chars: Vec<char> = text.chars().collect();
    let width = tab_width.max(1);
    let brackets = scan_brackets(&chars);
    let mut starts = vec![0];
    starts.extend(
        chars
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i + 1),
    );
    let line_of = |at: usize| starts.partition_point(|&s| s <= at) - 1;
    let indent_of = |line: usize| {
        let start = starts[line];
        let length = chars[start..]
            .iter()
            .take_while(|&&c| c == ' ' || c == '\t')
            .count();
        (start, start + length)
    };
    let is_blank = |line: usize| {
        let (_, end) = indent_of(line);
        end == chars.len() || chars[end] == '\n' || chars[end] == '\r'
    };
    let step = |level: isize, b: Brackets| level + (b.net + b.leading_closers).clamp(-1, 1);
    let unit = if tabs {
        "\t".to_owned()
    } else {
        " ".repeat(width)
    };
    // Merge selections into runs of whole lines. A run made by one cursor
    // keeps the cursor's place in the text.
    let mut runs: Vec<(usize, usize, Option<usize>)> = vec![];
    for s in normalize(selections, chars.len()) {
        let first = line_of(s.start());
        let mut last = line_of(s.end());
        if s.end() > s.start() && s.end() == starts[last] && last > first {
            last -= 1;
        }
        let caret = (s.start() == s.end()).then_some(s.head);
        match runs.last_mut() {
            Some(run) if run.1 + 1 >= first => {
                run.1 = run.1.max(last);
                run.2 = None;
            }
            _ => runs.push((first, last, caret)),
        }
    }
    let mut changes = vec![];
    for (first, last, caret) in runs {
        let mut level = match (0..first).rev().find(|&l| !is_blank(l)) {
            Some(above) => {
                let (start, end) = indent_of(above);
                let own = (columns(&chars[start..end], width) / width) as isize;
                step(own, brackets.get(above).copied().unwrap_or_default())
            }
            None => 0,
        };
        let mut output = String::new();
        for line in first..=last {
            let (start, end) = indent_of(line);
            let stop = line_end(&chars, start);
            let b = brackets.get(line).copied().unwrap_or_default();
            if !b.in_comment && !is_blank(line) {
                let own = (level - isize::from(b.leading_closers > 0)).max(0);
                output.push_str(&unit.repeat(own as usize));
                level = step(own, b);
            } else if b.in_comment {
                output.extend(&chars[start..end]);
            }
            output.extend(&chars[end..stop]);
            if line < last {
                output.extend(&chars[stop..starts[line + 1]]);
            }
        }
        let start = starts[first];
        let end = line_end(&chars, starts[last]);
        let length = output.chars().count();
        let (anchor, head) = match caret {
            Some(caret) => {
                let (_, old_indent) = indent_of(first);
                let new_indent = output
                    .chars()
                    .take_while(|&c| c == ' ' || c == '\t')
                    .count();
                let at = (new_indent + caret.saturating_sub(old_indent)).min(length);
                (at, at)
            }
            None => (0, length),
        };
        changes.push(Change {
            from: Selection::caret(start),
            start,
            end,
            text: output,
            anchor,
            head,
        });
    }
    apply(&chars, changes, selections)
}
