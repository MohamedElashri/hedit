//! Built-in file and folder associations and artwork for the workbench.
use super::{cached_texture, paint_texture, Icon};
use crate::syntax::Language;
use eframe::egui::{self, Color32, TextureHandle};
use std::borrow::Cow;

pub const FILE_ICON_SIZE: f32 = 16.0;

// White vector masks let the same cached textures serve both theme palettes.
// The marks use paths so they stay independent of the workbench font.
macro_rules! file_icons {
    ($($icon:ident, $dark:literal, $light:literal, $path:literal;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum FileIcon {
            Generic,
            $($icon,)*
        }

        impl FileIcon {
            #[cfg(test)]
            const ALL: &'static [Self] = &[Self::Generic, $(Self::$icon,)*];

            fn svg(self) -> &'static [u8] {
                match self {
                    Self::Generic => Icon::File.svg(),
                    $(Self::$icon => concat!(
                        r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="white" stroke-width="1.45" stroke-linejoin="round" stroke-linecap="round" d=""#,
                        $path,
                        r#"" /></svg>"#,
                    ).as_bytes(),)*
                }
            }

            fn tint(self, ui: &egui::Ui, fallback: Color32) -> Color32 {
                let (dark, light): (u32, u32) = match self {
                    Self::Generic => return fallback,
                    $(Self::$icon => ($dark, $light),)*
                };
                let rgb = if ui.visuals().dark_mode { dark } else { light };
                let [_, red, green, blue] = rgb.to_be_bytes();
                Color32::from_rgb(red, green, blue)
            }
        }
    }
}

