//! Tables as native `#table` calls (`typst/writer.py:_table_call`,
//! `_rich_table_call`): column count or widths, alignment, a
//! `table.header`, `table.cell(colspan:, rowspan:, align:)` for spans,
//! `table.hline()` for separators, wrapped in `#figure` when captioned or
//! labelled.

use tmark_ir::{Align, Caption, Cell, Column, Inline, Row, Table, TableConfig, TableModel};

use super::escape;
use super::Typst;

fn align_name(align: Align) -> &'static str {
    match align {
        Align::Left | Align::Justify => "left",
        Align::Center => "center",
        Align::Right => "right",
    }
}

/// `X` → `1fr`, `NN%` as is, else `auto`; every column stretched when none
/// of them declares a width.
fn columns_spec(model: &TableModel) -> String {
    let leaves: Vec<_> = model.columns.iter().flat_map(Column::leaves).collect();
    let widths: Vec<String> = leaves
        .iter()
        .map(|l| match l.config.width.as_deref() {
            Some("X") => "1fr".to_string(),
            Some(w) if w.ends_with('%') => w.to_string(),
            Some(w) if !w.is_empty() && w != "auto" => w.to_string(),
            _ => "auto".to_string(),
        })
        .collect();
    if widths.iter().all(|w| w == "auto") {
        // No column declares a width. The LaTeX writer hands that table to
        // `tabularx` with every column an `X`, so it fills the line; leaving
        // Typst on `auto` here would set the same table at its content width
        // instead, narrower and ragged against the text around it.
        let stretched = vec!["1fr"; leaves.len()];
        format!("({},)", stretched.join(", "))
    } else {
        format!("({},)", widths.join(", "))
    }
}

impl Typst<'_> {
    pub(crate) fn table(
        &mut self,
        t: &Table,
        config: Option<&TableConfig>,
        caption: Option<&Caption>,
    ) {
        let model = match config {
            Some(config) => apply_config(&t.model, config),
            None => t.model.clone(),
        };
        let caption_text = caption.map(|c| self.render_inlines(&c.content));
        let label = t
            .attrs
            .id()
            .or(caption.and_then(|c| c.attrs.id()))
            .map(escape::label);
        let call = self.table_call(&model);
        if caption_text.is_some() || label.is_some() {
            self.out.push("#figure(\n");
            let lines: Vec<&str> = call.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                self.out.push("  ");
                self.out.push(line);
                self.out
                    .push(if i + 1 == lines.len() { ",\n" } else { "\n" });
            }
            if let Some(text) = caption_text {
                if let Some(c) = caption {
                    self.out.begin(c.meta.id);
                }
                self.out.push(&format!("  caption: [{text}],\n"));
                if let Some(c) = caption {
                    self.out.end(c.meta.id);
                }
            }
            self.out.push(")");
            if let Some(label) = label {
                self.out.push(&format!(" <{label}>"));
            }
            self.out.push("\n");
        } else {
            self.out.push("#");
            self.out.push(&call);
            self.out.push("\n");
        }
    }

    fn table_call(&mut self, model: &TableModel) -> String {
        let leaves: Vec<_> = model.columns.iter().flat_map(Column::leaves).collect();
        let total = leaves.len();
        let aligns: Vec<&str> = leaves
            .iter()
            .map(|l| align_name(l.config.align.unwrap_or(Align::Left)))
            .collect();
        let mut lines = vec![
            "table(".to_string(),
            format!("  columns: {},", columns_spec(model)),
            format!("  align: ({},),", aligns.join(", ")),
        ];
        if leaves.iter().any(|l| l.name.is_some()) {
            let mut cells: Vec<String> = Vec::new();
            let depth = model.header_depth();
            for level in 0..depth {
                self.header_level(&model.columns, level, depth, &mut cells);
            }
            lines.push(format!("  table.header({}),", cells.join(", ")));
        }
        lines.extend(self.section_lines(&model.rows, total));
        if !model.footer.is_empty() {
            lines.push("  table.hline(),".to_string());
            lines.extend(self.section_lines(&model.footer, total));
        }
        lines.push(")".to_string());
        lines.join("\n")
    }

    fn section_lines(&mut self, rows: &[Row], total: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for row in rows {
            match row {
                Row::Separator(s) => {
                    lines.push(if s.double_rule {
                        "  table.hline(stroke: 1pt),".to_string()
                    } else {
                        "  table.hline(),".to_string()
                    });
                    if let Some(label) = &s.label {
                        lines.push(format!(
                            "  table.cell(colspan: {total})[_{}_],",
                            escape::markup(label)
                        ));
                    }
                }
                Row::Data(d) => {
                    let cells: Vec<String> = d
                        .cells
                        .iter()
                        .filter(|c| !c.absorbed)
                        .map(|c| self.cell(c))
                        .collect();
                    lines.push(format!("  {},", cells.join(", ")));
                }
            }
        }
        lines
    }

    /// The header of a column: its inline Markdown when it carries any
    /// (`LeafColumn::title`), the escaped plain name otherwise.
    fn header_text(&mut self, title: &[Inline], name: Option<&str>) -> String {
        if title.is_empty() {
            return escape::markup(name.unwrap_or(""));
        }
        self.contained(|w| w.render_inlines(title))
            .replace('\n', " ")
    }

    fn cell(&mut self, cell: &Cell) -> String {
        let body = self.contained(|w| w.render_inlines(&cell.content));
        let body = body.replace('\n', " ");
        let mut args: Vec<String> = Vec::new();
        if cell.cols > 1 {
            args.push(format!("colspan: {}", cell.cols));
        }
        if cell.rows > 1 {
            args.push(format!("rowspan: {}", cell.rows));
        }
        if let Some(align) = cell.align {
            args.push(format!("align: {}", align_name(align)));
        }
        if args.is_empty() {
            format!("[{body}]")
        } else {
            format!("table.cell({})[{body}]", args.join(", "))
        }
    }
}

