//! Native, portable color themes. Serialized colors are ordinary #RRGGBB strings.
use eframe::egui::{self, Color32, Stroke};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb(pub [u8; 3]);
impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b])
    }
    pub fn color(self) -> Color32 {
        Color32::from_rgb(self.0[0], self.0[1], self.0[2])
    }
    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.0[0], self.0[1], self.0[2])
    }
    fn luminance(self) -> f32 {
        self.0
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(c, weight)| {
                let c = c as f32 / 255.0;
                weight
                    * if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }
            })
            .sum()
    }
}
impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.strip_prefix('#').unwrap_or(&value);
        if value.len() != 6 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Use a six-digit hex color, for example #64D8CB.".into());
        }
        Ok(Self([
            u8::from_str_radix(&value[0..2], 16).map_err(|e| e.to_string())?,
            u8::from_str_radix(&value[2..4], 16).map_err(|e| e.to_string())?,
            u8::from_str_radix(&value[4..6], 16).map_err(|e| e.to_string())?,
        ]))
    }
}
impl From<Rgb> for String {
    fn from(value: Rgb) -> Self {
        value.hex()
    }
}

/// Every color role in a theme, in serialization and catalog order.
macro_rules! roles {
    ($( $field:ident, $variant:ident, $key:literal, $label:literal, $group:ident; )*) => {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct Theme {
            pub version: u32,
            pub name: String,
            pub dark: bool,
            $( pub $field: Rgb, )*
        }
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Role { $( $variant, )* }
        impl Role {
            pub const ALL: [Self; [$($key),*].len()] = [$(Self::$variant),*];
            /// The JSON key for this role.
            pub fn key(self) -> &'static str { match self { $( Self::$variant => $key, )* } }
            pub fn label(self) -> &'static str { match self { $( Self::$variant => $label, )* } }
            pub fn group(self) -> Group { match self { $( Self::$variant => Group::$group, )* } }
        }
        impl Theme {
            pub fn color_mut(&mut self, role: Role) -> &mut Rgb {
                match role { $( Role::$variant => &mut self.$field, )* }
            }
            pub fn color(&self, role: Role) -> Rgb {
                match role { $( Role::$variant => self.$field, )* }
            }
        }
    };
}
roles! {
    background, Background, "background", "Editor background", Interface;
    surface, Surface, "surface", "Surface", Interface;
    panel, Panel, "panel", "Sidebar / toolbar", Interface;
    elevated, Elevated, "elevated", "Controls / hover", Interface;
    border, Border, "border", "Borders", Interface;
    text, Text, "text", "Editor text", Interface;
    muted, Muted, "muted", "Secondary text", Interface;
    accent, Accent, "accent", "Accent / cursor", Interface;
    selection, Selection, "selection", "Selection background", Interface;
    selection_text, SelectionText, "selection_text", "Selection text", Interface;
    warning, Warning, "warning", "Warning", Status;
    error, Error, "error", "Error", Status;
    success, Success, "success", "Success", Status;
    comment, Comment, "comment", "Comments", Syntax;
    keyword, Keyword, "keyword", "Keywords", Syntax;
    string, String, "string", "Strings", Syntax;
    number, Number, "number", "Numbers", Syntax;
    cuda, Cuda, "cuda", "CUDA qualifiers", Syntax;
    function, Function, "function", "Functions", Syntax;
    type_name, Type, "type", "Types", Syntax;
    constant, Constant, "constant", "Constants", Syntax;
    operator, Operator, "operator", "Operators", Syntax;
    property, Property, "property", "Properties / keys", Syntax;
    preprocessor, Preprocessor, "preprocessor", "Preprocessor / attributes", Syntax;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Interface,
    Status,
    Syntax,
}
impl Group {
    pub const ALL: [Self; 3] = [Self::Interface, Self::Syntax, Self::Status];
    pub fn label(self) -> &'static str {
        match self {
            Self::Interface => "Interface",
            Self::Status => "Status",
            Self::Syntax => "Syntax",
        }
    }
}