file_icons! {
    Code,       0x9bb7ec, 0x395c98, "M5.5 4.5 2 8l3.5 3.5m5-7L14 8l-3.5 3.5M9 3 7 13";
    Rust,       0xffa47d, 0x9b4024, "M2.5 13.5v-11h6A3 3 0 0 1 8.5 8h-6m5.5 0 3 5.5h2.5M5 4.8h3.5";
    Python,     0x85c3fc, 0x27649a, "M5 5V3a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v4a1 1 0 0 1-1 1H6a1 1 0 0 0-1 1v3H3a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h4m4-1h2a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1H9m2 0v2a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1v-1";
    JavaScript, 0xf4d35e, 0x845d00, "M2 2h12v12H2zM6.5 6v4.5c0 1.2-1.7 1.6-2.2.6m4.5-.4c.4.7 2.7 1.1 2.7-.3 0-1.6-2.6-.9-2.6-2.6 0-1.2 1.7-1.5 2.6-.8";
    TypeScript, 0x8ec7ff, 0x245b9a, "M2 2h12v12H2zM4 6.7h4m-2 0v4.8m3.3-.7c.4.7 2.5 1 2.5-.3 0-1.5-2.5-.9-2.5-2.5 0-1.1 1.5-1.5 2.3-.8";
    TypeDeclaration, 0xa5cdf7, 0x365e8d, "M3 2.2h6l4 4v7.6H3zM9 2.2v4h4M5.7 8.5h4.6M8 8.5v3";
    React,      0x81dcea, 0x147184, "M2 8c0-3 12-3 12 0S2 11 2 8Zm3-5.2C7.6 1.3 13.6 11.7 11 13.2S2.4 4.3 5 2.8Zm6 0c2.6 1.5-3.4 11.9-6 10.4S8.4 1.3 11 2.8ZM8 8h.01";
    Go,         0x73d9ef, 0x126d83, "M1.7 5.5h4m-3.9 2.6h2.5m-2.1 2.6h2.2m5.1-4.6C8 4.3 5.5 6 5.5 8.4c0 2.6 3.2 3.6 4.5 1.3V8.5H7.9m6.2-.6c0-1-1.7-1.3-2 0-.3 1-.4 2.6.6 2.8 1 .2 1.4-1.4 1.4-2.8Z";
    C,          0xa7bbfa, 0x4f5a9d, "M8 1.8 14 5.3v5.4L8 14.2 2 10.7V5.3zM10 6.2a2.8 2.8 0 1 0 0 3.6";
    Cpp,        0x8fb9ff, 0x245c9a, "M6.6 2 12 5v2.2m0 3.4v.4l-5.4 3L1.5 11V5zM7.9 6.2a2.6 2.6 0 1 0 0 3.6M9.1 8.5h2.2m-1.1-1.1v2.2m2.3-1.1h2.2m-1.1-1.1v2.2";
    DotNet,     0xd1b1ff, 0x6c429e, "M6.9 4.6a4.1 4.1 0 1 0 0 6.8M10 5l-1 6m3.8-6-1 6M8.3 7.1h5.3m-5.7 2h5.3";
    Java,       0xffae85, 0x9e4625, "M4 8h7v2.2a3 3 0 0 1-3 3H7a3 3 0 0 1-3-3zM11 8h1a1.6 1.6 0 1 1 0 3.2h-1M2.5 14h10M7 6c-2-2 2-2.5 1-4.5m2 4.5c-1-1 1-2 1-3";
    Kotlin,     0xcfadfb, 0x70519a, "M2.2 2.6h4.2v10.8H2.2zM6.5 7.9l6-5.3m-6 5.3 6 5.5";
    Scala,      0xfa9c9e, 0x9d3a3e, "M3 4.4 13 2v2.3L3 6.8zm0 4.3 10-2.4v2.3L3 11zm0 4.3 10-2.4v2.3L3 15z";
    Ruby,       0xff929d, 0xa73552, "M1.8 6.2 5 2.8h6l3.2 3.4L8 13.6zM1.8 6.2h12.4M5 2.8l1 3.4 2 7.4 2-7.4 1-3.4";
    Php,        0xbcb8ff, 0x56569d, "M1.5 8a6.5 4.2 0 1 0 13 0 6.5 4.2 0 1 0-13 0Zm4 2.5.7-5H8a1.4 1.4 0 0 1 0 2.8H5.8m4.8-2.8-.7 5m.4-2.5h1.5";
    Lua,        0xa7b6ff, 0x565da1, "M11.9 6a5.3 5.3 0 1 1-3.8-3.2M13 3h.01M10 6h.01";
    Swift,      0xffad71, 0x9c4515, "M2 4c2.5 3 4.9 5 8 6-2.1-2-3.8-4-5-6 2.1 2.2 4.2 3.4 6 4-.1-1.9-1.2-4-2.2-5.5 3.7 2.5 5.6 5.5 4.3 8.2 1 1.4 1.1 2.3.9 3.3-1.4-1.6-2.2-1.7-3.5-1.3-4 1-7.1-.7-9-3.5 2.5 1.8 4.4 2.1 6.3 1.8C5.5 9.8 3.3 7.2 2 4Z";
    Zig,        0xffbf61, 0x895600, "M4 3h9.5L5.5 13H12M4 3 2 6h3m7 7 2-3h-3";
    Shell,      0x8cdbb1, 0x287354, "M2 3.2h12v9.6H2zM4.3 6.1 6.7 8l-2.4 1.9m4.2.2h3";
    Html,       0xf4a180, 0x984620, "M2.8 2h10.4l-1.1 10.1L8 14l-4.1-1.9zM6 5.3v4.8m4-4.8v4.8M6 7.7h4";
    Css,        0xa6b6ff, 0x4e5aa5, "M2.8 2h10.4l-1.1 10.1L8 14l-4.1-1.9zM9.9 5.7a2.4 2.4 0 1 0 0 3.8";
    Json,       0xedd478, 0x806400, "M6 2.3H5c-1.1 0-1.5.7-1.5 1.6v2.5L2 8l1.5 1.6v2.5c0 .9.4 1.6 1.5 1.6h1m4-11.4h1c1.1 0 1.5.7 1.5 1.6v2.5L14 8l-1.5 1.6v2.5c0 .9-.4 1.6-1.5 1.6h-1";
    Yaml,       0xddabec, 0x82478e, "M2.5 4.2h3.2v3.2H2.5zM10.3 4.2h3.2v3.2h-3.2zM6.4 10.9h3.2v3.2H6.4zM4.1 7.4v1.8h7.8V7.4M8 9.2v1.7";
    Toml,       0xe2b394, 0x865438, "M4.6 3H2.5v10h2.1m6.8-10h2.1v10h-2.1M5.5 5.7h5M8 5.7v4.6";
    Settings,   0xb9c5d6, 0x526178, "M2.5 4h5m3 0h3m-6-1.5v3m-5 2.5h8m2 0h1m-3-1.5v3m-8 2.5h2m3 0h6m-9-1.5v3";
    Data,       0x9fd5ac, 0x386b48, "M2 3h12v10H2zM2 6.5h12M2 9.8h12M6.5 3v10m3.6-10v10";
    Database,   0xd6a9ee, 0x75458e, "M2.6 4.2a5.4 2 0 1 0 10.8 0 5.4 2 0 1 0-10.8 0zm0 0v7.5a5.4 2 0 0 0 10.8 0V4.2M2.6 8a5.4 2 0 0 0 10.8 0";
    Markdown,   0x9bc7f1, 0x366596, "M1.7 3.5h12.6v9H1.7zM4 10V6l2 2 2-2v4m3.2-4v4m-1.5-1.5 1.5 1.5 1.5-1.5";
    Book,       0xa9c6ef, 0x42628f, "M8 4.5C6 3.2 3.8 3.2 1.8 3.6v8.5c2-.6 4.2-.3 6.2 1 2-1.3 4.2-1.6 6.2-1V3.6c-2-.4-4.2-.4-6.2.9Zm0 0v8.6";
    Text,       0xc0c8d5, 0x586576, "M3.3 2.2h6l3.4 3.3v8.3H3.3zM9.3 2.2v3.3h3.4M5.6 8h4.8m-4.8 3h4.8";
    Pdf,        0xff9aa0, 0xa23943, "M2.5 2.1h7l4 4v7.8h-11zM9.5 2.1v4h4M5 11.1c2-1.2 2.6-2 2.6-4 0 1.9 1 3 3.4 3.4-2.4-.4-4.5-.2-6 .6Z";
    Image,      0xa8d99b, 0x466f36, "M2 2.8h12v10.4H2zM4.7 5.5h.01M2 11l3.3-3.2 2.5 2.5L11 7l3 2.8";
    Audio,      0xf2a8cf, 0x8f3f70, "M7 11.5V3.8l6-1.6v7.7m-6 1.6a2 1.4 0 1 1-4 0 2 1.4 0 1 1 4 0Zm6-1.6a2 1.4 0 1 1-4 0 2 1.4 0 1 1 4 0Zm-6-4 6-1.6";
    Video,      0xe7abf1, 0x824797, "M2 3.1h12v9.8H2zm4.5 2.5 4 2.4-4 2.4z";
    Archive,    0xe1b783, 0x805722, "M2.5 2.5h11v3h-11zm1 3v8h9v-8m-5.9 3h2.8";
    Font,       0xb4c9e3, 0x4d6585, "M2.5 12.5 7 3l4.5 9.5M4.1 9h5.8M12 9l1.6 3.5";
    Git,        0xffa183, 0xa04227, "M8 1.4 14.6 8 8 14.6 1.4 8zM5.5 4v6m0-4.5L10 8m-4.5 2h.01M10 8h.01";
    Package,    0xe4bd8d, 0x815627, "M8 2l6 3.1v5.8L8 14l-6-3.1V5.1zM2 5.1l6 3.2 6-3.2M8 8.3V14M5 3.6l6 3.1";
    Container,  0x7dc9f5, 0x20658b, "M2 7.2h9.5l2.6-1v2.4l-2.2 2.8A3 3 0 0 1 9.5 13H5a3 3 0 0 1-3-3zm2 0V4.3h2.5v2.9m0 0V3.2H9v4";
    Build,      0xf3c078, 0x865312, "M3 12.5 9.2 6.3M8 3.4l1.2-1.2H12l2 2v2.5l-1.3 1.2-3.5-3.5L8 5.6zM2.1 11.5l2.4 2.4";
    Test,       0xbca9fa, 0x6747a5, "M6 2.3h4m-3 0v4L3.8 12a1.1 1.1 0 0 0 1 1.6h6.4a1.1 1.1 0 0 0 1-1.6L9 6.3v-4M5.1 10h5.8";
    Lock,       0xe8c679, 0x7a5a18, "M3.5 7h9v6.6h-9zM5.5 7V5a2.5 2.5 0 0 1 5 0v2M8 9.7v1.5";
    Web,        0x84cfdf, 0x246b7b, "M2 8a6 6 0 1 0 12 0A6 6 0 1 0 2 8Zm6-6c-3.2 3.3-3.2 8.7 0 12 3.2-3.3 3.2-8.7 0-12ZM2 8h12";
    Workflow,   0x9cb8fb, 0x4a5e9b, "M2 2h5v4H2zM9 10h5v4H9zM4.5 6v6H9m2.5-7.5h1.8m-.9-.9v1.8";
    Note,       0xf0cd7a, 0x83600b, "M2.8 2.8h10.4v7.3l-3.1 3.1H2.8zM10.1 13.2v-3.1h3.1M5.2 5.5h5.6M5.2 8h3.4";
    Binary,     0xb8c5d7, 0x536278, "M3.1 3.6h4v3.8h-4zm5.8 5h4v3.8h-4zm1.7-5h.3v3.8m-5.8 1.2h.3v3.8";
}

