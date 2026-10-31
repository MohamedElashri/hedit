//! Loads, migrates, saves, and reloads the settings, key binding, and snippet files.
use super::quality::Config;
use super::*;
use hedit::{projects, settings::Settings};
use std::{path::Path, time::SystemTime};

pub(super) const SETTINGS_FILE: &str = "settings.json";
pub(super) const KEYBINDINGS_FILE: &str = "keybindings.json";
pub(super) const SNIPPETS_FILE: &str = "snippets.json";
pub(super) const MACROS_FILE: &str = "macros.json";
/// Write settings this long after the last change, so a dragged slider
/// becomes one write.
const SETTLE: Duration = Duration::from_millis(400);

#[cfg(test)]
thread_local! {
    /// Tests keep settings in memory unless they choose a folder for their thread.
    pub(super) static TEST_CONFIG_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) struct SettingsFiles {
    /// The configuration directory; `None` keeps settings in memory only (tests).
    pub dir: Option<PathBuf>,
    /// The settings as last read from or written to disk.
    pub saved: Settings,
    pub changed_at: Option<Instant>,
    /// The file's modification time when last read or written.
    pub stamp: Option<SystemTime>,
    /// The file could not be parsed; do not overwrite it until it is fixed.
    pub broken: bool,
    /// Key bindings and snippets as last written, and when they changed.
    pub config_saved: Config,
    pub config_changed_at: Option<Instant>,
    /// The exclusion patterns folder walks currently use.
    pub excludes: Option<(Vec<String>, Vec<String>)>,
}

/// The pre-0.1.4 editor-settings.json, which also held formatting defaults.
#[derive(Default, serde::Deserialize)]
#[serde(default)]
struct Legacy {
    defaults: projects::Defaults,
    keybindings: Vec<quality::Binding>,
    snippets: Option<Vec<hedit::intelligence::Snippet>>,
}

fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Settings from window.ron before they moved to settings.json.
fn from_storage(storage: Option<&dyn eframe::Storage>) -> Settings {
    let mut settings = Settings::default();
    let Some(storage) = storage else {
        return settings;
    };
    macro_rules! migrate {
        ($($key:literal => $field:ident),* $(,)?) => {$(
            if let Some(value) = eframe::get_value(storage, $key) {
                settings.$field = value;
            }
        )*};
    }
    migrate!(
        "wrap" => word_wrap,
        "line_numbers" => line_numbers,
        "show_tabs" => show_tab_bar,
        "show_status" => show_status_bar,
        "highlight_line" => highlight_line,
        "show_whitespace" => draw_white_space,
        "indent_guides" => draw_indent_guides,
        "minimap" => show_minimap,
        "auto_indent" => auto_indent,
        "smart_indent" => smart_indent,
        "soft_tabs" => smart_backspace,
        "sidebar" => show_sidebar,
        "folder_sidebar" => show_folders,
    );
    if let Some(size) = eframe::get_value::<f32>(storage, "font_size").filter(|s| s.is_finite()) {
        settings.font_size = size.clamp(10.0, 28.0);
    }
    if let Some(column) = eframe::get_value::<u16>(storage, "wrap_column") {
        settings.wrap_width = column.min(hedit::settings::MAX_COLUMN);
    }
    if let Some(column) = eframe::get_value::<u16>(storage, "ruler_column").filter(|&c| c > 0) {
        settings.rulers = vec![column.min(hedit::settings::MAX_COLUMN)];
    }
    if let Some(path) = storage
        .get_string("debugger")
        .filter(|s| !s.trim().is_empty())
    {
        settings.cuda_gdb_path = path;
    }
    let theme_name = |key: &str| {
        storage
            .get_string(key)
            .and_then(|json| Theme::from_json(&json).ok())
            .map(|theme| theme.name)
    };
    for (key, field) in [
        ("theme", &mut settings.color_theme),
        ("theme_dark", &mut settings.dark_color_theme),
        ("theme_light", &mut settings.light_color_theme),
    ] {
        if let Some(name) = theme_name(key) {
            *field = name;
        }
    }
    if let Some(follow) = eframe::get_value(storage, "theme_follow_system") {
        settings.follow_system_appearance = follow;
    }
    settings
}