/// Roles added in theme version 2. Older files inherit them from a version-1 role.
const DERIVED: [(Role, Role); 6] = [
    (Role::Function, Role::Text),
    (Role::Type, Role::Text),
    (Role::Constant, Role::Number),
    (Role::Operator, Role::Text),
    (Role::Property, Role::Keyword),
    (Role::Preprocessor, Role::Keyword),
];

impl Default for Theme {
    fn default() -> Self {
        builtin(INK).expect("Hedit Ink is built in")
    }
}

impl Serialize for Theme {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(Role::ALL.len() + 3))?;
        map.serialize_entry("version", &self.version)?;
        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("dark", &self.dark)?;
        for role in Role::ALL {
            map.serialize_entry(role.key(), &self.color(role))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let fields = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let text = |key: &str| -> Result<Option<String>, D::Error> {
            fields
                .get(key)
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| D::Error::custom(format!("`{key}` must be a string")))
                })
                .transpose()
        };
        let base = match text("base")? {
            Some(name) => builtin(&name)
                .ok_or_else(|| D::Error::custom(format!("unknown base theme `{name}`")))?,
            None => Theme::default(),
        };
        let mut theme = Theme {
            name: base.name.clone(),
            ..base
        };
        for (key, value) in &fields {
            match key.as_str() {
                "version" => {
                    theme.version = value
                        .as_u64()
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or_else(|| D::Error::custom("`version` must be a number"))?
                }
                "name" => theme.name = text("name")?.unwrap_or_default(),
                "dark" => {
                    theme.dark = value
                        .as_bool()
                        .ok_or_else(|| D::Error::custom("`dark` must be true or false"))?
                }
                "base" => {}
                key => {
                    let role = Role::ALL
                        .into_iter()
                        .find(|role| role.key() == key)
                        .ok_or_else(|| D::Error::custom(format!("unknown field `{key}`")))?;
                    let color = text(key)?.expect("field is present");
                    *theme.color_mut(role) = Rgb::try_from(color)
                        .map_err(|e| D::Error::custom(format!("`{key}`: {e}")))?;
                }
            }
        }
        if !(1..=VERSION).contains(&theme.version) {
            return Err(D::Error::custom(format!(
                "unsupported theme version {}; expected 1 or {VERSION}",
                theme.version
            )));
        }
        for (role, source) in DERIVED {
            if !fields.contains_key(role.key()) && fields.contains_key(source.key()) {
                *theme.color_mut(role) = theme.color(source);
            }
        }
        // Files may use an older format; in memory every theme is current.
        theme.version = VERSION;
        Ok(theme)
    }
}

pub const INK: &str = "Hedit Ink";
pub const PAPER: &str = "Hedit Paper";

