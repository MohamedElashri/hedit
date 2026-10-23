//! Bounded native highlighting, filename associations, and stable session IDs.
pub mod grammar;
mod keywords;
mod lexer;

use crate::theme::Theme;
use eframe::egui;

// Keep names, IDs, associations, and the selector in one catalog.
macro_rules! languages {
    ($( $variant:ident, $id:literal, $label:literal, $extensions:expr, $filenames:expr; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Language { $( $variant, )* }
        impl Language {
            pub const ALL: [Self; [$(stringify!($variant)),*].len()] = [$(Self::$variant),*];
            pub fn id(self) -> &'static str { match self { $( Self::$variant => $id, )* } }
            pub fn label(self) -> &'static str { match self { $( Self::$variant => $label, )* } }
            pub fn extensions(self) -> &'static [&'static str] { match self { $( Self::$variant => &$extensions, )* } }
            pub fn filenames(self) -> &'static [&'static str] { match self { $( Self::$variant => &$filenames, )* } }
        }
    }
}

languages! {
    PlainText, "plain-text", "Plain text", [], [];
    Assembly, "assembly", "Assembly", ["asm", "s", "inc"], [];
    Batch, "batch", "Batch", ["bat", "cmd"], [];
    C, "c", "C", ["c", "h", "i"], [];
    Cpp, "cpp", "C++", ["cc", "cpp", "cxx", "c++", "hpp", "hxx", "hh", "h++", "inl", "ipp", "tpp", "ii"], [];
    CSharp, "csharp", "C#", ["cs", "csx"], [];
    CMake, "cmake", "CMake", ["cmake"], ["CMakeLists.txt", "CMakeCache.txt"];
    Css, "css", "CSS", ["css"], [];
    Csv, "csv", "CSV / TSV", ["csv", "tsv"], [];
    Cuda, "cuda", "CUDA", ["cu", "cuh"], [];
    Dart, "dart", "Dart", ["dart"], [];
    Diff, "diff", "Diff / Patch", ["diff", "patch"], [];
    Dockerfile, "dockerfile", "Dockerfile", ["dockerfile", "containerfile"], ["Dockerfile", "Containerfile"];
    Elixir, "elixir", "Elixir", ["ex", "exs"], [];
    Erlang, "erlang", "Erlang", ["erl", "hrl"], [];
    Fortran, "fortran", "Fortran", ["f", "for", "f77", "f90", "f95", "f03", "f08"], [];
    Go, "go", "Go", ["go"], [];
    Graphql, "graphql", "GraphQL", ["graphql", "gql"], [];
    Groovy, "groovy", "Groovy", ["groovy", "gradle"], ["Jenkinsfile"];
    Haskell, "haskell", "Haskell", ["hs", "lhs"], [];
    Hcl, "hcl", "HCL / Terraform", ["hcl", "tf", "tfvars"], [];
    Html, "html", "HTML", ["html", "htm", "xhtml", "shtml"], [];
    Ini, "ini", "INI / Config", ["ini", "cfg", "conf", "properties", "env", "editorconfig", "gitignore", "gitattributes"], [".env", ".editorconfig", ".gitconfig", ".gitignore", ".gitattributes", ".npmrc", ".npmignore"];
    Java, "java", "Java", ["java"], [];
    JavaScript, "javascript", "JavaScript", ["js", "jsx", "mjs", "cjs"], [];
    Json, "json", "JSON", ["json", "jsonl", "ndjson", "ipynb", "geojson"], [];
    Julia, "julia", "Julia", ["jl"], [];
    Kotlin, "kotlin", "Kotlin", ["kt", "kts"], [];
    Latex, "latex", "LaTeX", ["tex", "sty", "cls", "bib"], [];
    Less, "less", "Less", ["less"], [];
    Log, "log", "Log", ["log"], [];
    Lua, "lua", "Lua", ["lua"], [];
    Makefile, "makefile", "Makefile", ["mk", "mak"], ["Makefile", "GNUmakefile", "BSDmakefile"];
    Markdown, "markdown", "Markdown", ["md", "markdown", "mdown"], [];
    Matlab, "matlab", "MATLAB / Octave", ["m"], [];
    Nix, "nix", "Nix", ["nix"], [];
    ObjectiveC, "objective-c", "Objective-C", ["mm"], [];
    Perl, "perl", "Perl", ["pl", "pm", "t"], [];
    Php, "php", "PHP", ["php", "phtml", "php3", "php4", "php5", "phps"], [];
    PowerShell, "powershell", "PowerShell", ["ps1", "psm1", "psd1"], [];
    Protobuf, "protobuf", "Protocol Buffers", ["proto"], [];
    Python, "python", "Python", ["py", "pyi", "pyw", "pyx", "pxd"], ["SConstruct", "SConscript"];
    R, "r", "R", ["r", "rmd", "rprofile"], [".Rprofile"];
    Ruby, "ruby", "Ruby", ["rb", "rake", "gemspec", "ru"], ["Gemfile", "Rakefile", "Guardfile", "Vagrantfile", "Brewfile", ".irbrc"];
    Rust, "rust", "Rust", ["rs"], [];
    Scala, "scala", "Scala", ["scala", "sc", "sbt"], [];
    Scss, "scss", "SCSS / Sass", ["scss", "sass"], [];
    Shell, "shell", "Shell", ["sh", "bash", "zsh", "ksh", "fish"], [".bashrc", ".bash_profile", ".profile", ".zshrc", ".zprofile", ".zshenv", ".bash_aliases"];
    Sql, "sql", "SQL", ["sql"], [];
    Swift, "swift", "Swift", ["swift"], [];
    Toml, "toml", "TOML", ["toml"], ["Cargo.lock", "uv.lock", "poetry.lock", "Pipfile"];
    TypeScript, "typescript", "TypeScript", ["ts", "tsx", "mts", "cts"], [];
    Verilog, "verilog", "Verilog / SystemVerilog", ["v", "vh", "sv", "svh"], [];
    Vhdl, "vhdl", "VHDL", ["vhd", "vhdl"], [];
    VisualBasic, "visual-basic", "Visual Basic", ["vb", "vbs", "bas"], [];
    Xml, "xml", "XML", ["xml", "xsd", "xsl", "xslt", "svg", "plist", "csproj", "fsproj", "vbproj", "props", "targets", "ui", "qrc", "rss", "atom"], [];
    Yaml, "yaml", "YAML", ["yaml", "yml"], [];
    Zig, "zig", "Zig", ["zig", "zon"], [];
}

