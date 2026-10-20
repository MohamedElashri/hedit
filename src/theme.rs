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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub version: u32,
    pub name: String,
    pub dark: bool,
    pub background: Rgb,
    pub surface: Rgb,
    pub panel: Rgb,
    pub elevated: Rgb,
    pub border: Rgb,
    pub text: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub selection: Rgb,
    pub selection_text: Rgb,
    pub warning: Rgb,
    pub error: Rgb,
    pub success: Rgb,
    pub comment: Rgb,
    pub keyword: Rgb,
    pub string: Rgb,
    pub number: Rgb,
    pub cuda: Rgb,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Hedit Ink".into(),
            dark: true,
            background: Rgb::new(16, 22, 31),
            surface: Rgb::new(22, 30, 41),
            panel: Rgb::new(19, 26, 36),
            elevated: Rgb::new(32, 43, 57),
            border: Rgb::new(43, 57, 73),
            text: Rgb::new(220, 230, 240),
            muted: Rgb::new(141, 160, 180),
            accent: Rgb::new(100, 216, 203),
            selection: Rgb::new(38, 78, 84),
            selection_text: Rgb::new(238, 252, 250),
            warning: Rgb::new(237, 190, 123),
            error: Rgb::new(246, 151, 151),
            success: Rgb::new(147, 211, 161),
            comment: Rgb::new(133, 156, 163),
            keyword: Rgb::new(190, 167, 244),
            string: Rgb::new(176, 210, 154),
            number: Rgb::new(237, 190, 123),
            cuda: Rgb::new(100, 216, 203),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Ink,
    Paper,
    Nord,
    Ember,
}
impl Preset {
    pub const ALL: [Self; 4] = [Self::Ink, Self::Paper, Self::Nord, Self::Ember];
    pub fn name(self) -> &'static str {
        match self {
            Self::Ink => "Hedit Ink",
            Self::Paper => "Hedit Paper",
            Self::Nord => "Nord",
            Self::Ember => "Ember",
        }
    }
    pub fn theme(self) -> Theme {
        let mut t = Theme {
            name: self.name().into(),
            ..Theme::default()
        };
        let colors: Option<[&str; 18]> = match self {
            Self::Ink => None,
            Self::Paper => {
                t.dark = false;
                Some([
                    "#FAF8F3", "#F0EEE8", "#F4F2EC", "#E5E7DF", "#CCD2CB", "#243832", "#556B62",
                    "#147568", "#D1E8DE", "#163E33", "#8B5918", "#AA3540", "#287847", "#59776B",
                    "#7354A3", "#426D26", "#95552A", "#147568",
                ])
            }
            Self::Nord => Some([
                "#2E3440", "#353C4A", "#303744", "#434C5E", "#4C566A", "#ECEFF4", "#A3B1C6",
                "#88C0D0", "#44556D", "#ECEFF4", "#EBCB8B", "#E69A9E", "#A3BE8C", "#A3B1C6",
                "#B48EAD", "#A3BE8C", "#EBCB8B", "#8FBCBB",
            ]),
            Self::Ember => Some([
                "#211C20", "#2B242A", "#261F25", "#3A2D35", "#51404B", "#F0E4DE", "#BAA39E",
                "#F5AE82", "#62463F", "#FFF3E7", "#ECC98D", "#EE9B9B", "#ACCAAA", "#B5A19F",
                "#D4ACD9", "#BED098", "#EBC28B", "#82CEC2",
            ]),
        };
        if let Some(colors) = colors {
            for (role, color) in Role::ALL.into_iter().zip(colors) {
                *t.color_mut(role) =
                    Rgb::try_from(color.to_owned()).expect("preset colors are valid");
            }
        }
        t
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Background,
    Surface,
    Panel,
    Elevated,
    Border,
    Text,
    Muted,
    Accent,
    Selection,
    SelectionText,
    Warning,
    Error,
    Success,
    Comment,
    Keyword,
    String,
    Number,
    Cuda,
}
impl Role {
    pub const ALL: [Self; 18] = [
        Self::Background,
        Self::Surface,
        Self::Panel,
        Self::Elevated,
        Self::Border,
        Self::Text,
        Self::Muted,
        Self::Accent,
        Self::Selection,
        Self::SelectionText,
        Self::Warning,
        Self::Error,
        Self::Success,
        Self::Comment,
        Self::Keyword,
        Self::String,
        Self::Number,
        Self::Cuda,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Background => "Editor background",
            Self::Surface => "Surface",
            Self::Panel => "Sidebar / toolbar",
            Self::Elevated => "Controls / hover",
            Self::Border => "Borders",
            Self::Text => "Editor text",
            Self::Muted => "Secondary text",
            Self::Accent => "Accent / cursor",
            Self::Selection => "Selection background",
            Self::SelectionText => "Selection text",
            Self::Warning => "Warning",
            Self::Error => "Error",
            Self::Success => "Recovery success",
            Self::Comment => "Syntax · comments",
            Self::Keyword => "Syntax · keywords",
            Self::String => "Syntax · strings",
            Self::Number => "Syntax · numbers",
            Self::Cuda => "Syntax · CUDA",
        }
    }
}
impl Theme {
    pub fn color_mut(&mut self, role: Role) -> &mut Rgb {
        match role {
            Role::Background => &mut self.background,
            Role::Surface => &mut self.surface,
            Role::Panel => &mut self.panel,
            Role::Elevated => &mut self.elevated,
            Role::Border => &mut self.border,
            Role::Text => &mut self.text,
            Role::Muted => &mut self.muted,
            Role::Accent => &mut self.accent,
            Role::Selection => &mut self.selection,
            Role::SelectionText => &mut self.selection_text,
            Role::Warning => &mut self.warning,
            Role::Error => &mut self.error,
            Role::Success => &mut self.success,
            Role::Comment => &mut self.comment,
            Role::Keyword => &mut self.keyword,
            Role::String => &mut self.string,
            Role::Number => &mut self.number,
            Role::Cuda => &mut self.cuda,
        }
    }
    pub fn color(&self, role: Role) -> Rgb {
        let mut copy = self.clone();
        *copy.color_mut(role)
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err(format!(
                "Unsupported theme version {}. Expected 1.",
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
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
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
            w.corner_radius = egui::CornerRadius::same(5);
        }
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent.color());
        v.widgets.active.bg_fill = self.selection.color();
        v.widgets.active.fg_stroke = Stroke::new(1.0, self.selection_text.color());
        v.text_cursor.stroke = Stroke::new(1.5, self.accent.color());
        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.scroll.bar_width = 7.0;
        ctx.set_style(style);
    }
}
