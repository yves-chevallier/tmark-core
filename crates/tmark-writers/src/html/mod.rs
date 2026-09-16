//! The HTML writer: the `<article>` innerHTML of a web page (design 07).
//! Plain CommonMark constructs render as the reference implementation
//! does, so the CommonMark suite expectations apply to them; TMark
//! constructs use classes a stylesheet can address. `media=print` nodes are
//! skipped; with `source_map` on, every block carries
//! `data-src="file:start-end"`.

pub mod escape;

use std::collections::BTreeMap;

use tmark_ir::{
    plain_text, Admonition, Align, Aside, Attrs, Block, BlockQuote, BulletList, Caption,
    CaptionKind, Cell, CodeBlock, Column, DefinitionList, Div, Document, Figure, Header, Image,
    Inline, Keystroke, Link, ListItem, MathBlock, Meta, Note, OrderedList, Para, QuoteKind, Ref,
    Row, SpanNode, Table, TableModel, Target, Task,
};
use tmark_registry::{Resolution, Resolved};

use crate::common::{abbr, epigraph, logos, media, refs, text, zero, Out};
use crate::{Backend, Body, Media, Requires, Writer, WriterOptions};

/// The HTML writer.
#[derive(Debug, Default, Clone, Copy)]
pub struct HtmlWriter;

impl Writer for HtmlWriter {
    fn backend(&self) -> Backend {
        Backend::Html
    }

    fn write(&self, doc: &Document, res: &Resolved, opts: &WriterOptions) -> Body {
        let mut w = Html {
            doc,
            res,
            opts,
            out: Out::new(opts.source_map),
            req: Requires::default(),
            numbers: number_labels(res),
            notes: Vec::new(),
            abbr_keys: abbr::keys(doc),
            tex_logos: logos::enabled(doc),
            lang: refs::language(opts.lang.as_deref(), doc, res),
            container: 0,
        };
        w.blocks(&epigraph::blocks(doc));
        w.footnotes();
        w.bibliography();
        w.req.close();
        let (text, map) = w.out.finish();
        Body {
            text,
            map,
            requires: w.req,
        }
    }
}

/// Numbers of every label, by lower-cased id: the resolved number for a
/// TMark-numbered series, a document-order count per prefix otherwise
/// (HTML has no backend counter). A subfigure takes no number of its own:
/// its container's, suffixed with its letter (spec §Image, Figure).
pub(crate) fn number_labels(res: &Resolved) -> BTreeMap<String, String> {
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    let mut out = BTreeMap::new();
    let mut subfigures: Vec<&tmark_registry::Label> = Vec::new();
    for label in &res.labels.in_order {
        let Some(prefix) = &label.prefix else {
            continue;
        };
        if label.subfigure.is_some() {
            subfigures.push(label);
            continue;
        }
        let number = match refs::number(res, prefix, &label.key) {
            Some(n) => n,
            None => {
                if label.host == tmark_registry::Host::Anchor {
                    continue;
                }
                let n = counts.entry(prefix.to_ascii_lowercase()).or_insert(0);
                *n += 1;
                n.to_string()
            }
        };
        out.insert(label.id.to_ascii_lowercase(), number);
    }
    // The container may be labelled by the caption *after* it, so its own
    // number is only known once the pass above is over.
    for label in subfigures {
        let Some(subfigure) = &label.subfigure else {
            continue;
        };
        let Some(parent) = subfigure
            .parent
            .as_deref()
            .and_then(|id| out.get(&id.to_ascii_lowercase()))
        else {
            continue;
        };
        let number = match &subfigure.letter {
            Some(letter) => format!("{parent}{letter}"),
            None => parent.clone(),
        };
        out.insert(label.id.to_ascii_lowercase(), number);
    }
    out
}

struct Html<'a> {
    doc: &'a Document,
    res: &'a Resolved,
    opts: &'a WriterOptions,
    out: Out,
    req: Requires,
    numbers: BTreeMap<String, String>,
    /// Footnotes referenced, in order: label and body.
    notes: Vec<(String, Vec<Block>)>,
    /// Acronym keys substituted in running text (`common::abbr`).
    abbr_keys: Vec<String>,
    /// Feature `typography.tex-logos` (spec §TeX logos).
    pub(crate) tex_logos: bool,
    /// The language of the label words (`refs::language`).
    lang: Option<String>,
    /// Depth inside a container: anything that is not the document's
    /// top-level block sequence (block quote, callout, figure, div, tab,
    /// list item, cell, aside, footnote). A `HorizontalRule` there is
    /// `<hr class="rule" />`, the separator twin of the page-breaking
    /// divider (spec §HorizontalRule, challenge C48).
    container: usize,
}