impl Typst<'_> {
    fn header_level(
        &mut self,
        columns: &[Column],
        level: usize,
        depth: usize,
        cells: &mut Vec<String>,
    ) {
        for column in columns {
            match column {
                Column::Leaf(leaf) => {
                    if level == 0 {
                        let name = self.header_text(&leaf.title, leaf.name.as_deref());
                        if depth > 1 {
                            cells.push(format!("table.cell(rowspan: {depth})[{name}]"));
                        } else {
                            cells.push(format!("[{name}]"));
                        }
                    }
                }
                Column::Group(g) => {
                    if level == 0 {
                        let width = g.columns.iter().map(|c| c.leaves().len()).sum::<usize>();
                        let name = self.header_text(&g.title, Some(&g.name));
                        cells.push(format!(
                            "table.cell(colspan: {width}, align: center)[{name}]"
                        ));
                    } else {
                        self.header_level(&g.columns, level - 1, depth - 1, cells);
                    }
                }
            }
        }
    }
}

/// Positional column configs and the settings of a `yaml table-config`.
fn apply_config(model: &TableModel, config: &TableConfig) -> TableModel {
    let mut model = model.clone();
    let mut configs = config.columns.iter();
    fn walk(columns: &mut [Column], configs: &mut std::slice::Iter<'_, tmark_ir::ColumnConfig>) {
        for column in columns {
            match column {
                Column::Leaf(leaf) => {
                    if let Some(c) = configs.next() {
                        if c.align.is_some() {
                            leaf.config.align = c.align;
                        }
                        if c.width.is_some() {
                            leaf.config.width = c.width.clone();
                        }
                        if c.width_group.is_some() {
                            leaf.config.width_group = c.width_group.clone();
                        }
                    }
                }
                Column::Group(g) => walk(&mut g.columns, configs),
            }
        }
    }
    walk(&mut model.columns, &mut configs);
    model
}
