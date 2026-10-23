use super::quality::{Config, FileOperation};
use super::*;
use hedit::{projects, search};
use std::path::PathBuf;

const SYMBOLS_FIELD: &str = "symbols-query";
// Option+arrows move by word on macOS, so navigate with Control+- there.
#[cfg(target_os = "macos")]
const NAV_BACK: (Modifiers, Key) = (Modifiers::CTRL, Key::Minus);
#[cfg(target_os = "macos")]
const NAV_FORWARD: (Modifiers, Key) = (
    Modifiers {
        ctrl: true,
        shift: true,
        ..Modifiers::NONE
    },
    Key::Minus,
);
// Cmd+Space is Spotlight on macOS.
#[cfg(target_os = "macos")]
const COMPLETE: (Modifiers, Key) = (Modifiers::CTRL, Key::Space);
#[cfg(not(target_os = "macos"))]
const COMPLETE: (Modifiers, Key) = (Modifiers::COMMAND, Key::Space);
#[cfg(not(target_os = "macos"))]
const NAV_BACK: (Modifiers, Key) = (Modifiers::ALT, Key::ArrowLeft);
#[cfg(not(target_os = "macos"))]
const NAV_FORWARD: (Modifiers, Key) = (Modifiers::ALT, Key::ArrowRight);
const SNIPPETS_FIELD: &str = "snippets-query";
const FOLDER_FIELDS: [&str; 3] = ["folders-find", "folders-replace", "folders-filter"];

