//! A bounded C/CUDA lexer. No plugins, language server, or runtime downloads.
use crate::theme::Theme;
use eframe::egui::{self, FontId, TextFormat};

pub fn is_cuda(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with(".cu") || name.ends_with(".cuh")
}
pub fn is_c_family(name: &str) -> bool {
    let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    is_cuda(name)
        || matches!(
            extension.as_str(),
            "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "hh" | "inl"
        )
}
pub fn language_label(name: &str) -> &'static str {
    if is_cuda(name) {
        "CUDA C++"
    } else if is_c_family(name) {
        "C / C++"
    } else {
        "Plain text"
    }
}

pub fn highlight(text: &str, font_size: f32, wrap_width: f32, cuda: bool) -> egui::text::LayoutJob {
    highlight_with_theme(text, font_size, wrap_width, cuda, &Theme::default())
}

pub fn highlight_with_theme(
    text: &str,
    font_size: f32,
    wrap_width: f32,
    cuda: bool,
    theme: &Theme,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let plain = theme.text.color();
    let mut append = |s: &str, color| {
        job.append(
            s,
            0.0,
            TextFormat {
                font_id: FontId::monospace(font_size),
                color,
                ..Default::default()
            },
        )
    };
    if !cuda {
        append(text, plain);
        return job;
    }
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let first = rest.chars().next().unwrap();
        let (len, color) = if rest.starts_with("//") {
            (rest.find('\n').unwrap_or(rest.len()), theme.comment.color())
        } else if let Some(stripped) = rest.strip_prefix("/*") {
            (
                stripped.find("*/").map_or(rest.len(), |n| n + 4),
                theme.comment.color(),
            )
        } else if first == '"' || first == '\'' {
            let mut escaped = false;
            let mut len = first.len_utf8();
            for c in rest[first.len_utf8()..].chars() {
                len += c.len_utf8();
                if !escaped && c == first {
                    break;
                }
                escaped = !escaped && c == '\\';
            }
            (len, theme.string.color())
        } else if first.is_alphanumeric() || first == '_' {
            let len = rest
                .char_indices()
                .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                .map_or(rest.len(), |(n, _)| n);
            let word = &rest[..len];
            let color = if matches!(
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
            ) {
                theme.cuda.color()
            } else if matches!(
                word,
                "int"
                    | "float"
                    | "double"
                    | "void"
                    | "bool"
                    | "char"
                    | "unsigned"
                    | "const"
                    | "auto"
                    | "size_t"
                    | "return"
                    | "if"
                    | "else"
                    | "for"
                    | "while"
                    | "struct"
                    | "class"
                    | "template"
                    | "include"
                    | "define"
                    | "nullptr"
                    | "constexpr"
                    | "static"
                    | "namespace"
                    | "using"
                    | "enum"
                    | "switch"
                    | "case"
                    | "break"
                    | "continue"
                    | "true"
                    | "false"
                    | "new"
                    | "delete"
                    | "public"
                    | "private"
                    | "protected"
            ) {
                theme.keyword.color()
            } else if first.is_ascii_digit() {
                theme.number.color()
            } else {
                plain
            };
            (len, color)
        } else {
            (first.len_utf8(), plain)
        };
        append(&rest[..len], color);
        at += len;
    }
    job
}
