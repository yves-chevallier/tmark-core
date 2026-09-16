//! The web lowering: what a MkDocs page needs so that Material renders the
//! TMark constructs it does not know (TeXSmith
//! `specs/migration/web-profile.md`, design 07 §Web lowering).
//!
//! `lower_web` walks the tree, emits one `NodeEdit` per construct of the
//! per-construct table and applies them through `tmark_fmt::edit_many`, so
//! every byte outside a recognised construct is untouched: mkdocstrings,
//! tabs, icons, critic, `!!!` callouts and custom fences pass through. A
//! replacement is Markdown in the `Mkdocs` spelling, an HTML wrapper the
//! standard extension set reads (`md_in_html`, `attr_list`), or a reference
//! expanded with the numbers of the resolution. Nested content is
//! re-indented by `Out`. The lowering is pure: locations are opaque strings
//! and included files come through the `Loader`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tmark_fmt::{edit_many, print_node_with, NodeEdit, Profile, Replacement};
use tmark_ir::{
    plain_text, registry, Admonition, Aside, Block, Caption, CaptionKind, Cell, CodeBlock, Column,
    Diagnostic, Document, Figure, Image, Inline, Meta, NodeId, NodeRef, Ref, RefItem, Row, Side,
    Span, SpanNode, Table, Target, Var,
};
use tmark_registry::{join, Loader, Resolution, Resolved};

use crate::common::{media, refs, Out};
use crate::html::{self, escape};
use crate::Media;

/// What `@sec:x` shows on the web, where Material numbers no section.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SectionRefs {
    /// The heading text (`[Heading title](#sec:x)`).
    #[default]
    Title,
    /// The counter's `ref` template with the number the resolution gave
    /// (`[Section 2](#sec:x)`).
    Number,
}

/// How citations are lowered.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Citations {
    /// The HTML writer's built-in author-year style: `<a class="ts-cite">`
    /// plus a `References` list appended to the page.
    #[default]
    Inline,
    /// Pandoc `[@key]` citations for a site with `mkdocs-bibtex`.
    Passthrough,
}

/// Options of [`lower_web`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebOptions {
    pub sections: SectionRefs,
    pub citations: Citations,
    /// Language of the words the lowering adds itself (`and`, the
    /// `References` heading); the label words come from the resolution.
    /// Defaults to the resolution's language.
    pub lang: Option<String>,
    /// Prefix of every CSS class emitted (`ts-counter`, `ts-aside`).
    pub css_prefix: String,
}

impl Default for WebOptions {
    fn default() -> Self {
        WebOptions {
            sections: SectionRefs::Title,
            citations: Citations::Inline,
            lang: None,
            css_prefix: "ts-".to_string(),
        }
    }
}

/// What [`lower_web`] produces.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Lowered {
    /// The page, every construct of the table lowered, everything else as
    /// written; the `References` list appended when citations were
    /// lowered inline.
    pub text: String,
    /// The lowering's own diagnostics (the resolution's are in `Resolved`).
    pub diagnostics: Vec<Diagnostic>,
    /// The `References` list alone (`<ol class="ts-bibliography">`), for a
    /// plugin that places it itself; `None` without inline citations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bibliography: Option<String>,
}

/// Lower `text`, the source `doc` was parsed from, for a MkDocs page.
/// `res` is its resolution (`Numbering::All` and the site's `book` for a
/// site build); `loader` serves the text of included files.
pub fn lower_web(
    text: &str,
    doc: &Document,
    res: &Resolved,
    loader: &dyn Loader,
    opts: &WebOptions,
) -> Lowered {
    let mut lowerer = Lowerer::new(doc, res, loader, opts);
    let file = File {
        text,
        doc,
        path: res.path.clone(),
    };
    let mut out = lowerer.file(&file);
    let bibliography = lowerer.bibliography();
    if let Some(list) = &bibliography {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("\n## {}\n\n{list}\n", lowerer.word("References")));
    }
    Lowered {
        text: out,
        diagnostics: lowerer.diagnostics,
        bibliography,
    }
}

/// One source file being lowered: the main document or an include.
struct File<'a> {
    text: &'a str,
    doc: &'a Document,
    path: PathBuf,
}

impl File<'_> {
    /// The span lies in this text.
    fn valid(&self, span: Span) -> bool {
        let (start, end) = (span.start as usize, span.end as usize);
        span.file == self.doc.file
            && start <= end
            && end <= self.text.len()
            && self.text.is_char_boundary(start)
            && self.text.is_char_boundary(end)
    }

    fn slice(&self, span: Span) -> &str {
        if self.valid(span) {
            &self.text[span.start as usize..span.end as usize]
        } else {
            ""
        }
    }
}

/// A splice: the node, its span and the text replacing it.
struct Edit {
    id: NodeId,
    span: Span,
    text: String,
}

struct Lowerer<'a> {
    main: &'a Document,
    res: &'a Resolved,
    loader: &'a dyn Loader,
    opts: &'a WebOptions,
    /// Display number of every label by lower-cased id: the resolution's
    /// number, a document-order count for a series the resolution did
    /// not number, `3b` for a sub-figure.
    numbers: BTreeMap<String, String>,
    /// Cited keys, first seen first (the `References` list).
    citations: Vec<String>,
    diagnostics: Vec<Diagnostic>,
    /// Files being lowered, outermost first: a cycle stops here.
    stack: Vec<PathBuf>,
    /// The front matter as JSON, for `{{ key }}`.
    front_matter: serde_json::Value,
    /// The block being lowered is a direct child of a `markdown="1"`
    /// wrapper, so it is written at the wrapper's own column.
    ///
    /// Inside such a wrapper `md_in_html` reads an *indented* HTML block
    /// asymmetrically: the opening tag is data, because it is not at the
    /// start of a line, while the closing tag is still matched against
    /// the stack of open Markdown blocks. The indented `</div>` of a
    /// `<div markdown>` a nested callout body holds therefore closes the
    /// *wrapper*, and the rest of the page falls inside that container.
    /// A callout lowered here takes the HTML wrapper, whose body starts
    /// at column zero, rather than the `!!!` form, whose body is indented
    /// by four.
    html_parent: bool,
}

impl<'a> Lowerer<'a> {
    fn new(
        doc: &'a Document,
        res: &'a Resolved,
        loader: &'a dyn Loader,
        opts: &'a WebOptions,
    ) -> Self {
        let numbers = html::number_labels(res);
        let mut front_matter =
            serde_json::to_value(&doc.front_matter.keys).unwrap_or(serde_json::Value::Null);
        if let (serde_json::Value::Object(keys), serde_json::Value::Object(extra)) =
            (&mut front_matter, &doc.front_matter.extra)
        {
            for (k, v) in extra {
                keys.insert(k.clone(), v.clone());
            }
        }
        Lowerer {
            main: doc,
            res,
            loader,
            opts,
            numbers,
            citations: Vec::new(),
            diagnostics: Vec::new(),
            stack: Vec::new(),
            front_matter,
            html_parent: false,
        }
    }

    fn class(&self, name: &str) -> String {
        format!("{}{name}", self.opts.css_prefix)
    }

    /// The primary subtag of the language the lowering writes its own
    /// words and its label words in (`refs::language`).
    fn lang(&self) -> String {
        refs::language(self.opts.lang.as_deref(), self.main, self.res)
            .as_deref()
            .unwrap_or("en")
            .split(['-', '_'])
            .next()
            .unwrap_or("en")
            .to_ascii_lowercase()
    }

