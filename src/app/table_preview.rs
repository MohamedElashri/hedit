//! Read-only delimited text tables. Rows retain their source line after sorting.
use super::*;
use egui_extras::{Column, TableBuilder};
use std::collections::HashMap;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_ROWS: usize = 20_000;
const MAX_COLUMNS: usize = 128;
const MAX_CELLS: usize = 200_000;

struct Row {
    line: usize,
    cells: Vec<String>,
}

struct Table {
    rows: Vec<Row>,
    columns: usize,
    truncated: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    Start,
    Bare,
    Quoted,
    Closed,
}

fn parse(text: &str, delimiter: char) -> Result<Table, String> {
    let mut table = Table {
        rows: vec![],
        columns: 0,
        truncated: false,
    };
    let mut field = Field::Start;
    let mut value = String::new();
    let mut row = Row {
        line: 1,
        cells: vec![],
    };
    let mut line = 1;
    let mut cells = 0;
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if row.cells.len() >= MAX_COLUMNS {
            table.truncated = true;
            return Ok(table);
        }
        if ch == '"' {
            match field {
                Field::Start => {
                    field = Field::Quoted;
                    continue;
                }
                Field::Quoted => {
                    if chars.next_if_eq(&'"').is_some() {
                        value.push('"');
                    } else {
                        field = Field::Closed;
                    }
                    continue;
                }
                _ => return Err(format!("Unexpected quote on line {line}.")),
            }
        }
        let crlf = ch == '\r' && chars.next_if_eq(&'\n').is_some();
        let newline = ch == '\n' || crlf;
        if newline {
            line += 1;
        }
        if field == Field::Quoted {
            value.push(ch);
            if crlf {
                value.push('\n');
            }
        } else if ch == delimiter || newline {
            row.cells.push(std::mem::take(&mut value));
            field = Field::Start;
            if newline {
                cells += row.cells.len();
                if table.rows.len() >= MAX_ROWS || cells > MAX_CELLS {
                    table.truncated = true;
                    return Ok(table);
                }
                table.columns = table.columns.max(row.cells.len());
                table.rows.push(row);
                row = Row {
                    line,
                    cells: vec![],
                };
            }
        } else if field == Field::Closed {
            return Err(format!(
                "Expected a delimiter after the quoted field on line {line}."
            ));
        } else {
            field = Field::Bare;
            value.push(ch);
        }
    }
    if field == Field::Quoted {
        return Err(format!(
            "Unclosed quoted field in the record starting on line {}.",
            row.line
        ));
    }
    if field != Field::Start || !row.cells.is_empty() {
        row.cells.push(value);
        if table.rows.len() >= MAX_ROWS
            || row.cells.len() > MAX_COLUMNS
            || cells + row.cells.len() > MAX_CELLS
        {
            table.truncated = true;
            return Ok(table);
        }
        table.columns = table.columns.max(row.cells.len());
        table.rows.push(row);
    }
    Ok(table)
}

#[derive(Default)]
pub(super) struct Previews {
    views: HashMap<u64, View>,
}

#[derive(Default)]
struct View {
    shown: bool,
    revision: Option<(u64, char)>,
    table: Option<Result<Table, String>>,
    order: Vec<usize>,
    sort: Option<(usize, bool)>,
    no_headers: bool,
}

impl View {
    fn update(&mut self, revision: u64, text: &str, delimiter: char) {
        if self.revision == Some((revision, delimiter)) {
            return;
        }
        self.table = Some(parse(text, delimiter));
        self.revision = Some((revision, delimiter));
        self.reorder();
    }

