//! Tables (writers-and-passes.md §2 "Tables"): a plain pipe table renders
//! as `table.tex` did (`tabularx` with `X` columns, bold header,
//! `booktabs` rules); a model table follows `extensions/tables/layout.py`
//! (environment choice, column spec with the `\tabcolsep` discount) and
//! `renderer.py` (`\cmidrule`, `\multirow`/`\multicolumn`, labelled
//! separators, footer, `longtable` heads).

use tmark_ir::{
    Align, Caption, Cell, Column, ColumnConfig, Inline, Row, Table, TableConfig, TableModel,
};

use super::escape;
use super::Latex;

/// A leaf column after inheritance and width-group resolution
/// (`layout.py:_ResolvedLeaf`).
#[derive(Clone, Debug)]
struct Leaf {
    align: Align,
    width: Option<String>,
    width_group: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Env {
    Tabular,
    Tabularx,
    Longtable,
}

struct Layout {
    env: Env,
    total_width: Option<String>,
    colspec: String,
    leaves: Vec<Leaf>,
}

/// `ALIGN_WRAPPERS` (`constants.py:109`).
fn wrapper(align: Align) -> &'static str {
    match align {
        Align::Left => ">{\\raggedright\\arraybackslash}",
        Align::Right => ">{\\raggedleft\\arraybackslash}",
        Align::Center => ">{\\centering\\arraybackslash}",
        Align::Justify => "",
    }
}

fn align_char(align: Option<Align>) -> char {
    match align {
        Some(Align::Left) => 'l',
        Some(Align::Right) => 'r',
        Some(Align::Center) | None => 'c',
        Some(Align::Justify) => 'l',
    }
}

fn percent(value: &str) -> Option<f64> {
    value.strip_suffix('%').and_then(|n| n.parse::<f64>().ok())
}

/// `_normalise_width`: `auto`/absent → `None`, `NN%` → a `\linewidth`
/// fraction, else verbatim.
fn normalise_width(raw: Option<&str>) -> Option<String> {
    match raw {
        None | Some("auto") => None,
        Some(w) => Some(match percent(w) {
            Some(p) => super::figure::percent_linewidth(p / 100.0),
            None => w.to_string(),
        }),
    }
}