    /// A word of the lowering's own, localised.
    fn word(&self, en: &str) -> &'static str {
        match (self.lang().as_str(), en) {
            ("fr", "and") => "et",
            ("de", "and") => "und",
            ("fr", "References") => "Références",
            ("de", "References") => "Literatur",
            (_, "and") => "and",
            _ => "References",
        }
    }

    // ------------------------------------------------------------- files

    /// The lowered text of one file: every edit of its tree, spliced.
    fn file(&mut self, f: &File) -> String {
        let mut edits = Vec::new();
        self.blocks(f, &f.doc.blocks, &mut edits);
        for note in &f.doc.footnotes {
            self.blocks(f, &note.content, &mut edits);
        }
        let node_edits = edits
            .into_iter()
            .map(|e| NodeEdit {
                id: e.id,
                replacement: Replacement::Text(e.text),
            })
            .collect();
        edit_many(f.text, f.doc, node_edits).unwrap_or_else(|_| f.text.to_string())
    }

    /// Records a splice, unless it would change nothing.
    fn push(&mut self, out: &mut Vec<Edit>, f: &File, meta: &Meta, text: String) {
        if !f.valid(meta.span) || f.slice(meta.span) == text {
            return;
        }
        out.push(Edit {
            id: meta.id,
            span: meta.span,
            text,
        });
    }

    // ------------------------------------------------------------ blocks

    /// The edits of a block sequence: a block that lowers as a whole is
    /// one edit (its caption and table configuration emptied); any other
    /// block contributes the edits of its children.
    fn blocks(&mut self, f: &File, blocks: &[Block], out: &mut Vec<Edit>) {
        let mut i = 0;
        while i < blocks.len() {
            let block = &blocks[i];
            let attached = attached(blocks, i);
            let enclosing = line_prefix(f.text, block.meta().span.start);
            match self.block_lowering(f, block, attached.caption, &enclosing) {
                Some(text) => {
                    let text = reindent(&text, &enclosing);
                    self.push(out, f, block.meta(), text);
                    i += 1 + attached.consumed.len();
                    for consumed in attached.consumed {
                        self.push(out, f, consumed, String::new());
                    }
                }
                None => {
                    self.children(f, block, out);
                    i += 1;
                }
            }
        }
    }

    /// Blocks inside a lowered wrapper, written into `out` (which carries
    /// the wrapper's indentation): each block's own lowering, else its
    /// source with the inner splices applied. `html` says the wrapper is
    /// an HTML one (`markdown="1"`), which these blocks enter at column
    /// zero — see [`Lowerer::html_parent`].
    fn nested(&mut self, f: &File, blocks: &[Block], out: &mut Out, html: bool) {
        let html_parent = std::mem::replace(&mut self.html_parent, html);
        let mut written = false;
        let mut i = 0;
        while i < blocks.len() {
            let block = &blocks[i];
            let attached = attached(blocks, i);
            let enclosing = line_prefix(f.text, block.meta().span.start);
            let text = match self.block_lowering(f, block, attached.caption, &enclosing) {
                Some(text) => {
                    i += 1 + attached.consumed.len();
                    text
                }
                None => {
                    i += 1;
                    self.verbatim(f, block, &enclosing)
                }
            };
            if text.is_empty() {
                continue;
            }
            if written {
                out.blank_line();
            }
            written = true;
            out.push(&text);
            out.ensure_newline();
        }
        self.html_parent = html_parent;
    }

    /// A block as written, with the splices of its children, its
    /// continuation lines freed of the enclosing indentation.
    ///
    /// Its children keep the indentation the source gives them — a list
    /// item's callout stays inside its item — so they are not at the
    /// wrapper's column and [`Lowerer::html_parent`] does not hold for
    /// them.
    fn verbatim(&mut self, f: &File, block: &Block, enclosing: &str) -> String {
        let span = block.meta().span;
        let html_parent = std::mem::replace(&mut self.html_parent, false);
        let mut edits = Vec::new();
        self.children(f, block, &mut edits);
        self.html_parent = html_parent;
        dedent(&apply(f.slice(span), span.start, edits), enclosing)
    }

    /// The edits of a block's children, for a block kept as written.
    fn children(&mut self, f: &File, block: &Block, out: &mut Vec<Edit>) {
        match block {
            Block::Para(p) => {
                if let Some(lead) = &p.lead {
                    self.inlines(f, lead, out);
                }
                self.inlines(f, &p.content, out);
            }
            Block::Plain(p) => self.inlines(f, &p.content, out),
            Block::Header(h) => self.inlines(f, &h.content, out),
            Block::Caption(c) => self.inlines(f, &c.content, out),
            Block::BlockQuote(q) => self.blocks(f, &q.content, out),
            Block::Figure(g) => self.blocks(f, &g.content, out),
            Block::Div(d) => self.blocks(f, &d.content, out),
            Block::Admonition(a) => {
                if let Some(title) = &a.title {
                    self.inlines(f, title, out);
                }
                self.blocks(f, &a.content, out);
            }
            Block::BulletList(l) => {
                for item in &l.items {
                    self.blocks(f, &item.content, out);
                }
            }
            Block::OrderedList(l) => {
                for item in &l.items {
                    self.blocks(f, &item.content, out);
                }
            }
            Block::DefinitionList(d) => {
                for (term, definitions) in &d.items {
                    self.inlines(f, term, out);
                    for definition in definitions {
                        self.blocks(f, definition, out);
                    }
                }
            }
            Block::Table(t) => {
                for row in t.model.rows.iter().chain(&t.model.footer) {
                    if let Row::Data(row) = row {
                        for cell in &row.cells {
                            self.inlines(f, &cell.content, out);
                        }
                    }
                }
            }
            Block::CodeBlock(_)
            | Block::HorizontalRule(_)
            | Block::TableConfig(_)
            | Block::MathBlock(_)
            | Block::RawBlock(_)
            | Block::Include(_)
            | Block::Comment(_) => {}
        }
    }

    /// The whole-block lowering of the per-construct table, `None` for a
    /// block kept as written (its children may still splice).
    fn block_lowering(
        &mut self,
        f: &File,
        block: &Block,
        caption: Option<&Caption>,
        enclosing: &str,
    ) -> Option<String> {
        if media::block_skipped(block, Media::Web) {
            return Some(String::new());
        }
        match block {
            Block::Para(p) => match p.content.as_slice() {
                [Inline::Image(image)] => {
                    if media::inline_skipped(&Inline::Image(image.clone()), Media::Web) {
                        return Some(String::new());
                    }
                    match caption {
                        Some(caption) => Some(self.figure_image(f, image, caption)),
                        None => (image.attrs.media() == Some("web"))
                            .then(|| self.image_text(f, image, true)),
                    }
                }
                [Inline::Aside(aside)] if f.slice(p.meta.span).starts_with(":::") => {
                    Some(self.aside_block(f, aside))
                }
                _ => None,
            },
            Block::Header(h) => (h.attrs.media() == Some("web")).then(|| {
                let mut h = h.clone();
                h.attrs.kv.retain(|(k, _)| k != "media");
                print_node_with(NodeRef::Block(&Block::Header(h)), Profile::Mkdocs)
            }),
            Block::CodeBlock(c) => match caption {
                Some(caption) => Some(self.listing(f, c, caption, enclosing)),
                None => self.fence(f, c),
            },
            Block::Table(t) => self.table(f, t, caption, enclosing),
            Block::TableConfig(_) => Some(String::new()),
            Block::Figure(figure) => Some(self.figure(f, figure, caption)),
            Block::Admonition(a) => self.admonition(f, a),
            Block::MathBlock(m) => m.attrs.id().map(|id| {
                let mut bare = m.clone();
                bare.attrs = Default::default();
                format!(
                    "<div id=\"{}\" class=\"{}\" markdown=\"1\">\n{}\n</div>",
                    escape::attr(id),
                    self.class("equation"),
                    print_node_with(NodeRef::Block(&Block::MathBlock(bare)), Profile::Mkdocs)
                )
            }),
            Block::RawBlock(r) => match r.format.as_str() {
                "html" => Some(r.text.clone()),
                "latex" | "typst" => Some(String::new()),
                _ => None,
            },
            Block::Include(i) => self.include(f, i),
            Block::Div(d) => self.div(f, d),
            Block::Plain(_)
            | Block::BlockQuote(_)
            | Block::BulletList(_)
            | Block::OrderedList(_)
            | Block::DefinitionList(_)
            | Block::HorizontalRule(_)
            | Block::Caption(_)
            | Block::Comment(_) => None,
        }
    }

    // ----------------------------------------------------------- figures

    /// `<figcaption markdown="span">` with the caption label when the
    /// float is numbered.
    fn figcaption(&mut self, f: &File, caption: &Caption, id: Option<&str>) -> String {
        let mut out = String::from("<figcaption markdown=\"span\">");
        if let Some(label) = id.and_then(|id| self.label_of_id(id)) {
            out.push_str(&format!(
                "<span class=\"{}\">{}:</span> ",
                self.class("caption-label"),
                escape::text(&label)
            ));
        }
        out.push_str(&self.inlines_text(f, &caption.content));
        out.push_str("</figcaption>");
        out
    }

    /// The label text of a defined id (`Figure 3`, `FW-01`): the series'
    /// `ref` template over its name and the display number.
    fn label_of_id(&self, id: &str) -> Option<String> {
        let number = self.numbers.get(&id.to_ascii_lowercase())?;
        let label = self.res.labels.get(id);
        let prefix = label.and_then(|l| l.prefix.as_deref());
        Some(match prefix {
            Some(prefix) => refs::template(
                &refs::reference_template(self.res, prefix),
                &refs::label_word(self.res, prefix, prefix, Some(&self.lang())),
                number,
            )
            .trim()
            .to_string(),
            None => number.clone(),
        })
    }

    /// An image as Markdown: as written, or reprinted without its id (the
    /// figure wrapper carries it) or without `media`.
    fn image_text(&self, f: &File, image: &Image, strip_id: bool) -> String {
        let strip_media = image.attrs.media() == Some("web");
        if !(strip_id && image.attrs.id.is_some()) && !strip_media {
            let source = f.slice(image.meta.span);
            if !source.is_empty() {
                return source.to_string();
            }
        }
        let mut image = image.clone();
        if strip_id {
            image.attrs.id = None;
            image.attrs.id_span = None;
        }
        image.attrs.kv.retain(|(k, _)| k != "media");
        print_node_with(NodeRef::Inline(&Inline::Image(image)), Profile::Mkdocs)
    }

    fn figure_image(&mut self, f: &File, image: &Image, caption: &Caption) -> String {
        let id = caption.attrs.id().or(image.attrs.id()).map(str::to_string);
        let mut out = String::from("<figure markdown=\"span\"");
        if let Some(id) = &id {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        out.push_str(">\n");
        out.push_str(&self.image_text(f, image, true));
        out.push('\n');
        out.push_str(&self.figcaption(f, caption, id.as_deref()));
        out.push_str("\n</figure>");
        out
    }

    /// `::: figure`: sub-figures in a grid, `(a)`, `(b)` under each image.
    fn figure(&mut self, f: &File, figure: &Figure, caption: Option<&Caption>) -> String {
        let (content, inner) = match figure.content.split_last() {
            Some((Block::Caption(c), rest)) if caption.is_none() => (rest, Some(c)),
            _ => (figure.content.as_slice(), None),
        };
        let caption = caption.or(inner);
        let id = figure
            .attrs
            .id()
            .or(caption.and_then(|c| c.attrs.id()))
            .map(str::to_string);
        // The images the resolution turned into sub-figures, in its own
        // order: the `(a)` markers below carry the letters it gave them.
        let images = tmark_registry::figure_images(figure);
        let all_images = !images.is_empty();
        let mut out = String::from("<figure");
        out.push_str(if all_images {
            " markdown=\"span\""
        } else {
            " markdown=\"1\""
        });
        if let Some(id) = &id {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        let mut classes: Vec<String> = figure.attrs.classes.clone();
        if images.len() > 1 {
            classes.insert(0, self.class("subfigures"));
        }
        if !classes.is_empty() {
            out.push_str(&format!(" class=\"{}\"", escape::attr(&classes.join(" "))));
        }
        if let Some(cols) = figure.attrs.get("cols") {
            out.push_str(&format!(
                " style=\"--{}cols:{}\"",
                self.opts.css_prefix,
                escape::attr(cols)
            ));
        }
        out.push_str(">\n");
        if all_images {
            for (i, image) in images.iter().enumerate() {
                out.push_str(&self.image_text(f, image, false));
                out.push('\n');
                if images.len() > 1 {
                    out.push_str(&format!(
                        "<span class=\"{}\">({})</span>\n",
                        self.class("subcaption"),
                        tmark_registry::subfigure_letter(i)
                    ));
                }
            }
        } else {
            let mut body = Out::scratch();
            self.nested(f, content, &mut body, true);
            out.push('\n');
            out.push_str(&body.finish_text());
            out.push_str("\n\n");
        }
        if let Some(caption) = caption {
            out.push_str(&self.figcaption(f, caption, id.as_deref()));
            out.push('\n');
        }
        out.push_str("</figure>");
        out
    }

    fn listing(
        &mut self,
        f: &File,
        code: &CodeBlock,
        caption: &Caption,
        enclosing: &str,
    ) -> String {
        let id = caption.attrs.id().or(code.options.id()).map(str::to_string);
        let mut out = format!("<figure markdown=\"1\" class=\"{}\"", self.class("listing"));
        if let Some(id) = &id {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        out.push_str(">\n\n");
        let body = self
            .fence(f, code)
            .unwrap_or_else(|| dedent(f.slice(code.meta.span), enclosing));
        out.push_str(&body);
        out.push_str("\n\n");
        out.push_str(&self.figcaption(f, caption, id.as_deref()));
        out.push_str("\n</figure>");
        out
    }

    /// A fence whose info string carries `include="file"` (spec
    /// §Includes): the file's text becomes the body and the attribute is
    /// dropped, `title=` and the rest kept. `pymdownx.superfences` refuses
    /// an option it does not know and renders the whole fence as one
    /// inline code span, so a page that follows the deprecation of
    /// `--8<--` would otherwise lose every listing. A file the loader
    /// cannot serve is `include-missing`, and the fence is reprinted the
    /// same way with an **empty** body: keeping the bytes would keep
    /// `include=` too, and that inline code span swallows the paragraph
    /// after it. `None` leaves the bytes alone: a fence with no
    /// `include=`, or the `--8<--` spelling, which is
    /// `pymdownx.snippets`' to expand, like a block snippet.
    fn fence(&mut self, f: &File, code: &CodeBlock) -> Option<String> {
        let path = code.options.get("include")?.to_string();
        let opener = f.slice(code.meta.span).lines().next().unwrap_or("");
        if !opener.contains("include=") {
            return None;
        }
        let loaded = self.loader.load(&f.path, &path);
        let text = loaded.unwrap_or_else(|| {
            self.diagnostics.push(Diagnostic::new(
                tmark_ir::Code::IncludeMissing,
                code.meta.span,
                format!("included file `{path}` not found; the fence is printed empty"),
            ));
            String::new()
        });
        let mut spliced = code.clone();
        spliced.options.kv.retain(|(k, _)| k != "include");
        spliced.text = text.trim_end_matches('\n').to_string();
        Some(print_node_with(
            NodeRef::Block(&Block::CodeBlock(spliced)),
            Profile::Mkdocs,
        ))
    }

    // ------------------------------------------------------------ tables

    /// A pipe table with a caption gets the figure wrapper, the table
    /// itself as written; a `yaml table` fence becomes an HTML table whose
    /// cells `md_in_html` reads in span mode.
    fn table(
        &mut self,
        f: &File,
        table: &Table,
        caption: Option<&Caption>,
        enclosing: &str,
    ) -> Option<String> {
        let yaml = f.slice(table.meta.span).starts_with(['`', '~']);
        if !yaml && caption.is_none() {
            return None;
        }
        let id = table
            .attrs
            .id()
            .or(caption.and_then(|c| c.attrs.id()))
            .map(str::to_string);
        let body = if yaml {
            self.table_html(
                f,
                table,
                if caption.is_none() {
                    id.as_deref()
                } else {
                    None
                },
            )
        } else {
            let mut edits = Vec::new();
            self.children(f, &Block::Table(table.clone()), &mut edits);
            dedent(
                &apply(f.slice(table.meta.span), table.meta.span.start, edits),
                enclosing,
            )
        };
        let Some(caption) = caption else {
            return Some(body);
        };
        let mut out = format!("<figure markdown=\"1\" class=\"{}\"", self.class("table"));
        if let Some(id) = &id {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        out.push_str(">\n");
        out.push_str(&self.figcaption(f, caption, id.as_deref()));
        out.push_str("\n\n");
        out.push_str(&body);
        out.push_str("\n\n</figure>");
        Some(out)
    }

    fn table_html(&mut self, f: &File, table: &Table, id: Option<&str>) -> String {
        let model = &table.model;
        let mut out = format!(
            "<table class=\"{}\" data-{}table=\"1\"",
            self.class("table"),
            self.opts.css_prefix
        );
        if let Some(id) = id {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        out.push_str(" markdown=\"block\">\n");
        let leaves: Vec<_> = model.columns.iter().flat_map(Column::leaves).collect();
        if leaves.iter().any(|l| l.name.is_some()) {
            out.push_str("<thead markdown=\"block\">\n");
            let depth = model.header_depth();
            for level in 0..depth {
                out.push_str("<tr markdown=\"block\">\n");
                let header = self.header_level(f, &model.columns, level, depth);
                out.push_str(&header);
                out.push_str("</tr>\n");
            }
            out.push_str("</thead>\n");
        }
        out.push_str("<tbody markdown=\"block\">\n");
        self.rows(f, &mut out, &model.rows, &leaves);
        out.push_str("</tbody>\n");
        if !model.footer.is_empty() {
            out.push_str("<tfoot markdown=\"block\">\n");
            self.rows(f, &mut out, &model.footer, &leaves);
            out.push_str("</tfoot>\n");
        }
        out.push_str("</table>");
        out
    }

    fn rows(&mut self, f: &File, out: &mut String, rows: &[Row], leaves: &[&tmark_ir::LeafColumn]) {
        for row in rows {
            match row {
                Row::Separator(s) => {
                    let mut classes = self.class("separator");
                    if s.double_rule {
                        classes.push(' ');
                        classes.push_str(&self.class("double"));
                    }
                    out.push_str(&format!("<tr class=\"{classes}\">"));
                    if let Some(label) = &s.label {
                        out.push_str(&format!(
                            "<td colspan=\"{}\"><em>{}</em></td>",
                            leaves.len(),
                            escape::text(label)
                        ));
                    }
                    out.push_str("</tr>\n");
                }
                Row::Data(d) => {
                    out.push_str("<tr markdown=\"block\">\n");
                    for (i, cell) in d.cells.iter().enumerate() {
                        if cell.absorbed {
                            continue;
                        }
                        let align = cell.align.or(leaves.get(i).and_then(|l| l.config.align));
                        out.push_str(&self.cell(f, cell, align));
                    }
                    out.push_str("</tr>\n");
                }
            }
        }
    }

    /// Header rows of a table model, one `<th>` per column of the level.
    fn header_level(&mut self, f: &File, columns: &[Column], level: usize, depth: usize) -> String {
        let mut out = String::new();
        for column in columns {
            match column {
                Column::Leaf(leaf) => {
                    if level == 0 {
                        let span = depth - level;
                        out.push_str("<th markdown=\"span\"");
                        if span > 1 {
                            out.push_str(&format!(" rowspan=\"{span}\""));
                        }
                        out.push_str(&html::align_attr(leaf.config.align));
                        out.push('>');
                        out.push_str(&self.header_text(f, &leaf.title, leaf.name.as_deref()));
                        out.push_str("</th>\n");
                    }
                }
                Column::Group(group) => {
                    if level == 0 {
                        let cols = group
                            .columns
                            .iter()
                            .map(|c| c.leaves().len())
                            .sum::<usize>();
                        out.push_str(&format!(
                            "<th markdown=\"span\" colspan=\"{cols}\"{}>",
                            html::align_attr(group.config.align)
                        ));
                        out.push_str(&self.header_text(f, &group.title, Some(&group.name)));
                        out.push_str("</th>\n");
                    } else {
                        out.push_str(&self.header_level(f, &group.columns, level - 1, depth - 1));
                    }
                }
            }
        }
        out
    }

    /// The header of a column: its inline Markdown when it carries any
    /// (`LeafColumn::title`), the escaped plain name otherwise.
    fn header_text(&mut self, f: &File, title: &[Inline], name: Option<&str>) -> String {
        if title.is_empty() {
            return escape::text(name.unwrap_or(""));
        }
        self.inlines_text(f, title)
    }

    fn cell(&mut self, f: &File, cell: &Cell, align: Option<tmark_ir::Align>) -> String {
        let mut td = String::from("<td markdown=\"span\"");
        if cell.rows > 1 {
            td.push_str(&format!(" rowspan=\"{}\"", cell.rows));
        }
        if cell.cols > 1 {
            td.push_str(&format!(" colspan=\"{}\"", cell.cols));
        }
        td.push_str(&html::align_attr(align));
        td.push('>');
        td.push_str(&self.inlines_text(f, &cell.content));
        td.push_str("</td>\n");
        td
    }

    // ---------------------------------------------------------- callouts

    /// `::: type` → `!!! type "T"` (`???`, `???+` when collapsed); a
    /// numbered kind, an id or anything the `!!!` line cannot carry →
    /// `<div class="admonition …" markdown="1">` (`<details>` when
    /// collapsed). A `!!!` source stays as written, unless its title
    /// lowers to something the marker line cannot carry: PyMdownX reads
    /// the title up to the next `"`, so a counter or a reference that
    /// becomes a `<span …>` there needs the wrapper too.
    ///
    /// A callout inside an HTML wrapper takes the wrapper as well, as
    /// written or not: the marker form indents its body, and an indented
    /// HTML block inside a `markdown="1"` wrapper closes the wrapper
    /// instead of itself ([`Lowerer::html_parent`]).
    fn admonition(&mut self, f: &File, a: &Admonition) -> Option<String> {
        let title = a.title.as_ref().map(|t| self.inlines_text(f, t));
        let source = f.slice(a.meta.span);
        if !self.html_parent
            && (source.starts_with("!!!") || source.starts_with("???"))
            && title.as_deref().map_or(true, |t| !t.contains(['"', '\n']))
        {
            return None;
        }
        let collapsed = a.attrs.get("collapsed");
        let marker = match collapsed {
            None => Some("!!!"),
            Some("true") => Some("???"),
            Some("false") => Some("???+"),
            Some(_) => None,
        };
        let counter = self.kind_counter(&a.kind);
        let numbered = a.attrs.id.is_some() || counter.is_some();
        let plain = !self.html_parent
            && !numbered
            && a.attrs.kv.iter().all(|(k, _)| k == "collapsed")
            && !a.content.is_empty()
            && bare(&a.kind)
            && a.attrs.classes.iter().all(|c| bare(c))
            && title
                .as_deref()
                .map_or(true, |t| !t.contains(['"', '\n']) && t == t.trim());
        if let (true, Some(marker)) = (plain, marker) {
            let mut out = Out::scratch();
            out.push(marker);
            out.push(" ");
            out.push(&a.kind);
            for class in &a.attrs.classes {
                out.push(" ");
                out.push(class);
            }
            if let Some(title) = &title {
                out.push(&format!(" \"{title}\""));
            }
            out.push("\n");
            out.push_prefix("    ");
            self.nested(f, &a.content, &mut out, false);
            out.pop_prefix();
            return Some(out.finish_text());
        }
        let details = collapsed.is_some();
        let (tag, title_tag) = if details {
            ("details", "summary")
        } else {
            ("div", "p")
        };
        let mut classes: Vec<&str> = if details { vec![] } else { vec!["admonition"] };
        classes.push(&a.kind);
        classes.extend(a.attrs.classes.iter().map(String::as_str));
        let mut out = format!("<{tag} class=\"{}\"", escape::attr(&classes.join(" ")));
        if let Some(id) = a.attrs.id() {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        out.push_str(" markdown=\"1\"");
        if details && collapsed != Some("true") {
            out.push_str(" open");
        }
        out.push_str(">\n");
        let word = counter
            .as_deref()
            .and_then(|p| self.res.counters.get(p))
            .and_then(|c| c.name.clone())
            .or_else(|| registry::admonition(&a.kind).map(|k| k.label.to_string()))
            .unwrap_or_else(|| capitalise(&a.kind));
        let number = a
            .attrs
            .id()
            .and_then(|id| self.numbers.get(&id.to_ascii_lowercase()).cloned());
        let heading = match (number, title) {
            (Some(n), Some(t)) => format!("{word} {n} ({t})"),
            (Some(n), None) => format!("{word} {n}"),
            (None, Some(t)) => t,
            (None, None) => word,
        };
        out.push_str(&format!(
            "<{title_tag} class=\"admonition-title\">{heading}</{title_tag}>\n"
        ));
        let mut body = Out::scratch();
        self.nested(f, &a.content, &mut body, true);
        let body = body.finish_text();
        if !body.is_empty() {
            out.push('\n');
            out.push_str(&body);
            out.push('\n');
        }
        out.push_str(&format!("\n</{tag}>"));
        Some(out)
    }

    /// The counter prefix of a theorem-like kind, declared in the front
    /// matter or predeclared.
    fn kind_counter(&self, kind: &str) -> Option<String> {
        self.main
            .front_matter
            .keys
            .press
            .declare
            .admonitions
            .get(kind)
            .and_then(|d| d.counter.clone())
            .or_else(|| registry::admonition(kind).and_then(|k| k.counter.map(str::to_string)))
    }

    /// `::: div {.x}` → `<div class="x" markdown="1">` … `</div>` (spec
    /// §Div: `md_in_html` is the only container a Python-Markdown site
    /// renders, and the compatibility table's `<div markdown>` row is what
    /// the `mkdocs` profile emits). It is also where `/// html | div[…]`
    /// lands, so a page keeps the layout the author wrote.
    ///
    /// Only `div` is lowered: `tabs` and `tab` are Material's own
    /// construct (`=== "Title"`), `multicolumn` has no Material element,
    /// and an unknown name is the spec's degradation contract — all three
    /// keep their bytes. A container the author already wrote as HTML
    /// keeps its bytes too: there is nothing to splice.
    fn div(&mut self, f: &File, d: &tmark_ir::Div) -> Option<String> {
        if d.name != "div" || f.slice(d.meta.span).starts_with('<') {
            return None;
        }
        let mut out = String::from("<div");
        if let Some(id) = d.attrs.id() {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        if !d.attrs.classes.is_empty() {
            out.push_str(&format!(
                " class=\"{}\"",
                escape::attr(&d.attrs.classes.join(" "))
            ));
        }
        for (k, v) in &d.attrs.kv {
            out.push_str(&format!(" {}=\"{}\"", k, escape::attr(v)));
        }
        out.push_str(" markdown=\"1\">\n\n");
        let mut body = Out::scratch();
        self.nested(f, &d.content, &mut body, true);
        out.push_str(&body.finish_text());
        out.push_str("\n\n</div>");
        Some(out)
    }

    fn aside_block(&mut self, f: &File, aside: &Aside) -> String {
        let mut out = format!("<aside class=\"{}\"", self.class("aside"));
        if let Some(side) = aside.side {
            out.push_str(&format!(" data-side=\"{}\"", side_name(side)));
        }
        out.push_str(" markdown=\"1\">\n\n");
        let mut body = Out::scratch();
        self.nested(f, &aside.content, &mut body, true);
        out.push_str(&body.finish_text());
        out.push_str("\n\n</aside>");
        out
    }

    // ---------------------------------------------------------- includes

    /// `{include}(f)`: the lowered text of the included file, from the
    /// resolution's parse of it and the loader's text. `--8<--` stays for
    /// `snippets`.
    fn include(&mut self, f: &File, include: &tmark_ir::Include) -> Option<String> {
        if f.slice(include.meta.span).starts_with("--8<--") {
            return None;
        }
        let from = match &include.base {
            Some(base) => join(&f.path, &format!("{base}/")),
            None => f.path.clone(),
        };
        let target = join(&from, &include.path);
        if self.stack.contains(&target) || target == self.res.path {
            return None;
        }
        let text = self.loader.load(&from, &include.path)?;
        let id = self
            .res
            .files
            .iter()
            .find(|(_, p)| *p == target)
            .map(|(id, _)| *id);
        let doc = id.and_then(|id| self.res.included.iter().find(|(i, _)| *i == id));
        let Some((_, doc)) = doc else {
            self.diagnostics.push(Diagnostic {
                severity: tmark_ir::Severity::Info,
                ..Diagnostic::new(
                    tmark_ir::Code::IncludeMissing,
                    include.meta.span,
                    format!(
                        "`{}` is not among the files the resolution parsed; the include is left as written",
                        include.path
                    ),
                )
            });
            return None;
        };
        let file = File {
            text: &text,
            doc,
            path: target.clone(),
        };
        self.stack.push(target);
        let out = self.file(&file);
        self.stack.pop();
        Some(out.trim_end_matches('\n').to_string())
    }

    // ----------------------------------------------------------- inlines

    fn inlines(&mut self, f: &File, inlines: &[Inline], out: &mut Vec<Edit>) {
        let mut removed_previous = false;
        for (i, inline) in inlines.iter().enumerate() {
            match self.inline_lowering(f, inline) {
                Some(text) => {
                    if text.is_empty() {
                        self.collapse_space(f, inlines, i, removed_previous, out);
                    }
                    removed_previous = text.is_empty();
                    self.push(out, f, inline.meta(), text);
                }
                None => {
                    removed_previous = false;
                    self.inline_children(f, inline, out);
                }
            }
        }
    }

    /// A removed inline takes one adjacent space with it (the zero-width
    /// collapse of design 07): the space before it, else the one after.
    /// Spaces live in `Str` runs or in `Space` nodes.
    fn collapse_space(
        &mut self,
        f: &File,
        inlines: &[Inline],
        i: usize,
        removed_previous: bool,
        out: &mut Vec<Edit>,
    ) {
        let before = if removed_previous {
            None
        } else {
            i.checked_sub(1).map(|j| &inlines[j])
        };
        match before {
            Some(Inline::Space(space)) => {
                self.push(out, f, &space.meta, String::new());
                return;
            }
            Some(Inline::Str(run)) if f.slice(run.meta.span).ends_with([' ', '\t']) => {
                let trimmed = f
                    .slice(run.meta.span)
                    .trim_end_matches([' ', '\t'])
                    .to_string();
                self.push(out, f, &run.meta, trimmed);
                return;
            }
            _ => {}
        }
        match inlines.get(i + 1) {
            Some(Inline::Space(space)) => self.push(out, f, &space.meta, String::new()),
            Some(Inline::Str(run)) if f.slice(run.meta.span).starts_with([' ', '\t']) => {
                let trimmed = f
                    .slice(run.meta.span)
                    .trim_start_matches([' ', '\t'])
                    .to_string();
                self.push(out, f, &run.meta, trimmed);
            }
            _ => {}
        }
    }

    fn inline_children(&mut self, f: &File, inline: &Inline, out: &mut Vec<Edit>) {
        match inline {
            Inline::Emph(n) => self.inlines(f, &n.content, out),
            Inline::Strong(n) => self.inlines(f, &n.content, out),
            Inline::Quoted(n) => self.inlines(f, &n.content, out),
            Inline::Link(n) => self.inlines(f, &n.content, out),
            Inline::Note(n) => self.blocks(f, &n.content, out),
            _ => {}
        }
    }

    /// An inline sequence lowered: its source with the splices applied
    /// when the spans are those of the text, else each inline printed
    /// (a title parsed from an attribute, a cell of a `yaml table`).
    fn inlines_text(&mut self, f: &File, inlines: &[Inline]) -> String {
        let (Some(first), Some(last)) = (inlines.first(), inlines.last()) else {
            return String::new();
        };
        // Spans are those of the text when they are in range, in order,
        // and every literal run reads back from its span. A fragment the
        // lowering could not place — a title parsed from an attribute
        // value with an escape in it, a cell a `yaml table` payload does
        // not spell verbatim — carries the span of the construct it came
        // from, and reads back as a slice of that construct instead.
        let mut previous = first.meta().span.start;
        let sequential = inlines.iter().all(|i| {
            let s = i.meta().span;
            let ok = f.valid(s) && s.end > s.start && s.start >= previous;
            previous = s.end;
            ok
        }) && {
            let mut runs_match = true;
            tmark_ir::walk_inlines(inlines, &mut |n: NodeRef| match n {
                NodeRef::Inline(Inline::Str(run)) => {
                    let slice = f.slice(run.meta.span);
                    runs_match &= slice == run.text
                        || (slice.contains('\\') && slice.replace('\\', "") == run.text);
                }
                // A code span or a math span holds its source verbatim,
                // delimiters apart (a line end reads back as a space).
                NodeRef::Inline(Inline::Code(run)) => {
                    runs_match &= f.slice(run.meta.span).contains(run.text.trim());
                }
                NodeRef::Inline(Inline::Math(run)) => {
                    runs_match &= f.slice(run.meta.span).contains(run.text.trim());
                }
                _ => {}
            });
            runs_match
        };
        if sequential {
            let span = Span::new(f.doc.file, first.meta().span.start, last.meta().span.end);
            let mut edits = Vec::new();
            self.inlines(f, inlines, &mut edits);
            return apply(f.slice(span), span.start, edits);
        }
        let mut out = String::new();
        for inline in inlines {
            match self.inline_lowering(f, inline) {
                Some(text) => out.push_str(&text),
                None => out.push_str(&print_node_with(NodeRef::Inline(inline), Profile::Mkdocs)),
            }
        }
        out
    }

    /// The inline rows of the per-construct table; `None` keeps the
    /// inline as written (its children may still splice).
    fn inline_lowering(&mut self, f: &File, inline: &Inline) -> Option<String> {
        if media::inline_skipped(inline, Media::Web) {
            return Some(String::new());
        }
        match inline {
            Inline::Ref(r) => Some(self.reference(f, r)),
            Inline::Link(l) => self.link(f, l),
            Inline::CounterItem(c) => {
                let id = format!("{}:{}", c.prefix, c.key);
                let number = self
                    .numbers
                    .get(&id.to_ascii_lowercase())
                    .cloned()
                    .unwrap_or_else(|| refs::unresolved(&id));
                Some(format!(
                    "<span class=\"{}\" id=\"{}\" data-counter=\"{}\" data-key=\"{}\">{}</span>",
                    self.class("counter"),
                    escape::attr(&id),
                    escape::attr(&c.prefix),
                    escape::attr(&c.key),
                    escape::text(&number)
                ))
            }
            Inline::IndexEntry(e) => {
                let mut out = format!("<span class=\"{}\"", self.class("index"));
                for (i, level) in e.path.iter().enumerate() {
                    let key = if i == 0 {
                        "data-tag".to_string()
                    } else {
                        format!("data-tag{i}")
                    };
                    out.push_str(&format!(" {key}=\"{}\"", escape::attr(&plain_text(level))));
                }
                if let Some(registry) = &e.registry {
                    out.push_str(&format!(" data-registry=\"{}\"", escape::attr(registry)));
                }
                if e.main {
                    out.push_str(" data-main");
                }
                out.push_str("></span>");
                Some(out)
            }
            Inline::SmallCaps(n) => {
                let content = self.inlines_text(f, &n.content);
                Some(format!(
                    "<span class=\"{}\">{content}</span>",
                    self.class("smallcaps")
                ))
            }
            Inline::Underline(n) => {
                let content = self.inlines_text(f, &n.content);
                Some(format!("<u>{content}</u>"))
            }
            Inline::Span(s) => Some(self.span(f, s)),
            Inline::Highlight(_)
            | Inline::Strikeout(_)
            | Inline::Subscript(_)
            | Inline::Superscript(_)
            | Inline::Keystroke(_) => Some(self.sugar(f, inline)),
            Inline::Code(c) if c.lang.is_some() => Some(self.sugar(f, inline)),
            Inline::Aside(a) => Some(self.aside_inline(f, a)),
            Inline::RawInline(r) => Some(if r.format == "html" {
                r.text.clone()
            } else {
                String::new()
            }),
            Inline::Var(v) => self.var(v),
            Inline::Image(image) if image.attrs.media() == Some("web") => {
                Some(self.image_text(f, image, false))
            }
            _ => None,
        }
    }

    /// The `Mkdocs` spelling of a PyMdownX construct (`==x==`, `++ctrl+s++`,
    /// `` `#!py x` ``), or the HTML element when the profile had to keep
    /// the role (which a site would show literally).
    fn sugar(&mut self, f: &File, inline: &Inline) -> String {
        let printed = print_node_with(NodeRef::Inline(inline), Profile::Mkdocs);
        if !printed.starts_with('{') {
            return printed;
        }
        match inline {
            Inline::Highlight(n) => format!("<mark>{}</mark>", self.inlines_text(f, &n.content)),
            Inline::Strikeout(n) => format!("<del>{}</del>", self.inlines_text(f, &n.content)),
            Inline::Subscript(n) => format!("<sub>{}</sub>", self.inlines_text(f, &n.content)),
            Inline::Superscript(n) => format!("<sup>{}</sup>", self.inlines_text(f, &n.content)),
            Inline::Keystroke(n) => {
                let keys: Vec<String> = n
                    .keys
                    .iter()
                    .map(|k| {
                        format!(
                            "<kbd>{}</kbd>",
                            escape::text(&crate::common::text::key_label(k))
                        )
                    })
                    .collect();
                format!("<span class=\"keys\">{}</span>", keys.join("+"))
            }
            Inline::Code(c) => {
                let mut plain = c.clone();
                plain.lang = None;
                print_node_with(NodeRef::Inline(&Inline::Code(plain)), Profile::Mkdocs)
            }
            _ => printed,
        }
    }

    /// Critic markup back to its own spelling: `pymdownx.critic` is in the
    /// standard extension set (spec §Conformance, Table "extensions"), so
    /// the site renders the annotation itself.
    fn critic(&mut self, f: &File, kind: tmark_ir::Critic<'_>) -> String {
        match kind {
            tmark_ir::Critic::Insert(content) => {
                format!("{{++{}++}}", self.inlines_text(f, content))
            }
            tmark_ir::Critic::Delete(content) => {
                format!("{{--{}--}}", self.inlines_text(f, content))
            }
            tmark_ir::Critic::Substitute { old, new } => format!(
                "{{~~{}~>{}~~}}",
                self.inlines_text(f, old),
                self.inlines_text(f, new)
            ),
            tmark_ir::Critic::Comment(text) => format!("{{>>{text}<<}}"),
        }
    }

    /// `[x]{#id .c lang=fr}`: `<span>` with its attributes; `media=web`
    /// unwraps. An anchor on its own keeps its Markdown spelling, the
    /// empty link `[](){#id}`: Python-Markdown stashes raw HTML out of
    /// the element tree, and `mkdocs-autorefs` registers the anchors it
    /// finds *in* that tree, so a `<span id>` would be an id no page of
    /// the site could point at.
    fn span(&mut self, f: &File, s: &SpanNode) -> String {
        if let Some(kind) = tmark_ir::critic(s) {
            return self.critic(f, kind);
        }
        let content = self.inlines_text(f, &s.content);
        if s.attrs.media() == Some("web") {
            return content;
        }
        if s.content.is_empty() && s.attrs.id.is_some() {
            return print_node_with(NodeRef::Inline(&Inline::Span(s.clone())), Profile::Mkdocs);
        }
        let mut out = String::from("<span");
        if let Some(id) = s.attrs.id() {
            out.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        if !s.attrs.classes.is_empty() {
            out.push_str(&format!(
                " class=\"{}\"",
                escape::attr(&s.attrs.classes.join(" "))
            ));
        }
        for (k, v) in &s.attrs.kv {
            match k.as_str() {
                "lang" => out.push_str(&format!(" lang=\"{}\"", escape::attr(v))),
                "media" => {}
                other => out.push_str(&format!(
                    " data-{}=\"{}\"",
                    escape::attr(other),
                    escape::attr(v)
                )),
            }
        }
        out.push('>');
        out.push_str(&content);
        out.push_str("</span>");
        out
    }

    /// `{aside}[…]` inside a paragraph: a span, since an `<aside>` at the
    /// start of a line would open an HTML block.
    fn aside_inline(&mut self, f: &File, aside: &Aside) -> String {
        let mut out = format!("<span class=\"{}\"", self.class("aside"));
        if let Some(side) = aside.side {
            out.push_str(&format!(" data-side=\"{}\"", side_name(side)));
        }
        out.push('>');
        for (i, block) in aside.content.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            match block {
                Block::Para(p) => out.push_str(&self.inlines_text(f, &p.content)),
                Block::Plain(p) => out.push_str(&self.inlines_text(f, &p.content)),
                other => out.push_str(&print_node_with(NodeRef::Block(other), Profile::Mkdocs)),
            }
        }
        out.push_str("</span>");
        out
    }

    /// `{{ key }}`: the front-matter value when it is a scalar.
    fn var(&self, v: &Var) -> Option<String> {
        let mut value = &self.front_matter;
        for key in &v.path {
            value = value.get(key)?;
        }
        match value {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    // -------------------------------------------------------- references

    /// The resolution recorded for `key` on `node`, in this file first.
    fn lookup(&self, f: &File, node: NodeId, key: &str) -> Resolution {
        self.res
            .refs
            .iter()
            .filter(|r| r.node == node && r.key == key)
            .min_by_key(|r| r.span.file != f.doc.file)
            .map(|r| r.resolution.clone())
            .unwrap_or(Resolution::Unresolved)
    }

    /// A link to an anchor, `[text](#id)`, and its deprecated
    /// reference-style spelling `[text][id]` (spec §Ref).
    ///
    /// A same-page anchor keeps its bytes: `#id` is exactly what the
    /// rendered page answers to. A label that lives on another document of
    /// the book is spliced with that document's location —
    /// `[text](other-page.md#id)` — the way `@id` is lowered, so the site
    /// resolves a cross-page reference from the site map and owes
    /// `mkdocs-autorefs` nothing. The reference-style spelling is rewritten
    /// canonically in both cases: `[text][id]` is brackets to a plain
    /// CommonMark parser, and the lowering owes the site Markdown any
    /// parser understands. Where it names no label it is literal text and
    /// keeps its bytes.
    fn link(&mut self, f: &File, l: &tmark_ir::Link) -> Option<String> {
        let (key, reference_style) = match &l.target {
            Target::Anchor(id) => (id, false),
            Target::Reference(id) => (id, true),
            Target::Url(_) | Target::Document(_) => return None,
        };
        let destination = match self.lookup(f, l.meta.id, key) {
            Resolution::Label { .. } if !reference_style => return None,
            // The label *as declared*, the way `@key` is lowered: TMark
            // matches a key case-insensitively and an HTML `id` does not,
            // so `[x][claim]` must address the `{#Claim}` that is there.
            Resolution::Label { .. } => format!(
                "#{}",
                self.res
                    .labels
                    .get(key)
                    .map_or(key.as_str(), |l| l.id.as_str())
            ),
            Resolution::Sibling { label, location } => {
                if l.content.is_empty() {
                    return Some(format!(
                        "[{}]({})",
                        link_text(&label),
                        destination(&location)
                    ));
                }
                destination(&location)
            }
            _ => return None,
        };
        let text = self.inlines_text(f, &l.content);
        Some(format!("[{}]({destination})", link_text(&text)))
    }

    /// `@…` in the spellings of the per-construct table.
    fn reference(&mut self, f: &File, r: &Ref) -> String {
        let items: Vec<(&RefItem, Resolution)> = r
            .items
            .iter()
            .map(|item| (item, self.lookup(f, r.meta.id, &item.key)))
            .collect();
        let all_citations = items
            .iter()
            .all(|(_, res)| matches!(res, Resolution::Citation { .. }));
        if self.opts.citations == Citations::Passthrough && all_citations && !items.is_empty() {
            return pandoc(r, refs::narrative(None, self.main));
        }
        if let Some(text) = self.plural_labels(&items) {
            return text;
        }
        let mut parts: Vec<String> = Vec::new();
        let mut citations: Vec<String> = Vec::new();
        for (item, resolution) in &items {
            let body = match resolution {
                Resolution::Label { prefix, .. } => {
                    let (text, id) = self.label_text(item, prefix.as_deref());
                    format!("[{}](#{})", link_text(&text), id)
                }
                Resolution::Sibling { label, location } => {
                    let text = self.sibling_text(item).unwrap_or_else(|| label.clone());
                    format!("[{}]({})", link_text(&text), destination(location))
                }
                Resolution::Citation { key } => {
                    if self.opts.citations == Citations::Passthrough {
                        citations.push(pandoc_item(item));
                    } else {
                        if !self.citations.contains(key) {
                            self.citations.push(key.clone());
                        }
                        let mut text = String::new();
                        if let Some(prefix) = &item.prefix {
                            text.push_str(prefix);
                            text.push(' ');
                        }
                        text.push_str(&format!(
                            "<a class=\"{}\" href=\"#ref-{}\">{}</a>",
                            self.class("cite"),
                            escape::attr(key),
                            escape::text(&refs::author_year(self.res, key, item.suppress_author))
                        ));
                        if let Some(suffix) = &item.suffix {
                            text.push_str(", ");
                            text.push_str(suffix);
                        }
                        citations.push(text);
                    }
                    continue;
                }
                Resolution::Glossary { term } => {
                    let definition = self.res.glossary.get(term).cloned().unwrap_or_default();
                    let shown = item
                        .key
                        .split_once(':')
                        .map_or(item.key.as_str(), |(_, t)| t);
                    format!(
                        "<abbr title=\"{}\">{}</abbr>",
                        escape::attr(&definition),
                        escape::text(shown)
                    )
                }
                Resolution::Doi { doi } => {
                    format!(
                        "[doi:{}](https://doi.org/{})",
                        link_text(doi),
                        destination(doi)
                    )
                }
                Resolution::External { label, page, .. } => match page {
                    Some(page) => format!("{label} p. {page}"),
                    None => label.clone(),
                },
                Resolution::Ambiguous | Resolution::Unresolved => refs::unresolved(&item.key),
            };
            let mut part = String::new();
            if !matches!(resolution, Resolution::External { .. }) {
                if let Some(prefix) = &item.prefix {
                    part.push_str(prefix);
                    part.push(' ');
                }
            }
            part.push_str(&body);
            if let Some(suffix) = &item.suffix {
                if !matches!(resolution, Resolution::External { .. }) {
                    part.push_str(", ");
                    part.push_str(suffix);
                }
            }
            parts.push(part);
        }
        let mut out = parts.join(if r.bracketed { "; " } else { ", " });
        if !citations.is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            if self.opts.citations == Citations::Passthrough {
                out.push_str(&format!("[{}]", citations.join("; ")));
            } else {
                out.push_str(&format!("({})", citations.join("; ")));
            }
        }
        out
    }

    /// `@[fig:a; fig:b]`: labels of one series → `[Figures 3 and 4](#fig:a)`.
    fn plural_labels(&self, items: &[(&RefItem, Resolution)]) -> Option<String> {
        if items.len() < 2 {
            return None;
        }
        let mut prefix: Option<&str> = None;
        let mut numbers = Vec::new();
        for (item, resolution) in items {
            let Resolution::Label { prefix: p, .. } = resolution else {
                return None;
            };
            let p = p.as_deref()?;
            if item.prefix.is_some() || item.suffix.is_some() || prefix.is_some_and(|q| q != p) {
                return None;
            }
            prefix = Some(p);
            numbers.push(self.numbers.get(&item.key.to_ascii_lowercase())?.clone());
        }
        let prefix = prefix?;
        let (first, _) = items[0];
        let template = refs::reference_template(self.res, prefix);
        let list = match numbers.split_last() {
            Some((last, rest)) if !rest.is_empty() => {
                format!("{} {} {last}", rest.join(", "), self.word("and"))
            }
            _ => numbers.join(", "),
        };
        let text = if template.contains("{name}") {
            let word = refs::label_word(self.res, prefix, &first.key, Some(&self.lang()));
            let plural = match self.lang().as_str() {
                "de" => word,
                _ => format!("{word}s"),
            };
            refs::template(&template, &plural, &list)
        } else {
            list
        };
        let id = self
            .res
            .labels
            .get(&first.key)
            .map(|l| l.id.clone())
            .unwrap_or_else(|| first.key.clone());
        Some(format!("[{}](#{})", link_text(text.trim()), id))
    }

    /// The text and target of a label reference: the heading title for a
    /// section (`SectionRefs::Title`), else the `ref` template over the
    /// label word and the display number.
    fn label_text(&self, item: &RefItem, prefix: Option<&str>) -> (String, String) {
        let label = self.res.labels.get(&item.key);
        let id = label
            .map(|l| l.id.clone())
            .unwrap_or_else(|| item.key.clone());
        let number = self.numbers.get(&item.key.to_ascii_lowercase()).cloned();
        let heading = prefix.and_then(registry::prefix).is_some_and(|p| p.heading);
        if heading && self.opts.sections == SectionRefs::Title {
            if let Some(title) = label.and_then(|l| l.title.as_deref()) {
                if !title.trim().is_empty() {
                    return (title.trim().to_string(), id);
                }
            }
        }
        let text = match (prefix, number) {
            (Some(p), Some(n)) => refs::template(
                &refs::reference_template(self.res, p),
                &refs::label_word(self.res, p, &item.key, Some(&self.lang())),
                &n,
            )
            .trim()
            .to_string(),
            (_, Some(n)) => n,
            (_, None) => label
                .and_then(|l| l.title.clone())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| id.clone()),
        };
        (text, id)
    }

    /// The text of a reference to another page's label, by the rules of
    /// [`Self::label_text`] over the `BookLabel` the site map gave: the
    /// heading title for a section under `SectionRefs::Title`, else the
    /// `ref` template of the series as this page declares it.
    fn sibling_text(&self, item: &RefItem) -> Option<String> {
        let book = self.res.sibling(&item.key)?;
        let prefix = book.prefix.as_deref();
        let heading = book.kind == tmark_registry::Host::Header
            || prefix.and_then(registry::prefix).is_some_and(|p| p.heading);
        let title = book.title.as_deref().filter(|t| !t.trim().is_empty());
        if heading && self.opts.sections == SectionRefs::Title {
            if let Some(title) = title {
                return Some(title.trim().to_string());
            }
        }
        Some(match (prefix, &book.number) {
            (Some(p), Some(n)) => refs::template(
                &refs::reference_template(self.res, p),
                &refs::label_word(self.res, p, &item.key, Some(&self.lang())),
                n,
            )
            .trim()
            .to_string(),
            (_, Some(n)) => n.clone(),
            (_, None) => title.map_or_else(|| book.key.clone(), |t| t.trim().to_string()),
        })
    }

    /// The `References` list of the keys cited inline, in citation order.
    fn bibliography(&self) -> Option<String> {
        if self.citations.is_empty() {
            return None;
        }
        let mut out = format!("<ol class=\"{}\">\n", self.class("bibliography"));
        for key in &self.citations {
            out.push_str(&format!(
                "<li id=\"ref-{}\">{}</li>\n",
                escape::attr(key),
                html::bibliography_entry(self.res, key)
            ));
        }
        out.push_str("</ol>");
        Some(out)
    }
}

// ------------------------------------------------------------- helpers

/// The blocks a float consumes with it: its `yaml table-config` and its
/// caption line, which the IR keeps right after the host.
struct Attached<'a> {
    caption: Option<&'a Caption>,
    consumed: Vec<&'a Meta>,
}

fn attached(blocks: &[Block], i: usize) -> Attached<'_> {
    let block = &blocks[i];
    let mut consumed = Vec::new();
    let mut at = i + 1;
    if let (Block::Table(_), Some(Block::TableConfig(c))) = (block, blocks.get(at)) {
        consumed.push(&c.meta);
        at += 1;
    }
    let caption = match blocks.get(at) {
        Some(Block::Caption(c)) if caption_matches(block, c.kind) => Some(c),
        _ => None,
    };
    if let Some(c) = caption {
        consumed.push(&c.meta);
    } else {
        // A configuration without a caption still goes (print concerns),
        // but on its own turn.
        consumed.clear();
    }
    Attached { caption, consumed }
}

/// The caption kinds a block hosts (design C7).
fn caption_matches(block: &Block, kind: CaptionKind) -> bool {
    match (block, kind) {
        (Block::Table(_), CaptionKind::Table) => true,
        (Block::CodeBlock(_), CaptionKind::Listing) => true,
        (Block::Figure(_), CaptionKind::Figure) => true,
        (Block::Para(p), CaptionKind::Figure) => matches!(p.content.as_slice(), [Inline::Image(_)]),
        _ => false,
    }
}

/// Splices `edits`, all inside `slice` (which starts at `base` in the
/// file), from first to last.
fn apply(slice: &str, base: u32, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|e| (e.span.start, e.span.end));
    let end = base + slice.len() as u32;
    let mut out = String::with_capacity(slice.len());
    let mut at = base;
    for edit in edits {
        if edit.span.start < at || edit.span.end > end {
            continue;
        }
        out.push_str(&slice[(at - base) as usize..(edit.span.start - base) as usize]);
        out.push_str(&edit.text);
        at = edit.span.end;
    }
    out.push_str(&slice[(at - base) as usize..]);
    out
}

/// What the continuation lines of a block starting at `start` carry: the
/// text before it on its line, list markers turned into spaces, `>` kept.
fn line_prefix(text: &str, start: u32) -> String {
    let start = (start as usize).min(text.len());
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    text[line_start..start]
        .chars()
        .map(|c| {
            if c == '>' || c.is_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect()
}

/// Puts `prefix` in front of every line but the first (trimmed on an
/// empty line): a replacement spliced inside a list item or a quote.
fn reindent(text: &str, prefix: &str) -> String {
    if prefix.is_empty() || !text.contains('\n') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(if line.is_empty() {
                prefix.trim_end()
            } else {
                prefix
            });
        }
        out.push_str(line);
    }
    out
}

/// The inverse of [`reindent`]: frees the continuation lines of a source
/// slice from the enclosing prefix.
fn dedent(text: &str, prefix: &str) -> String {
    if prefix.is_empty() || !text.contains('\n') {
        return text.to_string();
    }
    let trimmed = prefix.trim_end();
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line = if i == 0 {
            line
        } else if let Some(rest) = line.strip_prefix(prefix) {
            rest
        } else if let Some(rest) = line.strip_prefix(trimmed) {
            rest
        } else {
            line
        };
        out.push_str(line);
    }
    out
}

/// A name a `!!!` line can carry bare.
fn bare(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
}

/// Link text: brackets escaped.
fn link_text(text: &str) -> String {
    text.replace('[', "\\[").replace(']', "\\]")
}

/// A link destination: `<…>` when it holds whitespace or parentheses.
fn destination(url: &str) -> String {
    if url.contains(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '<' | '>')) {
        format!("<{}>", url.replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

/// Pandoc's spelling of a citation group (`mkdocs-bibtex`). Pandoc's bare
/// `@key` (`@key [locator]`) is the narrative citation, so a bare TMark
/// `@key` is `[@key]` unless `citations.narrative` is on, and a lone
/// `+key` item is Pandoc's bare form (spec §Cite, C51). Inside a group
/// Pandoc has no narrative flag: `+` is dropped there.
fn pandoc(r: &Ref, narrative: bool) -> String {
    if let [item] = r.items.as_slice() {
        let narrative = item.narrative || (narrative && !r.bracketed);
        if narrative && item.prefix.is_none() && !item.suppress_author {
            return match &item.suffix {
                Some(suffix) => format!("@{} [{suffix}]", item.key),
                None => format!("@{}", item.key),
            };
        }
    }
    let items: Vec<String> = r.items.iter().map(pandoc_item).collect();
    format!("[{}]", items.join("; "))
}

fn pandoc_item(item: &RefItem) -> String {
    let mut out = String::new();
    if let Some(prefix) = &item.prefix {
        out.push_str(prefix);
        out.push(' ');
    }
    if item.suppress_author {
        out.push('-');
    }
    out.push('@');
    out.push_str(&item.key);
    if let Some(suffix) = &item.suffix {
        out.push_str(", ");
        out.push_str(suffix);
    }
    out
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "left",
        Side::Right => "right",
        Side::Outer => "outer",
        Side::Inner => "inner",
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indentation_helpers() {
        assert_eq!(line_prefix("- a\n  b", 2), "  ");
        assert_eq!(line_prefix("> - a", 4), ">   ");
        assert_eq!(line_prefix("a", 0), "");
        assert_eq!(reindent("x\ny\n\nz", "  "), "x\n  y\n\n  z");
        assert_eq!(reindent("x\ny", ""), "x\ny");
        assert_eq!(dedent("x\n  y\n\n  z", "  "), "x\ny\n\nz");
        assert_eq!(dedent("x\n> y\n>\n> z", "> "), "x\ny\n\nz");
    }

    #[test]
    fn splices_from_first_to_last() {
        let edits = vec![
            Edit {
                id: NodeId(2),
                span: Span::new(Default::default(), 13, 15),
                text: "B".into(),
            },
            Edit {
                id: NodeId(1),
                span: Span::new(Default::default(), 10, 12),
                text: "A".into(),
            },
        ];
        assert_eq!(apply("aa bb cc", 10, edits), "A B cc");
    }

    #[test]
    fn pandoc_spellings() {
        let mut r = Ref::default();
        r.items.push(RefItem {
            key: "ein05".into(),
            ..Default::default()
        });
        assert_eq!(pandoc(&r, false), "[@ein05]");
        assert_eq!(pandoc(&r, true), "@ein05");
        r.bracketed = true;
        assert_eq!(pandoc(&r, true), "[@ein05]");
        r.items[0].narrative = true;
        assert_eq!(pandoc(&r, false), "@ein05");
        r.items[0].suffix = Some("p. 3".into());
        assert_eq!(pandoc(&r, false), "@ein05 [p. 3]");
        r.items[0].narrative = false;
        r.items[0].suffix = None;
        r.items[0].suffix = Some("p. 3".into());
        r.items.push(RefItem {
            key: "ko20".into(),
            prefix: Some("see".into()),
            suppress_author: true,
            ..Default::default()
        });
        assert_eq!(pandoc(&r, true), "[@ein05, p. 3; see -@ko20]");
    }
}
