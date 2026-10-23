//! Grammar-based highlighting with Sublime Text syntax definitions (via
//! syntect and bat's syntax collection), colored with hedit theme roles.
//!
//! [`Grammar`] keeps per-line parser state, so after an edit only the changed
//! lines and those whose state changed as a result are parsed again.
use super::{lexer, Language};
use crate::theme::{Rgb, Theme};
use eframe::egui::Color32;
use std::str::FromStr;
use std::sync::{Arc, Mutex, OnceLock};
use syntect::highlighting::{
    HighlightIterator, HighlightState, Highlighter, ScopeSelectors, StyleModifier,
    Theme as SyntectTheme, ThemeItem, ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

/// Lines longer than this (for example minified files) use the simple lexer,
/// since grammar regexes can be slow on them.
pub const MAX_LINE: usize = 20_000;

fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(two_face::syntax::extra_newlines)
}

/// The syntax definition for a language, by name. Languages without one keep
/// the simple lexer: plain text, logs, CSV (whose lexer marks separators), and
/// languages missing from the collection.
fn syntax(language: Language) -> Option<&'static SyntaxReference> {
    use Language::*;
    let name = match language {
        PlainText | Log | Csv | PowerShell | VisualBasic => return None,
        Assembly => "x86_64 Assembly",
        Batch => "Batch File",
        C => "C",
        Cpp | Cuda => "C++",
        CSharp => "C#",
        CMake => "CMake",
        Css => "CSS",
        Dart => "Dart",
        Diff => "Diff",
        Dockerfile => "Dockerfile",
        Elixir => "Elixir",
        Erlang => "Erlang",
        Fortran => "Fortran (Modern)",
        Go => "Go",
        Graphql => "GraphQL",
        Groovy => "Groovy",
        Haskell => "Haskell",
        Hcl => "Terraform",
        Html => "HTML",
        Ini => "INI",
        Java => "Java",
        JavaScript => "JavaScript",
        Json => "JSON",
        Julia => "Julia",
        Kotlin => "Kotlin",
        Latex => "LaTeX",
        Less => "Less",
        Lua => "Lua",
        Makefile => "Makefile",
        Markdown => "Markdown",
        Matlab => "MATLAB",
        Nix => "Nix",
        ObjectiveC => "Objective-C++",
        Perl => "Perl",
        Php => "PHP",
        Protobuf => "Protocol Buffer",
        Python => "Python",
        R => "R",
        Ruby => "Ruby",
        Rust => "Rust",
        Scala => "Scala",
        Scss => "SCSS",
        Shell => "Bourne Again Shell (bash)",
        Sql => "SQL",
        Swift => "Swift",
        Toml => "TOML",
        TypeScript => "TypeScript",
        Verilog => "Verilog",
        Vhdl => "VHDL",
        Xml => "XML",
        Yaml => "YAML",
        Zig => "Zig",
    };
    syntaxes().find_syntax_by_name(name)
}

/// Whether a language is highlighted by a grammar rather than the lexer.
pub fn has_grammar(language: Language) -> bool {
    syntax(language).is_some()
}

/// TextMate scopes for each hedit color role, most general first. More
/// specific selectors win, so `keyword.operator` beats `keyword`.
fn rules(theme: &Theme) -> Vec<(&'static str, Rgb)> {
    vec![
        ("comment, punctuation.definition.comment", theme.comment),
        ("string, punctuation.definition.string, markup.raw", theme.string),
        ("constant.numeric", theme.number),
        (
            "constant.language, constant.character, constant.other, variable.other.constant, support.constant",
            theme.constant,
        ),
        ("keyword, storage, entity.name.tag, markup.heading", theme.keyword),
        ("keyword.operator", theme.operator),
        (
            "entity.name.function, support.function, variable.function, meta.function-call.identifier",
            theme.function,
        ),
        (
            "entity.name.type, entity.name.class, entity.name.struct, entity.name.enum, entity.name.trait, entity.name.union, support.type, support.class, entity.other.inherited-class, storage.type.primitive, storage.type.numeric",
            theme.type_name,
        ),
        (
            "entity.other.attribute-name, variable.other.member, variable.other.property, support.type.property-name, meta.mapping.key string, meta.mapping.key, entity.name.tag.yaml",
            theme.property,
        ),
        (
            "meta.preprocessor keyword, meta.preprocessor punctuation, keyword.control.import.include, meta.annotation, meta.attribute, meta.decorator, punctuation.definition.annotation, punctuation.definition.decorator",
            theme.preprocessor,
        ),
        ("markup.inserted", theme.success),
        ("markup.deleted, invalid", theme.error),
        ("markup.changed", theme.warning),
    ]
}

