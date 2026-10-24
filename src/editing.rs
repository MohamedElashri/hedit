//! UTF-8-safe editing commands. Cursor positions count Unicode scalars, as egui does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    SelectLine,
    Duplicate,
    DeleteLine,
    InsertBefore,
    InsertAfter,
    MoveUp,
    MoveDown,
    JoinLines,
    Indent,
    Unindent,
    Uppercase,
    Lowercase,
    SortLines,
    SortLinesIgnoreCase,
    ReverseLines,
    UniqueLines,
    TrimWhitespace,
    ToSpaces,
    ToTabs,
    ToggleComment,
    ToLf,
    ToCrlf,
}

pub struct Edit {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
}

fn byte(text: &str, character: usize) -> usize {
    text.char_indices()
        .nth(character)
        .map_or(text.len(), |(index, _)| index)
}
fn body(line: &str) -> (&str, &str) {
    if let Some(text) = line.strip_suffix("\r\n") {
        (text, "\r\n")
    } else if let Some(text) = line.strip_suffix('\n') {
        (text, "\n")
    } else {
        (line, "")
    }
}
fn line_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let left = text[..start].rfind('\n').map_or(0, |n| n + 1);
    let right = if start != end && text[..end].ends_with('\n') {
        end
    } else {
        text[end..].find('\n').map_or(text.len(), |n| end + n + 1)
    };
    (left, right)
}
fn swap(left: &str, right: &str) -> String {
    if right.ends_with('\n') {
        format!("{right}{left}")
    } else {
        let (left_body, ending) = body(left);
        format!("{right}{ending}{left_body}")
    }
}

pub fn comment_prefix(language: &str) -> Option<&'static str> {
    match language {
        "rust" | "c" | "cpp" | "cuda" | "javascript" | "typescript" | "java" | "go" | "swift"
        | "kotlin" | "scala" | "csharp" | "dart" | "php" => Some("//"),
        "python" | "shell" | "bash" | "ruby" | "perl" | "r" | "julia" | "toml" | "yaml"
        | "makefile" | "cmake" | "dockerfile" | "powershell" => Some("#"),
        "sql" | "lua" | "haskell" => Some("--"),
        "latex" | "tex" | "matlab" => Some("%"),
        _ => None,
    }
}

