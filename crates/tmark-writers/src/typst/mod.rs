//! The Typst writer: the content after a template's `#show` rules (design
//! 07). It mirrors the LaTeX writer construct for construct; contract
//! functions are the `#ts-*` names of `texsmith.typ` (fragment-contracts.md
//! §1, §3 rule 7), math goes through `mitex` (writers-and-passes.md §4).

pub mod escape;
mod inline;
pub mod math;
mod table;

use std::collections::BTreeMap;

use tmark_ir::{
    Admonition, Attrs, Block, BlockQuote, Caption, CaptionKind, CodeBlock, DefinitionList, Div,
    Document, Figure, Header, Image, Inline, ListItem, ListStyle, MathBlock, OrderedList, Para,
    RawBlock, Task,
};
use tmark_registry::Resolved;

use crate::common::{abbr, logos, media, refs, Out};
use crate::{Backend, Body, Media, Requires, Writer, WriterOptions};

/// The Typst writer.
#[derive(Debug, Default, Clone, Copy)]
pub struct TypstWriter;

impl Writer for TypstWriter {
    fn backend(&self) -> Backend {
        Backend::Typst
    }

    fn write(&self, doc: &Document, res: &Resolved, opts: &WriterOptions) -> Body {
        let mut w = Typst {
            doc,
            res,
            opts,
            out: Out::new(opts.source_map),
            req: Requires::default(),
            acronyms: BTreeMap::new(),
            abbr_keys: abbr::keys(doc),
            tex_logos: logos::enabled(doc),
            lang: refs::language(opts.lang.as_deref(), doc, res),
            narrative: refs::narrative(opts.citations.narrative, doc),
            container: 0,
        };
        w.blocks(&doc.blocks);
        w.req.close();
        let (text, map) = w.out.finish();
        Body {
            text,
            map,
            requires: w.req,
        }
    }
}

pub(crate) struct Typst<'a> {
    doc: &'a Document,
    res: &'a Resolved,
    opts: &'a WriterOptions,
    out: Out,
    req: Requires,
    /// Acronym term → `#ts-acr` key, first seen first (same rule as LaTeX).
    acronyms: BTreeMap<String, String>,
    abbr_keys: Vec<String>,
    /// Feature `typography.tex-logos` (spec §TeX logos).
    pub(crate) tex_logos: bool,
    /// The language of the label words (`refs::language`).
    lang: Option<String>,
    /// A bare `@key` takes `form: "prose"` (`refs::narrative`).
    narrative: bool,
    /// Depth inside a container: anything that is not the document's
    /// top-level block sequence (block quote, callout, figure, div, tab,
    /// list item, cell, aside, footnote). A `HorizontalRule` there is
    /// `#ts-rule()`: Typst refuses a page break inside a container
    /// (spec §HorizontalRule, challenge C48).
    container: usize,
}

