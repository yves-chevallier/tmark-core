//! Tables: GFM pipe tables and `yaml table` / `yaml table-config` fences
//! lowered to the semantic `TableModel`. Spec §Table. The YAML payloads
//! are read by `table_yaml` (the port of the Python schema); this file
//! parses the cell text as inline Markdown and reports the findings.

use tmark_ir::{
    Align, Cell, Column, ColumnConfig, DataRow, Inline, LeafColumn, Row, Separator, Span,
    TableModel, TableSettings,
};
use tmark_markdown::mdast::{AlignKind, Node};

use super::table_yaml::{self, RawRow};
use super::{plain_text, Ctx, Lowerer};

impl Lowerer {
    /// A GFM pipe table: the header row names the leaf columns.
    pub fn lower_pipe_table(
        &mut self,
        table: &tmark_markdown::mdast::Table,
        ctx: &Ctx,
    ) -> TableModel {
        let mut model = TableModel::default();
        for (row_index, row) in table.children.iter().enumerate() {
            let Node::TableRow(row) = row else { continue };
            let mut cells = Vec::new();
            for (col, cell) in row.children.iter().enumerate() {
                let Node::TableCell(cell) = cell else {
                    continue;
                };
                let content = self.lower_inlines(&cell.children, ctx).inlines;
                if row_index == 0 {
                    let align = table.align.get(col).and_then(|a| match a {
                        AlignKind::Left => Some(Align::Left),
                        AlignKind::Center => Some(Align::Center),
                        AlignKind::Right => Some(Align::Right),
                        AlignKind::None => None,
                    });
                    let name = plain_text(&content);
                    model.columns.push(Column::Leaf(LeafColumn {
                        name: (!name.is_empty()).then_some(name),
                        title: rich_title(content),
                        config: ColumnConfig {
                            align,
                            ..Default::default()
                        },
                    }));
                } else {
                    cells.push(Cell::new(content));
                }
            }
            if row_index > 0 {
                model.rows.push(Row::Data(DataRow {
                    cells,
                    named: false,
                }));
            }
        }
        model
    }

    /// A `yaml table` fence. `Err` when the payload is not YAML or not a
    /// mapping (the fence stays a code block); otherwise the model and
    /// whether a `table-*` diagnostic was reported, in which case the
    /// model is a best effort and the caller keeps the source.
    ///
    /// `payload` is the fence body with its offsets, when the body is a
    /// verbatim slice of the source: a cell is then located in it and what
    /// is parsed from the cell carries spans of the file (spec §Round-trip
    /// and source spans).
    pub fn lower_yaml_table(
        &mut self,
        text: &str,
        span: Span,
        ctx: &Ctx,
        payload: Option<usize>,
    ) -> Result<(TableModel, bool), String> {
        let raw = table_yaml::parse_table(text)?;
        let rejected = !raw.findings.is_empty();
        for (code, message) in raw.findings {
            self.diag(code, span, message);
        }
        let cells = Payload {
            text,
            at: payload,
            span,
        };
        let mut columns = raw.columns;
        self.column_titles(&mut columns, ctx, &cells);
        let model = TableModel {
            settings: raw.settings,
            columns,
            rows: self.rows(raw.rows, ctx, &cells),
            footer: self.rows(raw.footer, ctx, &cells),
        };
        Ok((model, rejected))
    }

    /// A `yaml table-config` fence; same contract as [`Self::lower_yaml_table`].
    pub fn lower_yaml_table_config(
        &mut self,
        text: &str,
        span: Span,
    ) -> Result<(Vec<ColumnConfig>, TableSettings, bool), String> {
        let raw = table_yaml::parse_table_config(text)?;
        let rejected = !raw.findings.is_empty();
        for (code, message) in raw.findings {
            self.diag(code, span, message);
        }
        Ok((raw.columns, raw.settings, rejected))
    }

    /// The header of every column of a `yaml table` parsed as inline
    /// Markdown. `name` stays the scalar as written: it is the key of
    /// named-row mode and what the printer writes back.
    fn column_titles(&mut self, columns: &mut [Column], ctx: &Ctx, cells: &Payload) {
        for column in columns {
            match column {
                Column::Leaf(leaf) => {
                    if let Some(name) = leaf.name.clone() {
                        let content = self.cell_content(&name, ctx, cells);
                        leaf.title = rich_title(content);
                    }
                }
                Column::Group(group) => {
                    let content = self.cell_content(&group.name.clone(), ctx, cells);
                    group.title = rich_title(content);
                    self.column_titles(&mut group.columns, ctx, cells);
                }
            }
        }
    }

    fn rows(&mut self, rows: Vec<RawRow>, ctx: &Ctx, cells: &Payload) -> Vec<Row> {
        rows.into_iter()
            .map(|row| match row {
                RawRow::Separator { label, double_rule } => {
                    Row::Separator(Separator { label, double_rule })
                }
                RawRow::Data { cells: data, named } => Row::Data(DataRow {
                    cells: data
                        .into_iter()
                        .map(|cell| {
                            if cell.absorbed {
                                Cell::absorbed()
                            } else {
                                Cell {
                                    content: self.cell_content(&cell.text, ctx, cells),
                                    rows: cell.rows,
                                    cols: cell.cols,
                                    align: cell.align,
                                    absorbed: false,
                                }
                            }
                        })
                        .collect(),
                    named,
                }),
            })
            .collect()
    }

    /// The text of one cell parsed as inline Markdown, anchored where the
    /// payload spells it.
    fn cell_content(&mut self, text: &str, ctx: &Ctx, cells: &Payload) -> Vec<Inline> {
        let anchor = cells
            .locate(text)
            .map_or(cells.span, |(start, end)| self.span_of(ctx, start, end));
        self.lower_fragment(text, anchor)
    }
}

/// The body of a `yaml table` fence and where it sits in the source.
struct Payload<'a> {
    text: &'a str,
    /// Local offset of `text` in the lowering's text; `None` when the body
    /// is not a verbatim slice of it (an indented fence, whose body the
    /// tokenizer dedents).
    at: Option<usize>,
    /// The fence, for a cell the payload does not spell verbatim.
    span: Span,
}

impl Payload<'_> {
    /// The local offsets of `text` in the payload, when it occurs there
    /// exactly once. A cell whose text is not in the payload (a YAML
    /// escape, a folded scalar) or occurs twice (two cells reading the
    /// same) has no source of its own: the fence is its span.
    fn locate(&self, text: &str) -> Option<(usize, usize)> {
        let at = self.at?;
        if text.is_empty() {
            return None;
        }
        let found = self.text.find(text)?;
        if self.text[found + text.len()..].contains(text) {
            return None;
        }
        Some((at + found, at + found + text.len()))
    }
}

/// The header of a column as inline Markdown, kept only when it carries
/// markup a plain `name` cannot hold (spec §Table: "Inline Markdown
/// survives inside cells in all forms"). A plain header is the common case
/// and stays `name` alone, so the IR of an ordinary table is unchanged.
pub(crate) fn rich_title(content: Vec<Inline>) -> Vec<Inline> {
    match content.as_slice() {
        [] | [Inline::Str(_)] => Vec::new(),
        _ => content,
    }
}
