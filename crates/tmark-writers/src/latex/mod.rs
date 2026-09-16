//! The LaTeX writer: what goes between `\begin{document}` and
//! `\end{document}` (design 07). Behaviour catalogue:
//! `specs/migration/writers-and-passes.md` §2; contract macro names and
//! key serialisation: `specs/migration/fragment-contracts.md` §1, §3, §5.
//!
//! Structural LaTeX (`\emph`, `figure`, `tabularx`, `\footnote`, `\href`,
//! `\label`) is written here and reports its packages; everything a
//! template may restyle is a contract macro (`\tslead`, `tscallout`,
//! `tscode`, `\tskeys`, …) reported in `Requires.fragments`.

pub mod escape;
mod figure;
mod inline;
mod table;

/// An image source without the MkDocs theme variant (`#only-light`).
pub(crate) use figure::strip_theme_variant as figure_src;

use std::collections::BTreeMap;

use tmark_ir::{
    Admonition, Attrs, Block, BlockQuote, Caption, CaptionKind, CodeBlock, DefinitionList, Div,
    Document, Header, Inline, ListItem, ListStyle, MathBlock, OrderedList, Para, RawBlock, Task,
};
use tmark_registry::Resolved;

use crate::common::{abbr, epigraph, logos, media, refs, Out};
use crate::{Backend, Body, CodeEngine, Media, Requires, Writer, WriterOptions};

/// The LaTeX writer.
#[derive(Debug, Default, Clone, Copy)]
pub struct LatexWriter;

impl Writer for LatexWriter {
    fn backend(&self) -> Backend {
        Backend::Latex
    }

    fn write(&self, doc: &Document, res: &Resolved, opts: &WriterOptions) -> Body {
        let mut w = Latex {
            doc,
            res,
            opts,
            out: Out::new(opts.source_map),
            req: Requires::default(),
            in_box: 0,
            in_cell: false,
            container: 0,
            acronyms: BTreeMap::new(),
            abbr_keys: abbr::keys(doc),
            lang: refs::language(opts.lang.as_deref(), doc, res),
            tex_logos: logos::enabled(doc),
            narrative: refs::narrative(opts.citations.narrative, doc),
        };
        w.blocks(&epigraph::blocks(doc));
        w.req.close();
        let (text, map) = w.out.finish();
        Body {
            text,
            map,
            requires: w.req,
        }
    }
}

/// The writer's state: the document, its resolution, the options, the
/// buffer, the `Requires` under construction, and the only mutable
/// context the emitters need (writers-and-passes.md §1).
pub(crate) struct Latex<'a> {
    doc: &'a Document,
    res: &'a Resolved,
    opts: &'a WriterOptions,
    out: Out,
    req: Requires,
    /// Depth of `tscallout`/`tscode` boxes: a figure inside one uses
    /// `\captionof` (no float in a box).
    in_box: usize,
    /// Inside a table cell.
    in_cell: bool,
    /// Depth inside a container: anything that is not the document's
    /// top-level block sequence (block quote, callout, figure, div, tab,
    /// list item, cell, aside, footnote). A `HorizontalRule` there is
    /// `\tsrule`, never the page-breaking `\tsdivider` (spec
    /// §HorizontalRule, challenge C48).
    container: usize,
    /// Acronym term → `\tsacr` key, first seen first.
    acronyms: BTreeMap<String, String>,
    /// Acronym keys substituted in running text (`common::abbr`).
    abbr_keys: Vec<String>,
    /// Feature `typography.tex-logos` (spec §TeX logos).
    pub(crate) tex_logos: bool,
    /// The language of the label words (`refs::language`).
    lang: Option<String>,
    /// A bare `@key` is `\textcite` rather than `\cite` (`refs::narrative`).
    narrative: bool,
}