/// Load settings and the key binding and snippet files, migrating older
/// storage on first run. Problems are returned as messages for the banner.
pub(super) fn load(
    storage: Option<&dyn eframe::Storage>,
    session: &Path,
) -> (Settings, Config, SettingsFiles, Vec<String>) {
    #[cfg(test)]
    let dir = TEST_CONFIG_DIR.with(|dir| dir.borrow().clone());
    #[cfg(not(test))]
    let dir = hedit::settings::config_dir();
    let mut errors = Vec::new();
    let legacy_path = session.join("editor-settings.json");
    let legacy: Option<Legacy> = legacy_path
        .exists()
        .then(|| projects::read_json(&legacy_path, 1024 * 1024))
        .and_then(|result| {
            result
                .map_err(|e| errors.push(format!("Editor settings: {e}")))
                .ok()
        });
    let mut settings = from_storage(storage);
    let mut config = Config::default();
    if let Some(legacy) = &legacy {
        settings.set_defaults(&legacy.defaults);
        config.keybindings = legacy.keybindings.clone();
        if let Some(snippets) = &legacy.snippets {
            config.snippets = snippets.clone();
        }
    }
    let mut files = SettingsFiles {
        dir: dir.clone(),
        saved: settings.clone(),
        changed_at: None,
        stamp: None,
        broken: false,
        config_saved: Config::default(),
        config_changed_at: None,
        excludes: None,
    };
    let Some(dir) = dir else {
        return (settings, config, files, errors);
    };
    let path = dir.join(SETTINGS_FILE);
    if path.exists() {
        match Settings::load(&path) {
            Ok((loaded, warnings)) => {
                settings = loaded;
                errors.extend(warnings);
            }
            Err(error) => {
                errors.push(format!(
                    "{error}\nhedit is using your previous settings and will not overwrite the file until it is fixed."
                ));
                files.broken = true;
            }
        }
        files.saved = settings.clone();
    } else if let Err(error) = settings.save(&path) {
        errors.push(format!("Cannot create {}: {error}", path.display()));
    } else {
        files.saved = settings.clone();
    }
    files.stamp = stamp(&path);
    let bindings = dir.join(KEYBINDINGS_FILE);
    if !bindings.exists() {
        if let Err(error) = projects::atomic_json(&bindings, &config.keybindings) {
            errors.push(format!("Cannot create {}: {error}", bindings.display()));
        }
    }
    let snippets = dir.join(SNIPPETS_FILE);
    if !snippets.exists() {
        if let Err(error) = projects::atomic_json(&snippets, &config.snippets) {
            errors.push(format!("Cannot create {}: {error}", snippets.display()));
        }
    }
    match projects::read_json(&dir.join(KEYBINDINGS_FILE), 1024 * 1024) {
        Ok(bindings) => config.keybindings = bindings,
        Err(error) => errors.push(format!("{KEYBINDINGS_FILE}: {error}")),
    }
    match projects::read_json(&dir.join(SNIPPETS_FILE), 4 * 1024 * 1024) {
        Ok(snippets) => config.snippets = snippets,
        Err(error) => errors.push(format!("{SNIPPETS_FILE}: {error}")),
    }
    // Created only once a macro is saved.
    if dir.join(MACROS_FILE).exists() {
        match projects::read_json(&dir.join(MACROS_FILE), 4 * 1024 * 1024) {
            Ok(macros) => config.macros = macros,
            Err(error) => errors.push(format!("{MACROS_FILE}: {error}")),
        }
    }
    files.config_saved = config.clone();
    (settings, config, files, errors)
}