pub fn apply(
    text: &str,
    anchor: usize,
    head: usize,
    operation: Operation,
    width: usize,
    use_tabs: bool,
    comment: Option<&str>,
) -> Edit {
    let width = width.clamp(1, 8);
    let count = text.chars().count();
    let anchor = anchor.min(count);
    let head = head.min(count);
    let a = byte(text, anchor.min(head));
    let b = byte(text, anchor.max(head));
    let (mut start, mut end) = line_range(text, a, b);
    let selected = a != b;
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    if matches!(operation, Operation::Uppercase | Operation::Lowercase) {
        start = a;
        end = b;
    } else if !selected
        && matches!(
            operation,
            Operation::SortLines
                | Operation::SortLinesIgnoreCase
                | Operation::ReverseLines
                | Operation::UniqueLines
                | Operation::TrimWhitespace
                | Operation::ToSpaces
                | Operation::ToTabs
        )
    {
        start = 0;
        end = text.len();
    }
    let mut result = Edit {
        text: text.into(),
        anchor,
        head,
    };
    if matches!(operation, Operation::ToLf | Operation::ToCrlf) {
        // Rewrite the whole buffer, mapping cursors across CRLF pairs without
        // converting a literal standalone carriage return into a line break.
        let ending = if operation == Operation::ToCrlf {
            "\r\n"
        } else {
            "\n"
        };
        result.text.clear();
        let mut characters = text.chars().enumerate().peekable();
        let mut offset = 0;
        while let Some((i, ch)) = characters.next() {
            if i == anchor {
                result.anchor = offset;
            }
            if i == head {
                result.head = offset;
            }
            if ch == '\r' && characters.peek().is_some_and(|(_, next)| *next == '\n') {
                continue;
            }
            if ch == '\n' {
                result.text.push_str(ending);
                offset += ending.len();
            } else {
                result.text.push(ch);
                offset += 1;
            }
        }
        if anchor == count {
            result.anchor = offset;
        }
        if head == count {
            result.head = offset;
        }
        return result;
    }
    if matches!(operation, Operation::InsertBefore | Operation::InsertAfter) {
        let (left, right) = line_range(text, byte(text, head), byte(text, head));
        let content = body(&text[left..right]).0;
        let indent: String = content
            .chars()
            .take_while(|ch| matches!(ch, ' ' | '\t'))
            .collect();
        let (at, insertion, prefix) = if operation == Operation::InsertBefore {
            (left, format!("{indent}{eol}"), indent.chars().count())
        } else if right > left && text[..right].ends_with('\n') {
            (right, format!("{indent}{eol}"), indent.chars().count())
        } else {
            (
                right,
                format!("{eol}{indent}"),
                eol.chars().count() + indent.chars().count(),
            )
        };
        result.text.insert_str(at, &insertion);
        result.head = text[..at].chars().count() + prefix;
        result.anchor = result.head;
        return result;
    }
    if operation == Operation::SelectLine {
        result.anchor = text[..start].chars().count();
        result.head = text[..end].chars().count();
        return result;
    }
    if operation == Operation::Duplicate {
        let (left, right) = if selected { (a, b) } else { (start, end) };
        let original = &text[left..right];
        let insertion = if !selected && !original.ends_with('\n') {
            format!("{eol}{original}")
        } else {
            original.to_owned()
        };
        result.text.insert_str(right, &insertion);
        let extra = if insertion.starts_with(eol) && !selected && !original.ends_with('\n') {
            eol.chars().count()
        } else {
            0
        };
        if selected {
            result.anchor = text[..right].chars().count();
            result.head = result.anchor + insertion.chars().count();
        } else {
            result.head = text[..right].chars().count()
                + extra
                + text[left..byte(text, head)].chars().count();
            result.anchor = result.head;
        }
        return result;
    }
    if matches!(operation, Operation::MoveUp | Operation::MoveDown) {
        if start == end {
            return result;
        }
        let (replace_start, replace_end, replacement, new_start) = if operation == Operation::MoveUp
        {
            if start == 0 {
                return result;
            }
            let previous = line_range(text, start - 1, start - 1).0;
            (
                previous,
                end,
                swap(&text[previous..start], &text[start..end]),
                previous,
            )
        } else {
            if end == text.len() {
                return result;
            }
            let next_end = line_range(text, end, end).1;
            let next = &text[end..next_end];
            let shift = if next.ends_with('\n') {
                next.len()
            } else {
                next.len() + body(&text[start..end]).1.len()
            };
            (
                start,
                next_end,
                swap(&text[start..end], next),
                start + shift,
            )
        };
        result
            .text
            .replace_range(replace_start..replace_end, &replacement);
        let new_start_chars = result.text[..new_start].chars().count();
        let old_start_chars = text[..start].chars().count();
        result.anchor = new_start_chars + anchor.saturating_sub(old_start_chars);
        result.head = new_start_chars + head.saturating_sub(old_start_chars);
        let count = result.text.chars().count();
        result.anchor = result.anchor.min(count);
        result.head = result.head.min(count);
        return result;
    }
    if operation == Operation::JoinLines && !selected && end < text.len() {
        end = line_range(text, end, end).1;
    }
    let block = &text[start..end];
    let lines: Vec<_> = block.split_inclusive('\n').collect();
    if operation == Operation::JoinLines && lines.len() < 2 {
        return result;
    }
    let replacement = match operation {
        Operation::Indent if block.is_empty() => {
            if use_tabs {
                "\t".into()
            } else {
                " ".repeat(width)
            }
        }
        Operation::DeleteLine => String::new(),
        Operation::Uppercase => block.to_uppercase(),
        Operation::Lowercase => block.to_lowercase(),
        Operation::JoinLines => {
            let mut joined = String::new();
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    joined.push(' ');
                }
                let content = body(line).0.trim_end_matches([' ', '\t']);
                joined.push_str(if i == 0 {
                    content
                } else {
                    content.trim_start_matches([' ', '\t'])
                });
            }
            joined.push_str(lines.last().map_or("", |line| body(line).1));
            joined
        }
        Operation::SortLines | Operation::SortLinesIgnoreCase | Operation::ReverseLines => {
            let mut contents: Vec<_> = lines.iter().map(|line| body(line).0).collect();
            if operation == Operation::SortLines {
                contents.sort();
            } else if operation == Operation::SortLinesIgnoreCase {
                contents.sort_by_cached_key(|line| line.to_lowercase());
            } else {
                contents.reverse();
            }
            contents
                .iter()
                .zip(&lines)
                .map(|(content, original)| format!("{content}{}", body(original).1))
                .collect()
        }
        Operation::UniqueLines => {
            let mut seen = std::collections::HashSet::new();
            let unique: Vec<_> = lines
                .iter()
                .map(|line| body(line).0)
                .filter(|line| seen.insert(*line))
                .collect();
            let ending = lines.last().map_or("", |line| body(line).1);
            // Keep the final newline policy even when the last line was a duplicate.
            unique
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    format!(
                        "{line}{}",
                        if i + 1 == unique.len() {
                            ending
                        } else {
                            body(lines[i]).1
                        }
                    )
                })
                .collect()
        }
        _ => {
            let uncomment = comment.is_some_and(|prefix| {
                lines
                    .iter()
                    .filter(|line| !body(line).0.trim().is_empty())
                    .all(|line| body(line).0.trim_start().starts_with(prefix))
            });
            lines
                .iter()
                .map(|line| {
                    let (content, ending) = body(line);
                    let indentation = content.len() - content.trim_start_matches([' ', '\t']).len();
                    let leading = &content[..indentation];
                    let rest = &content[indentation..];
                    let converted = match operation {
                        Operation::Indent => format!(
                            "{}{content}",
                            if use_tabs {
                                "\t".into()
                            } else {
                                " ".repeat(width)
                            }
                        ),
                        Operation::Unindent => {
                            let remove = if content.starts_with('\t') {
                                1
                            } else {
                                content
                                    .bytes()
                                    .take(width)
                                    .take_while(|c| *c == b' ')
                                    .count()
                            };
                            content[remove..].to_owned()
                        }
                        Operation::TrimWhitespace => content.trim_end_matches([' ', '\t']).into(),
                        Operation::ToSpaces | Operation::ToTabs => {
                            let columns = leading.chars().fold(0, |col, ch| {
                                col + if ch == '\t' { width - col % width } else { 1 }
                            });
                            let indent = if operation == Operation::ToSpaces {
                                " ".repeat(columns)
                            } else {
                                format!(
                                    "{}{}",
                                    "\t".repeat(columns / width),
                                    " ".repeat(columns % width)
                                )
                            };
                            format!("{indent}{rest}")
                        }
                        Operation::ToggleComment => {
                            if rest.is_empty() {
                                content.into()
                            } else if let Some(prefix) = comment {
                                if uncomment {
                                    format!(
                                        "{leading}{}",
                                        rest.strip_prefix(prefix)
                                            .unwrap_or(rest)
                                            .strip_prefix(' ')
                                            .unwrap_or_else(|| rest
                                                .strip_prefix(prefix)
                                                .unwrap_or(rest))
                                    )
                                } else {
                                    format!("{leading}{prefix} {rest}")
                                }
                            } else {
                                content.into()
                            }
                        }
                        _ => content.into(),
                    };
                    format!("{converted}{ending}")
                })
                .collect()
        }
    };
    result.text.replace_range(start..end, &replacement);
    let left = text[..start].chars().count();
    let right = left + replacement.chars().count();
    if selected
        || matches!(
            operation,
            Operation::SortLines
                | Operation::SortLinesIgnoreCase
                | Operation::ReverseLines
                | Operation::UniqueLines
                | Operation::Uppercase
                | Operation::Lowercase
        )
    {
        (result.anchor, result.head) = if anchor > head {
            (right, left)
        } else {
            (left, right)
        };
    } else {
        let previous = text[start..byte(text, head)].chars().count();
        let delta = replacement.chars().count() as isize - block.chars().count() as isize;
        let cursor = if matches!(
            operation,
            Operation::Indent | Operation::Unindent | Operation::ToggleComment
        ) {
            previous.saturating_add_signed(delta)
        } else {
            previous
        };
        result.head = (left + cursor).min(right);
        result.anchor = result.head;
    }
    result
}