/// `_scale_percent`: a group's width shared equally among its leaves.
fn scale_percent(raw: &str, factor: f64) -> Option<String> {
    let p = percent(raw)?;
    let scaled = p * factor;
    if scaled <= 0.0 {
        return None;
    }
    if (scaled - scaled.round()).abs() < 1e-9 {
        return Some(format!("{}%", scaled.round() as i64));
    }
    let mut s = format!("{scaled:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    Some(format!("{s}%"))
}

fn flatten(
    column: &Column,
    align: Option<Align>,
    width: Option<&str>,
    group: Option<&str>,
    out: &mut Vec<Leaf>,
) {
    match column {
        Column::Leaf(leaf) => out.push(Leaf {
            align: leaf.config.align.or(align).unwrap_or(Align::Left),
            width: leaf
                .config
                .width
                .clone()
                .or_else(|| width.map(str::to_string)),
            width_group: leaf
                .config
                .width_group
                .clone()
                .or_else(|| group.map(str::to_string)),
        }),
        Column::Group(g) => {
            let leaves = g.columns.iter().map(|c| c.leaves().len()).sum::<usize>();
            let share = match &g.config.width {
                Some(w) if leaves > 0 => {
                    Some(scale_percent(w, 1.0 / leaves as f64).unwrap_or_else(|| w.clone()))
                }
                _ => None,
            };
            let child_align = g.config.align.or(align);
            let child_width = share.as_deref().or(width);
            let child_group = g.config.width_group.as_deref().or(group);
            for child in &g.columns {
                flatten(child, child_align, child_width, child_group, out);
            }
        }
    }
}

fn resolve_width_groups(leaves: &mut [Leaf]) {
    let mut widths: Vec<(String, String)> = Vec::new();
    for leaf in leaves.iter() {
        if let (Some(g), Some(w)) = (&leaf.width_group, &leaf.width) {
            if !widths.iter().any(|(k, _)| k == g) {
                widths.push((g.clone(), w.clone()));
            }
        }
    }
    for leaf in leaves.iter_mut() {
        if let Some(g) = &leaf.width_group {
            if let Some((_, w)) = widths.iter().find(|(k, _)| k == g) {
                leaf.width = Some(w.clone());
            }
        }
    }
}

fn is_flex(width: Option<&str>) -> bool {
    matches!(width, Some("X") | Some("auto"))
}

fn column_spec(leaf: &Leaf, env: Env, use_x_for_auto: bool) -> String {
    let wrapper = wrapper(leaf.align);
    if is_flex(leaf.width.as_deref()) {
        return format!("{wrapper}X");
    }
    match normalise_width(leaf.width.as_deref()) {
        None => {
            if env == Env::Tabularx && use_x_for_auto {
                format!("{wrapper}X")
            } else {
                align_char(Some(leaf.align)).to_string()
            }
        }
        // `p{}` sizes the content; the `\tabcolsep` padding is discounted
        // so declared widths are the column footprint (0.5.4).
        Some(width) => format!("{wrapper}p{{\\dimexpr {width}-2\\tabcolsep\\relax}}"),
    }
}

fn build_colspec(leaves: &[Leaf], env: Env) -> String {
    let has_marker = leaves
        .iter()
        .any(|l| l.width_group.is_some() || is_flex(l.width.as_deref()));
    leaves
        .iter()
        .map(|l| column_spec(l, env, !has_marker || l.width_group.is_some()))
        .collect()
}

/// `compute_layout`.
fn layout(model: &TableModel) -> Layout {
    let mut leaves = Vec::new();
    for column in &model.columns {
        flatten(column, None, None, None, &mut leaves);
    }
    resolve_width_groups(&mut leaves);
    let mut total_width = normalise_width(Some(model.settings.width.as_str()));
    let flexible = leaves.iter().any(|l| is_flex(l.width.as_deref()))
        || (total_width.is_some() && leaves.iter().any(|l| l.width.is_none()));
    let mut env = if model.settings.long == Some(true) {
        Env::Longtable
    } else if flexible {
        Env::Tabularx
    } else {
        Env::Tabular
    };
    if env == Env::Tabularx && total_width.is_none() {
        total_width = Some("\\linewidth".to_string());
    }
    let mut colspec = build_colspec(&leaves, env);
    if env == Env::Tabularx && !colspec.contains('X') {
        env = Env::Tabular;
        colspec = build_colspec(&leaves, env);
    }
    if env == Env::Tabular {
        total_width = None;
    }
    Layout {
        env,
        total_width,
        colspec,
        leaves,
    }
}

/// A `yaml table-config` fence applied to the model it follows: columns
/// positionally, settings whole.
fn apply_config(model: &TableModel, config: &TableConfig) -> TableModel {
    let mut model = model.clone();
    let mut configs = config.columns.iter();
    fn walk(columns: &mut [Column], configs: &mut std::slice::Iter<'_, ColumnConfig>) {
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
    let defaults = tmark_ir::TableSettings::default();
    if config.settings.width != defaults.width {
        model.settings.width = config.settings.width.clone();
    }
    if config.settings.placement.is_some() {
        model.settings.placement = config.settings.placement.clone();
    }
    if config.settings.long.is_some() {
        model.settings.long = config.settings.long;
    }
    model
}

/// Rung 1 of the ladder: no groups, spans, separators, footer, widths or
/// settings — the `table.tex` path.
fn is_plain(model: &TableModel) -> bool {
    model.settings == tmark_ir::TableSettings::default()
        && model.footer.is_empty()
        && model.columns.iter().all(|c| match c {
            Column::Leaf(leaf) => leaf.config.width.is_none() && leaf.config.width_group.is_none(),
            Column::Group(_) => false,
        })
        && model.rows.iter().all(|r| match r {
            Row::Data(d) => d
                .cells
                .iter()
                .all(|c| c.rows == 1 && c.cols == 1 && !c.absorbed && c.align.is_none()),
            Row::Separator(_) => false,
        })
}

impl Latex<'_> {
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
            .id
            .clone()
            .or_else(|| caption.and_then(|c| c.attrs.id.clone()));
        self.req.package("booktabs");
        if is_plain(&model) {
            self.plain_table(&model, caption_text.as_deref(), label.as_deref(), caption);
        } else {
            self.model_table(&model, caption_text.as_deref(), label.as_deref(), caption);
        }
    }

    /// `\begin{table}[H]\centering\caption{…}\label{…}\vspace{0.5em}` or
    /// `\begin{center}` around a tabular.
    fn open_float(
        &mut self,
        caption: Option<&str>,
        label: Option<&str>,
        placement: Option<&str>,
        node: Option<&Caption>,
    ) {
        match caption {
            Some(caption) => {
                self.req.package("float");
                self.out
                    .push(&format!("\\begin{{table}}[{}]\n", placement.unwrap_or("H")));
                self.out.push("\\centering\n");
                if let Some(node) = node {
                    self.out.begin(node.meta.id);
                }
                self.out.push(&format!("\\caption{{{caption}}}\n"));
                if let Some(label) = label {
                    self.out
                        .push(&format!("\\label{{{}}}\n", escape::label(label)));
                }
                if let Some(node) = node {
                    self.out.end(node.meta.id);
                }
                self.out.push("\\vspace{0.5em}\n");
            }
            None => {
                self.out.push("\\begin{center}\n");
                if let Some(label) = label {
                    self.out
                        .push(&format!("\\label{{{}}}\n", escape::label(label)));
                }
            }
        }
    }

    fn close_float(&mut self, caption: Option<&str>) {
        self.out.push(if caption.is_some() {
            "\\end{table}\n"
        } else {
            "\\end{center}\n"
        });
    }

    /// A cell's LaTeX, with a leading `[` brace-protected: the row break
    /// and every booktabs rule take an optional argument
    /// (`escape::guard_bracket`).
    fn cell_text(&mut self, cell: &Cell) -> String {
        let was = self.in_cell;
        self.in_cell = true;
        let text = self.contained(|w| w.render_inlines(&cell.content));
        self.in_cell = was;
        let mut text = text.trim().to_string();
        escape::guard_bracket(&mut text);
        text
    }

    /// The header of a column: its inline Markdown when it carries any
    /// (`LeafColumn::title`), the escaped plain name otherwise.
    fn header_text(&mut self, title: &[Inline], name: Option<&str>) -> String {
        if title.is_empty() {
            let mut text = escape::prose(name.unwrap_or(""));
            escape::guard_bracket(&mut text);
            return text;
        }
        let was = self.in_cell;
        self.in_cell = true;
        let text = self.contained(|w| w.render_inlines(title));
        self.in_cell = was;
        let mut text = text.trim().to_string();
        escape::guard_bracket(&mut text);
        text
    }

    /// `table.tex`.
    fn plain_table(
        &mut self,
        model: &TableModel,
        caption: Option<&str>,
        label: Option<&str>,
        node: Option<&Caption>,
    ) {
        self.req.package("tabularx");
        let leaves: Vec<_> = model.columns.iter().flat_map(Column::leaves).collect();
        self.open_float(caption, label, None, node);
        let colspec: String = leaves
            .iter()
            .map(|l| format!("{}X", wrapper(l.config.align.unwrap_or(Align::Left))))
            .collect();
        self.out.push(&format!(
            "\\begin{{tabularx}}{{\\linewidth}}{{{colspec}}}\n"
        ));
        self.out.push("\\toprule\n");
        if leaves.iter().any(|l| l.name.is_some()) {
            let header: Vec<String> = leaves
                .iter()
                .map(|l| {
                    format!(
                        "\\textbf{{{}}}",
                        self.header_text(&l.title, l.name.as_deref())
                    )
                })
                .collect();
            self.out.push(&header.join(" & "));
            self.out.push(" \\\\\n\\midrule\n");
        }
        for row in &model.rows {
            if let Row::Data(d) = row {
                let cells: Vec<String> = d.cells.iter().map(|c| self.cell_text(c)).collect();
                self.out.push(&cells.join(" & "));
                self.out.push(" \\\\\n");
            }
        }
        self.out.push("\\bottomrule\n\\end{tabularx}\n");
        self.close_float(caption);
    }

    /// `yaml_table.tex` with `renderer.py`.
    fn model_table(
        &mut self,
        model: &TableModel,
        caption: Option<&str>,
        label: Option<&str>,
        node: Option<&Caption>,
    ) {
        let layout = layout(model);
        let total = layout.leaves.len();
        let header = self.header_lines(model, total);
        let body = self.section_lines(&model.rows, &layout.leaves);
        let footer = self.section_lines(&model.footer, &layout.leaves);
        match layout.env {
            Env::Longtable => {
                self.req.package("longtable");
                self.out
                    .push(&format!("\\begin{{longtable}}{{{}}}\n", layout.colspec));
                if let Some(caption) = caption {
                    if let Some(node) = node {
                        self.out.begin(node.meta.id);
                    }
                    self.out.push(&format!("\\caption{{{caption}}}"));
                    if let Some(label) = label {
                        self.out
                            .push(&format!("\\label{{{}}}", escape::label(label)));
                    }
                    if let Some(node) = node {
                        self.out.end(node.meta.id);
                    }
                    self.out.push("\\\\\n");
                }
                for head in ["\\endfirsthead\n", "\\endhead\n"] {
                    self.out.push("\\toprule\n");
                    if !header.is_empty() {
                        self.out.push(&header);
                        self.out.push("\\midrule\n");
                    }
                    self.out.push(head);
                }
                self.out.push(&body);
                if !footer.is_empty() {
                    self.out.push("\\midrule\n");
                    self.out.push(&footer);
                }
                self.out.push("\\bottomrule\n\\end{longtable}\n");
            }
            env => {
                self.open_float(caption, label, model.settings.placement.as_deref(), node);
                match env {
                    Env::Tabularx => {
                        self.req.package("tabularx");
                        self.out.push(&format!(
                            "\\begin{{tabularx}}{{{}}}{{{}}}\n",
                            layout.total_width.as_deref().unwrap_or("\\linewidth"),
                            layout.colspec
                        ));
                    }
                    _ => self
                        .out
                        .push(&format!("\\begin{{tabular}}{{{}}}\n", layout.colspec)),
                }
                self.out.push("\\toprule\n");
                if !header.is_empty() {
                    self.out.push(&header);
                    self.out.push("\\midrule\n");
                }
                self.out.push(&body);
                if !footer.is_empty() {
                    self.out.push("\\midrule\n");
                    self.out.push(&footer);
                }
                self.out.push("\\bottomrule\n");
                self.out.push(match env {
                    Env::Tabularx => "\\end{tabularx}\n",
                    _ => "\\end{tabular}\n",
                });
                self.close_float(caption);
            }
        }
    }

    /// Header rows from the column tree: a group is a `\multicolumn` over
    /// its leaves with a `\cmidrule(lr)` under it, a leaf shallower than
    /// the hierarchy a `\multirow`.
    fn header_lines(&mut self, model: &TableModel, total: usize) -> String {
        let depth = model.header_depth();
        let leaves: Vec<_> = model.columns.iter().flat_map(Column::leaves).collect();
        if depth == 0 || leaves.iter().all(|l| l.name.is_none()) {
            return String::new();
        }
        let mut lines: Vec<String> = Vec::new();
        for level in 0..depth {
            let mut cells: Vec<String> = Vec::new();
            let mut rules: Vec<String> = Vec::new();
            let mut cursor = 0;
            self.header_level(
                &model.columns,
                level,
                depth,
                &mut cursor,
                &mut cells,
                &mut rules,
            );
            debug_assert_eq!(cursor, total);
            lines.push(format!("{} \\\\", cells.join(" & ")));
            if level + 1 < depth && !rules.is_empty() {
                lines.push(rules.join(" "));
            }
        }
        if depth > 1 {
            self.req.package("multirow");
        }
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }

    /// Data rows with row spans threaded across rows (`_render_generic_row`).
    fn section_lines(&mut self, rows: &[Row], leaves: &[Leaf]) -> String {
        let total = leaves.len();
        let mut lines: Vec<String> = Vec::new();
        // Columns covered by a row span from above: (remaining rows, cols).
        let mut active: Vec<Option<(u32, u32)>> = vec![None; total];
        for row in rows {
            match row {
                Row::Separator(s) => {
                    active.iter_mut().for_each(|a| *a = None);
                    let rule = if s.double_rule {
                        "\\midrule[\\heavyrulewidth]"
                    } else {
                        "\\midrule"
                    };
                    match &s.label {
                        Some(label) if !label.trim().is_empty() => lines.push(format!(
                            "{rule}\n\\addlinespace\n\\multicolumn{{{total}}}{{l}}{{\\textit{{{}}}}} \\\\\n\\addlinespace",
                            escape::prose(label)
                        )),
                        _ => lines.push(rule.to_string()),
                    }
                }
                Row::Data(d) => {
                    let mut parts: Vec<String> = Vec::new();
                    let mut i = 0;
                    while i < total {
                        if let Some((remaining, cols)) = active[i] {
                            parts.push(if cols > 1 {
                                format!("\\multicolumn{{{cols}}}{{c}}{{}}")
                            } else {
                                String::new()
                            });
                            active[i] = (remaining > 1).then_some((remaining - 1, cols));
                            i += cols as usize;
                            continue;
                        }
                        let Some(cell) = d.cells.get(i) else {
                            parts.push(String::new());
                            i += 1;
                            continue;
                        };
                        if cell.absorbed {
                            // Covered by a column span to the left.
                            i += 1;
                            continue;
                        }
                        let mut tex = self.cell_text(cell);
                        if cell.rows > 1 {
                            self.req.package("multirow");
                            tex = format!("\\multirow{{{}}}{{*}}{{{tex}}}", cell.rows);
                        }
                        if cell.cols > 1 {
                            tex = format!(
                                "\\multicolumn{{{}}}{{{}}}{{{tex}}}",
                                cell.cols,
                                align_char(cell.align)
                            );
                        } else if let Some(align) = cell.align {
                            tex = format!(
                                "\\multicolumn{{1}}{{{}}}{{{tex}}}",
                                align_char(Some(align))
                            );
                        }
                        parts.push(tex);
                        if cell.rows > 1 {
                            active[i] = Some((cell.rows - 1, cell.cols));
                        }
                        i += cell.cols.max(1) as usize;
                    }
                    lines.push(format!("{} \\\\", parts.join(" & ")));
                }
            }
        }
        if lines.is_empty() {
            return String::new();
        }
        let mut out = lines.join("\n");
        out.push('\n');
        out
    }
}