/// A built-in palette. Colors follow `Role::ALL`.
struct Builtin {
    name: &'static str,
    dark: bool,
    colors: [&'static str; Role::ALL.len()],
}

#[rustfmt::skip]
const BUILTINS: &[Builtin] = &[
    // background surface panel elevated border text muted accent selection selection_text
    // warning error success | comment keyword string number cuda function type constant operator property preprocessor
    Builtin { name: INK, dark: true, colors: [
        "#10161F", "#161E29", "#131A24", "#202B39", "#2B3949", "#DCE6F0", "#8DA0B4", "#64D8CB", "#264E54", "#EEFCFA",
        "#EDBE7B", "#F69797", "#93D3A1",
        "#859CA3", "#BEA7F4", "#B0D29A", "#EDBE7B", "#64D8CB", "#DCE6F0", "#DCE6F0", "#EDBE7B", "#DCE6F0", "#BEA7F4", "#BEA7F4"] },
    Builtin { name: "One Dark", dark: true, colors: [
        "#282C34", "#2C313A", "#21252B", "#333842", "#3E4451", "#ABB2BF", "#8B929E", "#528BFF", "#3E4451", "#E6E9EF",
        "#E5C07B", "#E06C75", "#98C379",
        "#7F848E", "#C678DD", "#98C379", "#D19A66", "#56B6C2", "#61AFEF", "#E5C07B", "#D19A66", "#56B6C2", "#E06C75", "#C678DD"] },
    Builtin { name: "Dracula", dark: true, colors: [
        "#282A36", "#2E303E", "#21222C", "#3A3C4E", "#44475A", "#F8F8F2", "#A4ACCF", "#BD93F9", "#44475A", "#F8F8F2",
        "#FFB86C", "#FF5555", "#50FA7B",
        "#6272A4", "#FF79C6", "#F1FA8C", "#BD93F9", "#8BE9FD", "#50FA7B", "#8BE9FD", "#BD93F9", "#FF79C6", "#66D9EF", "#FF79C6"] },
    Builtin { name: "Monokai", dark: true, colors: [
        "#272822", "#2D2E27", "#1E1F1C", "#3E3D32", "#49483E", "#F8F8F2", "#A59F85", "#A6E22E", "#49483E", "#F8F8F2",
        "#E6DB74", "#F92672", "#A6E22E",
        "#88846F", "#F92672", "#E6DB74", "#AE81FF", "#66D9EF", "#A6E22E", "#66D9EF", "#AE81FF", "#F92672", "#FD971F", "#F92672"] },
    Builtin { name: "Nord", dark: true, colors: [
        "#2E3440", "#353C4A", "#2B303B", "#3B4252", "#434C5E", "#D8DEE9", "#A3B1C6", "#88C0D0", "#434C5E", "#ECEFF4",
        "#EBCB8B", "#BF616A", "#A3BE8C",
        "#7B88A1", "#81A1C1", "#A3BE8C", "#B48EAD", "#8FBCBB", "#88C0D0", "#8FBCBB", "#81A1C1", "#81A1C1", "#8FBCBB", "#5E81AC"] },
    Builtin { name: "Gruvbox Dark", dark: true, colors: [
        "#282828", "#32302F", "#1D2021", "#3C3836", "#504945", "#EBDBB2", "#A89984", "#FE8019", "#504945", "#FBF1C7",
        "#FABD2F", "#FB4934", "#B8BB26",
        "#928374", "#FB4934", "#B8BB26", "#D3869B", "#8EC07C", "#B8BB26", "#FABD2F", "#D3869B", "#8EC07C", "#83A598", "#8EC07C"] },
    Builtin { name: "Solarized Dark", dark: true, colors: [
        "#002B36", "#073642", "#00252F", "#0A4250", "#194E5B", "#93A1A1", "#839496", "#268BD2", "#0A4F5E", "#EEE8D5",
        "#B58900", "#DC322F", "#859900",
        "#657B83", "#859900", "#2AA198", "#D33682", "#6C71C4", "#268BD2", "#B58900", "#CB4B16", "#859900", "#268BD2", "#CB4B16"] },
    Builtin { name: "Tokyo Night", dark: true, colors: [
        "#1A1B26", "#1F2335", "#16161E", "#292E42", "#3B4261", "#C0CAF5", "#9AA5CE", "#7AA2F7", "#33467C", "#E3E9FF",
        "#E0AF68", "#F7768E", "#9ECE6A",
        "#565F89", "#BB9AF7", "#9ECE6A", "#FF9E64", "#7DCFFF", "#7AA2F7", "#2AC3DE", "#FF9E64", "#89DDFF", "#73DACA", "#7DCFFF"] },
    Builtin { name: "Catppuccin Mocha", dark: true, colors: [
        "#1E1E2E", "#24273A", "#181825", "#313244", "#45475A", "#CDD6F4", "#A6ADC8", "#CBA6F7", "#45475A", "#E6EBFF",
        "#F9E2AF", "#F38BA8", "#A6E3A1",
        "#9399B2", "#CBA6F7", "#A6E3A1", "#FAB387", "#94E2D5", "#89B4FA", "#F9E2AF", "#FAB387", "#89DCEB", "#B4BEFE", "#F5C2E7"] },
    Builtin { name: "GitHub Dark", dark: true, colors: [
        "#0D1117", "#161B22", "#010409", "#21262D", "#30363D", "#E6EDF3", "#9198A1", "#4493F8", "#264F78", "#F0F6FC",
        "#D29922", "#F85149", "#3FB950",
        "#8B949E", "#FF7B72", "#A5D6FF", "#79C0FF", "#FFA657", "#D2A8FF", "#FFA657", "#79C0FF", "#FF7B72", "#79C0FF", "#FF7B72"] },
    Builtin { name: "Rosé Pine", dark: true, colors: [
        "#191724", "#1F1D2E", "#1F1D2E", "#26233A", "#403D52", "#E0DEF4", "#908CAA", "#EBBCBA", "#403D52", "#F4F2FF",
        "#F6C177", "#EB6F92", "#9CCFD8",
        "#6E6A86", "#31748F", "#F6C177", "#EBBCBA", "#9CCFD8", "#EBBCBA", "#9CCFD8", "#EBBCBA", "#908CAA", "#C4A7E7", "#EB6F92"] },
    Builtin { name: PAPER, dark: false, colors: [
        "#FAF8F3", "#F0EEE8", "#F4F2EC", "#E5E7DF", "#CCD2CB", "#243832", "#556B62", "#147568", "#D1E8DE", "#163E33",
        "#8B5918", "#AA3540", "#287847",
        "#59776B", "#7354A3", "#426D26", "#95552A", "#147568", "#243832", "#243832", "#95552A", "#243832", "#7354A3", "#7354A3"] },
    Builtin { name: "One Light", dark: false, colors: [
        "#FAFAFA", "#F0F0F1", "#EAEAEB", "#E5E5E6", "#DBDBDC", "#383A42", "#696C77", "#526FFF", "#E5E5E6", "#232324",
        "#C18401", "#E45649", "#50A14F",
        "#A0A1A7", "#A626A4", "#50A14F", "#986801", "#0184BC", "#4078F2", "#C18401", "#986801", "#0184BC", "#E45649", "#A626A4"] },
    Builtin { name: "GitHub Light", dark: false, colors: [
        "#FFFFFF", "#F6F8FA", "#F6F8FA", "#EAEEF2", "#D0D7DE", "#1F2328", "#59636E", "#0969DA", "#B6E3FF", "#1F2328",
        "#9A6700", "#D1242F", "#1A7F37",
        "#6E7781", "#CF222E", "#0A3069", "#0550AE", "#953800", "#8250DF", "#953800", "#0550AE", "#CF222E", "#0550AE", "#CF222E"] },
    Builtin { name: "Solarized Light", dark: false, colors: [
        "#FDF6E3", "#F5EFDC", "#EEE8D5", "#E6DFC8", "#D8D0B8", "#586E75", "#657B83", "#268BD2", "#E2DBC4", "#073642",
        "#B58900", "#DC322F", "#859900",
        "#93A1A1", "#859900", "#2AA198", "#D33682", "#6C71C4", "#268BD2", "#B58900", "#CB4B16", "#859900", "#268BD2", "#CB4B16"] },
    Builtin { name: "Gruvbox Light", dark: false, colors: [
        "#FBF1C7", "#F9F5D7", "#F2E5BC", "#EBDBB2", "#D5C4A1", "#3C3836", "#7C6F64", "#AF3A03", "#D5C4A1", "#282828",
        "#B57614", "#9D0006", "#79740E",
        "#928374", "#9D0006", "#79740E", "#8F3F71", "#427B58", "#79740E", "#B57614", "#8F3F71", "#427B58", "#076678", "#427B58"] },
    Builtin { name: "Catppuccin Latte", dark: false, colors: [
        "#EFF1F5", "#E9ECF2", "#E6E9EF", "#DCE0E8", "#CCD0DA", "#4C4F69", "#6C6F85", "#8839EF", "#CCD0DA", "#363A4F",
        "#DF8E1D", "#D20F39", "#40A02B",
        "#7C7F93", "#8839EF", "#40A02B", "#FE640B", "#179299", "#1E66F5", "#DF8E1D", "#FE640B", "#04A5E5", "#7287FD", "#EA76CB"] },
    Builtin { name: "Rosé Pine Dawn", dark: false, colors: [
        "#FAF4ED", "#FFFAF3", "#FFFAF3", "#F2E9E1", "#DFDAD9", "#575279", "#797593", "#D7827E", "#DFDAD9", "#464261",
        "#EA9D34", "#B4637A", "#56949F",
        "#9893A5", "#286983", "#EA9D34", "#D7827E", "#56949F", "#D7827E", "#56949F", "#D7827E", "#797593", "#907AA9", "#B4637A"] },
];

/// The built-in themes, dark themes first, each group in catalog order.
pub fn builtins() -> &'static [Theme] {
    static THEMES: std::sync::OnceLock<Vec<Theme>> = std::sync::OnceLock::new();
    THEMES.get_or_init(|| {
        let mut themes: Vec<_> = BUILTINS
            .iter()
            .map(|builtin| {
                let mut theme = Theme {
                    version: VERSION,
                    name: builtin.name.into(),
                    dark: builtin.dark,
                    ..ZERO
                };
                for (role, color) in Role::ALL.into_iter().zip(builtin.colors) {
                    *theme.color_mut(role) =
                        Rgb::try_from(color.to_owned()).expect("built-in colors are valid");
                }
                theme
            })
            .collect();
        themes.sort_by_key(|theme| !theme.dark);
        themes
    })
}