impl Typst<'_> {
    /// Runs `f` one container deep (see `container`).
    fn contained<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.container += 1;
        let out = f(self);
        self.container -= 1;
        out
    }

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
            Block::HorizontalRule(_) => {
                self.req.fragment("ts-typesetting");
                self.out.push(if self.container > 0 {
                    "#ts-rule()\n"
                } else {
                    "#ts-divider()\n"
                });
            }
            Block::Table(t) => self.table(t, config, caption),
            Block::TableConfig(_) => {}
            Block::Caption(c) => {
                let word = refs::caption_word(c.kind, self.lang.as_deref());
                self.out.push(&format!("{word}: "));
                self.inlines(&c.content);
                if let Some(id) = c.attrs.id() {
                    self.out.push(&format!(" <{}>", escape::label(id)));
                }
                self.out.ensure_newline();
            }
            Block::Figure(f) => self.figure(f, caption),
            Block::Admonition(a) => self.admonition(a),
            Block::Div(d) => self.div(d, caption),
            Block::MathBlock(m) => self.math_block(m),
            Block::RawBlock(r) => self.raw_block(r),
            Block::Include(i) => {
                let stem = i.path.strip_suffix(".md").unwrap_or(&i.path);
                self.req.assets.push(crate::AssetRef {
                    src: i.path.clone(),
                    node: i.meta.id,
                    attrs: Vec::new(),
                });
                self.out
                    .push(&format!("#include \"{}.typ\"\n", escape::string(stem)));
            }
            Block::Comment(_) => {}
        }
        self.out.end(block.meta().id);
    }

    fn para(&mut self, p: &Para, caption: Option<&Caption>) {
        match p.content.as_slice() {
            // An icon (`Image{.icon}`) is never a figure.
            [Inline::Image(image)] if !image.attrs.has_class("icon") => {
                self.figure_image(image, caption, None);
                return;
            }
            [Inline::Link(link)] if matches!(link.content.as_slice(), [Inline::Image(_)]) => {
                if let (Inline::Image(image), tmark_ir::Target::Url(url)) =
                    (&link.content[0], &link.target)
                {
                    self.figure_image(image, caption, Some(url));
                    return;
                }
            }
            _ => {}
        }
        if let Some(lead) = &p.lead {
            self.req.fragment("ts-typesetting");
            self.out.push("#ts-lead[");
            self.inlines(lead);
            self.out.push("] ");
        }
        self.inlines(&p.content);
        self.out.trim_trailing_spaces();
        self.out.ensure_newline();
    }

    /// `= Title <id>`; `#heading(numbering: none, …)` when not numbered,
    /// per document or per heading (`.unnumbered` keeps the outline entry,
    /// `.unlisted` drops it: spec §Header); the label is the explicit id,
    /// or the implicit id when a reference targets it.
    fn header(&mut self, h: &Header) {
        let level = (i32::from(h.level) + i32::from(self.opts.headings.base_level) - 1).max(1);
        let title = self.render_inlines(&h.content);
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let label = match h.attrs.id() {
            Some(id) => Some(escape::label(id)),
            None => refs::referenced_implicit_id(self.res, h.meta.id).map(escape::label),
        };
        let unlisted = h.attrs.has_class("unlisted");
        let unnumbered = unlisted || h.attrs.has_class("unnumbered");
        if self.opts.headings.numbered && !unnumbered {
            self.out
                .push(&format!("{} {title}", "=".repeat(level as usize)));
        } else {
            let outlined = if unlisted || !self.opts.headings.numbered {
                ", outlined: false"
            } else {
                ""
            };
            self.out.push(&format!(
                "#heading(level: {level}, numbering: none{outlined})[{title}]"
            ));
        }
        if let Some(label) = label.filter(|l| !l.is_empty()) {
            self.out.push(&format!(" <{label}>"));
        }
        self.out.ensure_newline();
    }

    /// A bare fence when the block carries no option, else `#ts-code(…)`
    /// around the fence with the label after the call.
    fn code_block(&mut self, c: &CodeBlock, caption: Option<&Caption>) {
        let body = c.text.trim_end_matches('\n');
        let fence = escape::fence_for(body);
        let lang = c.lang.as_deref().filter(|l| *l != "text").unwrap_or("");
        let args = self.code_args(&c.options, caption);
        let labelled = c.options.id().is_some() || caption.is_some_and(|c| c.attrs.id().is_some());
        if args.is_empty() && !labelled {
            self.out.push(&format!("{fence}{lang}\n{body}\n{fence}\n"));
            return;
        }
        self.req.fragment("ts-code");
        self.out.push(&format!("#ts-code({})[\n", args.join(", ")));
        self.out.push(&format!("{fence}{lang}\n{body}\n{fence}\n"));
        self.out.push("]");
        // The label attaches to the element the call returns.
        if let Some(id) = c.options.id().or(caption.and_then(|c| c.attrs.id())) {
            self.out.push(&format!(" <{}>", escape::label(id)));
        }
        self.out.push("\n");
    }

    /// `title`, `linenums`, `hl_lines`, `id`, the caption, other attributes.
    fn code_args(&mut self, options: &Attrs, caption: Option<&Caption>) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        if let Some(title) = options.get("title") {
            args.push(format!("title: [{}]", escape::markup(title)));
        }
        if let Some(lines) = options.get("linenums") {
            match lines.trim() {
                "" | "1" | "true" | "yes" => args.push("linenums: 1".to_string()),
                "false" | "no" => {}
                start => args.push(format!("linenums: {}", escape::string(start))),
            }
        }
        if let Some(hl) = options.get("hl_lines") {
            let ranges: Vec<String> = hl
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|s| !s.is_empty())
                .map(|r| format!("\"{}\"", escape::string(r)))
                .collect();
            if !ranges.is_empty() {
                args.push(format!("hl-lines: ({},)", ranges.join(", ")));
            }
        }
        if let Some(caption) = caption {
            let text = self.render_inlines(&caption.content);
            args.push(format!("caption: [{text}]"));
        }
        if !options.classes.is_empty() {
            let classes: Vec<String> = options
                .classes
                .iter()
                .map(|c| format!("\"{}\"", escape::string(c)))
                .collect();
            args.push(format!("class: ({},)", classes.join(", ")));
        }
        for (k, v) in &options.kv {
            if matches!(
                k.as_str(),
                "lang" | "title" | "linenums" | "hl_lines" | "include" | "media" | "engine"
            ) {
                continue;
            }
            args.push(format!("{}: \"{}\"", escape::label(k), escape::string(v)));
        }
        args
    }

    fn block_quote(&mut self, q: &BlockQuote) {
        if q.attrs.has_class("epigraph") {
            self.epigraph(&q.content, q.attrs.get("source"));
            return;
        }
        self.out.push("#quote(block: true)[\n");
        self.contained(|w| w.blocks(&q.content));
        self.out.ensure_newline();
        self.out.push("]\n");
    }

    fn epigraph(&mut self, content: &[Block], source: Option<&str>) {
        self.req.fragment("ts-typesetting");
        self.out.push("#ts-epigraph");
        if let Some(source) = source {
            self.out
                .push(&format!("(source: [{}])", escape::markup(source)));
        }
        self.out.push("[");
        let body = self.render_blocks_inline(content);
        self.out.push(&body);
        self.out.push("]\n");
    }

    /// `- item`; a task item wraps its content in `#ts-task("done")[…]`.
    fn bullet_list(&mut self, items: &[ListItem]) {
        for item in items {
            self.out.push("- ");
            match item.task {
                Some(task) => {
                    self.req.fragment("ts-todolist");
                    let state = match task {
                        Task::Open => "open",
                        Task::Done => "done",
                        Task::Partial => "partial",
                    };
                    self.out.push(&format!("#ts-task(\"{state}\")["));
                    self.item_body(&item.content);
                    self.out.push("]");
                }
                None => self.item_body(&item.content),
            }
            self.out.ensure_newline();
        }
    }

    /// `+ item`, explicit numbers for another start, a `#set enum` for a
    /// numbering style.
    fn ordered_list(&mut self, l: &OrderedList) {
        let numbering = match l.style {
            ListStyle::Decimal | ListStyle::Generic => None,
            ListStyle::LowerAlpha => Some("a."),
            ListStyle::UpperAlpha => Some("A."),
            ListStyle::LowerRoman => Some("i."),
            ListStyle::UpperRoman => Some("I."),
        };
        if let Some(numbering) = numbering {
            self.out
                .push(&format!("#[\n#set enum(numbering: \"{numbering}\")\n"));
        }
        for (i, item) in l.items.iter().enumerate() {
            if l.start != 1 {
                self.out.push(&format!("{}. ", l.start + i as u32));
            } else {
                self.out.push("+ ");
            }
            self.item_body(&item.content);
            self.out.ensure_newline();
        }
        if numbering.is_some() {
            self.out.push("]\n");
        }
    }

    /// The first paragraph on the marker's line, the rest indented.
    fn item_body(&mut self, content: &[Block]) {
        self.contained(|w| w.item_body_inner(content));
    }

    fn item_body_inner(&mut self, content: &[Block]) {
        self.out.push_prefix("  ");
        let (first, rest) = match content.split_first() {
            Some((Block::Para(p), rest)) => {
                if let Some(lead) = &p.lead {
                    self.req.fragment("ts-typesetting");
                    self.out.push("#ts-lead[");
                    self.inlines(lead);
                    self.out.push("] ");
                }
                self.inlines(&p.content);
                (true, rest)
            }
            Some((Block::Plain(p), rest)) => {
                self.inlines(&p.content);
                (true, rest)
            }
            _ => (false, content),
        };
        self.out.trim_trailing_spaces();
        for block in rest {
            if media::block_skipped(block, Media::Print) {
                continue;
            }
            if first || matches!(block, Block::BulletList(_) | Block::OrderedList(_)) {
                self.out.ensure_newline();
            } else {
                self.out.blank_line();
            }
            self.block(block, None, None);
        }
        self.out.pop_prefix();
    }

    /// `/ term: definition`.
    fn definition_list(&mut self, d: &DefinitionList) {
        for (term, definitions) in &d.items {
            let term = self.render_inlines(term);
            if definitions.is_empty() {
                self.out.push(&format!("/ {term}: \n"));
            }
            for definition in definitions {
                self.out.push(&format!("/ {term}: "));
                self.item_body(definition);
                self.out.ensure_newline();
            }
        }
    }

    /// `#ts-callout(kind: "note", title: […], id: "x", collapsed: true)[…]`.
    fn admonition(&mut self, a: &Admonition) {
        self.req.fragment("ts-callouts");
        let mut args = vec![format!("kind: \"{}\"", escape::string(&a.kind))];
        if let Some(title) = &a.title {
            let title = self.render_inlines(title);
            args.push(format!("title: [{title}]"));
        }
        args.extend(attr_args(&a.attrs, &["collapsed"]));
        self.out
            .push(&format!("#ts-callout({})[\n", args.join(", ")));
        self.contained(|w| w.blocks(&a.content));
        self.out.ensure_newline();
        self.out.push("]\n");
    }

    /// `Div{name}`: `epigraph`, `code` (the fence when the highlight pass
    /// kept it), else `#ts-div("name", key: value)[…]`.
    fn div(&mut self, d: &Div, caption: Option<&Caption>) {
        match d.name.as_str() {
            "epigraph" => self.epigraph(&d.content, d.attrs.get("source")),
            "code" => {
                // The pygments payload is LaTeX; fall back to the fence
                // when the pass kept it.
                for block in &d.content {
                    if let Block::CodeBlock(c) = block {
                        self.code_block(c, caption);
                    }
                }
            }
            // Print has no interaction: the tabs follow each other (spec §Tabs).
            "tabs" => self.contained(|w| w.blocks(&d.content)),
            name => {
                self.req.fragment("ts-typesetting");
                let mut args = vec![format!("\"{}\"", escape::string(name))];
                args.extend(attr_args(&d.attrs, &[]));
                self.out.push(&format!("#ts-div({})[\n", args.join(", ")));
                self.contained(|w| w.blocks(&d.content));
                self.out.ensure_newline();
                self.out.push("]");
                // The id is a label a reference can reach (spec §Anchor);
                // `ts-div` only forwards its body, so the writer attaches
                // it (typst 0.15 attaches a label to a call's content).
                if let Some(id) = d.attrs.id() {
                    self.out.push(&format!(" <{}>", escape::label(id)));
                }
                self.out.push("\n");
            }
        }
    }

    fn math_block(&mut self, m: &MathBlock) {
        let rendered = math::render(&m.text, true, m.attrs.id());
        if rendered.mitex {
            self.req.package(math::MITEX_PACKAGE);
        }
        if rendered.labelled {
            self.req.fragment("ts-equations");
        }
        self.out.push(&rendered.text);
        self.out.push("\n");
    }

    fn raw_block(&mut self, r: &RawBlock) {
        if r.format == "typst" {
            self.out.push(r.text.trim_end_matches('\n'));
            self.out.push("\n");
        }
    }

    // ---------------------------------------------------------- figures

    /// `#figure(image("src", width: 60%), caption: […]) <id>`; a labelled
    /// image without caption keeps the `#figure` so the label attaches.
    pub(crate) fn figure_image(
        &mut self,
        image: &Image,
        caption: Option<&Caption>,
        link: Option<&str>,
    ) {
        if image.src.is_empty() {
            return;
        }
        self.req.assets.push(crate::AssetRef {
            src: image.src.clone(),
            node: image.meta.id,
            attrs: image.attrs.kv.clone(),
        });
        let call = image_call(image);
        let call = match link {
            Some(url) => format!("link(\"{}\", {call})", escape::string(url)),
            None => call,
        };
        let alt = self.render_inlines(&image.alt);
        let text = match caption {
            Some(c) => Some(self.render_inlines(&c.content)),
            None if !alt.is_empty() => Some(alt),
            None => None,
        };
        let label = caption
            .and_then(|c| c.attrs.id())
            .or(image.attrs.id())
            .map(escape::label);
        self.out.begin(image.meta.id);
        self.out.push(&format!("#figure(\n  {call},\n"));
        if let Some(text) = text {
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
        self.out.end(image.meta.id);
    }

    /// `::: figure`: one image, a grid of sub-figures, or a table with the
    /// figure's caption.
    fn figure(&mut self, f: &Figure, caption: Option<&Caption>) {
        let (content, inner) = match f.content.split_last() {
            Some((Block::Caption(c), rest)) if caption.is_none() => (rest, Some(c)),
            _ => (f.content.as_slice(), None),
        };
        let caption = caption.or(inner);
        // The images the resolution turned into sub-figures, in its own
        // order (spec §Image, Figure).
        let images = tmark_registry::figure_images(f);
        if let [Block::Table(t)] = content {
            let mut t = t.clone();
            if t.attrs.id.is_none() {
                t.attrs.id = f.attrs.id.clone();
            }
            self.table(&t, None, caption);
            return;
        }
        let label = f
            .attrs
            .id()
            .or(caption.and_then(|c| c.attrs.id()))
            .map(escape::label);
        if let [image] = images.as_slice() {
            if f.attrs.id.is_none() {
                self.figure_image(image, caption, None);
                return;
            }
        }
        let cols = f
            .attrs
            .get("cols")
            .and_then(|c| c.parse::<usize>().ok())
            .filter(|c| *c > 0)
            .unwrap_or(images.len().max(1));
        if images.len() > 1 {
            // The letters restart under every container.
            self.out
                .push("#counter(figure.where(kind: \"ts-subfigure\")).update(0)\n");
        }
        self.out.push("#figure(\n");
        if images.is_empty() {
            // A plain float (spec §Image, Figure: prose, a listing, a
            // table with the images): its blocks as blocks, so that an
            // image inside is a `#figure` of its own, numbered like any
            // other and carrying its label, as LaTeX's `figure` in
            // `figure` does. As inline content the images shrank to
            // 1em boxes and lost their labels (review 07 F10).
            let body = self.contained(|w| {
                let mut buf = std::mem::replace(&mut w.out, Out::scratch());
                w.blocks(content);
                std::mem::swap(&mut w.out, &mut buf);
                buf.finish_text()
            });
            self.out.push("  [\n");
            self.out.push(body.trim_end());
            self.out.push("\n  ],\n");
        } else if images.len() == 1 {
            self.out.push(&format!("  {},\n", image_call(images[0])));
        } else {
            self.out
                .push(&format!("  grid(columns: {cols}, gutter: 1em,\n"));
            for image in &images {
                self.req.assets.push(crate::AssetRef {
                    src: image.src.clone(),
                    node: image.meta.id,
                    attrs: image.attrs.kv.clone(),
                });
                // `ts-subfigure` is a figure of its own `kind`: it never
                // advances the figure counter (one number per container,
                // spec §Image, Figure) and its caption carries the `(a)`
                // marker. The caption is always there, empty alt or not.
                let alt = self.render_inlines(&image.alt);
                let call = format!("ts-subfigure({}, caption: [{alt}])", image_call(image));
                let cell = match image.attrs.id() {
                    Some(id) => format!("    [#{call} <{}>],\n", escape::label(id)),
                    None => format!("    {call},\n"),
                };
                self.out.push(&cell);
            }
            self.out.push("  ),\n");
        }
        if let Some(c) = caption {
            let text = self.render_inlines(&c.content);
            self.out.push(&format!("  caption: [{text}],\n"));
        }
        self.out.push(")");
        if let Some(label) = label {
            self.out.push(&format!(" <{label}>"));
        }
        self.out.push("\n");
    }

    // ---------------------------------------------------------- helpers

    pub(crate) fn render_inlines(&mut self, inlines: &[Inline]) -> String {
        let mut buf = std::mem::replace(&mut self.out, Out::scratch());
        self.inlines(inlines);
        std::mem::swap(&mut self.out, &mut buf);
        buf.finish_text()
    }

    /// Blocks as one content run, paragraphs separated by `#parbreak()`.
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
                        s.push_str(&format!("#ts-lead[{}] ", self.render_inlines(lead)));
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
        parts.join(" #parbreak() ")
    }
}