impl FileIcon {
    /// Exact names and compound suffixes take priority over the syntax catalog.
    pub fn for_name(name: &str) -> Self {
        let lower = ascii_lowercase(name);
        let lower = lower.as_ref();
        match lower {
            "cargo.toml" | "cargo.lock" => return Self::Rust,
            "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "pnpm-workspace.yaml"
            | ".npmrc"
            | ".npmignore"
            | ".yarnrc"
            | ".yarnrc.yml"
            | "bun.lock"
            | "bun.lockb" => return Self::Package,
            "tsconfig.json" | "jsconfig.json" => return Self::TypeScript,
            "pyproject.toml" | "poetry.lock" | "uv.lock" | "pipfile" | "pipfile.lock"
            | "requirements.txt" | "setup.cfg" | ".python-version" => return Self::Python,
            "go.mod" | "go.sum" | "go.work" | "go.work.sum" => return Self::Go,
            ".gitignore"
            | ".gitattributes"
            | ".gitmodules"
            | ".gitconfig"
            | ".gitkeep"
            | ".git-blame-ignore-revs" => return Self::Git,
            "compose.yml"
            | "compose.yaml"
            | "docker-compose.yml"
            | "docker-compose.yaml"
            | ".dockerignore"
            | ".containerignore" => return Self::Container,
            "makefile" | "gnumakefile" | "bsdmakefile" | "cmakelists.txt" | "cmakecache.txt"
            | "justfile" | "meson.build" | "meson_options.txt" | "build.ninja" | "build.zig"
            | "build.zig.zon" => return Self::Build,
            "readme" | "readme.md" | "readme.txt" | "readme.rst" | "readme.adoc" => {
                return Self::Book
            }
            "license" | "licence" | "license.md" | "license.txt" | "copying" | "notice"
            | "notice.txt" => return Self::Lock,
            "changelog" | "changelog.md" | "changes.md" | "todo" | "todo.md" => return Self::Note,
            ".env" | ".editorconfig" | ".clang-format" | ".clang-tidy" | ".prettierrc"
            | ".eslintrc" => return Self::Settings,
            ".gitlab-ci.yml" | "jenkinsfile" | "azure-pipelines.yml" => return Self::Workflow,
            ".bashrc" | ".bash_profile" | ".zshrc" | ".zprofile" | ".zshenv" | ".profile" => {
                return Self::Shell;
            }
            _ => {}
        }
        if lower.starts_with(".env.") {
            return Self::Settings;
        }
        if (lower.starts_with("tsconfig.") || lower.starts_with("jsconfig."))
            && lower.ends_with(".json")
        {
            return Self::TypeScript;
        }
        if [".d.ts", ".d.mts", ".d.cts"]
            .into_iter()
            .any(|suffix| lower.ends_with(suffix))
        {
            return Self::TypeDeclaration;
        }

        match lower.rsplit_once('.') {
            Some((_, "jsx" | "tsx")) => Self::React,
            Some((
                _,
                "png" | "jpg" | "jpeg" | "svg" | "webp" | "gif" | "avif" | "ico" | "bmp" | "tif"
                | "tiff" | "heic" | "jxl" | "psd",
            )) => Self::Image,
            Some((_, "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" | "mid")) => {
                Self::Audio
            }
            Some((_, "mp4" | "webm" | "mov" | "avi" | "mkv" | "m4v" | "mpeg")) => Self::Video,
            Some((
                _,
                "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "tgz" | "7z" | "rar" | "deb" | "rpm"
                | "jar" | "whl",
            )) => Self::Archive,
            Some((_, "ttf" | "otf" | "woff" | "woff2")) => Self::Font,
            Some((_, "sqlite" | "sqlite3" | "db" | "duckdb")) => Self::Database,
            Some((_, "parquet" | "feather" | "root" | "xlsx" | "ods")) => Self::Data,
            Some((_, "vue" | "svelte" | "astro" | "mdx")) => Self::Web,
            Some((_, "lock" | "pem" | "crt" | "key" | "p12")) => Self::Lock,
            Some((_, "exe" | "dll" | "so" | "dylib" | "bin" | "wasm" | "o")) => Self::Binary,
            Some((_, "txt" | "rst" | "adoc" | "org" | "doc" | "docx" | "odt" | "rtf")) => {
                Self::Text
            }
            Some((_, "pdf")) => Self::Pdf,
            Some((_, "jsonc" | "json5")) => Self::Json,
            _ => Self::for_language(Language::detect(name)),
        }
    }