fn color(rgb: Rgb) -> syntect::highlighting::Color {
    let [r, g, b] = rgb.0;
    syntect::highlighting::Color { r, g, b, a: 255 }
}

/// A syntect theme that paints scopes with `theme`'s roles.
fn syntect_theme(theme: &Theme) -> Arc<SyntectTheme> {
    static CACHE: Mutex<Option<(Theme, Arc<SyntectTheme>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached, built)) = cache.as_ref() {
        if cached == theme {
            return Arc::clone(built);
        }
    }
    let built = Arc::new(SyntectTheme {
        name: Some(theme.name.clone()),
        author: None,
        settings: ThemeSettings {
            foreground: Some(color(theme.text)),
            background: Some(color(theme.background)),
            ..Default::default()
        },
        scopes: rules(theme)
            .into_iter()
            .map(|(selectors, rgb)| ThemeItem {
                scope: ScopeSelectors::from_str(selectors).expect("selectors are valid"),
                style: StyleModifier {
                    foreground: Some(color(rgb)),
                    background: None,
                    font_style: None,
                },
            })
            .collect(),
    });
    *cache = Some((theme.clone(), Arc::clone(&built)));
    built
}

#[derive(Clone)]
struct Line {
    /// Parser and highlighter state at the start of the line.
    parse: ParseState,
    highlight: HighlightState,
    /// The line's bytes, including its newline.
    len: usize,
    /// Colored byte runs covering the line.
    spans: Vec<(usize, Color32)>,
}

/// Highlighted text with per-line state for incremental updates.
#[derive(Clone, Default)]
pub struct Grammar {
    key: Option<(Language, Theme)>,
    text: String,
    lines: Vec<Line>,
    /// Colored byte runs for `text`.
    runs: Arc<Vec<(usize, Color32)>>,
}

impl Grammar {
    /// The runs for exactly this text, if they are already highlighted.
    pub fn cached(
        &self,
        text: &str,
        language: Language,
        theme: &Theme,
    ) -> Option<Arc<Vec<(usize, Color32)>>> {
        (self
            .key
            .as_ref()
            .is_some_and(|(l, t)| *l == language && t == theme)
            && self.text == text)
            .then(|| Arc::clone(&self.runs))
    }
    /// Roughly how many bytes [`Self::highlight`] would parse: the changed
    /// region when warm, everything when cold. Large costs belong off the UI thread.
    pub fn cost(&self, text: &str, language: Language, theme: &Theme) -> usize {
        let warm = self
            .key
            .as_ref()
            .is_some_and(|(l, t)| *l == language && t == theme)
            && !self.lines.is_empty();
        if !warm {
            return text.len();
        }
        let prefix = common_prefix(&self.text, text);
        let suffix = common_suffix(&self.text[prefix..], &text[prefix..]);
        text.len() - prefix - suffix
    }