/// A built-in theme by name, ignoring case.
pub fn builtin(name: &str) -> Option<Theme> {
    builtins()
        .iter()
        .find(|theme| theme.name.eq_ignore_ascii_case(name))
        .cloned()
}

pub const VERSION: u32 = 2;
const BLACK: Rgb = Rgb::new(0, 0, 0);
const ZERO: Theme = Theme {
    version: VERSION,
    name: String::new(),
    dark: true,
    background: BLACK,
    surface: BLACK,
    panel: BLACK,
    elevated: BLACK,
    border: BLACK,
    text: BLACK,
    muted: BLACK,
    accent: BLACK,
    selection: BLACK,
    selection_text: BLACK,
    warning: BLACK,
    error: BLACK,
    success: BLACK,
    comment: BLACK,
    keyword: BLACK,
    string: BLACK,
    number: BLACK,
    cuda: BLACK,
    function: BLACK,
    type_name: BLACK,
    constant: BLACK,
    operator: BLACK,
    property: BLACK,
    preprocessor: BLACK,
};

impl Theme {
    pub fn paper() -> Self {
        builtin(PAPER).expect("Hedit Paper is built in")
    }
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=VERSION).contains(&self.version) {
            return Err(format!(
                "Unsupported theme version {}. Expected 1 or {VERSION}.",
                self.version
            ));
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return Err("Theme name must contain 1–80 characters.".into());
        }
        Ok(())
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        let theme: Self = serde_json::from_str(json).map_err(|e| format!("Invalid theme: {e}"))?;
        theme.validate()?;
        Ok(theme)
    }
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        let theme = Theme {
            version: VERSION,
            ..self.clone()
        };
        serde_json::to_string_pretty(&theme).map_err(|e| e.to_string())
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        use std::io::Read;
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 {
            return Err("Theme files must be at most 64 KiB.".into());
        }
        let mut json = String::new();
        file.take(64 * 1024 + 1)
            .read_to_string(&mut json)
            .map_err(|e| e.to_string())?;
        if json.len() > 64 * 1024 {
            return Err("Theme files must be at most 64 KiB.".into());
        }
        Self::from_json(&json)
    }
    pub fn export(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        file.write_all(self.to_json()?.as_bytes())
            .map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        crate::session::sync_directory(parent)
    }
    /// The fill under a hovered or open menu entry or button: the surface tinted
    /// toward the accent, so it stands out on both the menu bar and popups.
    pub fn hover(&self) -> Rgb {
        let mix = |surface: u8, accent: u8| {
            (surface as f32 + (accent as f32 - surface as f32) * 0.3).round() as u8
        };
        let (s, a) = (self.surface.0, self.accent.0);
        Rgb([mix(s[0], a[0]), mix(s[1], a[1]), mix(s[2], a[2])])
    }
    pub fn contrast(a: Rgb, b: Rgb) -> f32 {
        let (a, b) = (a.luminance(), b.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    pub fn apply(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = Some(self.text.color());
        v.panel_fill = self.panel.color();
        v.window_fill = self.surface.color();
        v.extreme_bg_color = self.background.color();
        v.faint_bg_color = self.surface.color();
        v.code_bg_color = self.background.color();
        v.hyperlink_color = self.accent.color();
        v.warn_fg_color = self.warning.color();
        v.error_fg_color = self.error.color();
        v.selection.bg_fill = self.selection.color();
        v.selection.stroke = Stroke::new(1.0, self.selection_text.color());
        v.window_stroke = Stroke::new(1.0, self.border.color());
        v.widgets.noninteractive.bg_fill = self.surface.color();
        v.widgets.noninteractive.weak_bg_fill = self.surface.color();
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border.color());
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.muted.color());
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.bg_fill = self.elevated.color();
            w.weak_bg_fill = self.surface.color();
            w.bg_stroke = Stroke::new(1.0, self.border.color());
            w.fg_stroke = Stroke::new(1.0, self.text.color());
            w.corner_radius = egui::CornerRadius::same(2);
        }
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent.color());
        // Menus drop button strokes, so hover and open states need a visible fill.
        v.widgets.hovered.weak_bg_fill = self.hover().color();
        v.widgets.open.weak_bg_fill = self.hover().color();
        v.widgets.active.bg_fill = self.selection.color();
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.selection_text.color());
        v.text_cursor.stroke = Stroke::new(1.5, self.accent.color());
        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(7.0, 4.0);
        style.spacing.scroll.bar_width = 7.0;
        // Pin egui's own light/dark choice so a system appearance change cannot
        // swap in its default style for the other mode.
        ctx.set_theme(if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        ctx.set_style(style);
    }
}