    fn for_language(language: Language) -> Self {
        match language {
            Language::PlainText => Self::Generic,
            Language::Rust => Self::Rust,
            Language::Python => Self::Python,
            Language::JavaScript => Self::JavaScript,
            Language::TypeScript => Self::TypeScript,
            Language::Go => Self::Go,
            Language::C => Self::C,
            Language::Cpp | Language::Cuda => Self::Cpp,
            Language::CSharp | Language::VisualBasic => Self::DotNet,
            Language::Java => Self::Java,
            Language::Kotlin => Self::Kotlin,
            Language::Scala => Self::Scala,
            Language::Ruby => Self::Ruby,
            Language::Php => Self::Php,
            Language::Lua => Self::Lua,
            Language::Swift => Self::Swift,
            Language::Zig => Self::Zig,
            Language::Shell | Language::PowerShell | Language::Batch => Self::Shell,
            Language::Html => Self::Html,
            Language::Css | Language::Scss | Language::Less => Self::Css,
            Language::Json => Self::Json,
            Language::Yaml => Self::Yaml,
            Language::Toml => Self::Toml,
            Language::Hcl | Language::Ini => Self::Settings,
            Language::Csv => Self::Data,
            Language::Sql => Self::Database,
            Language::Markdown => Self::Markdown,
            Language::Latex | Language::Log => Self::Text,
            Language::Diff => Self::Git,
            Language::Dockerfile => Self::Container,
            Language::CMake | Language::Makefile | Language::Nix => Self::Build,
            _ => Self::Code,
        }
    }

