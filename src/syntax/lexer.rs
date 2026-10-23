use super::{Language, Theme};
use eframe::egui::{self, Color32, FontId, TextFormat};

pub(super) fn highlight(
    text: &str,
    font_size: f32,
    wrap_width: f32,
    language: Language,
    theme: &Theme,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut append = |text: &str, color| {
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: FontId::monospace(font_size),
                color,
                ..Default::default()
            },
        );
    };
    if language == Language::PlainText {
        append(text, theme.text.color());
        return job;
    }
    // Merge adjacent tokens of the same color instead of creating a layout
    // section per punctuation/whitespace character on a large page.
    let mut at = 0;
    let mut run_start = 0;
    let mut run_color = theme.text.color();
    let mut markup_tag = Markup::Text;
    while at < text.len() {
        let (len, color) = token(text, at, language, theme, &mut markup_tag);
        if color != run_color {
            if at > run_start {
                append(&text[run_start..at], run_color);
            }
            run_start = at;
            run_color = color;
        }
        at += len;
    }
    if at > run_start {
        append(&text[run_start..at], run_color);
    }
    job
}

fn token(
    text: &str,
    at: usize,
    language: Language,
    theme: &Theme,
    markup_tag: &mut Markup,
) -> (usize, Color32) {
    use Language::*;
    let rest = &text[at..];
    let first = rest.chars().next().unwrap();
    let line_len = || rest.find('\n').unwrap_or(rest.len());
    if matches!(language, Html | Xml) {
        return markup(rest, markup_tag, theme);
    }
    if let Some(token) = format_token(text, at, language, theme) {
        return token;
    }
    let c_style = matches!(
        language,
        C | Cpp
            | Cuda
            | Go
            | JavaScript
            | TypeScript
            | Rust
            | CSharp
            | Dart
            | Groovy
            | Hcl
            | Java
            | Kotlin
            | ObjectiveC
            | Php
            | Protobuf
            | Scala
            | Swift
            | Verilog
            | Zig
            | Scss
            | Less
    );
    if let Some(len) = directive(text, at, language) {
        return (len, theme.preprocessor.color());
    }
    if matches!(language, C | Cpp | Cuda | ObjectiveC) && first == '<' {
        let line = &text[text[..at].rfind('\n').map_or(0, |n| n + 1)..at];
        if line.trim_start().starts_with('#') && line.contains("include") {
            if let Some(end) = rest
                .find(['>', '\n'])
                .filter(|&n| rest.as_bytes()[n] == b'>')
            {
                return (end + 1, theme.string.color());
            }
        }
    }
    if c_style && rest.starts_with("//") {
        return (line_len(), theme.comment.color());
    }
    if (c_style || matches!(language, Css | Nix)) && rest.starts_with("/*") {
        return (
            block_comment(rest, matches!(language, Rust | Swift | Kotlin | Scala)),
            theme.comment.color(),
        );
    }
    if first == '#'
        && matches!(
            language,
            Python
                | Shell
                | Toml
                | Yaml
                | CMake
                | Dockerfile
                | Elixir
                | Graphql
                | Hcl
                | Ini
                | Julia
                | Makefile
                | Nix
                | Perl
                | Php
                | PowerShell
                | R
                | Ruby
        )
        && (!matches!(language, Shell | Yaml)
            || at == 0
            || text[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace))
    {
        return (line_len(), theme.comment.color());
    }
    if language == Markdown {
        if (at == 0 || text.as_bytes()[at - 1] == b'\n') && matches!(first, '#' | '>') {
            return (
                line_len(),
                if first == '#' {
                    theme.keyword.color()
                } else {
                    theme.comment.color()
                },
            );
        }
        if rest.starts_with("```") {
            return (delimited(rest, "```"), theme.string.color());
        }
        if first == '`' {
            return (quoted(rest, first), theme.string.color());
        }
        return (first.len_utf8(), theme.text.color());
    }
    if matches!(
        language,
        Python | Toml | Julia | Kotlin | Scala | Groovy | Dart | Swift
    ) && (rest.starts_with("\"\"\"") || rest.starts_with("'''"))
    {
        return (delimited(rest, &rest[..3]), theme.string.color());
    }
    let lifetime = language == Rust
        && first == '\''
        && rest[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
        && rest[1..].chars().nth(1) != Some('\'');
    let transpose = matches!(language, Matlab | Julia)
        && text[..at]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || matches!(c, ')' | ']' | '.'));
    let quote = first == '"'
        || (first == '\'' && !matches!(language, Json | Log | Csv) && !lifetime && !transpose)
        || (first == '`' && matches!(language, Go | JavaScript | TypeScript | Shell | R | Julia));
    if quote {
        let len = quoted(rest, first);
        let key = matches!(language, Json | Yaml) && rest[len..].trim_start().starts_with(':');
        return (
            len,
            if key {
                theme.property.color()
            } else {
                theme.string.color()
            },
        );
    }
    if first.is_ascii_digit() {
        let len = rest
            .char_indices()
            .find(|&(i, c)| {
                !(c.is_ascii_alphanumeric()
                    || c == '_'
                    || c == '.'
                    || (matches!(c, '+' | '-')
                        && i > 0
                        && matches!(rest.as_bytes()[i - 1], b'e' | b'E' | b'p' | b'P')))
            })
            .map_or(rest.len(), |(n, _)| n);
        return (len, theme.number.color());
    }
    if first.is_alphabetic() || first == '_' || first == '$' {
        let len = rest
            .char_indices()
            .find(|(_, c)| {
                !(c.is_alphanumeric()
                    || matches!(c, '_' | '$')
                    || (*c == '-'
                        && matches!(language, Css | Scss | Less | PowerShell | CMake | Hcl)))
            })
            .map_or(rest.len(), |(n, _)| n);
        let word = &rest[..len];
        let after = rest[len..].trim_start();
        let property = (matches!(language, Css | Scss | Less | Yaml) && after.starts_with(':'))
            || (matches!(language, Ini | Toml | Hcl | Nix) && after.starts_with('='))
            || (language == CMake && after.starts_with('('));
        let code = is_code(language);
        let color = if language == Cuda && cuda_keyword(word) {
            theme.cuda
        } else if property {
            theme.property
        } else if super::keywords::contains(language, word) {
            theme.keyword
        } else if code
            && (after.starts_with('(') || (language == Rust && rest[len..].starts_with('!')))
        {
            theme.function
        } else if code
            && word.len() > 1
            && word.bytes().any(|b| b.is_ascii_uppercase())
            && word
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            theme.constant
        } else if code && first.is_uppercase() {
            theme.type_name
        } else if language == Log {
            match word {
                "ERROR" | "FATAL" | "CRITICAL" | "error" | "fatal" | "critical" => theme.error,
                "WARN" | "WARNING" | "warn" | "warning" => theme.warning,
                "INFO" | "info" => theme.success,
                "DEBUG" | "TRACE" | "debug" | "trace" => theme.comment,
                _ => theme.text,
            }
        } else {
            theme.text
        };
        return (len, color.color());
    }
    if is_code(language) && OPERATORS.contains(first) {
        let len = rest
            .find(|c: char| !OPERATORS.contains(c))
            .unwrap_or(rest.len());
        return (len, theme.operator.color());
    }
    (first.len_utf8(), theme.text.color())
}