pub(super) fn shortcut(value: &str) -> Result<Option<egui::KeyboardShortcut>, String> {
    if value.trim().is_empty() {
        return Ok(None);
    }
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for part in value.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "primary" => modifiers.command = true,
            "ctrl" | "control" => {
                modifiers.ctrl = true;
                if !cfg!(target_os = "macos") {
                    modifiers.command = true;
                }
            }
            "cmd" | "command" => {
                modifiers.mac_cmd = true;
                if cfg!(target_os = "macos") {
                    modifiers.command = true;
                }
            }
            "shift" => modifiers.shift = true,
            "alt" | "option" => modifiers.alt = true,
            _ => {
                if key.is_some() {
                    return Err(format!("Invalid shortcut: {value}"));
                }
                key = Key::from_name(part).or_else(|| Key::from_name(&part.to_uppercase()));
                if key.is_none() {
                    return Err(format!("Unknown key in shortcut: {value}"));
                }
            }
        }
    }
    Ok(Some(egui::KeyboardShortcut::new(
        modifiers,
        key.ok_or("Shortcut needs a key.")?,
    )))
}
impl HeditApp {
    pub(super) fn shortcut_text(&self, command: Command) -> String {
        self.quality
            .config
            .keybindings
            .iter()
            .find(|b| b.command == command)
            .map_or_else(
                || command.shortcut(),
                |b| {
                    b.shortcut.replace(
                        "Primary",
                        if cfg!(target_os = "macos") {
                            "Cmd"
                        } else {
                            "Ctrl"
                        },
                    )
                },
            )
    }
    pub(super) fn overridden(&self, command: Command) -> bool {
        self.quality
            .config
            .keybindings
            .iter()
            .any(|b| b.command == command)
    }
    pub(super) fn filter_overridden_clipboard(&mut self, ctx: &egui::Context) {
        let mut editor_id =
            egui::Id::new(("editor", self.active().view.id, self.active().page.start));
        if self.panes.mode != 0 && self.panes.focused > 0 {
            editor_id = editor_id.with(self.panes.focused);
        }
        if !ctx.memory(|m| m.has_focus(editor_id)) {
            return;
        }
        let copy = self.overridden(Command::Copy);
        let cut = self.overridden(Command::Cut);
        let paste = self.overridden(Command::Paste);
        let has_paste = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))));
        let requested = if has_paste {
            std::mem::take(&mut self.quality.paste_requested)
        } else {
            false
        };
        ctx.input_mut(|i| {
            i.events.retain(|event| match event {
                egui::Event::Copy => !copy,
                egui::Event::Cut => !cut,
                egui::Event::Paste(_) => !paste || requested,
                _ => true,
            })
        });
    }
    pub(super) fn quality_shortcuts(&mut self, ctx: &egui::Context) {
        // Enter belongs to a window only while its own field has focus, so it
        // still types a newline in the editor beside an open window.
        let focused = ctx.memory(|m| m.focused());
        let field = |name: &str| focused == Some(egui::Id::new(name));
        if self.quality.completion_open {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown)) {
                self.quality.completion_index = (self.quality.completion_index + 1)
                    .min(self.quality.candidates.len().saturating_sub(1));
                self.quality.completion_focus = true;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                self.quality.completion_index = self.quality.completion_index.saturating_sub(1);
                self.quality.completion_focus = true;
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                if let Some(word) = self
                    .quality
                    .candidates
                    .get(self.quality.completion_index)
                    .cloned()
                {
                    self.commit_completion(word);
                }
                self.quality.completion_open = false;
                self.focus_editor = true;
            }
        } else if self.quality.snippets_open
            && field(SNIPPETS_FIELD)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            let query = self.quality.snippet_query.to_lowercase();
            let language = self.active().language().id();
            if let Some(snippet) = self
                .quality
                .config
                .snippets
                .iter()
                .find(|s| {
                    s.language.as_deref().is_none_or(|l| l == language)
                        && format!("{} {}", s.trigger, s.description)
                            .to_lowercase()
                            .contains(&query)
                })
                .cloned()
            {
                self.insert_snippet(snippet, false);
                self.quality.snippets_open = false;
            }
        } else if self.quality.symbols_open
            && field(SYMBOLS_FIELD)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            let query = self.quality.symbol_query.to_lowercase();
            if let Some(symbol) = self
                .quality
                .symbols
                .iter()
                .find(|s| s.name.to_lowercase().contains(&query))
            {
                let path = symbol.path.clone();
                let line = symbol.line;
                self.quality.symbols_open = false;
                if path.as_os_str().is_empty() {
                    self.goto = line.to_string();
                    self.goto_line(ctx);
                } else {
                    self.pending_paths
                        .push_back(PathBuf::from(format!("{}:{line}", path.display())));
                }
            }
        } else if self.quality.folders_open
            && FOLDER_FIELDS.into_iter().any(field)
            && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
        {
            self.folder_search(ctx, false);
        }

        let mut id = egui::Id::new(("editor", self.active().view.id, self.active().page.start));
        if self.panes.mode != 0 && self.panes.focused > 0 {
            id = id.with(self.panes.focused);
        }
        let focused = ctx.memory(|m| m.has_focus(id));
        for binding in self.quality.config.keybindings.clone() {
            if binding.editor_only && !focused {
                continue;
            }
            if let Ok(Some(shortcut)) = shortcut(&binding.shortcut) {
                if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                    self.execute(ctx, binding.command);
                }
            }
        }
        for (modifiers, key, command) in [
            (
                Modifiers {
                    shift: true,
                    ..Modifiers::COMMAND
                },
                Key::F,
                Command::FindFiles,
            ),
            (Modifiers::SHIFT, Key::F3, Command::FindPrevious),
            (Modifiers::COMMAND, Key::R, Command::Symbols),
            (NAV_BACK.0, NAV_BACK.1, Command::NavBack),
            (NAV_FORWARD.0, NAV_FORWARD.1, Command::NavForward),
            (COMPLETE.0, COMPLETE.1, Command::Complete),
        ] {
            if !self.overridden(command) && ctx.input_mut(|i| i.consume_key(modifiers, key)) {
                self.execute(ctx, command);
            }
        }
    }
    pub(super) fn close_tab_group(&mut self, right: bool) {
        let current = self.active().view.id;
        let active = self.workspace.active;
        for index in (0..self.workspace.open.len()).rev() {
            if self.workspace.open[index].view.id == current
                || self.workspace.open[index].view.pinned
                || right && index <= active
            {
                continue;
            }
            if let Err(e) = self.workspace.close(index) {
                self.error = Some(e);
                break;
            }
        }
        self.changed();
    }
    pub(super) fn finish_preview(&mut self) {
        if !self.quality.preview_request {
            return;
        }
        self.quality.preview_request = false;
        let current = self.active().view.id;
        if let Some(previous) = self.quality.preview.take() {
            if previous != current {
                if let Some(index) = self
                    .workspace
                    .open
                    .iter()
                    .position(|b| b.view.id == previous && !b.page.info.dirty && !b.view.pinned)
                {
                    let result = self.workspace.close(index);
                    self.report(result);
                }
            }
        }
        if !self.active().view.pinned && !self.active().page.info.dirty {
            self.quality.preview = Some(current);
        }
    }
    pub(super) fn quality_windows(&mut self, ctx: &egui::Context) {
        if self.quality.folders_open {
            self.folder_search_window(ctx);
        }
        if self.quality.symbols_open {
            self.symbols_window(ctx);
        }
        if self.quality.diff_open {
            self.diff_window(ctx);
        }
        if self.quality.file_action.is_some() {
            self.file_action_window(ctx);
        }
        if self.quality.settings_open {
            self.advanced_settings_window(ctx);
        }
        if self.quality.encoding_open {
            self.encoding_window(ctx);
        }
        if self.quality.completion_open {
            self.completion_window(ctx);
        }
        if self.quality.snippets_open {
            self.snippets_window(ctx);
        }
        if self.quality.recent_open {
            let mut open = true;
            let mut selected = None;
            egui::Window::new("Recent projects")
                .open(&mut open)
                .collapsible(false)
                .max_width((ctx.screen_rect().width() - 32.0).max(300.0))
                .max_height((ctx.screen_rect().height() - 32.0).max(220.0))
                .vscroll(true)
                .default_width(480.0)
                .show(ctx, |ui| {
                    for path in &self.quality.recent {
                        if ui.button(path.display().to_string()).clicked() {
                            selected = Some(path.clone());
                        }
                    }
                });
            self.quality.recent_open = open;
            if let Some(path) = selected {
                self.quality.recent_open = false;
                self.load_project(ctx, Some(path));
            }
        }
    }
    fn folder_search_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut find = false;
        let mut preview = false;
        let mut apply = false;
        let mut selected = None;
        egui::Window::new("Find in folders").open(&mut open).max_width((ctx.screen_rect().width()-32.0).max(300.0)).max_height((ctx.screen_rect().height()-32.0).max(220.0)).vscroll(true).collapsible(false).default_width(760.0).default_height((ctx.screen_rect().height()-90.0).min(520.0)).show(ctx,|ui|{
            ui.horizontal(|ui|{ui.label("Find");let response=ui.add(egui::TextEdit::singleline(&mut self.query).id(egui::Id::new(FOLDER_FIELDS[0])));if self.quality.folder_focus{response.request_focus();self.quality.folder_focus=false;}});
            ui.horizontal(|ui|{ui.label("Replace");ui.add(egui::TextEdit::singleline(&mut self.replacement).id(egui::Id::new(FOLDER_FIELDS[1])));});
            ui.horizontal(|ui|{ui.label("Files");ui.add(egui::TextEdit::singleline(&mut self.quality.filter).id(egui::Id::new(FOLDER_FIELDS[2])).hint_text("*.rs, *.cu — empty means all files"));});
            ui.horizontal_wrapped(|ui|{ui.checkbox(&mut self.quality.options.regex,"Regex");ui.checkbox(&mut self.quality.options.case_sensitive,"Case sensitive");ui.checkbox(&mut self.quality.options.whole_word,"Whole words");});
            ui.horizontal(|ui|{find=ui.add_enabled(!self.busy,egui::Button::new("Find all")).clicked();preview=ui.add_enabled(!self.busy,egui::Button::new("Preview replacements")).clicked();if self.busy&&ui.button("Cancel search").clicked(){self.cancel.store(true,Ordering::Relaxed);}});
            ui.weak("Search respects .gitignore. Files over 16 MiB and unsupported/binary files are skipped. Open unsaved text is not searched here.");
            let results=&mut self.quality.results;
            ui.label(format!("{} matches · {} files skipped{}",results.hits.len(),results.skipped,if results.truncated{" · result limit reached"}else{""}));
            let valid=self.quality.preview_query.as_ref().is_some_and(|(q,r,f,o)|*q==self.query&&*r==self.replacement&&*f==self.quality.filter&&o.regex==self.quality.options.regex&&o.case_sensitive==self.quality.options.case_sensitive&&o.whole_word==self.quality.options.whole_word);
            if !results.plans.is_empty(){
                ui.label("Review changes below. Open dirty/followed files are skipped when applying.");
                egui::ScrollArea::vertical().id_salt("replacement-files").max_height((ctx.screen_rect().height()-600.0).clamp(60.0,130.0)).show(ui,|ui|{
                    for (index,plan) in results.plans.iter_mut().enumerate(){ui.horizontal(|ui|{ui.checkbox(&mut plan.selected,"");if ui.selectable_label(self.quality.preview_diff==Some(index),plan.path.display().to_string()).clicked(){self.quality.preview_diff=Some(index);}});}
                });
                if let Some(plan)=self.quality.preview_diff.and_then(|i|results.plans.get(i)){
                    let index=self.quality.preview_diff.unwrap();if self.quality.preview_rendered.as_ref().is_none_or(|(i,_)|*i!=index){self.quality.preview_rendered=Some((index,search::diff(&plan.before,&plan.after,&plan.path)));}let diff=&self.quality.preview_rendered.as_ref().unwrap().1;egui::ScrollArea::both().id_salt("replacement-diff").max_height((ctx.screen_rect().height()-550.0).clamp(80.0,190.0)).show(ui,|ui|{ui.label(RichText::new(diff).monospace());});
                }
                apply=ui.add_enabled(!self.busy&&valid&&!results.truncated&&results.plans.iter().any(|p|p.selected),egui::Button::new("Apply reviewed replacements")).clicked();
                if !valid{ui.colored_label(self.theme.warning.color(),"Search settings changed. Generate a fresh preview before applying.");}
            }else{
                egui::ScrollArea::vertical().id_salt("folder-hits").max_height((ctx.screen_rect().height()-280.0).clamp(120.0,320.0)).show_rows(ui,22.0,results.hits.len(),|ui,range|{for hit in &results.hits[range] {if ui.selectable_label(false,format!("{}:{}:{}  {}",hit.path.display(),hit.line,hit.column,hit.text)).clicked(){selected=Some(PathBuf::from(format!("{}:{}:{}",hit.path.display(),hit.line,hit.column)));}}});
            }
        });
        self.quality.folders_open = open;
        if find {
            self.folder_search(ctx, false);
        } else if preview {
            self.folder_search(ctx, true);
        } else if apply {
            self.apply_folder_replacements(ctx);
        }
        if let Some(path) = selected {
            self.pending_paths.push_back(path);
        }
    }
    fn symbols_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut reload = false;
        let mut selected = None;
        egui::Window::new("Go to symbol").open(&mut open).max_width((ctx.screen_rect().width()-32.0).max(300.0)).max_height((ctx.screen_rect().height()-32.0).max(220.0)).vscroll(true).collapsible(false).default_width(620.0).show(ctx,|ui|{
            let response=ui.add(egui::TextEdit::singleline(&mut self.quality.symbol_query).id(egui::Id::new(SYMBOLS_FIELD)));if self.quality.symbol_focus{response.request_focus();self.quality.symbol_focus=false;}
            ui.horizontal(|ui|{reload=ui.checkbox(&mut self.quality.symbols_project,"All project folders").changed();reload|=ui.add_enabled(!self.busy,egui::Button::new("Refresh symbols")).clicked();});
            ui.weak("Functions, classes and types detected from source lines; generated and ignored folders are skipped.");
            let query=self.quality.symbol_query.to_lowercase();
            egui::ScrollArea::vertical().max_height(360.0).show(ui,|ui|{for symbol in self.quality.symbols.iter().filter(|s|s.name.to_lowercase().contains(&query)){
                if ui.selectable_label(false,format!("{} — {}:{}",symbol.name,symbol.path.display(),symbol.line)).clicked(){selected=Some((symbol.path.clone(),symbol.line));}
            }});
        });
        self.quality.symbols_open = open;
        if reload && !self.busy {
            self.list_symbols(ctx);
        }
        if let Some((path, line)) = selected {
            self.quality.symbols_open = false;
            if path.as_os_str().is_empty() {
                self.goto = line.to_string();
                self.goto_line(ctx);
            } else {
                self.pending_paths
                    .push_back(PathBuf::from(format!("{}:{line}", path.display())));
            }
        }
    }
    fn diff_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut disk = false;
        let mut mine = false;
        egui::Window::new(if self.quality.git_diff{"Git changes compared with HEAD"}else{"Disk and editor comparison"}).open(&mut open).max_width((ctx.screen_rect().width()-32.0).max(300.0)).max_height((ctx.screen_rect().height()-32.0).max(220.0)).vscroll(true).collapsible(false).default_width(760.0).show(ctx,|ui|{
            if self.quality.diff.is_empty(){ui.label("No changes.");}
            egui::ScrollArea::both().max_height(460.0).show(ui,|ui|{let mut text=self.quality.diff.as_str();ui.add(egui::TextEdit::multiline(&mut text).code_editor().desired_width(f32::INFINITY));});
            if !self.quality.git_diff {
                let valid=!self.busy&&self.quality.diff_target.as_ref().is_some_and(|(id,revision,_)|*id==self.active().view.id&&*revision==self.active().page.info.revision)&&!self.active().page.info.paged;
                ui.label("Taking the disk version keeps your previous text in Undo. Keeping the editor version overwrites the compared disk version.");
                ui.horizontal(|ui|{disk=ui.add_enabled(valid,egui::Button::new("Take disk version")).clicked();mine=ui.add_enabled(valid,egui::Button::new("Keep editor version and save")).clicked();});
            }
        });
        self.quality.diff_open = open;
        if disk || mine {
            if let Some((id, revision, expected)) = self.quality.diff_target.clone() {
                let editor = Arc::clone(&self.workspace.editor);
                self.quality.diff_open = false;
                self.job(ctx, move || {
                    let shared = editor.get(id)?;
                    let mut doc = shared.lock().map_err(|e| e.to_string())?;
                    if doc.info().revision != revision {
                        return Err("The buffer changed; compare it again.".into());
                    }
                    if disk {
                        let page = doc.take_disk_version(&expected, revision)?;
                        Ok(Action::Page(id, page, None))
                    } else {
                        doc.accept_disk_version(&expected)?;
                        Ok(Action::Saved(id, doc.save(None)?))
                    }
                });
            }
        }
    }
    fn file_action_window(&mut self, ctx: &egui::Context) {
        let (path, operation) = self.quality.file_action.clone().unwrap();
        let mut open = true;
        let mut submit = false;
        let title = match operation {
            FileOperation::Rename => "Rename",
            FileOperation::Move => "Move",
            FileOperation::Duplicate => "Duplicate",
            FileOperation::Trash => "Move to system trash",
        };
        egui::Window::new(title).open(&mut open).max_width((ctx.screen_rect().width()-32.0).max(300.0)).max_height((ctx.screen_rect().height()-32.0).max(220.0)).vscroll(true).collapsible(false).show(ctx,|ui|{
            ui.label(path.display().to_string());
            if matches!(operation,FileOperation::Rename|FileOperation::Duplicate){ui.text_edit_singleline(&mut self.quality.file_name);}
            if operation==FileOperation::Trash{ui.label("Open buffers stay recoverable. Restore the disk file using your system trash.");}
            submit=ui.add_enabled(!self.busy,egui::Button::new(title)).clicked();
        });
        if !open {
            self.quality.file_action = None;
        } else if submit {
            self.perform_file_action(ctx);
        }
    }
    fn advanced_settings_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut save = false;
        let project = self.quality.settings_project;
        egui::Window::new(if project{"Project formatting settings"}else{"Key bindings and snippets"}).open(&mut open).max_width((ctx.screen_rect().width()-32.0).max(300.0)).max_height((ctx.screen_rect().height()-32.0).max(220.0)).vscroll(true).collapsible(false).default_width(740.0).show(ctx,|ui|{
            if !project{
                ui.label("Keybindings override defaults. Use Primary for Ctrl/Cmd, e.g. Primary+Shift+F. An empty shortcut unbinds a command.");
                ui.label("Snippets support ${SELECTION} and $0 for the final cursor. Type a trigger then Tab, or use Tools → Insert snippet.");
                ui.collapsing("Command identifiers",|ui|{egui::ScrollArea::vertical().max_height(120.0).show(ui,|ui|{for command in Command::ALL{ui.label(format!("{} — {}",serde_json::to_value(command).unwrap().as_str().unwrap(),command.label()));}});});
            }
            if project{ui.label("Formatting: indent_style, indent_size, end_of_line, insert_final_newline, trim_trailing_whitespace, charset. Per-language entries use syntax identifiers such as rust, python and cpp.");}else{ui.label("Formatting defaults are under Settings → Formatting.");}
            egui::ScrollArea::vertical().max_height((ctx.screen_rect().height()-260.0).clamp(100.0,380.0)).show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.quality.settings_json).code_editor().desired_rows(18).desired_width(f32::INFINITY));});
            save=ui.add_enabled(!self.busy,egui::Button::new("Save settings")).clicked();
        });
        self.quality.settings_open = open;
        if save {
            let result = (|| {
                if project {
                    let settings: projects::Defaults =
                        serde_json::from_str(&self.quality.settings_json)
                            .map_err(|e| e.to_string())?;
                    settings.formatting.validate()?;
                    for f in settings.languages.values() {
                        f.validate()?;
                    }
                    if let Some(path) = &self.quality.project_path {
                        let (mut definition, _) = projects::load(path)?;
                        definition.settings = settings.clone();
                        projects::atomic_json(path, &definition)?;
                    }
                    self.quality.project_settings = settings;
                } else {
                    let config: Config = serde_json::from_str(&self.quality.settings_json)
                        .map_err(|e| e.to_string())?;
                    let mut commands = std::collections::HashSet::new();
                    let mut shortcuts = Vec::new();
                    for binding in &config.keybindings {
                        if !commands.insert(serde_json::to_string(&binding.command).unwrap()) {
                            return Err("A command has more than one keyboard override.".into());
                        }
                        if let Some(key) = shortcut(&binding.shortcut)? {
                            if shortcuts.contains(&key) {
                                return Err("Two commands use the same shortcut.".into());
                            }
                            shortcuts.push(key);
                        }
                    }
                    if config.snippets.len() > 1000
                        || config
                            .snippets
                            .iter()
                            .any(|s| s.trigger.len() > 256 || s.body.len() > 64 * 1024)
                    {
                        return Err("Snippet collection exceeds supported limits.".into());
                    }
                    self.save_config(config)?;
                }
                Ok(())
            })();
            self.report(result);
            self.apply_format_preferences();
        }
    }
    fn encoding_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut convert = false;
        let mut reopen = false;
        let mut choose = false;
        egui::Window::new("File encoding").open(&mut open).max_width((ctx.screen_rect().width()-32.0).max(300.0)).max_height((ctx.screen_rect().height()-32.0).max(220.0)).vscroll(true).collapsible(false).show(ctx,|ui|{
            ui.label(format!("Current: {}",self.active().page.info.encoding));
            egui::ComboBox::from_label("Encoding").selected_text(&self.quality.encoding).show_ui(ui,|ui|{for &label in hedit::text_encoding::LABELS{ui.selectable_value(&mut self.quality.encoding,label.to_owned(),label);}});
            ui.label("Conversion rejects characters the target encoding cannot represent. UTF-16 BOMs are detected automatically; legacy encodings must be selected explicitly.");
            ui.add_enabled_ui(!self.busy&&!self.active().follow,|ui|{
                convert=ui.button("Use encoding for next save").clicked();
                reopen=ui.add_enabled(!self.active().page.info.dirty&&self.active().page.info.path.is_some(),egui::Button::new("Reopen current file with encoding")).clicked();
                choose=ui.button("Open file with encoding…").clicked();
            });
        });
        self.quality.encoding_open = open;
        let label = self.quality.encoding.clone();
        if convert {
            let id = self.active().view.id;
            let editor = Arc::clone(&self.workspace.editor);
            self.job(ctx, move || {
                let shared = editor.get(id)?;
                let mut doc = shared.lock().map_err(|e| e.to_string())?;
                doc.set_encoding(&label)?;
                Ok(Action::Page(id, doc.page(0)?, None))
            });
        } else if reopen || choose {
            let old = (!choose)
                .then(|| self.active().page.info.path.clone().map(PathBuf::from))
                .flatten();
            let id = self.active().view.id;
            let editor = Arc::clone(&self.workspace.editor);
            self.job(ctx, move || {
                let path = match old.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("Open with encoding")
                        .pick_file()
                }) {
                    Some(p) => p,
                    None => return Ok(Action::Nothing),
                };
                let mut doc = Document::open_with_encoding(
                    if choose { editor.id() } else { id },
                    &path,
                    &label,
                )?;
                let page = doc.page(0)?;
                if choose {
                    Ok(Action::Add(doc, page, 0))
                } else {
                    *editor.get(id)?.lock().map_err(|e| e.to_string())? = doc;
                    Ok(Action::Page(id, page, None))
                }
            });
        }
    }
    fn completion_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut selected = None;
        egui::Window::new("Complete word")
            .open(&mut open)
            .collapsible(false)
            .max_width((ctx.screen_rect().width() - 32.0).max(300.0))
            .max_height((ctx.screen_rect().height() - 32.0).max(220.0))
            .vscroll(true)
            .default_width(360.0)
            .show(ctx, |ui| {
                if self.quality.candidates.is_empty() {
                    ui.label("No matching words in open files.");
                }
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for (index, word) in self.quality.candidates.iter().enumerate() {
                            let response =
                                ui.selectable_label(index == self.quality.completion_index, word);
                            if self.quality.completion_focus
                                && index == self.quality.completion_index
                            {
                                response.request_focus();
                                response.scroll_to_me(None);
                            }
                            if response.clicked() {
                                selected = Some(word.clone());
                            }
                        }
                    });
            });
        self.quality.completion_open = open;
        self.quality.completion_focus = false;
        if let Some(word) = selected {
            self.commit_completion(word);
            self.quality.completion_open = false;
        }
    }
    pub(super) fn commit_completion(&mut self, word: String) {
        if !self.busy
            && self.active().page.info.revision == self.quality.completion_revision
            && self.active().view.id == self.quality.completion_id
        {
            let range = self.quality.completion_range.clone();
            let mut text = self.active().page.text.clone();
            if text.is_char_boundary(range.start) && text.is_char_boundary(range.end) {
                let cursor = text[..range.start].chars().count() + word.chars().count();
                text.replace_range(range, &word);
                self.apply_edit(
                    hedit::editing::Edit {
                        text,
                        anchor: cursor,
                        head: cursor,
                    },
                    None,
                );
            }
        }
    }
    fn snippets_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let mut selected = None;
        egui::Window::new("Insert snippet")
            .open(&mut open)
            .collapsible(false)
            .max_width((ctx.screen_rect().width() - 32.0).max(300.0))
            .max_height((ctx.screen_rect().height() - 32.0).max(220.0))
            .vscroll(true)
            .default_width(480.0)
            .show(ctx, |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.quality.snippet_query)
                        .id(egui::Id::new(SNIPPETS_FIELD)),
                );
                if self.quality.snippet_focus {
                    response.request_focus();
                    self.quality.snippet_focus = false;
                }
                let query = self.quality.snippet_query.to_lowercase();
                for snippet in &self.quality.config.snippets {
                    if snippet
                        .language
                        .as_deref()
                        .is_some_and(|l| l != self.active().language().id())
                    {
                        continue;
                    }
                    if format!("{} {}", snippet.trigger, snippet.description)
                        .to_lowercase()
                        .contains(&query)
                        && ui
                            .button(format!("{} — {}", snippet.trigger, snippet.description))
                            .clicked()
                    {
                        selected = Some(snippet.clone());
                    }
                }
            });
        self.quality.snippets_open = open;
        if let Some(snippet) = selected {
            if !self.busy {
                self.insert_snippet(snippet, false);
            }
            self.quality.snippets_open = false;
        }
    }
}