    fn reorder(&mut self) {
        let Some(Ok(table)) = &self.table else {
            self.order.clear();
            return;
        };
        self.order = (usize::from(!self.no_headers)..table.rows.len()).collect();
        if let Some((column, descending)) = self.sort {
            let cell = |row: usize| table.rows[row].cells.get(column).map_or("", String::as_str);
            self.order.sort_by(|&a, &b| {
                let order = cell(a).cmp(cell(b));
                if descending {
                    order.reverse()
                } else {
                    order
                }
            });
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, pane: usize) -> Option<usize> {
        let mut toggled = false;
        ui.horizontal_wrapped(|ui| {
            let mut headers = !self.no_headers;
            if ui.checkbox(&mut headers, "First row is header").changed() {
                self.no_headers = !headers;
                toggled = true;
            }
        });
        if toggled {
            self.reorder();
        }
        let Some(result) = &self.table else {
            return None;
        };
        let table = match result {
            Ok(table) => table,
            Err(error) => {
                ui.label(error);
                return None;
            }
        };
        if table.truncated {
            ui.weak("Table preview limit reached.");
        }
        if table.rows.is_empty() {
            if !table.truncated {
                ui.weak("No rows.");
            }
            return None;
        }
        let mut clicked_column = None;
        let mut jump = None;
        egui::ScrollArea::horizontal()
            .id_salt(("table-width", pane))
            .show(ui, |ui| {
                ui.set_min_width(66.0 + 148.0 * table.columns as f32);
                let height = ui.text_style_height(&egui::TextStyle::Body).max(18.0) + 4.0;
                TableBuilder::new(ui)
                    .id_salt(("table-rows", pane))
                    .striped(true)
                    .column(Column::exact(60.0))
                    .columns(Column::exact(140.0).clip(true), table.columns)
                    .header(height, |mut header| {
                        header.col(|ui| {
                            ui.strong("Line");
                        });
                        for column in 0..table.columns {
                            header.col(|ui| {
                                let label = if !self.no_headers {
                                    table.rows[0]
                                        .cells
                                        .get(column)
                                        .filter(|value| !value.is_empty())
                                        .map(|value| value.chars().take(120).collect::<String>())
                                } else {
                                    None
                                };
                                let mut label =
                                    label.unwrap_or_else(|| format!("Column {}", column + 1));
                                if let Some((_, descending)) =
                                    self.sort.filter(|(index, _)| *index == column)
                                {
                                    label.push_str(if descending { " ↓" } else { " ↑" });
                                }
                                if ui
                                    .add(egui::Button::new(&label).truncate())
                                    .on_hover_text(label)
                                    .clicked()
                                {
                                    clicked_column = Some(column);
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(height, self.order.len(), |mut row| {
                            let source = &table.rows[self.order[row.index()]];
                            row.col(|ui| {
                                if ui
                                    .small_button(source.line.to_string())
                                    .on_hover_text("Go to source line")
                                    .clicked()
                                {
                                    jump = Some(source.line);
                                }
                            });
                            for column in 0..table.columns {
                                row.col(|ui| {
                                    if let Some(cell) = source.cells.get(column) {
                                        let preview: String = cell.chars().take(160).collect();
                                        ui.add(egui::Label::new(preview).truncate()).on_hover_ui(
                                            |ui| {
                                                ui.label(
                                                    cell.chars().take(600).collect::<String>(),
                                                );
                                            },
                                        );
                                    }
                                });
                            }
                        });
                    });
            });
        if let Some(column) = clicked_column {
            let descending = self
                .sort
                .is_some_and(|(index, descending)| index == column && !descending);
            self.sort = Some((column, descending));
            self.reorder();
        }
        jump
    }
}

impl HeditApp {
    pub(super) fn table_delimiter(&self) -> Option<char> {
        let buffer = self.active();
        if buffer.page.info.paged || buffer.page.text.len() > MAX_BYTES {
            return None;
        }
        let extension = std::path::Path::new(buffer.page.info.path.as_ref()?)
            .extension()?
            .to_str()?;
        if extension.eq_ignore_ascii_case("csv") {
            Some(',')
        } else if extension.eq_ignore_ascii_case("tsv") {
            Some('\t')
        } else {
            None
        }
    }

    pub(super) fn toggle_table_preview(&mut self) {
        let id = self.active().view.id;
        let view = self.tables.views.entry(id).or_default();
        view.shown = !view.shown;
    }

    pub(super) fn retain_table_previews(&mut self) {
        self.tables.views.retain(|id, _| {
            self.workspace
                .open
                .iter()
                .any(|buffer| buffer.view.id == *id)
        });
    }

    pub(super) fn table_pane(&mut self, ui: &mut egui::Ui, pane: usize) -> bool {
        let Some(delimiter) = self.table_delimiter() else {
            return false;
        };
        let index = self.workspace.active;
        let id = self.workspace.open[index].view.id;
        let view = self.tables.views.entry(id).or_default();
        if ui.selectable_label(view.shown, "Table Preview").clicked() {
            view.shown = !view.shown;
        }
        if !view.shown {
            return false;
        }
        let area = ui.available_rect_before_wrap();
        let mut source = area;
        let mut table = area;
        if area.width() < 640.0 {
            source.max.y = area.center().y - 4.0;
            table.min.y = area.center().y + 4.0;
        } else {
            source.max.x = area.center().x - 4.0;
            table.min.x = area.center().x + 4.0;
        }
        ui.scope_builder(egui::UiBuilder::new().max_rect(source), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(source));
            self.editor_source_pane(ui, pane);
        });
        let buffer = &self.workspace.open[index];
        let view = self.tables.views.get_mut(&id).unwrap();
        view.update(buffer.page.info.revision, &buffer.page.text, delimiter);
        let jump = ui
            .scope_builder(egui::UiBuilder::new().max_rect(table), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(table));
                view.show(ui, pane)
            })
            .inner;
        if let Some(line) = jump {
            let ctx = ui.ctx().clone();
            self.jump_to_line(&ctx, line, None);
        }
        true
    }
}

#[cfg(test)]
#[test]
fn delimited_records_keep_source_lines_and_escaped_fields() {
    let table = parse(
        "Name,Note\r\nAda,\"small,\"\"quoted\"\"\r\nsecond line\"\r\nLinus,\r\n",
        ',',
    )
    .unwrap();
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.rows[1].line, 2);
    assert_eq!(table.rows[1].cells[1], "small,\"quoted\"\r\nsecond line");
    assert_eq!(table.rows[2].line, 4);
    assert_eq!(table.rows[2].cells[1], "");
    assert!(parse("invalid\t\"unclosed", '\t').is_err());
}