const OPERATORS: &str = "+-*/%=<>!&|^~?:";

/// Languages whose identifiers can be classified as functions, types, and constants.
fn is_code(language: Language) -> bool {
    use Language::*;
    !matches!(
        language,
        PlainText
            | Log
            | Csv
            | Json
            | Ini
            | Yaml
            | Toml
            | Markdown
            | Diff
            | Latex
            | Sql
            | Fortran
            | VisualBasic
            | Batch
            | Assembly
            | Vhdl
            | Html
            | Xml
            | Css
            | Scss
            | Less
            | CMake
            | Makefile
            | Dockerfile
    )
}

/// Preprocessor directives, attributes, and decorators at `at`.
fn directive(text: &str, at: usize, language: Language) -> Option<usize> {
    use Language::*;
    let rest = &text[at..];
    let word_end = |from: usize| {
        rest[from..]
            .find(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map_or(rest.len(), |n| n + from)
    };
    match language {
        C | Cpp | Cuda | ObjectiveC if rest.starts_with('#') => {
            let line = &text[text[..at].rfind('\n').map_or(0, |n| n + 1)..at];
            if !line.trim().is_empty() {
                return None;
            }
            let spaces = rest[1..].len() - rest[1..].trim_start_matches([' ', '\t']).len();
            Some(word_end(1 + spaces))
        }
        Rust if rest.starts_with("#[") || rest.starts_with("#![") => {
            let line = rest.find('\n').unwrap_or(rest.len());
            Some(rest[..line].rfind(']').map_or(line, |n| n + 1))
        }
        Python | Java | Kotlin | Scala | Swift | Dart | TypeScript | JavaScript | Groovy
            if rest.starts_with('@')
                && rest[1..].starts_with(|c: char| c.is_alphabetic() || c == '_') =>
        {
            Some(word_end(1))
        }
        _ => None,
    }
}

fn quoted(rest: &str, quote: char) -> usize {
    let mut escaped = false;
    for (at, c) in rest[quote.len_utf8()..].char_indices() {
        if !escaped && c == quote {
            return at + 2 * quote.len_utf8();
        }
        escaped = !escaped && c == '\\';
    }
    rest.len()
}

fn delimited(rest: &str, delimiter: &str) -> usize {
    rest[delimiter.len()..]
        .find(delimiter)
        .map_or(rest.len(), |n| n + 2 * delimiter.len())
}

fn block_comment(rest: &str, nested: bool) -> usize {
    let mut depth = 1;
    let bytes = rest.as_bytes();
    let mut at = 2;
    while at + 1 < bytes.len() {
        match &bytes[at..at + 2] {
            b"/*" if nested => {
                depth += 1;
                at += 2;
            }
            b"*/" => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return at;
                }
            }
            _ => at += 1,
        }
    }
    rest.len()
}