impl Latex<'_> {
    /// Runs `f` one container deep (see `container`).
    fn contained<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.container += 1;
        let out = f(self);
        self.container -= 1;
        out
    }

    // ------------------------------------------------------------ blocks

    /// Writes blocks separated by one blank line; a caption (and a
    /// `yaml table-config`) right after its host is consumed with it.
    pub(crate) fn blocks(&mut self, blocks: &[Block]) {
        let mut first = true;
        let mut i = 0;
        while i < blocks.len() {
            let block = &blocks[i];
            if media::block_skipped(block, Media::Print) {
                i += 1;
                continue;
            }
            let mut next = i + 1;
            let config = match (block, blocks.get(next)) {
                (Block::Table(_), Some(Block::TableConfig(c))) => {
                    next += 1;
                    Some(c)
                }
                _ => None,
            };
            let caption = match blocks.get(next) {
                Some(Block::Caption(c)) if caption_matches(block, c.kind) => {
                    next += 1;
                    Some(c)
                }
                _ => None,
            };
            if !first {
                self.out.blank_line();
            }
            first = false;
            self.block(block, config, caption);
            i = next;
        }
    }

    fn block(
        &mut self,
        block: &Block,
        config: Option<&tmark_ir::TableConfig>,
        caption: Option<&Caption>,
    ) {
        self.out.begin(block.meta().id);
        match block {
            Block::Para(p) => self.para(p, caption),
            Block::Plain(p) => {
                self.inlines(&p.content);
                self.out.ensure_newline();
            }
            Block::Header(h) => self.header(h),
            Block::CodeBlock(c) => self.code_block(c, caption),
            Block::BlockQuote(q) => self.block_quote(q),
            Block::BulletList(l) => self.bullet_list(&l.items),
            Block::OrderedList(l) => self.ordered_list(l),
            Block::DefinitionList(d) => self.definition_list(d),
            Block::HorizontalRule(_) => self.horizontal_rule(),
            Block::Table(t) => self.table(t, config, caption),
            Block::TableConfig(_) => {}
            Block::Caption(c) => self.caption_orphan(c),
            Block::Figure(f) => self.figure(f, caption),
            Block::Admonition(a) => self.admonition(a),
            Block::Div(d) => self.div(d, caption),
            Block::MathBlock(m) => self.math_block(m),
            Block::RawBlock(r) => self.raw_block(r),
            Block::Include(i) => self.include(i),
            Block::Comment(_) => {}
        }
        self.out.end(block.meta().id);
    }

    /// `Para`: a lone image is a figure, a lone aside is a margin note, a
    /// `{lead}[…]` run-in is `\tslead{…}` (fragment-contracts.md §1).
    fn para(&mut self, p: &Para, caption: Option<&Caption>) {
        match p.content.as_slice() {
            // An icon (`Image{.icon}`, the emoji pass) is never a figure.
            [Inline::Image(image)] if !image.attrs.has_class("icon") => {
                self.figure_image(image, caption, None, None);
                return;
            }
            [Inline::Link(link)] if matches!(link.content.as_slice(), [Inline::Image(_)]) => {
                if let (Inline::Image(image), tmark_ir::Target::Url(url)) =
                    (&link.content[0], &link.target)
                {
                    self.figure_image(image, caption, Some(url), None);
                    return;
                }
            }
            [Inline::Aside(aside)] => {
                self.inline(&Inline::Aside(aside.clone()));
                self.out.ensure_newline();
                return;
            }
            _ => {}
        }
        if let Some(lead) = &p.lead {
            self.req.fragment("ts-typesetting");
            self.out.push("\\tslead{");
            self.inlines(lead);
            self.out.push("} ");
        }
        self.inlines(&p.content);
        self.out.trim_trailing_spaces();
        self.out.ensure_newline();
    }

    /// `heading.tex`: level −1 `part`, 0 `chapter`, 1–3 `section` …
    /// `subsubsection`, 4–5 `paragraph`/`subparagraph` plus `\mbox{}\\`,
    /// beyond `\textbf{}`; `*` when not numbered, per document
    /// (`headings.numbered`) or per heading (`.unnumbered`, `.unlisted`:
    /// spec §Header; an unnumbered heading keeps its `\addcontentsline`,
    /// an unlisted one loses it); `\label{id}` with the explicit id, or
    /// the implicit id when a reference targets it.
    fn header(&mut self, h: &Header) {
        let level = i32::from(h.level) + i32::from(self.opts.headings.base_level) - 1;
        let command = match level {
            -1 => Some("part"),
            0 => Some("chapter"),
            1 => Some("section"),
            2 => Some("subsection"),
            3 => Some("subsubsection"),
            4 => Some("paragraph"),
            5 => Some("subparagraph"),
            _ => None,
        };
        let title = self
            .render_inlines(&h.content)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let label = match h.attrs.id() {
            Some(id) => Some(id.to_string()),
            None => refs::referenced_implicit_id(self.res, h.meta.id).map(str::to_string),
        };
        let unlisted = h.attrs.has_class("unlisted");
        let unnumbered = unlisted || h.attrs.has_class("unnumbered");
        match command {
            Some(command) => {
                self.out.push("\\");
                self.out.push(command);
                if !self.opts.headings.numbered || unnumbered {
                    self.out.push("*");
                }
                self.out.push(&format!("{{{title}}}"));
                if unnumbered && !unlisted && self.opts.headings.numbered {
                    let toc = if level < 0 { "part" } else { command };
                    self.out
                        .push(&format!("\\addcontentsline{{toc}}{{{toc}}}{{{title}}}"));
                }
            }
            None => self.out.push(&format!("\\textbf{{{title}}}")),
        }
        if let Some(label) = label {
            self.out
                .push(&format!("\\label{{{}}}", escape::label(&label)));
        }
        if level >= 4 && command.is_some() {
            self.out.push("\\mbox{}\\\\");
        }
        self.out.ensure_newline();
    }

    /// `tscode` (fragment-contracts.md §5): keys serialised in registry
    /// order, the body verbatim with a final newline.
    fn code_block(&mut self, c: &CodeBlock, caption: Option<&Caption>) {
        let keys = self.code_keys(c.lang.as_deref(), &c.options, caption, Some(&c.text), None);
        self.begin_code(&keys);
        self.out.push(&c.text);
        if !c.text.is_empty() && !c.text.ends_with('\n') {
            self.out.push("\n");
        }
        self.end_code();
    }

    fn begin_code(&mut self, keys: &str) {
        self.req.fragment("ts-code");
        if self.opts.code.engine == CodeEngine::Minted {
            self.req.shell_escape = true;
        }
        self.out.push("\\begin{tscode}");
        if !keys.is_empty() {
            self.out.push(&format!("[{keys}]"));
        }
        self.out.push("\n");
        self.in_box += 1;
    }

    fn end_code(&mut self) {
        self.in_box -= 1;
        self.out.ensure_newline();
        self.out.push("\\end{tscode}\n");
    }

    /// The `tscode` keys: `lang`, `title`, `linenums`, `hl_lines`, `id`,
    /// `stretch`, `caption`, then the other attributes (fragment-contracts.md
    /// §3 rule 4 and 6; `engine` last, set by the highlight pass).
    fn code_keys(
        &mut self,
        lang: Option<&str>,
        options: &Attrs,
        caption: Option<&Caption>,
        body: Option<&str>,
        engine: Option<&str>,
    ) -> String {
        let mut keys: Vec<String> = Vec::new();
        if let Some(lang) = lang.or(options.get("lang")) {
            keys.push(format!("lang={lang}"));
        }
        if let Some(title) = options.get("title") {
            keys.push(format!("title={{{}}}", escape::prose(title)));
        }
        if let Some(lines) = options.get("linenums") {
            match lines.trim() {
                "" | "1" | "true" | "yes" => keys.push("linenums".to_string()),
                "false" | "no" => {}
                start => keys.push(format!("linenums={start}")),
            }
        }
        if let Some(hl) = options.get("hl_lines") {
            let ranges: Vec<&str> = hl
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .collect();
            if !ranges.is_empty() {
                keys.push(format!("hl_lines={{{}}}", ranges.join(",")));
            }
        }
        if let Some(id) = options.id().or(caption.and_then(|c| c.attrs.id())) {
            keys.push(format!("id={}", escape::label(id)));
        }
        if let Some(stretch) = options.get("stretch") {
            keys.push(format!("stretch={stretch}"));
        } else if body.is_some_and(is_ascii_art) {
            // `writer.py:1237`: box-drawing characters want tight lines.
            keys.push("stretch=0.5".to_string());
        }
        if let Some(caption) = caption {
            let text = self.render_inlines(&caption.content);
            keys.push(format!("caption={{{text}}}"));
        }
        if !options.classes.is_empty() {
            keys.push(format!("class={{{}}}", options.classes.join(",")));
        }
        for (k, v) in &options.kv {
            if matches!(
                k.as_str(),
                "lang"
                    | "title"
                    | "linenums"
                    | "hl_lines"
                    | "stretch"
                    | "include"
                    | "media"
                    | "engine"
            ) {
                continue;
            }
            keys.push(format!("{k}={{{}}}", escape::escape(v)));
        }
        if let Some(engine) = engine.or(options.get("engine")) {
            keys.push(format!("engine={engine}"));
        }
        keys.join(", ")
    }

    /// `displayquote` (csquotes); `.epigraph` → `\tsepigraph`.
    fn block_quote(&mut self, q: &BlockQuote) {
        if q.attrs.has_class("epigraph") {
            self.epigraph(&q.content, q.attrs.get("source"));
            return;
        }
        self.req.package("csquotes");
        self.out.push("\\begin{displayquote}\n");
        self.contained(|w| w.blocks(&q.content));
        self.out.ensure_newline();
        self.out.push("\\end{displayquote}\n");
    }

    /// `\tsepigraph[source={…}]{text}` (fragment-contracts.md §1).
    fn epigraph(&mut self, content: &[Block], source: Option<&str>) {
        self.req.fragment("ts-typesetting");
        self.out.push("\\tsepigraph");
        if let Some(source) = source {
            self.out
                .push(&format!("[source={{{}}}]", escape::prose(source)));
        }
        self.out.push("{");
        let body = self.render_blocks_inline(content);
        self.out.push(&body);
        self.out.push("}\n");
    }

    /// `itemize`, or `tstasklist` when an item carries a task state.
    fn bullet_list(&mut self, items: &[ListItem]) {
        let tasks = items.iter().any(|i| i.task.is_some());
        if tasks {
            self.req.fragment("ts-todolist");
            self.out.push("\\begin{tstasklist}\n");
        } else {
            self.out.push("\\begin{itemize}\n");
        }
        for item in items {
            let marker = match item.task {
                Some(Task::Done) => "\\item[\\tsdone] ",
                Some(Task::Open) => "\\item[\\tstodo] ",
                Some(Task::Partial) => "\\item[\\tspartial] ",
                None => "\\item ",
            };
            self.item(marker, &item.content);
        }
        self.out.push(if tasks {
            "\\end{tstasklist}\n"
        } else {
            "\\end{itemize}\n"
        });
    }

    /// `enumerate`; `start` and `style` through `enumitem` keys.
    fn ordered_list(&mut self, l: &OrderedList) {
        let mut keys: Vec<String> = Vec::new();
        let label = match l.style {
            ListStyle::Decimal | ListStyle::Generic => None,
            ListStyle::LowerAlpha => Some("\\alph*."),
            ListStyle::UpperAlpha => Some("\\Alph*."),
            ListStyle::LowerRoman => Some("\\roman*."),
            ListStyle::UpperRoman => Some("\\Roman*."),
        };
        if let Some(label) = label {
            keys.push(format!("label={label}"));
        }
        if l.start != 1 {
            keys.push(format!("start={}", l.start));
        }
        self.out.push("\\begin{enumerate}");
        if !keys.is_empty() {
            self.req.package("enumitem");
            self.out.push(&format!("[{}]", keys.join(", ")));
        }
        self.out.push("\n");
        for item in &l.items {
            self.item("\\item ", &item.content);
        }
        self.out.push("\\end{enumerate}\n");
    }

    /// One `\item`: the first paragraph on the marker's line, a nested
    /// list on its own line (`writer.py:694`), other blocks after a blank.
    fn item(&mut self, marker: &str, content: &[Block]) {
        self.contained(|w| w.item_body(marker, content));
    }

    fn item_body(&mut self, marker: &str, content: &[Block]) {
        self.out.push(marker);
        let (lead, inline, rest): (Option<&Vec<Inline>>, Option<&[Inline]>, &[Block]) =
            match content.split_first() {
                Some((Block::Para(p), tail)) => (p.lead.as_ref(), Some(&p.content[..]), tail),
                Some((Block::Plain(p), tail)) => (None, Some(&p.content[..]), tail),
                _ => (None, None, content),
            };
        if let Some(lead) = lead {
            self.req.fragment("ts-typesetting");
            self.out.push("\\tslead{");
            self.inlines(lead);
            self.out.push("} ");
        }
        if let Some(inline) = inline {
            self.inlines(inline);
        }
        self.out.trim_trailing_spaces();
        for block in rest {
            if media::block_skipped(block, Media::Print) {
                continue;
            }
            if matches!(block, Block::BulletList(_) | Block::OrderedList(_)) {
                self.out.ensure_newline();
            } else {
                self.out.blank_line();
            }
            self.block(block, None, None);
        }
        self.out.ensure_newline();
    }

    /// `description_list.tex`: `\item[{term}] body`, the term repeated for
    /// each definition.
    fn definition_list(&mut self, d: &DefinitionList) {
        self.out.push("\\begin{description}\n");
        for (term, definitions) in &d.items {
            let term = self.render_inlines(term);
            if definitions.is_empty() {
                self.out.push(&format!("\\item[{{{term}}}]\n"));
            }
            for definition in definitions {
                self.item(&format!("\\item[{{{term}}}] "), definition);
            }
        }
        self.out.push("\\end{description}\n");
    }

    /// `\tsdivider` at the top level (decisions.md X2), `\tsrule` inside a
    /// container: a page break there would tear the container apart (spec
    /// §HorizontalRule, challenge C48).
    fn horizontal_rule(&mut self) {
        self.req.fragment("ts-typesetting");
        self.out.push(if self.container > 0 {
            "\\tsrule\n"
        } else {
            "\\tsdivider\n"
        });
    }

    /// A caption with no float next to it: its text, with its anchor.
    fn caption_orphan(&mut self, c: &Caption) {
        let word = refs::caption_word(c.kind, self.lang.as_deref());
        self.out.push(&format!("{word}: "));
        self.inlines(&c.content);
        if let Some(id) = c.attrs.id() {
            self.out.push(&format!("\\label{{{}}}", escape::label(id)));
        }
        self.out.ensure_newline();
    }

    /// `tscallout` (fragment-contracts.md §1, §3 rule 6): `kind`, `title`,
    /// `id`, `class`, then the attributes; `collapsed` stays bare.
    fn admonition(&mut self, a: &Admonition) {
        self.req.fragment("ts-callouts");
        let mut keys = vec![format!("kind={}", a.kind)];
        if let Some(title) = &a.title {
            let title = self.render_inlines(title);
            keys.push(format!("title={{{title}}}"));
        }
        keys.extend(attr_keys(&a.attrs, &["collapsed"]));
        self.out
            .push(&format!("\\begin{{tscallout}}[{}]\n", keys.join(", ")));
        self.in_box += 1;
        self.contained(|w| w.blocks(&a.content));
        self.in_box -= 1;
        self.out.ensure_newline();
        self.out.push("\\end{tscallout}\n");
    }

    /// `Div{name}` by name (writers-and-passes.md §1): `epigraph`, `code`
    /// (the highlight contract, decisions.md X3), anything else the
    /// generic `tsdiv` container of fragment-contracts.md §3.
    fn div(&mut self, d: &Div, caption: Option<&Caption>) {
        match d.name.as_str() {
            "epigraph" => self.epigraph(&d.content, d.attrs.get("source")),
            // Print has no interaction: the tabs of a set follow each other,
            "tabs" => self.contained(|w| w.blocks(&d.content)),
            "code" => {
                let keys = self.code_keys(None, &d.attrs, caption, None, Some("pygments"));
                self.begin_code(&keys);
                for block in &d.content {
                    match block {
                        Block::RawBlock(r) if r.format == "latex" => {
                            self.out.push(r.text.trim_end_matches('\n'));
                            self.out.push("\n");
                        }
                        Block::CodeBlock(c) => {
                            self.out.push(&c.text);
                            self.out.ensure_newline();
                        }
                        _ => {}
                    }
                }
                self.end_code();
            }
            name => {
                self.req.fragment("ts-typesetting");
                let keys = attr_keys(&d.attrs, &[]);
                self.out.push(&format!("\\begin{{tsdiv}}{{{name}}}"));
                if !keys.is_empty() {
                    self.out.push(&format!("[{}]", keys.join(", ")));
                }
                self.out.push("\n");
                self.contained(|w| w.blocks(&d.content));
                self.out.ensure_newline();
                self.out.push("\\end{tsdiv}\n");
            }
        }
    }

    /// Display math verbatim (`writer.py:376-393`): an `align`/`equation`
    /// environment bare, an anchored block as `equation` with its
    /// `\label`, anything else `$$ … $$`.
    fn math_block(&mut self, m: &MathBlock) {
        let body = m.text.trim();
        let label = m
            .attrs
            .id()
            .map(|id| format!("\\label{{{}}}", escape::label(id)));
        if let Some(env) = block_environment(body) {
            let _ = env;
            match &label {
                Some(label) => {
                    // The label goes after the `\begin{…}` line.
                    let mut lines = body.splitn(2, '\n');
                    self.out.push(lines.next().unwrap_or_default());
                    self.out.push(label);
                    self.out.push("\n");
                    if let Some(rest) = lines.next() {
                        self.out.push(rest);
                        self.out.push("\n");
                    }
                }
                None => {
                    self.out.push(body);
                    self.out.push("\n");
                }
            }
        } else if let Some(label) = &label {
            self.out.push("\\begin{equation}");
            self.out.push(label);
            self.out.push("\n");
            self.out.push(body);
            self.out.push("\n\\end{equation}\n");
        } else {
            self.out.push("$$\n");
            self.out.push(body);
            self.out.push("\n$$\n");
        }
    }

    fn raw_block(&mut self, r: &RawBlock) {
        if r.format == "latex" {
            self.out.push(r.text.trim_end_matches('\n'));
            self.out.push("\n");
        }
    }

    /// `\input{stem}` for an include TeXSmith did not splice (design 07).
    fn include(&mut self, i: &tmark_ir::Include) {
        let stem = i.path.strip_suffix(".md").unwrap_or(&i.path);
        self.req.assets.push(crate::AssetRef {
            src: i.path.clone(),
            node: i.meta.id,
            attrs: i
                .base
                .as_ref()
                .map(|b| vec![("base".to_string(), b.clone())])
                .unwrap_or_default(),
        });
        self.out
            .push(&format!("\\input{{{}}}\n", escape::escape(stem)));
    }

    // ----------------------------------------------------------- helpers

    /// Inlines rendered aside, trailing newlines trimmed.
    pub(crate) fn render_inlines(&mut self, inlines: &[Inline]) -> String {
        let mut buf = std::mem::replace(&mut self.out, Out::scratch());
        self.inlines(inlines);
        std::mem::swap(&mut self.out, &mut buf);
        buf.finish_text()
    }

    /// Blocks rendered aside as one run: paragraphs joined by `\par `
    /// (footnotes, asides, epigraphs).
    pub(crate) fn render_blocks_inline(&mut self, blocks: &[Block]) -> String {
        self.contained(|w| w.render_blocks_inline_body(blocks))
    }

    fn render_blocks_inline_body(&mut self, blocks: &[Block]) -> String {
        let mut parts: Vec<String> = Vec::new();
        for block in blocks {
            if media::block_skipped(block, Media::Print) {
                continue;
            }
            let part = match block {
                Block::Para(p) => {
                    let mut s = String::new();
                    if let Some(lead) = &p.lead {
                        self.req.fragment("ts-typesetting");
                        s.push_str(&format!("\\tslead{{{}}} ", self.render_inlines(lead)));
                    }
                    s.push_str(&self.render_inlines(&p.content));
                    s
                }
                Block::Plain(p) => self.render_inlines(&p.content),
                other => {
                    let mut buf = std::mem::replace(&mut self.out, Out::scratch());
                    self.block(other, None, None);
                    std::mem::swap(&mut self.out, &mut buf);
                    buf.finish_text()
                }
            };
            if !part.trim().is_empty() {
                parts.push(part.trim_end().to_string());
            }
        }
        parts.join("\\par ")
    }
}