impl Latex<'_> {
    /// One header level over the column tree (`_render_header`).
    fn header_level(
        &mut self,
        columns: &[Column],
        level: usize,
        depth: usize,
        cursor: &mut usize,
        cells: &mut Vec<String>,
        rules: &mut Vec<String>,
    ) {
        for column in columns {
            match column {
                Column::Leaf(leaf) => {
                    if level == 0 {
                        let name = self.header_text(&leaf.title, leaf.name.as_deref());
                        let span = depth;
                        cells.push(if span > 1 {
                            format!("\\multirow{{{span}}}{{*}}{{{name}}}")
                        } else {
                            name
                        });
                    }
                    // Rows below the leaf's own: empty slot (covered by the
                    // multirow above).
                    if level > 0 {
                        cells.push(String::new());
                    }
                    *cursor += 1;
                }
                Column::Group(g) => {
                    let width = g.columns.iter().map(|c| c.leaves().len()).sum::<usize>();
                    if level == 0 {
                        let name = self.header_text(&g.title, Some(&g.name));
                        cells.push(format!("\\multicolumn{{{width}}}{{c}}{{{name}}}"));
                        rules.push(format!(
                            "\\cmidrule(lr){{{}-{}}}",
                            *cursor + 1,
                            *cursor + width
                        ));
                        *cursor += width;
                    } else {
                        self.header_level(&g.columns, level - 1, depth - 1, cursor, cells, rules);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmark_ir::{ColumnGroup, LeafColumn, TableSettings};

    fn leaf(name: &str, width: Option<&str>) -> Column {
        Column::Leaf(LeafColumn {
            name: Some(name.into()),
            title: Vec::new(),
            config: ColumnConfig {
                width: width.map(str::to_string),
                ..Default::default()
            },
        })
    }

    #[test]
    fn layouts() {
        let model = TableModel {
            columns: vec![leaf("A", None), leaf("B", None)],
            ..Default::default()
        };
        let l = layout(&model);
        assert_eq!(l.env, Env::Tabular);
        assert_eq!(l.colspec, "ll");

        let model = TableModel {
            columns: vec![leaf("A", Some("30%")), leaf("B", Some("X"))],
            ..Default::default()
        };
        let l = layout(&model);
        assert_eq!(l.env, Env::Tabularx);
        assert_eq!(
            l.colspec,
            ">{\\raggedright\\arraybackslash}p{\\dimexpr 0.3\\linewidth-2\\tabcolsep\\relax}>{\\raggedright\\arraybackslash}X"
        );
        assert_eq!(l.total_width.as_deref(), Some("\\linewidth"));

        let model = TableModel {
            settings: TableSettings {
                width: "80%".into(),
                ..Default::default()
            },
            columns: vec![leaf("A", None), leaf("B", None)],
            ..Default::default()
        };
        let l = layout(&model);
        assert_eq!(l.env, Env::Tabularx);
        assert_eq!(l.total_width.as_deref(), Some("0.8\\linewidth"));

        let model = TableModel {
            columns: vec![
                leaf("A", None),
                Column::Group(ColumnGroup {
                    name: "G".into(),
                    title: Vec::new(),
                    columns: vec![leaf("B", None), leaf("C", None)],
                    config: ColumnConfig {
                        width: Some("50%".into()),
                        ..Default::default()
                    },
                }),
            ],
            ..Default::default()
        };
        let l = layout(&model);
        assert_eq!(l.leaves[1].width.as_deref(), Some("25%"));
        assert_eq!(l.leaves[2].width.as_deref(), Some("25%"));
    }
}