    /// Colored byte runs for `text`, reusing unchanged lines. `None` when the
    /// language has no grammar or a line is too long for one.
    pub fn highlight(
        &mut self,
        text: &str,
        language: Language,
        theme: &Theme,
    ) -> Option<Arc<Vec<(usize, Color32)>>> {
        let syntax = syntax(language)?;
        if text.split('\n').any(|line| line.len() > MAX_LINE) {
            return None;
        }
        let set = syntaxes();
        let syntect = syntect_theme(theme);
        let highlighter = Highlighter::new(&syntect);
        if self.key.as_ref() != Some(&(language, theme.clone())) {
            self.key = Some((language, theme.clone()));
            self.lines.clear();
            self.text.clear();
        }
        // Lines entirely before the first changed byte keep their results.
        let prefix = common_prefix(&self.text, text);
        let suffix = common_suffix(&self.text[prefix..], &text[prefix..]);
        let mut kept = 0;
        let mut start = 0;
        while kept < self.lines.len() && start + self.lines[kept].len <= prefix {
            start += self.lines[kept].len;
            kept += 1;
        }
        // A line that ended exactly at the old end of text may have grown.
        if kept == self.lines.len() && kept > 0 && start == self.text.len() {
            kept -= 1;
            start -= self.lines[kept].len;
        }
        // Move, never copy, line state: a large file has tens of thousands.
        let mut old_lines = std::mem::take(&mut self.lines);
        let old_len = self.text.len();
        let mut fresh: Vec<Line> = Vec::new();
        let mut reused: Option<usize> = None;
        let (mut parse, mut state) = match old_lines.get(kept) {
            Some(line) => (line.parse.clone(), line.highlight.clone()),
            None => (
                ParseState::new(syntax),
                HighlightState::new(&highlighter, ScopeStack::new()),
            ),
        };
        // Old lines starting inside the unchanged suffix, by their offset from
        // the end of the text, for reuse once the parser state matches again.
        let mut old_start = old_lines.iter().map(|l| l.len).sum::<usize>();
        let suffix_lines: Vec<(usize, usize)> = old_lines
            .iter()
            .enumerate()
            .rev()
            .map(|(i, l)| {
                old_start -= l.len;
                (i, old_start)
            })
            .filter(|&(_, at)| old_len - at <= suffix && at >= prefix)
            .map(|(i, at)| (old_len - at, i))
            .collect();
        let mut at = start;
        while at < text.len() {
            let from_end = text.len() - at;
            if let Some(&(_, index)) = suffix_lines.iter().find(|(d, _)| *d == from_end) {
                let old = &old_lines[index];
                if old.parse == parse && old.highlight == state {
                    reused = Some(index);
                    break;
                }
            }
            let end = text[at..].find('\n').map_or(text.len(), |n| at + n + 1);
            let line = &text[at..end];
            let start_parse = parse.clone();
            let start_state = state.clone();
            let ops = parse.parse_line(line, set).ok()?;
            let mut spans = Vec::new();
            for (style, piece) in HighlightIterator::new(&mut state, &ops, line, &highlighter) {
                let c = style.foreground;
                let color = Color32::from_rgb(c.r, c.g, c.b);
                match spans.last_mut() {
                    Some((len, last)) if *last == color => *len += piece.len(),
                    _ => spans.push((piece.len(), color)),
                }
            }
            // `parse` and `state` now describe the start of the next line;
            // store each line with the state it started in.
            fresh.push(Line {
                parse: start_parse,
                highlight: start_state,
                len: line.len(),
                spans,
            });
            at = end;
        }
        let tail = reused
            .map(|index| old_lines.split_off(index))
            .unwrap_or_default();
        old_lines.truncate(kept);
        old_lines.extend(fresh);
        old_lines.extend(tail);
        self.lines = old_lines;
        self.text = text.to_owned();
        let mut runs: Vec<(usize, Color32)> = Vec::new();
        for (len, color) in self.lines.iter().flat_map(|l| l.spans.iter().copied()) {
            match runs.last_mut() {
                Some((n, last)) if *last == color => *n += len,
                _ => runs.push((len, color)),
            }
        }
        if language == Language::Cuda {
            runs = cuda_overlay(text, runs, theme);
        }
        self.runs = Arc::new(runs);
        Some(Arc::clone(&self.runs))
    }
}

/// A [`Grammar`] that can move to a worker thread and back.
///
/// The cache is not `Send` only because parser state can hold Oniguruma
/// capture regions, which are plain heap allocations behind raw pointers.
/// Oniguruma keeps no thread-local or shared references to a region, and a
/// `Moved` is owned by exactly one thread at a time, so moving it is sound.
pub struct Moved(pub Grammar);
// SAFETY: see the type's documentation.
unsafe impl Send for Moved {}