impl HeditApp {
    pub(super) fn settings_path(&self) -> Option<PathBuf> {
        self.settings_files
            .dir
            .as_ref()
            .map(|d| d.join(SETTINGS_FILE))
    }
    /// Keep find options, the updater, and formatting in step with settings,
    /// and write changes once they settle.
    pub(super) fn persist_settings(&mut self) {
        self.sync_theme_settings();
        self.write_settings(false);
    }
    /// Write pending settings now, as when the app saves its state or exits.
    pub(super) fn flush_settings(&mut self) {
        self.sync_theme_settings();
        self.write_settings(true);
    }
    fn write_settings(&mut self, now: bool) {
        self.apply_excludes();
        self.write_config(now);
        self.settings.find_case_sensitive = self.quality.options.case_sensitive;
        self.settings.find_regex = self.quality.options.regex;
        self.settings.find_whole_word = self.quality.options.whole_word;
        if self.settings == self.settings_files.saved {
            self.settings_files.changed_at = None;
            return;
        }
        let changed = *self
            .settings_files
            .changed_at
            .get_or_insert_with(Instant::now);
        if (!now && changed.elapsed() < SETTLE) || self.settings.validate().is_err() {
            return;
        }
        self.settings_files.changed_at = None;
        self.settings_files.saved = self.settings.clone();
        let (Some(path), false) = (self.settings_path(), self.settings_files.broken) else {
            return;
        };
        match self.settings.save(&path) {
            Ok(()) => self.settings_files.stamp = stamp(&path),
            Err(error) => self.error = Some(format!("Cannot save settings: {error}")),
        }
    }
    /// How long until pending settings, key bindings, or snippets are written.
    pub(super) fn settings_due(&self) -> Option<Duration> {
        let due = |at: Option<Instant>, changed: bool| {
            at.map(|at| SETTLE.saturating_sub(at.elapsed()))
                .or_else(|| changed.then_some(SETTLE))
        };
        let files = &self.settings_files;
        [
            due(files.changed_at, self.settings != files.saved),
            due(
                files.config_changed_at,
                self.quality.config != files.config_saved,
            ),
        ]
        .into_iter()
        .flatten()
        .min()
    }
    /// Use new folder and file exclusion patterns once they are valid.
    fn apply_excludes(&mut self) {
        let wanted = (
            self.settings.folder_exclude_patterns.clone(),
            self.settings.file_exclude_patterns.clone(),
        );
        if self.settings_files.excludes.as_ref() == Some(&wanted)
            || hedit::folder::set_excludes(&wanted.0, &wanted.1).is_err()
        {
            return;
        }
        let first = self.settings_files.excludes.is_none();
        self.settings_files.excludes = Some(wanted);
        if !first {
            self.project.invalidate();
            self.refresh_folder();
        }
    }
    /// Write key bindings and snippets once edits settle.
    fn write_config(&mut self, now: bool) {
        if self.quality.config == self.settings_files.config_saved {
            self.settings_files.config_changed_at = None;
            return;
        }
        let changed = *self
            .settings_files
            .config_changed_at
            .get_or_insert_with(Instant::now);
        if !now && changed.elapsed() < SETTLE {
            return;
        }
        self.settings_files.config_changed_at = None;
        let config = self.quality.config.clone();
        self.settings_files.config_saved = config.clone();
        if let Err(error) = self.save_config(config) {
            self.error = Some(format!("Cannot save key bindings or snippets: {error}"));
        }
    }
    /// Apply settings that other parts of the app copy at startup.
    pub(super) fn apply_settings(&mut self) {
        self.quality.options.case_sensitive = self.settings.find_case_sensitive;
        self.quality.options.regex = self.settings.find_regex;
        self.quality.options.whole_word = self.settings.find_whole_word;
        self.apply_excludes();
        self.sync_folder_view();
    }
    /// Re-read settings.json when it changed on disk, such as after saving it
    /// in hedit or editing it elsewhere.
    pub(super) fn reload_settings(&mut self, ctx: &egui::Context) {
        let Some(path) = self.settings_path() else {
            return;
        };
        if stamp(&path) == self.settings_files.stamp {
            return;
        }
        self.settings_files.stamp = stamp(&path);
        match Settings::load(&path) {
            Ok((settings, warnings)) => {
                self.settings_files.broken = false;
                self.settings_files.saved = settings.clone();
                self.settings = settings;
                self.apply_settings();
                self.apply_format_preferences();
                let mut warnings = warnings;
                warnings.extend(self.apply_theme_settings(ctx));
                self.notice = "Settings reloaded.".into();
                if !warnings.is_empty() {
                    self.error = Some(warnings.join("\n"));
                }
            }
            Err(error) => {
                self.settings_files.broken = true;
                self.error = Some(format!(
                    "{error}\nYour previous settings stay in effect until the file is fixed."
                ));
            }
        }
    }
    /// Open settings.json in a tab, like Sublime's Preferences → Settings.
    pub(super) fn open_settings_file(&mut self, ctx: &egui::Context) {
        let Some(path) = self.settings_path() else {
            self.error = Some("No settings folder is available on this system.".into());
            return;
        };
        if !path.exists() {
            if let Err(error) = self.settings.save(&path) {
                self.error = Some(format!("Cannot create {}: {error}", path.display()));
                return;
            }
            self.settings_files.stamp = stamp(&path);
        }
        self.close_transient(ctx);
        self.open(ctx, Some(path));
    }
    /// Save key bindings and snippets to their files.
    pub(super) fn save_config(&mut self, config: Config) -> Result<(), String> {
        if let Some(dir) = &self.settings_files.dir {
            projects::atomic_json(&dir.join(KEYBINDINGS_FILE), &config.keybindings)?;
            projects::atomic_json(&dir.join(SNIPPETS_FILE), &config.snippets)?;
            if !config.macros.is_empty() || dir.join(MACROS_FILE).exists() {
                projects::atomic_json(&dir.join(MACROS_FILE), &config.macros)?;
            }
        }
        self.quality.config = config;
        Ok(())
    }
}