pub(super) fn cuda_keyword(word: &str) -> bool {
    matches!(
        word,
        "__global__"
            | "__device__"
            | "__host__"
            | "__shared__"
            | "__constant__"
            | "__managed__"
            | "__syncthreads"
            | "threadIdx"
            | "blockIdx"
            | "blockDim"
            | "gridDim"
            | "warpSize"
            | "atomicAdd"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Markup {
    Text,
    TagName,
    Attributes,
}

fn markup(rest: &str, state: &mut Markup, theme: &Theme) -> (usize, Color32) {
    if let Some(stripped) = rest.strip_prefix("<!--") {
        return (
            stripped.find("-->").map_or(rest.len(), |n| n + 7),
            theme.comment.color(),
        );
    }
    if let Some(stripped) = rest.strip_prefix("<![CDATA[") {
        return (
            stripped.find("]]>").map_or(rest.len(), |n| n + 12),
            theme.string.color(),
        );
    }
    let first = rest.chars().next().unwrap();
    if first == '<' {
        *state = Markup::TagName;
        return (1, theme.keyword.color());
    }
    if first == '>' {
        *state = Markup::Text;
        return (1, theme.keyword.color());
    }
    if first == '&' {
        if let Some(end) = rest
            .char_indices()
            .take(64)
            .find_map(|(at, c)| (c == ';').then_some(at))
        {
            return (end + 1, theme.string.color());
        }
    }
    if *state != Markup::Text {
        if matches!(first, '\'' | '"') {
            return (quoted(rest, first), theme.string.color());
        }
        if first.is_whitespace() {
            return (first.len_utf8(), theme.text.color());
        }
        if matches!(first, '=' | '/' | '?' | '!') {
            return (1, theme.keyword.color());
        }
        let len = rest
            .char_indices()
            .find(|(_, c)| {
                c.is_whitespace() || matches!(c, '<' | '>' | '=' | '/' | '?' | '\'' | '"')
            })
            .map_or(rest.len(), |(at, _)| at)
            .max(first.len_utf8());
        return (
            len,
            if *state == Markup::TagName {
                *state = Markup::Attributes;
                theme.keyword.color()
            } else {
                theme.property.color()
            },
        );
    }
    let len = rest
        .char_indices()
        .skip(1)
        .find(|(_, c)| matches!(c, '<' | '&'))
        .map_or(rest.len(), |(at, _)| at);
    (len, theme.text.color())
}

fn format_token(
    text: &str,
    at: usize,
    language: Language,
    theme: &Theme,
) -> Option<(usize, Color32)> {
    use Language::*;
    let rest = &text[at..];
    let first = rest.chars().next().unwrap();
    let line_start = at == 0 || text.as_bytes()[at - 1] == b'\n';
    let line_len = || rest.find('\n').unwrap_or(rest.len()).max(first.len_utf8());
    let comment = theme.comment.color();
    let string = theme.string.color();
    if language == Diff {
        return Some((
            line_len(),
            match first {
                '+' => theme.success.color(),
                '-' => theme.error.color(),
                '@' => theme.keyword.color(),
                _ => theme.text.color(),
            },
        ));
    }
    if language == Csv && matches!(first, ',' | '\t') {
        return Some((1, theme.accent.color()));
    }
    if language == PowerShell && rest.starts_with("<#") {
        return Some((rest[2..].find("#>").map_or(rest.len(), |n| n + 4), comment));
    }
    if language == Julia && rest.starts_with("#=") {
        return Some((rest[2..].find("=#").map_or(rest.len(), |n| n + 4), comment));
    }
    if language == Haskell && rest.starts_with("{-") {
        return Some((rest[2..].find("-}").map_or(rest.len(), |n| n + 4), comment));
    }
    if language == Lua && (rest.starts_with("--[[") || rest.starts_with("[[")) {
        let open = if rest.starts_with("--") { 4 } else { 2 };
        return Some((
            rest[open..].find("]]").map_or(rest.len(), |n| n + open + 2),
            if open == 4 { comment } else { string },
        ));
    }
    if matches!(language, Lua | Haskell | Sql | Vhdl) && rest.starts_with("--") {
        return Some((line_len(), comment));
    }
    if (matches!(language, Latex | Matlab | Erlang) && first == '%')
        || (matches!(language, Assembly | Ini) && first == ';')
        || (language == Assembly && first == '#')
        || (language == Fortran && first == '!')
        || (language == VisualBasic && first == '\'')
    {
        return Some((line_len(), comment));
    }
    if language == Sql && rest.starts_with("/*") {
        return Some((block_comment(rest, false), comment));
    }
    if line_start && matches!(language, Batch | Ruby) {
        let trimmed = rest.trim_start_matches([' ', '\t']);
        if language == Batch
            && (trimmed.starts_with("::")
                || trimmed
                    .get(..3)
                    .is_some_and(|s| s.eq_ignore_ascii_case("rem"))
                    && trimmed.chars().nth(3).is_none_or(char::is_whitespace))
        {
            return Some((line_len(), comment));
        }
        if language == Ruby && rest.starts_with("=begin") {
            return Some((rest.find("\n=end").map_or(rest.len(), |n| n + 5), comment));
        }
    }
    if language == Latex {
        if first == '\\' {
            let len = rest[1..]
                .char_indices()
                .find(|(_, c)| !c.is_alphabetic() && *c != '@')
                .map_or(rest.len(), |(n, _)| n + 1);
            return Some((
                if len == 1 {
                    rest.chars().take(2).map(char::len_utf8).sum()
                } else {
                    len
                },
                theme.keyword.color(),
            ));
        }
        if matches!(first, '{' | '}' | '$' | '&') {
            return Some((1, theme.accent.color()));
        }
    }
    if matches!(language, Css | Scss | Less) && first == '#' {
        let len = rest
            .char_indices()
            .skip(1)
            .find(|(_, c)| !c.is_alphanumeric() && !matches!(c, '_' | '-'))
            .map_or(rest.len(), |(n, _)| n);
        return Some((len, theme.number.color()));
    }
    if matches!(language, CMake | Makefile | Shell | PowerShell | Perl | Php) && first == '$' {
        let len = if rest.starts_with("${") || rest.starts_with("$(") {
            let end = if rest.starts_with("${") { '}' } else { ')' };
            rest.find(end).map_or(rest.len(), |n| n + 1)
        } else {
            rest.char_indices()
                .skip(1)
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                .map_or(rest.len(), |(n, _)| n)
        };
        return Some((len, string));
    }
    if language == Ini && first == '[' && line_start {
        return Some((
            rest.find(']').map_or_else(line_len, |n| n + 1),
            theme.keyword.color(),
        ));
    }
    if language == Nix && rest.starts_with("''") {
        return Some((delimited(rest, "''"), string));
    }
    None
}