    fn texture(self, ui: &egui::Ui, size: f32) -> TextureHandle {
        if self == Self::Generic {
            return Icon::File.texture(ui.ctx(), size);
        }
        cached_texture(ui.ctx(), size, egui::Id::new(("file-icon", self)), || {
            self.svg()
        })
    }

    pub fn image(self, ui: &egui::Ui, fallback: Color32) -> egui::Image<'static> {
        let texture = self.texture(ui, FILE_ICON_SIZE);
        egui::Image::new((texture.id(), egui::Vec2::splat(FILE_ICON_SIZE)))
            .fit_to_exact_size(egui::Vec2::splat(FILE_ICON_SIZE))
            .tint(self.tint(ui, fallback))
    }

    pub fn paint(self, ui: &egui::Ui, rect: egui::Rect, fallback: Color32) {
        if ui.is_rect_visible(rect) {
            let texture = self.texture(ui, rect.width());
            paint_texture(ui, rect, &texture, self.tint(ui, fallback));
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FolderIcon(Option<FileIcon>);

impl FolderIcon {
    pub fn for_name(name: &str) -> Self {
        let lower = ascii_lowercase(name);
        Self(match lower.as_ref() {
            "src" | "source" | "sources" | "app" | "apps" | "components" | "include" => {
                Some(FileIcon::Code)
            }
            "test" | "tests" | "__tests__" | "spec" | "specs" | "fixtures" | "testing" => {
                Some(FileIcon::Test)
            }
            "doc" | "docs" | "documentation" | "man" | "wiki" => Some(FileIcon::Book),
            "assets" | "images" | "img" | "icons" | "media" | "pictures" => Some(FileIcon::Image),
            "audio" | "sounds" | "music" => Some(FileIcon::Audio),
            "fonts" | "typefaces" => Some(FileIcon::Font),
            "lib" | "libs" | "vendor" | "node_modules" | "packages" | "crates" | "deps"
            | "third_party" | "third-party" => Some(FileIcon::Package),
            "build" | "dist" | "out" | "target" | "bin" | "obj" => Some(FileIcon::Build),
            ".git" => Some(FileIcon::Git),
            ".github" | ".gitlab" | ".circleci" | "workflows" | "ci" => Some(FileIcon::Workflow),
            "config" | "configs" | "settings" | ".config" | ".vscode" | ".idea" => {
                Some(FileIcon::Settings)
            }
            "script" | "scripts" | "tools" => Some(FileIcon::Shell),
            "data" | "datasets" => Some(FileIcon::Data),
            "db" | "database" | "migrations" => Some(FileIcon::Database),
            "public" | "static" | "web" | "www" => Some(FileIcon::Web),
            ".ssh" | "certs" | "certificates" => Some(FileIcon::Lock),
            _ => None,
        })
    }

    pub fn paint(self, ui: &egui::Ui, rect: egui::Rect, expanded: bool, fallback: Color32) {
        if !ui.is_rect_visible(rect) {
            return;
        }
        let Some(mark) = self.0 else {
            let icon = if expanded {
                Icon::FolderOpen
            } else {
                Icon::Folder
            };
            icon.paint(ui, rect, fallback);
            return;
        };
        let (body, mark_center) = if expanded {
            (OPEN_FOLDER, egui::vec2(0.3, 1.4))
        } else {
            (CLOSED_FOLDER, egui::vec2(0.0, 1.0))
        };
        let texture = cached_texture(
            ui.ctx(),
            rect.width(),
            egui::Id::new(("named-folder", expanded)),
            || body,
        );
        paint_texture(ui, rect, &texture, mark.tint(ui, fallback));
        mark.paint(
            ui,
            egui::Rect::from_center_size(rect.center() + mark_center, egui::Vec2::splat(9.0)),
            fallback,
        );
    }
}

const CLOSED_FOLDER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="white" fill-opacity=".17" stroke="white" stroke-width="1.15" stroke-linejoin="round" d="M2.6 3h3.6l1.6 1.5h5.6a1 1 0 0 1 1 1v6.8a1 1 0 0 1-1 1H2.6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1Z"/></svg>"#;
const OPEN_FOLDER: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 16 16"><path fill="none" stroke="white" stroke-width="1.15" stroke-linejoin="round" d="M1.6 12V4a1 1 0 0 1 1-1h3.6l1.6 1.5h4.6a1 1 0 0 1 1 1v1"/><path fill="white" fill-opacity=".17" stroke="white" stroke-width="1.15" stroke-linejoin="round" d="M1.6 12l1.7-5a1 1 0 0 1 1-.7h9.4a1 1 0 0 1 .95 1.3l-1.6 5a1 1 0 0 1-.95.7H2.6a1 1 0 0 1-1-1.3Z"/></svg>"#;

fn ascii_lowercase(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(name.to_ascii_lowercase())
    } else {
        Cow::Borrowed(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_and_compound_suffixes_beat_generic_associations() {
        for (name, icon) in [
            ("main.rs", FileIcon::Rust),
            ("Cargo.toml", FileIcon::Rust),
            ("PACKAGE.JSON", FileIcon::Package),
            ("tsconfig.build.json", FileIcon::TypeScript),
            ("index.d.ts", FileIcon::TypeDeclaration),
            ("index.ts", FileIcon::TypeScript),
            ("index.tsx", FileIcon::React),
            ("source.C", FileIcon::Cpp),
            ("source.c", FileIcon::C),
            ("photo.SVG", FileIcon::Image),
            ("bundle.tar.gz", FileIcon::Archive),
            ("compose.yml", FileIcon::Container),
            ("config.yml", FileIcon::Yaml),
            ("config.toml", FileIcon::Toml),
            ("Dockerfile.dev", FileIcon::Container),
            (".env.production", FileIcon::Settings),
            (".gitignore", FileIcon::Git),
            ("README.md", FileIcon::Book),
            ("notes.md", FileIcon::Markdown),
            ("extensionless", FileIcon::Generic),
            ("file.unknown-extension", FileIcon::Generic),
        ] {
            assert_eq!(FileIcon::for_name(name), icon, "{name}");
        }
    }

    #[test]
    fn folder_roles_share_aliases_and_have_a_generic_fallback() {
        assert_eq!(FolderIcon::for_name("SRC"), FolderIcon::for_name("source"));
        assert_eq!(FolderIcon::for_name("src").0, Some(FileIcon::Code));
        assert_eq!(FolderIcon::for_name("tests").0, Some(FileIcon::Test));
        assert_eq!(FolderIcon::for_name(".github").0, Some(FileIcon::Workflow));
        assert_eq!(FolderIcon::for_name("documents-2026").0, None);
        assert_eq!(FileIcon::for_name("src"), FileIcon::Generic);
    }

    #[test]
    fn every_file_and_folder_mask_has_visible_artwork_at_ui_sizes() {
        use super::super::rasterize;

        let icons = FileIcon::ALL.iter().map(|icon| icon.svg());
        for svg in icons.chain([CLOSED_FOLDER, OPEN_FOLDER]) {
            for pixels in [9, 16, 32] {
                let image = rasterize(svg, pixels).unwrap();
                assert!(image.pixels.iter().filter(|color| color.a() > 127).count() > 4);
            }
        }
        assert_ne!(rasterize(CLOSED_FOLDER, 16), rasterize(OPEN_FOLDER, 16));
    }
}