/// `#id` → `id`, classes → `class={a,b}`, `key=val` forwarded (`lang` and
/// `media` never); `bare` names keys emitted without a value when true.
fn attr_keys(attrs: &Attrs, bare: &[&str]) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(id) = attrs.id() {
        keys.push(format!("id={}", escape::label(id)));
    }
    if !attrs.classes.is_empty() {
        keys.push(format!("class={{{}}}", attrs.classes.join(",")));
    }
    for (k, v) in &attrs.kv {
        if matches!(k.as_str(), "lang" | "media") {
            continue;
        }
        if bare.contains(&k.as_str()) {
            match v.as_str() {
                "true" | "" => keys.push(k.clone()),
                "false" => {}
                other => keys.push(format!("{k}={other}")),
            }
        } else if v
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '%'))
        {
            keys.push(format!("{k}={v}"));
        } else {
            keys.push(format!("{k}={{{}}}", escape::prose(v)));
        }
    }
    keys
}

/// The caption kinds a block hosts (design C7).
fn caption_matches(block: &Block, kind: CaptionKind) -> bool {
    match (block, kind) {
        (Block::Table(_), CaptionKind::Table) => true,
        (Block::CodeBlock(_), CaptionKind::Listing) => true,
        (Block::Div(d), CaptionKind::Listing) => d.name == "code",
        (Block::Figure(_), CaptionKind::Figure) => true,
        (Block::Figure(_), CaptionKind::Table) => true,
        (Block::Para(p), CaptionKind::Figure) => match p.content.as_slice() {
            [Inline::Image(_)] => true,
            [Inline::Link(l)] => matches!(l.content.as_slice(), [Inline::Image(_)]),
            _ => false,
        },
        _ => false,
    }
}

/// `_payload_is_block_environment`: `align`, `align*`, `equation`,
/// `equation*` are emitted bare.
fn block_environment(body: &str) -> Option<&str> {
    let rest = body.trim_start().strip_prefix("\\begin{")?;
    let end = rest.find('}')?;
    let env = &rest[..end];
    matches!(
        env.to_ascii_lowercase().as_str(),
        "align" | "align*" | "equation" | "equation*"
    )
    .then_some(env)
}

/// `_is_ascii_art`: box-drawing characters.
fn is_ascii_art(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c,
            '┌' | '┬' | '─' | '┐' | '│' | '├' | '┼' | '┤' | '└' | '┴' | '┘'
        )
    })
}