impl Language {
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|language| language.id() == id)
    }

    pub fn matches_query(self, query: &str) -> bool {
        let query = query.trim().trim_start_matches('.').to_ascii_lowercase();
        self.label().to_ascii_lowercase().contains(&query)
            || self.id().contains(&query)
            || self
                .extensions()
                .iter()
                .any(|extension| extension.contains(&query))
            || self
                .filenames()
                .iter()
                .any(|name| name.to_ascii_lowercase().contains(&query))
    }

    /// Use the actual filename, not an unsaved file's first line/title.
    pub fn detect(name: &str) -> Self {
        let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
        // Unix C++ conventions: uppercase .C/.H differ from C's .c/.h.
        if name.ends_with(".C") || name.ends_with(".H") {
            return Self::Cpp;
        }
        if name.ends_with(".M") {
            return Self::ObjectiveC;
        }
        if let Some(language) = Self::ALL.into_iter().find(|language| {
            language
                .filenames()
                .iter()
                .any(|filename| name.eq_ignore_ascii_case(filename))
        }) {
            return language;
        }
        let name = name.to_ascii_lowercase();
        if name.starts_with("dockerfile.") || name.starts_with("containerfile.") {
            return Self::Dockerfile;
        }
        // Environment variants are common configuration files, including secret files.
        if name.starts_with(".env.") {
            return Self::Ini;
        }
        let Some((_, extension)) = name.rsplit_once('.') else {
            return Self::PlainText;
        };
        Self::ALL
            .into_iter()
            .find(|language| language.extensions().contains(&extension))
            .unwrap_or(Self::PlainText)
    }
}

pub fn is_cuda(name: &str) -> bool {
    Language::detect(name) == Language::Cuda
}
pub fn is_c_family(name: &str) -> bool {
    matches!(
        Language::detect(name),
        Language::C | Language::Cpp | Language::Cuda | Language::ObjectiveC
    )
}
pub fn language_label(name: &str) -> &'static str {
    Language::detect(name).label()
}

/// Convenience helpers for the CUDA tools preview and existing library callers.
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
    highlight_language(
        text,
        font_size,
        wrap_width,
        if cuda {
            Language::Cuda
        } else {
            Language::PlainText
        },
        theme,
    )
}
pub fn highlight_language(
    text: &str,
    font_size: f32,
    wrap_width: f32,
    language: Language,
    theme: &Theme,
) -> egui::text::LayoutJob {
    lexer::highlight(text, font_size, wrap_width, language, theme)
}

/// A layout of `text` colored by grammar runs of `(byte length, color)`.
pub fn layout_runs(
    text: &str,
    runs: &[(usize, egui::Color32)],
    font_size: f32,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = wrap_width;
    let mut at = 0;
    for &(len, color) in runs {
        let end = (at + len).min(text.len());
        job.append(
            &text[at..end],
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::monospace(font_size),
                color,
                ..Default::default()
            },
        );
        at = end;
    }
    if at < text.len() {
        job.append(&text[at..], 0.0, Default::default());
    }
    job
}

/// A one-off highlight with the grammar when there is one, else the lexer,
/// for small samples such as the theme preview.
pub fn highlight_sample(
    text: &str,
    font_size: f32,
    wrap_width: f32,
    language: Language,
    theme: &Theme,
) -> egui::text::LayoutJob {
    match grammar::Grammar::default().highlight(text, language, theme) {
        Some(runs) => layout_runs(text, &runs, font_size, wrap_width),
        None => highlight_language(text, font_size, wrap_width, language, theme),
    }
}