/// A theme saved in the user's theme folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserTheme {
    pub path: std::path::PathBuf,
    pub theme: Theme,
}

/// At most this many files are read from a theme folder.
pub const LIBRARY_LIMIT: usize = 200;

/// Read `*.json` themes from `dir`, sorted by name. Unreadable files are reported, not fatal.
pub fn load_library(dir: &Path) -> (Vec<UserTheme>, Vec<String>) {
    let mut themes = Vec::new();
    let mut errors = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (themes, errors),
        Err(error) => return (themes, vec![format!("{}: {error}", dir.display())]),
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json") && path.is_file())
        .collect();
    paths.sort();
    if paths.len() > LIBRARY_LIMIT {
        errors.push(format!(
            "Only the first {LIBRARY_LIMIT} themes in {} were loaded.",
            dir.display()
        ));
        paths.truncate(LIBRARY_LIMIT);
    }
    for path in paths {
        match Theme::load(&path) {
            Ok(theme) => themes.push(UserTheme { path, theme }),
            Err(error) => errors.push(format!(
                "{}: {error}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    themes.sort_by_key(|t| t.theme.name.to_lowercase());
    (themes, errors)
}

/// `name`, or `name 2`, `name 3`, … so it differs from every name in `taken`, ignoring case.
pub fn unique_name<'a>(name: &str, taken: impl Iterator<Item = &'a str> + Clone) -> String {
    let name = name.trim();
    let free = |candidate: &str| {
        !taken
            .clone()
            .any(|other| other.trim().eq_ignore_ascii_case(candidate))
    };
    if free(name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| format!("{name} {n}"))
        .find(|candidate| free(candidate))
        .expect("a free name exists")
}

/// Write `theme` into `dir`, replacing `existing` or choosing a new file name.
pub fn save_to_library(
    dir: &Path,
    theme: &Theme,
    existing: Option<&Path>,
) -> Result<std::path::PathBuf, String> {
    if builtin(&theme.name).is_some() {
        return Err(format!(
            "“{}” is a built-in theme. Choose another name.",
            theme.name.trim()
        ));
    }
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = match existing {
        Some(path) => path.to_owned(),
        None => {
            let slug = slug(&theme.name);
            (1..)
                .map(|n| {
                    dir.join(if n == 1 {
                        format!("{slug}.json")
                    } else {
                        format!("{slug}-{n}.json")
                    })
                })
                .find(|path| !path.exists())
                .expect("a free file name exists")
        }
    };
    theme.export(&path)?;
    Ok(path)
}

/// A lowercase, hyphenated file stem for a theme name.
pub fn slug(name: &str) -> String {
    let slug = name
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        "theme".into()
    } else {
        slug
    }
}