impl Html<'_> {
    /// Runs `f` one container deep (see `container`).
    fn contained<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.container += 1;
        let out = f(self);
        self.container -= 1;
        out
    }
    fn src(&self, meta: &Meta) -> String {
        if self.opts.source_map {
            format!(
                " data-src=\"{}:{}-{}\"",
                meta.span.file.0, meta.span.start, meta.span.end
            )
        } else {
            String::new()
        }
    }

    /// `id`, `class` and `lang` of an attribute list, plus extra classes.
    fn attrs(&self, attrs: &Attrs, classes: &[&str]) -> String {
        let mut s = String::new();
        if let Some(id) = attrs.id() {
            s.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        let mut all: Vec<&str> = classes.to_vec();
        all.extend(attrs.classes.iter().map(String::as_str));
        if !all.is_empty() {
            s.push_str(&format!(" class=\"{}\"", escape::attr(&all.join(" "))));
        }
        if let Some(lang) = attrs.lang() {
            if self.opts.lang.as_deref() != Some(lang) {
                s.push_str(&format!(" lang=\"{}\"", escape::attr(lang)));
            }
        }
        s
    }

    // ------------------------------------------------------------ blocks

    fn blocks(&mut self, blocks: &[Block]) {
        let mut i = 0;
        while i < blocks.len() {
            let block = &blocks[i];
            if media::block_skipped(block, Media::Web) {
                i += 1;
                continue;
            }
            // A caption right after its host is consumed with it.
            let caption = match blocks.get(i + 1) {
                Some(Block::Caption(c)) if caption_matches(block, c.kind) => Some(c),
                _ => None,
            };
            self.block(block, caption);
            i += if caption.is_some() { 2 } else { 1 };
        }
    }

    fn block(&mut self, block: &Block, caption: Option<&Caption>) {
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
            Block::BulletList(l) => self.bullet_list(l),
            Block::OrderedList(l) => self.ordered_list(l),
            Block::DefinitionList(d) => self.definition_list(d),
            Block::HorizontalRule(_) => self.out.push(if self.container > 0 {
                "<hr class=\"rule\" />\n"
            } else {
                "<hr />\n"
            }),
            Block::Table(t) => self.table(t, caption),
            Block::TableConfig(_) => {}
            Block::Caption(c) => self.caption(c),
            Block::Figure(f) => self.figure(f, caption),
            Block::Admonition(a) => self.admonition(a),
            Block::Div(d) => self.div(d),
            Block::MathBlock(m) => self.math_block(m),
            Block::RawBlock(r) => {
                if r.format == "html" {
                    self.out.push(r.text.trim_end_matches('\n'));
                    self.out.push("\n");
                }
            }
            Block::Include(i) => {
                self.out
                    .push(&format!("<!-- include {} -->\n", escape::text(&i.path)));
                self.req.assets.push(crate::AssetRef {
                    src: i.path.clone(),
                    node: i.meta.id,
                    attrs: Vec::new(),
                });
            }
            Block::Comment(_) => {}
        }
        self.out.end(block.meta().id);
    }

    fn para(&mut self, p: &Para, caption: Option<&Caption>) {
        match p.content.as_slice() {
            [Inline::Image(image)] => {
                // An icon (`Image{.icon}`) is never a figure.
                if !image.attrs.has_class("icon") && (caption.is_some() || !image.attrs.is_empty())
                {
                    self.figure_image(image, caption, &p.meta);
                } else {
                    self.out.push(&format!("<p{}>", self.src(&p.meta)));
                    self.image(image);
                    self.out.push("</p>\n");
                }
                return;
            }
            [Inline::Aside(aside)] => {
                self.aside_block(aside, &p.meta);
                return;
            }
            _ => {}
        }
        self.out.push(&format!("<p{}>", self.src(&p.meta)));
        if let Some(lead) = &p.lead {
            self.out.push("<strong class=\"lead\">");
            self.inlines(lead);
            self.out.push("</strong>");
            if !p.content.is_empty() {
                self.out.push(" ");
            }
        }
        self.inlines(&p.content);
        self.out.push("</p>\n");
    }

    fn header(&mut self, h: &Header) {
        let level = h.level.clamp(1, 6);
        self.out.push(&format!(
            "<h{level}{}{}>",
            self.attrs(&h.attrs, &[]),
            self.src(&h.meta)
        ));
        self.inlines(&h.content);
        self.out.push(&format!("</h{level}>\n"));
    }

    fn code_block(&mut self, c: &CodeBlock, caption: Option<&Caption>) {
        let title = c.options.get("title");
        let framed = caption.is_some() || title.is_some();
        if framed {
            let mut attrs = c.options.clone();
            if attrs.id.is_none() {
                attrs.id = caption.and_then(|c| c.attrs.id.clone());
            }
            self.out.push(&format!(
                "<figure{}{}>\n",
                self.attrs(&attrs, &["listing"]),
                self.src(&c.meta)
            ));
            if let Some(title) = title {
                self.out.push(&format!(
                    "<div class=\"title\">{}</div>\n",
                    escape::text(title)
                ));
            }
        }
        let mut pre = String::from("<pre");
        if !framed {
            pre.push_str(&self.src(&c.meta));
        }
        pre.push_str("><code");
        if let Some(lang) = &c.lang {
            pre.push_str(&format!(" class=\"language-{}\"", escape::attr(lang)));
        }
        if let Some(lines) = c.options.get("linenums") {
            pre.push_str(&format!(" data-linenums=\"{}\"", escape::attr(lines)));
        }
        if let Some(hl) = c.options.get("hl_lines") {
            pre.push_str(&format!(" data-hl-lines=\"{}\"", escape::attr(hl)));
        }
        pre.push('>');
        self.out.push(&pre);
        self.out.push(&escape::text(&c.text));
        if !c.text.is_empty() && !c.text.ends_with('\n') {
            self.out.push("\n");
        }
        self.out.push("</code></pre>\n");
        if framed {
            if let Some(caption) = caption {
                self.figcaption(caption);
            }
            self.out.push("</figure>\n");
        }
    }

    /// `<blockquote>`; an epigraph carries its `source` in a `<footer>`
    /// (spec §BlockQuote), which is where the class expects it.
    fn block_quote(&mut self, q: &BlockQuote) {
        self.out.push(&format!(
            "<blockquote{}{}>\n",
            self.attrs(&q.attrs, &[]),
            self.src(&q.meta)
        ));
        self.contained(|w| w.blocks(&q.content));
        if q.attrs.has_class("epigraph") {
            if let Some(source) = q.attrs.get("source") {
                self.out
                    .push(&format!("<footer>{}</footer>\n", escape::text(source)));
            }
        }
        self.out.push("</blockquote>\n");
    }

    fn bullet_list(&mut self, l: &BulletList) {
        self.out.push(&format!("<ul{}>\n", self.src(&l.meta)));
        self.items(&l.items);
        self.out.push("</ul>\n");
    }

    fn ordered_list(&mut self, l: &OrderedList) {
        let mut open = format!("<ol{}", self.src(&l.meta));
        if l.start != 1 {
            open.push_str(&format!(" start=\"{}\"", l.start));
        }
        if let Some(t) = list_type(l.style) {
            open.push_str(&format!(" type=\"{t}\""));
        }
        open.push_str(">\n");
        self.out.push(&open);
        self.items(&l.items);
        self.out.push("</ol>\n");
    }

    /// A list is tight when no item holds more than one block besides a
    /// trailing nested list (`tmark-fmt` uses the same approximation).
    fn items(&mut self, items: &[ListItem]) {
        self.contained(|w| w.items_body(items));
    }

    fn items_body(&mut self, items: &[ListItem]) {
        let tight = items.iter().all(|i| tight_item(&i.content));
        for item in items {
            self.out.push("<li>");
            if let Some(task) = item.task {
                self.out.push(match task {
                    Task::Open => "<input type=\"checkbox\" disabled /> ",
                    Task::Done => "<input type=\"checkbox\" disabled checked /> ",
                    Task::Partial => "<input type=\"checkbox\" disabled data-partial /> ",
                });
            }
            if tight {
                let mut first = true;
                for block in &item.content {
                    match block {
                        Block::Para(p) if first => self.inlines(&p.content),
                        Block::Plain(p) if first => self.inlines(&p.content),
                        other => {
                            self.out.push("\n");
                            self.block(other, None);
                        }
                    }
                    first = false;
                }
            } else {
                self.out.push("\n");
                self.blocks(&item.content);
            }
            self.out.push("</li>\n");
        }
    }

    fn definition_list(&mut self, d: &DefinitionList) {
        self.contained(|w| w.definition_list_body(d));
    }

    fn definition_list_body(&mut self, d: &DefinitionList) {
        self.out.push(&format!("<dl{}>\n", self.src(&d.meta)));
        for (term, definitions) in &d.items {
            self.out.push("<dt>");
            self.inlines(term);
            self.out.push("</dt>\n");
            for definition in definitions {
                self.out.push("<dd>");
                if tight_item(definition) {
                    if let Some(first) = definition.first() {
                        self.inlines(inline_content(first));
                    }
                    for block in definition.iter().skip(1) {
                        self.out.push("\n");
                        self.block(block, None);
                    }
                } else {
                    self.out.push("\n");
                    self.blocks(definition);
                }
                self.out.push("</dd>\n");
            }
        }
        self.out.push("</dl>\n");
    }

    fn caption(&mut self, c: &Caption) {
        // An orphan caption: a paragraph that says what it is.
        self.out.push(&format!(
            "<p{}{}>{}: ",
            self.attrs(&c.attrs, &["caption"]),
            self.src(&c.meta),
            refs::caption_word(c.kind, self.lang.as_deref())
        ));
        self.inlines(&c.content);
        self.out.push("</p>\n");
    }

    fn figcaption(&mut self, c: &Caption) {
        self.out.begin(c.meta.id);
        self.out.push("<figcaption>");
        if let Some(id) = c.attrs.id() {
            if let Some(number) = self.numbers.get(&id.to_ascii_lowercase()) {
                let word = refs::caption_word(c.kind, self.lang.as_deref());
                self.out.push(&format!(
                    "<span class=\"caption-label\">{word} {number}</span> "
                ));
            }
        }
        self.inlines(&c.content);
        self.out.push("</figcaption>\n");
        self.out.end(c.meta.id);
    }

    fn figure(&mut self, f: &Figure, caption: Option<&Caption>) {
        // The caption may sit inside the container (its last block) or
        // right after it.
        let (content, inner) = match f.content.split_last() {
            Some((Block::Caption(c), rest)) if caption.is_none() => (rest, Some(c)),
            _ => (f.content.as_slice(), None),
        };
        let caption = caption.or(inner);
        let mut attrs = f.attrs.clone();
        if attrs.id.is_none() {
            attrs.id = caption.and_then(|c| c.attrs.id.clone());
        }
        self.out.push(&format!(
            "<figure{}{}>\n",
            self.attrs(&attrs, &[]),
            self.src(&f.meta)
        ));
        // The letters of the sub-figures, spelled as the resolution
        // numbered them (spec §Image, Figure).
        let letters: BTreeMap<tmark_ir::NodeId, String> = match tmark_registry::figure_images(f) {
            images if images.len() > 1 => images
                .iter()
                .enumerate()
                .map(|(n, image)| (image.meta.id, tmark_registry::subfigure_letter(n)))
                .collect(),
            _ => BTreeMap::new(),
        };
        self.container += 1;
        for block in content {
            match block {
                Block::Para(p) => {
                    let images: Vec<&Image> = p
                        .content
                        .iter()
                        .filter_map(|i| match i {
                            Inline::Image(img) => Some(img),
                            _ => None,
                        })
                        .collect();
                    if images.len() == p.content.iter().filter(|i| !is_break(i)).count()
                        && !images.is_empty()
                    {
                        for image in images {
                            self.image(image);
                            self.out.push("\n");
                            if let Some(letter) = letters.get(&image.meta.id) {
                                self.out.push(&format!(
                                    "<span class=\"subcaption\">({letter})</span>\n"
                                ));
                            }
                        }
                        continue;
                    }
                    self.block(block, None);
                }
                other => self.block(other, None),
            }
        }
        self.container -= 1;
        if let Some(caption) = caption {
            self.figcaption(caption);
        }
        self.out.push("</figure>\n");
    }

    fn figure_image(&mut self, image: &Image, caption: Option<&Caption>, meta: &Meta) {
        let mut attrs = Attrs::new();
        attrs.id = caption
            .and_then(|c| c.attrs.id.clone())
            .or_else(|| image.attrs.id.clone());
        self.out.push(&format!(
            "<figure{}{}>\n",
            self.attrs(&attrs, &[]),
            self.src(meta)
        ));
        self.image(image);
        self.out.push("\n");
        if let Some(caption) = caption {
            self.figcaption(caption);
        }
        self.out.push("</figure>\n");
    }

    fn admonition(&mut self, a: &Admonition) {
        let collapsed = a.attrs.get("collapsed");
        let details = collapsed.is_some();
        let tag = if details { "details" } else { "div" };
        let mut open = format!(
            "<{tag}{}{}",
            self.attrs(&a.attrs, &["admonition", &a.kind]),
            self.src(&a.meta)
        );
        if details && collapsed != Some("true") {
            open.push_str(" open");
        }
        open.push_str(">\n");
        self.out.push(&open);
        let title_tag = if details { "summary" } else { "p" };
        self.out
            .push(&format!("<{title_tag} class=\"admonition-title\">"));
        match &a.title {
            Some(title) => self.inlines(title),
            None => {
                let label = tmark_ir::registry::admonition(&a.kind)
                    .map(|a| a.label.to_string())
                    .unwrap_or_else(|| capitalise(&a.kind));
                self.out.push(&escape::text(&label));
            }
        }
        self.out.push(&format!("</{title_tag}>\n"));
        self.contained(|w| w.blocks(&a.content));
        self.out.push(&format!("</{tag}>\n"));
    }

    /// Prose with the TeX logo words as `<span class="tex-logo">` under
    /// `typography.tex-logos` (spec §TeX logos).
    fn prose(&mut self, text: &str) {
        if !self.tex_logos {
            self.out.push(&escape::text(text));
            return;
        }
        for segment in logos::split(text) {
            match segment {
                logos::Segment::Text(t) => self.out.push(&escape::text(t)),
                logos::Segment::Logo(name) => self.out.push(&logos::html(name)),
            }
        }
    }

    /// A tab set (spec §Tabs) in the shape of Material's `tabbed` output:
    /// the labels, then one `tabbed-block` per tab.
    fn tabs(&mut self, d: &Div) {
        self.out.push(&format!(
            "<div{}{}>\n<div class=\"tabbed-labels\">",
            self.attrs(&d.attrs, &["tabbed-set"]),
            self.src(&d.meta)
        ));
        for block in &d.content {
            if let Block::Div(tab) = block {
                let title = tab.attrs.get("title").unwrap_or_default();
                self.out
                    .push(&format!("<label>{}</label>", escape::text(title)));
            }
        }
        self.out.push("</div>\n<div class=\"tabbed-content\">\n");
        for block in &d.content {
            let Block::Div(tab) = block else { continue };
            let mut attrs = tab.attrs.clone();
            attrs.kv.retain(|(k, _)| k != "title");
            self.out.push(&format!(
                "<div{}{}>\n",
                self.attrs(&attrs, &["tabbed-block"]),
                self.src(&tab.meta)
            ));
            self.contained(|w| w.blocks(&tab.content));
            self.out.push("</div>\n");
        }
        self.out.push("</div>\n</div>\n");
    }

    fn div(&mut self, d: &Div) {
        if d.name == "code" {
            // The LaTeX highlight contract (decisions.md X3): nothing to show
            // here, the web highlights natively.
            return;
        }
        if d.name == "tabs" {
            self.tabs(d);
            return;
        }
        // `<div class="name …">` (spec §Div); the `div` container is the
        // hook for classes and an id alone, so its name is not a class.
        let classes: &[&str] = if d.name == "div" { &[] } else { &[&d.name] };
        self.out.push(&format!(
            "<div{}{}>\n",
            self.attrs(&d.attrs, classes),
            self.src(&d.meta)
        ));
        self.contained(|w| w.blocks(&d.content));
        self.out.push("</div>\n");
    }

    fn math_block(&mut self, m: &MathBlock) {
        self.out.push(&format!(
            "<div{}{}>\\[",
            self.attrs(&m.attrs, &["math", "display"]),
            self.src(&m.meta)
        ));
        self.out.push(&escape::text(m.text.trim()));
        self.out.push("\\]</div>\n");
    }

    fn aside_block(&mut self, aside: &Aside, meta: &Meta) {
        let class = match aside.side {
            Some(side) => format!(" class=\"{}\"", side_name(side)),
            None => String::new(),
        };
        self.out
            .push(&format!("<aside{class}{}>\n", self.src(meta)));
        self.contained(|w| w.blocks(&aside.content));
        self.out.push("</aside>\n");
    }

    // ------------------------------------------------------------ tables

    fn table(&mut self, t: &Table, caption: Option<&Caption>) {
        let model = &t.model;
        let mut attrs = t.attrs.clone();
        if attrs.id.is_none() {
            attrs.id = caption.and_then(|c| c.attrs.id.clone());
        }
        self.out.push(&format!(
            "<table{}{}>\n",
            self.attrs(&attrs, &[]),
            self.src(&t.meta)
        ));
        if let Some(caption) = caption {
            self.out.begin(caption.meta.id);
            self.out.push("<caption>");
            self.inlines(&caption.content);
            self.out.push("</caption>\n");
            self.out.end(caption.meta.id);
        }
        let leaves: Vec<_> = model.columns.iter().flat_map(Column::leaves).collect();
        let has_header = leaves.iter().any(|l| l.name.is_some());
        if has_header {
            self.out.push("<thead>\n");
            self.header_rows(model);
            self.out.push("</thead>\n");
        }
        self.out.push("<tbody>\n");
        self.rows(&model.rows, &leaves);
        self.out.push("</tbody>\n");
        if !model.footer.is_empty() {
            self.out.push("<tfoot>\n");
            self.rows(&model.footer, &leaves);
            self.out.push("</tfoot>\n");
        }
        self.out.push("</table>\n");
    }

    /// Header rows, one per level of the column hierarchy: a group spans
    /// its leaves, a leaf shallower than the depth spans the rows below.
    fn header_rows(&mut self, model: &TableModel) {
        let depth = model.header_depth();
        for level in 0..depth {
            self.out.push("<tr>\n");
            self.header_level(&model.columns, level, depth);
            self.out.push("</tr>\n");
        }
    }

    /// The header of a column: its inline Markdown when it carries any
    /// (`LeafColumn::title`), the escaped plain name otherwise.
    fn header_text(&mut self, title: &[Inline], name: Option<&str>) {
        if title.is_empty() {
            self.out.push(&escape::text(name.unwrap_or("")));
        } else {
            self.inlines(title);
        }
    }

    fn header_level(&mut self, columns: &[Column], level: usize, depth: usize) {
        for column in columns {
            match column {
                Column::Leaf(leaf) => {
                    if level == 0 {
                        let span = depth - level;
                        let mut th = String::from("<th");
                        if span > 1 {
                            th.push_str(&format!(" rowspan=\"{span}\""));
                        }
                        th.push_str(&align_attr(leaf.config.align));
                        th.push('>');
                        self.out.push(&th);
                        self.header_text(&leaf.title, leaf.name.as_deref());
                        self.out.push("</th>\n");
                    }
                }
                Column::Group(group) => {
                    if level == 0 {
                        let cols = group
                            .columns
                            .iter()
                            .map(|c| c.leaves().len())
                            .sum::<usize>();
                        self.out.push(&format!(
                            "<th colspan=\"{cols}\"{}>",
                            align_attr(group.config.align)
                        ));
                        self.header_text(&group.title, Some(&group.name));
                        self.out.push("</th>\n");
                    } else {
                        self.header_level(&group.columns, level - 1, depth - 1);
                    }
                }
            }
        }
    }

    fn rows(&mut self, rows: &[Row], leaves: &[&tmark_ir::LeafColumn]) {
        for row in rows {
            match row {
                Row::Separator(s) => {
                    let mut tr = String::from("<tr class=\"separator");
                    if s.double_rule {
                        tr.push_str(" double");
                    }
                    tr.push_str("\">");
                    self.out.push(&tr);
                    if let Some(label) = &s.label {
                        self.out.push(&format!(
                            "<td colspan=\"{}\"><em>{}</em></td>",
                            leaves.len(),
                            escape::text(label)
                        ));
                    }
                    self.out.push("</tr>\n");
                }
                Row::Data(d) => {
                    self.out.push("<tr>\n");
                    for (i, cell) in d.cells.iter().enumerate() {
                        if cell.absorbed {
                            continue;
                        }
                        let align = cell.align.or(leaves.get(i).and_then(|l| l.config.align));
                        self.cell(cell, align);
                    }
                    self.out.push("</tr>\n");
                }
            }
        }
    }

    fn cell(&mut self, cell: &Cell, align: Option<Align>) {
        let mut td = String::from("<td");
        if cell.rows > 1 {
            td.push_str(&format!(" rowspan=\"{}\"", cell.rows));
        }
        if cell.cols > 1 {
            td.push_str(&format!(" colspan=\"{}\"", cell.cols));
        }
        td.push_str(&align_attr(align));
        td.push('>');
        self.out.push(&td);
        self.contained(|w| w.inlines(&cell.content));
        self.out.push("</td>\n");
    }

    // ----------------------------------------------------------- inlines

    fn inlines(&mut self, inlines: &[Inline]) {
        let media = Media::Web;
        let zero = |i: &Inline| is_zero_width(i) || media::inline_skipped(i, media);
        let collapsed = zero::collapse(inlines, &zero);
        for inline in &collapsed {
            if media::inline_skipped(inline, media) {
                continue;
            }
            self.inline(inline);
        }
    }

    fn wrap(&mut self, tag: &str, content: &[Inline]) {
        self.out.push(&format!("<{tag}>"));
        self.inlines(content);
        self.out.push(&format!("</{tag}>"));
    }

    fn inline(&mut self, inline: &Inline) {
        match inline {
            Inline::Str(s) => {
                let keys = std::mem::take(&mut self.abbr_keys);
                for segment in abbr::split(&s.text, &keys) {
                    match segment {
                        abbr::Segment::Text(t) => self.prose(t),
                        abbr::Segment::Abbr(key) => self.abbr(&tmark_ir::Abbr {
                            meta: s.meta,
                            text: key.to_string(),
                        }),
                    }
                }
                self.abbr_keys = keys;
            }
            Inline::Space(_) => self.out.push(" "),
            Inline::SoftBreak(_) => self.out.push("\n"),
            Inline::LineBreak(_) => self.out.push("<br />\n"),
            Inline::Emph(n) => self.wrap("em", &n.content),
            Inline::Strong(n) => self.wrap("strong", &n.content),
            Inline::Strikeout(n) => self.wrap("del", &n.content),
            Inline::Underline(n) => self.wrap("u", &n.content),
            Inline::Highlight(n) => self.wrap("mark", &n.content),
            Inline::Subscript(n) => self.wrap("sub", &n.content),
            Inline::Superscript(n) => self.wrap("sup", &n.content),
            Inline::SmallCaps(n) => self.wrap("span class=\"smallcaps\"", &n.content),
            Inline::Quoted(n) => {
                let (open, close) = match n.kind {
                    QuoteKind::Double => ("“", "”"),
                    QuoteKind::Single => ("‘", "’"),
                };
                self.out.push(open);
                self.inlines(&n.content);
                self.out.push(close);
            }
            Inline::Code(n) => {
                match &n.lang {
                    Some(lang) => self
                        .out
                        .push(&format!("<code class=\"language-{}\">", escape::attr(lang))),
                    None => self.out.push("<code>"),
                }
                self.out.push(&escape::text(&n.text));
                self.out.push("</code>");
            }
            Inline::Math(n) => {
                let (open, close) = if n.display {
                    ("<span class=\"math display\">\\[", "\\]</span>")
                } else {
                    ("<span class=\"math inline\">\\(", "\\)</span>")
                };
                self.out.push(open);
                self.out.push(&escape::text(&n.text));
                self.out.push(close);
            }
            Inline::Link(n) => self.link(n),
            Inline::Ref(n) => self.reference(n),
            Inline::Note(n) => self.note(n),
            Inline::Image(n) => self.image(n),
            Inline::IndexEntry(_) => {}
            Inline::CounterItem(n) => {
                let id = format!("{}:{}", n.prefix, n.key);
                let number = refs::number(self.res, &n.prefix, &n.key)
                    .unwrap_or_else(|| refs::unresolved(&id));
                self.req.counters.insert(n.prefix.to_ascii_lowercase());
                self.out.push(&format!(
                    "<span id=\"{}\" class=\"counter\">{}</span>",
                    escape::attr(&id),
                    escape::text(&number)
                ));
            }
            Inline::Keystroke(n) => self.keystroke(n),
            Inline::Aside(n) => self.aside(n),
            Inline::Span(n) => self.span(n),
            Inline::Var(n) => {
                self.out
                    .push(&format!("{{{{ {} }}}}", escape::text(&n.path.join("."))));
            }
            Inline::Abbr(n) => self.abbr(n),
            Inline::Comment(_) => {}
            Inline::RawInline(n) => {
                if n.format == "html" {
                    self.out.push(&n.text);
                }
            }
            Inline::ProgressBar(n) => self.progress_bar(n),
        }
    }

    /// `<span class="progress thin"><progress value="45" max="100">…
    /// </progress><span class="progress-label">label</span></span>` (spec
    /// §ProgressBar: `<div class="progress">` is PyMdownX's block; the node
    /// is inline, so a span carries the classes).
    fn progress_bar(&mut self, n: &tmark_ir::ProgressBar) {
        let mut attrs = n.attrs.clone();
        attrs.classes.insert(0, "progress".to_string());
        let label = n
            .label
            .clone()
            .unwrap_or_else(|| format!("{}%", n.value_text()));
        self.out.push(&format!(
            "<span{}><progress value=\"{}\" max=\"100\">{}</progress><span class=\"progress-label\">{}</span></span>",
            self.attrs(&attrs, &[]),
            n.value_text(),
            escape::text(&label),
            escape::text(&label)
        ));
    }

    fn link(&mut self, n: &Link) {
        // Spec §Ref: a reference-style link that names no label is the
        // literal text CommonMark makes of it.
        if let Target::Reference(key) = &n.target {
            if !refs::refers(self.res, n.meta.id, key) {
                self.out.begin(n.meta.id);
                self.out.push("[");
                self.inlines(&n.content);
                self.out.push(&format!("][{}]", escape::text(key)));
                self.out.end(n.meta.id);
                return;
            }
        }
        let (href, class) = match &n.target {
            Target::Url(url) => (url.clone(), ""),
            Target::Anchor(id) | Target::Reference(id) => {
                (format!("#{id}"), " class=\"reference\"")
            }
            Target::Document(path) => (path.clone(), " class=\"document\""),
        };
        let mut a = format!("<a href=\"{}\"{class}", escape::attr(&href));
        if let Some(title) = &n.title {
            a.push_str(&format!(" title=\"{}\"", escape::attr(title)));
        }
        a.push('>');
        self.out.begin(n.meta.id);
        self.out.push(&a);
        if let Target::Anchor(id) | Target::Reference(id) = &n.target {
            // A textual reference: the web template.
            let mut buf = std::mem::replace(&mut self.out, Out::scratch());
            self.inlines(&n.content);
            std::mem::swap(&mut self.out, &mut buf);
            let text = buf.finish_text();
            let number = self
                .numbers
                .get(&id.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default();
            self.out.push(&refs::textual(
                &self.opts.refs.textual_web,
                &text,
                &number,
                "",
            ));
        } else {
            self.inlines(&n.content);
        }
        self.out.push("</a>");
        self.out.end(n.meta.id);
    }

    fn reference(&mut self, n: &Ref) {
        self.out.begin(n.meta.id);
        let mut citations: Vec<String> = Vec::new();
        let mut written = 0;
        for item in &n.items {
            let resolution = refs::lookup(self.res, n.meta.id, &item.key)
                .map(|r| r.resolution.clone())
                .unwrap_or(Resolution::Unresolved);
            if !matches!(resolution, Resolution::Citation { .. }) {
                if written > 0 {
                    self.out.push(if n.bracketed { "; " } else { ", " });
                }
                written += 1;
            }
            match resolution {
                Resolution::Label { prefix, .. } => {
                    if let Some(prefix) = &item.prefix {
                        self.out.push(&escape::text(prefix));
                        self.out.push(" ");
                    }
                    let number = self
                        .numbers
                        .get(&item.key.to_ascii_lowercase())
                        .cloned()
                        .unwrap_or_else(|| "?".to_string());
                    // An anchor with no number shows its text (spec
                    // §Anchor, `ref-unnumbered`).
                    let unnumbered = refs::unnumbered_text(self.res, &item.key);
                    let text = match (&prefix, unnumbered) {
                        (_, Some(text)) => text,
                        (Some(p), None) => refs::template(
                            &refs::reference_template(self.res, p),
                            &refs::label_word(self.res, p, &item.key, self.lang.as_deref()),
                            &number,
                        ),
                        (None, None) => number,
                    };
                    self.out.push(&format!(
                        "<a href=\"#{}\" class=\"reference\">{}</a>",
                        escape::attr(&item.key),
                        escape::text(text.trim())
                    ));
                    if let Some(suffix) = &item.suffix {
                        self.out.push(", ");
                        self.out.push(&escape::text(suffix));
                    }
                }
                Resolution::Citation { key } => {
                    self.req.cite(&key);
                    let mut text = String::new();
                    if let Some(prefix) = &item.prefix {
                        text.push_str(prefix);
                        text.push(' ');
                    }
                    text.push_str(&format!(
                        "<a href=\"#ref-{}\">{}</a>",
                        escape::attr(&key),
                        escape::text(&refs::author_year(self.res, &key, item.suppress_author))
                    ));
                    if let Some(suffix) = &item.suffix {
                        text.push_str(", ");
                        text.push_str(&escape::text(suffix));
                    }
                    citations.push(text);
                }
                Resolution::Glossary { term } => {
                    let expansion = self.res.glossary.get(&term).cloned().unwrap_or_default();
                    self.req.fragment("ts-glossary");
                    self.out.push(&format!(
                        "<a href=\"#gls-{}\" class=\"glossary\" title=\"{}\">{}</a>",
                        escape::attr(&term),
                        escape::attr(&expansion),
                        escape::text(&term)
                    ));
                }
                Resolution::Doi { doi } => {
                    self.out.push(&format!(
                        "<a href=\"https://doi.org/{}\">doi:{}</a>",
                        escape::attr(&doi),
                        escape::text(&doi)
                    ));
                }
                Resolution::Sibling { label, location } => {
                    // Another document of the book: link to it as given
                    // (design 06 §Site-wide resolution).
                    self.out.push(&format!(
                        "<a href=\"{}\" class=\"reference\">{}</a>",
                        escape::attr(&location),
                        escape::text(&label)
                    ));
                }
                Resolution::External { label, page, .. } => {
                    self.out.push(&escape::text(&label));
                    if let Some(page) = page {
                        self.out.push(&format!(" p. {page}"));
                    }
                }
                Resolution::Ambiguous | Resolution::Unresolved => {
                    self.out.push(&format!(
                        "<span class=\"unresolved\">{}</span>",
                        escape::text(&refs::unresolved(&item.key))
                    ));
                }
            }
        }
        if !citations.is_empty() {
            self.out.push("<span class=\"citation\">(");
            self.out.push(&citations.join("; "));
            self.out.push(")</span>");
        }
        self.out.end(n.meta.id);
    }

    fn note(&mut self, n: &Note) {
        let (label, content) = match &n.label {
            Some(label) => {
                let body = self
                    .doc
                    .footnotes
                    .iter()
                    .find(|f| f.label == *label)
                    .map(|f| f.content.clone())
                    .unwrap_or_default();
                (label.clone(), body)
            }
            None => ((self.notes.len() + 1).to_string(), n.content.clone()),
        };
        let index = match self.notes.iter().position(|(l, _)| *l == label) {
            Some(i) => i + 1,
            None => {
                self.notes.push((label.clone(), content));
                self.notes.len()
            }
        };
        self.out.push(&format!(
            "<sup class=\"footnote-ref\"><a href=\"#fn-{}\" id=\"fnref-{}\">{index}</a></sup>",
            escape::attr(&label),
            escape::attr(&label)
        ));
    }

    fn image(&mut self, n: &Image) {
        self.out.begin(n.meta.id);
        self.req.assets.push(crate::AssetRef {
            src: n.src.clone(),
            node: n.meta.id,
            attrs: n.attrs.kv.clone(),
        });
        if n.src.is_empty() {
            if let Some(lang) = n.attrs.get("generate") {
                // A generated diagram: the source, for a client-side renderer.
                self.out.push(&format!(
                    "<pre class=\"{}\">{}</pre>",
                    escape::attr(lang),
                    escape::text(n.attrs.get("code").unwrap_or_default())
                ));
            }
            self.out.end(n.meta.id);
            return;
        }
        let mut img = format!("<img src=\"{}\"", escape::attr(&n.src));
        img.push_str(&format!(" alt=\"{}\"", escape::attr(&plain_text(&n.alt))));
        if let Some(id) = n.attrs.id() {
            img.push_str(&format!(" id=\"{}\"", escape::attr(id)));
        }
        if let Some(width) = n.attrs.get("width") {
            img.push_str(&format!(" width=\"{}\"", escape::attr(width)));
        }
        if !n.attrs.classes.is_empty() {
            img.push_str(&format!(
                " class=\"{}\"",
                escape::attr(&n.attrs.classes.join(" "))
            ));
        }
        img.push_str(" />");
        self.out.push(&img);
        self.out.end(n.meta.id);
    }

    /// `<abbr title="expansion">KEY</abbr>` when the term has an expansion.
    fn abbr(&mut self, n: &tmark_ir::Abbr) {
        let expansion = self
            .doc
            .abbreviations
            .iter()
            .find(|a| a.key == n.text)
            .map(|a| a.expansion.clone())
            .or_else(|| self.res.glossary.get(&n.text.to_ascii_lowercase()).cloned());
        match expansion {
            Some(e) => self.out.push(&format!(
                "<abbr title=\"{}\">{}</abbr>",
                escape::attr(&e),
                escape::text(&n.text)
            )),
            None => self.out.push(&escape::text(&n.text)),
        }
    }

    fn keystroke(&mut self, n: &Keystroke) {
        let keys: Vec<String> = n
            .keys
            .iter()
            .map(|k| format!("<kbd>{}</kbd>", escape::text(&text::key_label(k))))
            .collect();
        self.out.push(&keys.join("+"));
    }

    fn aside(&mut self, n: &Aside) {
        let class = match n.side {
            Some(side) => format!(" class=\"{}\"", side_name(side)),
            None => String::new(),
        };
        self.out.push(&format!("<span role=\"note\"{class}>"));
        self.container += 1;
        for (i, block) in n.content.iter().enumerate() {
            if i > 0 {
                self.out.push(" ");
            }
            match block {
                Block::Para(_) | Block::Plain(_) => self.inlines(inline_content(block)),
                other => self.block(other, None),
            }
        }
        self.container -= 1;
        self.out.push("</span>");
    }

    /// Critic markup: the `ins`, `del` elements the annotation means, and a
    /// zero-width span for a comment — a reviewer's note is not published
    /// content on the web, but a stylesheet can reveal `data-comment`.
    fn critic(&mut self, kind: tmark_ir::Critic<'_>) {
        match kind {
            tmark_ir::Critic::Insert(content) => self.wrap_class("ins", "critic", content),
            tmark_ir::Critic::Delete(content) => self.wrap_class("del", "critic", content),
            tmark_ir::Critic::Substitute { old, new } => {
                self.wrap_class("del", "critic", old);
                self.wrap_class("ins", "critic", new);
            }
            tmark_ir::Critic::Comment(text) => self.out.push(&format!(
                "<span class=\"critic comment\" data-comment=\"{}\"></span>",
                escape::attr(text)
            )),
        }
    }

    fn wrap_class(&mut self, tag: &str, class: &str, content: &[Inline]) {
        self.out.push(&format!("<{tag} class=\"{class}\">"));
        self.inlines(content);
        self.out.push(&format!("</{tag}>"));
    }

    fn span(&mut self, n: &SpanNode) {
        if let Some(kind) = tmark_ir::critic(n) {
            self.critic(kind);
            return;
        }
        let mut open = String::from("<span");
        open.push_str(&self.attrs(&n.attrs, &[]));
        for (k, v) in &n.attrs.kv {
            if matches!(k.as_str(), "lang" | "media") {
                continue;
            }
            open.push_str(&format!(
                " data-{}=\"{}\"",
                escape::attr(k),
                escape::attr(v)
            ));
        }
        open.push('>');
        self.out.push(&open);
        self.inlines(&n.content);
        self.out.push("</span>");
    }

    // -------------------------------------------------------- end matter

    fn footnotes(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        let notes = std::mem::take(&mut self.notes);
        self.out.push("<section class=\"footnotes\">\n<ol>\n");
        for (label, content) in &notes {
            self.out
                .push(&format!("<li id=\"fn-{}\">", escape::attr(label)));
            if tight_item(content) {
                if let Some(first) = content.first() {
                    self.inlines(inline_content(first));
                }
            } else {
                self.out.push("\n");
                self.contained(|w| w.blocks(content));
            }
            self.out.push(&format!(
                " <a href=\"#fnref-{}\" class=\"footnote-backref\">↩</a></li>\n",
                escape::attr(label)
            ));
        }
        self.out.push("</ol>\n</section>\n");
    }

    fn bibliography(&mut self) {
        if self.req.citations.is_empty() {
            return;
        }
        let keys = self.req.citations.clone();
        self.out.push("<section class=\"bibliography\">\n<ol>\n");
        for key in &keys {
            self.out.push(&format!(
                "<li id=\"ref-{}\">{}</li>\n",
                escape::attr(key),
                bibliography_entry(self.res, key)
            ));
        }
        self.out.push("</ol>\n</section>\n");
    }
}