/// System fonts for scripts egui's own fonts lack, found once per process.
static FALLBACKS: std::sync::OnceLock<Vec<(String, &'static [u8], u32)>> =
    std::sync::OnceLock::new();

/// Find the fallback fonts, once; this reads font files.
pub(super) fn fallback_fonts() -> &'static [(String, &'static [u8], u32)] {
    FALLBACKS.get_or_init(|| {
        hedit::fonts::fallbacks()
            .into_iter()
            // Kept for the whole run, so egui can share them without copies.
            .map(|f| (f.name, &*Vec::leak(f.bytes), f.index))
            .collect()
    })
}

/// Find fallback fonts off the UI thread, then apply them with the editor font.
pub(super) fn load_fallback_fonts(tx: mpsc::Sender<Result<Action, String>>, ctx: &egui::Context) {
    let ctx = ctx.clone();
    thread::spawn(move || {
        fallback_fonts();
        let _ = tx.send(Ok(Action::FallbackFonts));
        ctx.request_repaint();
    });
}

/// Put the editor font first in the monospace family, or restore egui's own,
/// and let both families fall back to system fonts for other scripts.
pub(super) fn set_editor_font(ctx: &egui::Context, font: Option<(Vec<u8>, u32)>) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "terminal-arabic".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansArabic.ttf"
        ))),
    );
    for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("terminal-arabic".into());
    }
    // Characters the chosen font lacks, such as Latin letters in an Arabic-only
    // font, fall through to the built-in font and then the system fallbacks.
    if let Some((bytes, index)) = font {
        fonts.font_data.insert(
            "editor".into(),
            std::sync::Arc::new(egui::FontData {
                index,
                ..egui::FontData::from_owned(bytes)
            }),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "editor".into());
    }
    for (name, bytes, index) in FALLBACKS.get().into_iter().flatten() {
        fonts.font_data.insert(
            name.clone(),
            std::sync::Arc::new(egui::FontData {
                index: *index,
                ..egui::FontData::from_static(bytes)
            }),
        );
        for family in [egui::FontFamily::Monospace, egui::FontFamily::Proportional] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    fonts.font_data.insert(
        "reader-symbols".into(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/NotoSansSymbols-Regular.ttf"
        ))),
    );
    for (family, name, bytes) in [
        (
            "hedit-reader",
            "reader-regular",
            include_bytes!("../../assets/fonts/NotoSans-Regular.ttf").as_slice(),
        ),
        (
            "hedit-reader-strong",
            "reader-semibold",
            include_bytes!("../../assets/fonts/NotoSans-SemiBold.ttf").as_slice(),
        ),
    ] {
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
        let mut fallback = fonts.families[&egui::FontFamily::Proportional].clone();
        fallback.insert(0, "reader-symbols".into());
        fallback.insert(0, name.into());
        fonts
            .families
            .insert(egui::FontFamily::Name(family.into()), fallback);
    }
    ctx.set_fonts(fonts);
}

impl HeditApp {
    /// Apply the cursor and font settings once they settle.
    pub(super) fn apply_editor_style(&mut self, ctx: &egui::Context) {
        let (width, blink) = (self.settings.caret_width, self.settings.caret_blink);
        let cursor = ctx.style().visuals.text_cursor.clone();
        if cursor.stroke.width != width || cursor.blink != blink {
            ctx.all_styles_mut(|style| {
                style.visuals.text_cursor.stroke.width = width;
                style.visuals.text_cursor.blink = blink;
            });
        }
        // Wait for typing in the font field to settle before searching.
        if self.font_applied == self.settings.font_face || self.settings_files.changed_at.is_some()
        {
            return;
        }
        self.font_applied = self.settings.font_face.clone();
        let name = self.font_applied.trim().to_owned();
        if name.is_empty() {
            self.editor_font = None;
            set_editor_font(ctx, None);
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let result = hedit::fonts::load(&name)
                .map(|(bytes, index)| Action::Font(name.clone(), bytes, index))
                .map_err(|e| format!("Font “{name}”: {e} Using the built-in font."));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