/// `image("src", width: 60%)`.
fn image_call(image: &Image) -> String {
    let src = escape::string(crate::latex::figure_src(&image.src));
    match image.attrs.get("width") {
        Some(width) => format!("image(\"{src}\", width: {})", typst_width(width)),
        None => format!("image(\"{src}\")"),
    }
}

/// `60%` and Typst lengths pass through; a bare number is a percentage.
fn typst_width(width: &str) -> String {
    match width.trim() {
        w if w.ends_with('%') => w.to_string(),
        w if w.parse::<f64>().is_ok() => format!("{w}%"),
        w => escape::markup(w),
    }
}

/// `#id` → `id: "x"`, classes → `class: ("a", "b")`, `key=val` forwarded
/// as strings (`bare` keys as booleans); `lang` and `media` never.
fn attr_args(attrs: &Attrs, bare: &[&str]) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(id) = attrs.id() {
        args.push(format!("id: \"{}\"", escape::label(id)));
    }
    if !attrs.classes.is_empty() {
        let classes: Vec<String> = attrs
            .classes
            .iter()
            .map(|c| format!("\"{}\"", escape::string(c)))
            .collect();
        args.push(format!("class: ({},)", classes.join(", ")));
    }
    for (k, v) in &attrs.kv {
        if matches!(k.as_str(), "lang" | "media") {
            continue;
        }
        let key = k.replace('_', "-");
        if bare.contains(&k.as_str()) {
            match v.as_str() {
                "true" | "" => args.push(format!("{key}: true")),
                "false" => args.push(format!("{key}: false")),
                other => args.push(format!("{key}: \"{}\"", escape::string(other))),
            }
        } else if v.parse::<f64>().is_ok() {
            args.push(format!("{key}: {v}"));
        } else {
            args.push(format!("{key}: \"{}\"", escape::string(v)));
        }
    }
    args
}

fn caption_matches(block: &Block, kind: CaptionKind) -> bool {
    match (block, kind) {
        (Block::Table(_), CaptionKind::Table) => true,
        (Block::CodeBlock(_), CaptionKind::Listing) => true,
        (Block::Div(d), CaptionKind::Listing) => d.name == "code",
        (Block::Figure(_), CaptionKind::Figure | CaptionKind::Table) => true,
        (Block::Para(p), CaptionKind::Figure) => match p.content.as_slice() {
            [Inline::Image(_)] => true,
            [Inline::Link(l)] => matches!(l.content.as_slice(), [Inline::Image(_)]),
            _ => false,
        },
        _ => false,
    }
}