/// One entry of the built-in author-year bibliography (design 07
/// §Mapping rules, "Citations"): the fields joined, the DOI linked; the
/// key alone when the bibliography has no record.
pub(crate) fn bibliography_entry(res: &Resolved, key: &str) -> String {
    let Some(entry) = res.bibliography.get(key) else {
        return escape::text(key);
    };
    let field = |name: &str| entry.fields.get(name).cloned();
    let mut parts = Vec::new();
    if let Some(author) = field("author") {
        parts.push(escape::text(&author.replace(" and ", ", ")));
    }
    if let Some(year) = field("year").or_else(|| field("date")) {
        parts.push(format!("({})", escape::text(&year)));
    }
    if let Some(title) = field("title") {
        parts.push(format!("<em>{}</em>", escape::text(&title)));
    }
    if let Some(journal) = field("journal").or_else(|| field("journaltitle")) {
        parts.push(escape::text(&journal));
    }
    if let Some(doi) = field("doi") {
        parts.push(format!(
            "<a href=\"https://doi.org/{}\">doi:{}</a>",
            escape::attr(&doi),
            escape::text(&doi)
        ));
    }
    if parts.is_empty() {
        parts.push(escape::text(key));
    }
    parts.join(". ")
}

/// The inline content of a `Para` or `Plain`; empty for other blocks.
fn inline_content(block: &Block) -> &[Inline] {
    match block {
        Block::Para(p) => &p.content,
        Block::Plain(p) => &p.content,
        _ => &[],
    }
}