/// Color CUDA qualifiers and built-ins, which the C++ grammar does not know.
fn cuda_overlay(text: &str, runs: Vec<(usize, Color32)>, theme: &Theme) -> Vec<(usize, Color32)> {
    let mut colors: Vec<Color32> = Vec::with_capacity(text.len());
    for (len, color) in runs {
        colors.extend(std::iter::repeat_n(color, len));
    }
    let bytes = text.as_bytes();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_alphabetic() || bytes[at] == b'_' {
            let end = bytes[at..]
                .iter()
                .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
                .map_or(bytes.len(), |n| at + n);
            if lexer::cuda_keyword(&text[at..end]) {
                colors[at..end].fill(theme.cuda.color());
            }
            at = end;
        } else {
            at += 1;
        }
    }
    let mut runs: Vec<(usize, Color32)> = Vec::new();
    for color in colors {
        match runs.last_mut() {
            Some((n, last)) if *last == color => *n += 1,
            _ => runs.push((1, color)),
        }
    }
    runs
}

fn common_prefix(a: &str, b: &str) -> usize {
    let n = a
        .bytes()
        .zip(b.bytes())
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()));
    // Stay on a character boundary in both strings.
    (0..=n)
        .rev()
        .find(|&i| a.is_char_boundary(i) && b.is_char_boundary(i))
        .unwrap_or(0)
}

fn common_suffix(a: &str, b: &str) -> usize {
    a.bytes()
        .rev()
        .zip(b.bytes().rev())
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}

/// Build a hedit theme from a TextMate `.tmTheme` color scheme.
pub fn import_tm_theme(bytes: &[u8], fallback_name: &str) -> Result<Theme, String> {
    let scheme =
        syntect::highlighting::ThemeSet::load_from_reader(&mut std::io::Cursor::new(bytes))
            .map_err(|e| format!("Not a TextMate color scheme: {e}"))?;
    let highlighter = Highlighter::new(&scheme);
    let rgb = |c: syntect::highlighting::Color| Rgb::new(c.r, c.g, c.b);
    let settings = &scheme.settings;
    let background = settings.background.map_or(Rgb::new(255, 255, 255), rgb);
    let text = settings.foreground.map_or(Rgb::new(0, 0, 0), rgb);
    let style = |scope: &str| {
        let stack = ScopeStack::from_str(scope).expect("scope is valid");
        rgb(highlighter.style_for_stack(stack.as_slice()).foreground)
    };
    let mix = |a: Rgb, b: Rgb, t: f32| {
        Rgb(std::array::from_fn(|i| {
            (a.0[i] as f32 + (b.0[i] as f32 - a.0[i] as f32) * t).round() as u8
        }))
    };
    let dark = Theme::contrast(background, Rgb::new(0, 0, 0))
        < Theme::contrast(background, Rgb::new(255, 255, 255));
    let selection = settings.selection.map_or(mix(background, text, 0.2), rgb);
    let accent = settings.caret.map_or(style("entity.name.function"), rgb);
    let comment = style("comment");
    let theme = Theme {
        version: crate::theme::VERSION,
        name: scheme
            .name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| fallback_name.into()),
        dark,
        background,
        surface: mix(background, text, 0.04),
        panel: mix(background, text, if dark { -0.03 } else { 0.03 }),
        elevated: mix(background, text, 0.1),
        border: mix(background, text, 0.18),
        text,
        muted: mix(background, text, 0.65),
        accent,
        selection,
        selection_text: settings.selection_foreground.map_or(text, rgb),
        warning: style("markup.changed"),
        error: style("invalid"),
        success: style("markup.inserted"),
        comment,
        keyword: style("keyword"),
        string: style("string"),
        number: style("constant.numeric"),
        cuda: style("storage.modifier"),
        function: style("entity.name.function"),
        type_name: style("entity.name.type"),
        constant: style("constant.language"),
        operator: style("keyword.operator"),
        property: style("entity.other.attribute-name"),
        preprocessor: style("meta.preprocessor keyword"),
    };
    theme.validate()?;
    Ok(theme)
}