fn is_break(inline: &Inline) -> bool {
    matches!(
        inline,
        Inline::SoftBreak(_) | Inline::LineBreak(_) | Inline::Space(_)
    )
}

/// One paragraph, optionally followed by nested lists only.
fn tight_item(blocks: &[Block]) -> bool {
    match blocks.split_first() {
        None => true,
        Some((Block::Para(_) | Block::Plain(_), rest)) => rest
            .iter()
            .all(|b| matches!(b, Block::BulletList(_) | Block::OrderedList(_))),
        _ => false,
    }
}

/// The caption kinds a block hosts (design C7).
fn caption_matches(block: &Block, kind: CaptionKind) -> bool {
    match (block, kind) {
        (Block::Table(_), CaptionKind::Table) => true,
        (Block::CodeBlock(_), CaptionKind::Listing) => true,
        (Block::Div(d), CaptionKind::Listing) => d.name == "code",
        (Block::Figure(_), CaptionKind::Figure) => true,
        (Block::Para(p), CaptionKind::Figure) => {
            matches!(p.content.as_slice(), [Inline::Image(_)])
        }
        _ => false,
    }
}

/// Zero-width inlines (spec §Attributes): comments, index entries, asides,
/// spans that are only an anchor, raw text of another format.
pub fn is_zero_width(inline: &Inline) -> bool {
    match inline {
        Inline::Comment(_) | Inline::IndexEntry(_) | Inline::Aside(_) => true,
        // A critic comment is a reviewer's note: zero width on the web.
        Inline::Span(s) => {
            (s.content.is_empty() && s.attrs.id.is_some())
                || matches!(tmark_ir::critic(s), Some(tmark_ir::Critic::Comment(_)))
        }
        Inline::RawInline(r) => r.format != "html",
        _ => false,
    }
}

pub(crate) fn align_attr(align: Option<Align>) -> String {
    match align {
        Some(Align::Left) => " style=\"text-align: left\"".to_string(),
        Some(Align::Center) => " style=\"text-align: center\"".to_string(),
        Some(Align::Right) => " style=\"text-align: right\"".to_string(),
        Some(Align::Justify) => " style=\"text-align: justify\"".to_string(),
        None => String::new(),
    }
}

fn side_name(side: tmark_ir::Side) -> &'static str {
    match side {
        tmark_ir::Side::Left => "left",
        tmark_ir::Side::Right => "right",
        tmark_ir::Side::Outer => "outer",
        tmark_ir::Side::Inner => "inner",
    }
}

/// The `type` attribute of an ordered list (`pymdownx.fancylists`).
fn list_type(style: tmark_ir::ListStyle) -> Option<&'static str> {
    use tmark_ir::ListStyle;
    match style {
        ListStyle::Decimal | ListStyle::Generic => None,
        ListStyle::LowerAlpha => Some("a"),
        ListStyle::UpperAlpha => Some("A"),
        ListStyle::LowerRoman => Some("i"),
        ListStyle::UpperRoman => Some("I"),
    }
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
