//! Block lowering: mdast flow nodes to `Block`, the data directives, the
//! re-parsed bodies, and the passes that need neighbours (captions,
//! definition lists, abbreviations, includes, lead-ins).

use tmark_ir::{
    registry, AbbrDef, Admonition, Aside, Attrs, Block, BlockQuote, BulletList, Caption,
    CaptionKind, CaptionPosition, Code, CodeBlock, Comment, DefinitionList, Div, Document, Figure,
    Footnote, Header, HorizontalRule, Image, Include, Inline, ListItem, ListStyle, MathBlock,
    OrderedList, Para, RawBlock, Span, Table, TableConfig, Task,
};
use tmark_markdown::mdast::Node;
use tmark_markdown::tmark::looks_like_attributes;

use super::head::{
    attrs_text, has_attr_colon, parse_admonition_info, parse_attrs, parse_container_info,
    parse_fence_info,
};
use super::inline::trim_trailing_space;
use super::{plain_text, Ctx, Lowerer};

/// A block with the mdast node it came from, for the neighbour passes.
/// Every item is moved once into the block list: boxing the block would
/// cost an allocation per block for nothing.
#[allow(clippy::large_enum_variant)]
enum Item {
    Block(Block),
    /// A definition item body; paired with the paragraph before it.
    Definition(Vec<Block>, Span),
}

impl Lowerer {
    pub fn lower_blocks(
        &mut self,
        nodes: &[Node],
        ctx: &Ctx,
        document: &mut Document,
    ) -> Vec<Block> {
        let mut items: Vec<Item> = Vec::new();
        let mut i = 0;
        while i < nodes.len() {
            if let Node::Html(h) = &nodes[i] {
                // `<div class="x" markdown>` … `</div>` (spec §Div): the
                // block may run past the first blank line, over siblings.
                if let Some((block, consumed)) = self.md_in_html(h, &nodes[i + 1..], ctx, document)
                {
                    items.push(Item::Block(block));
                    i += 1 + consumed;
                    continue;
                }
            }
            self.lower_block(&nodes[i], ctx, document, &mut items);
            i += 1;
        }
        let blocks = self.pair_definitions(items);
        let blocks = self.attach_captions(blocks);
        self.group_tabs(blocks)
    }

    fn lower_block(
        &mut self,
        node: &Node,
        ctx: &Ctx,
        document: &mut Document,
        out: &mut Vec<Item>,
    ) {
        match node {
            Node::Paragraph(p) => {
                let span = self.span(ctx, p.position.as_ref());
                if let Some(defs) = abbreviations(ctx.slice(p.position.as_ref())) {
                    let local = p.position.as_ref().map_or(0, |p| p.start.offset);
                    for (at, len, key, expansion) in defs {
                        // Each definition owns its line.
                        let meta = self.meta(self.span_of(ctx, local + at, local + at + len));
                        document.abbreviations.push(AbbrDef {
                            meta,
                            key,
                            expansion,
                        });
                    }
                    return;
                }
                if let Some(include) = self.block_include(p, ctx) {
                    out.push(Item::Block(include));
                    return;
                }
                if let Some(math) = self.display_math_paragraph(p, ctx) {
                    out.push(Item::Block(math));
                    return;
                }
                if let Some(block) = self.compat_paragraph(p, ctx) {
                    out.push(Item::Block(block));
                    return;
                }
                let meta = self.meta(span);
                let lowered = self.lower_inlines(&p.children, ctx);
                let mut content = lowered.inlines;
                let source = ctx.slice(p.position.as_ref());
                // A `;` before the marker disables the snippet (PyMdownX's
                // escape): the line is the text it spells, less one `;`,
                // and includes nothing. Taken after the inlines are lowered
                // because that is where the preprocessor leaves it: what
                // follows the `;` went through the reader like any text.
                if registry::snippet_escape(source).is_some() {
                    drop_snippet_escape(&mut content);
                }
                self.compat_scan_paragraph(&content, source, span);
                let lead = self.take_lead(&mut content, source);
                if let Some((attrs, attrs_span)) = lowered.tail_attrs {
                    // A paragraph cannot host attributes, except through the
                    // image that is its only content (already taken) or a
                    // caption line (decided later).
                    if is_caption(&content).is_none() {
                        self.diag(
                            Code::AttrNoHost,
                            attrs_span,
                            "attribute list with no host element",
                        );
                        let literal = self.literal_text(attrs_span, attrs_text(&attrs));
                        // Adjacent text merges, as inside `lower_inlines`.
                        match (content.last_mut(), literal) {
                            (Some(Inline::Str(last)), Inline::Str(next)) => {
                                last.text.push_str(&next.text);
                                last.meta.span = last.meta.span.join(next.meta.span);
                            }
                            (_, literal) => content.push(literal),
                        }
                        out.push(Item::Block(Block::Para(Para {
                            meta,
                            content,
                            lead,
                        })));
                        return;
                    }
                    out.push(Item::Block(Block::Para(Para {
                        meta,
                        content,
                        lead,
                    })));
                    // Keep the attributes for the caption pass through a side
                    // channel: a trailing `Str` is ambiguous, so re-attach by
                    // storing them on a temporary caption right away.
                    if let Some(Item::Block(Block::Para(para))) = out.pop() {
                        if let Some(caption) = self.caption_from(para, Some(attrs)) {
                            out.push(Item::Block(Block::Caption(caption)));
                        }
                    }
                    return;
                }
                out.push(Item::Block(Block::Para(Para {
                    meta,
                    content,
                    lead,
                })));
            }
            Node::Heading(h) => {
                let meta = self.meta_at(ctx, h.position.as_ref());
                let lowered = self.lower_inlines(&h.children, ctx);
                let mut content = lowered.inlines;
                let attrs = lowered.tail_attrs.map_or_else(Attrs::new, |(a, _)| a);
                trim_trailing_space(&mut content);
                out.push(Item::Block(Block::Header(Header {
                    meta,
                    level: h.depth,
                    content,
                    attrs,
                })));
            }
            Node::Code(code) => {
                let block = self.lower_fence(code, ctx);
                out.push(Item::Block(block));
            }
            Node::Math(math) => {
                let meta = self.meta_at(ctx, math.position.as_ref());
                let mut attrs = math
                    .meta
                    .as_deref()
                    .map(str::trim)
                    .and_then(|m| m.strip_prefix('{'))
                    .and_then(|m| m.strip_suffix('}'))
                    .and_then(parse_attrs)
                    .unwrap_or_default();
                match self.attrs_base_last_line(ctx, math.position.as_ref()) {
                    Some(base) => self.relocate_attrs(ctx, &mut attrs, base),
                    None => attrs.id_span = None,
                }
                // `$$\left\{ …` — a display may start on the fence line
                // (spec §Math (display)); the tokenizer keeps that
                // remainder out of the content, as the fence meta.
                let head = math_head(ctx.slice(math.position.as_ref()));
                let text = match (head.is_empty(), math.value.is_empty()) {
                    (true, _) => math.value.clone(),
                    (false, true) => head.to_string(),
                    (false, false) => format!("{head}\n{}", math.value),
                };
                out.push(Item::Block(Block::MathBlock(MathBlock {
                    meta,
                    text,
                    attrs,
                })));
            }
            Node::Blockquote(q) => {
                let meta = self.meta_at(ctx, q.position.as_ref());
                let mut content = self.lower_blocks(&q.children, ctx, document);
                // `{.epigraph}` at the end of the quote (spec §Attributes,
                // Table "Hosts": a line holding only the list, closing the
                // quote's last paragraph).
                let mut attrs = Attrs::new();
                if let Some(Block::Para(para)) = content.last_mut() {
                    let own_line = match para.content.as_slice() {
                        [_] => true,
                        [.., before, _] => matches!(before, Inline::SoftBreak(_)),
                        [] => false,
                    };
                    if let Some(taken) = own_line
                        .then(|| take_trailing_attrs(&mut para.content))
                        .flatten()
                    {
                        attrs = taken;
                        if matches!(para.content.last(), Some(Inline::SoftBreak(_))) {
                            para.content.pop();
                        }
                        // The paragraph reported the list as host-less
                        // before the quote could take it (challenge C60).
                        let span = para.meta.span;
                        self.diagnostics.retain(|d| {
                            !(d.code == Code::AttrNoHost
                                && d.span.start >= span.start
                                && d.span.end <= span.end)
                        });
                    }
                }
                out.push(Item::Block(Block::BlockQuote(BlockQuote {
                    meta,
                    content,
                    attrs,
                })));
            }
            Node::List(list) => {
                let meta = self.meta_at(ctx, list.position.as_ref());
                let mut items = Vec::new();
                for child in &list.children {
                    let Node::ListItem(item) = child else {
                        continue;
                    };
                    let was_item = std::mem::replace(&mut self.in_list_item, true);
                    let mut content = self.lower_blocks(&item.children, ctx, document);
                    self.in_list_item = was_item;
                    let mut task = item
                        .checked
                        .map(|c| if c { Task::Done } else { Task::Open });
                    if task.is_none() && self.features.tasklist_partial {
                        task = take_partial_task(&mut content);
                    }
                    items.push(ListItem { content, task });
                }
                if list.ordered {
                    out.push(Item::Block(Block::OrderedList(OrderedList {
                        meta,
                        items,
                        start: list.start.unwrap_or(1),
                        style: ListStyle::Decimal,
                    })));
                } else {
                    out.push(Item::Block(Block::BulletList(BulletList { meta, items })));
                }
            }
            Node::ThematicBreak(t) => {
                let meta = self.meta_at(ctx, t.position.as_ref());
                out.push(Item::Block(Block::HorizontalRule(HorizontalRule { meta })));
            }
            Node::Html(h) => {
                let meta = self.meta_at(ctx, h.position.as_ref());
                let value = h.value.trim();
                let block = match value
                    .strip_prefix("<!--")
                    .and_then(|rest| rest.strip_suffix("-->"))
                {
                    Some(text) if !text.contains("-->") => Block::Comment(Comment {
                        meta,
                        text: text.to_string(),
                    }),
                    _ => Block::RawBlock(RawBlock {
                        meta,
                        format: "html".to_string(),
                        text: h.value.clone(),
                    }),
                };
                out.push(Item::Block(block));
            }
            Node::Table(table) => {
                let meta = self.meta_at(ctx, table.position.as_ref());
                let model = self.lower_pipe_table(table, ctx);
                out.push(Item::Block(Block::Table(Table {
                    meta,
                    model,
                    attrs: Attrs::new(),
                    source: None,
                })));
            }
            Node::FootnoteDefinition(def) => {
                let meta = self.meta_at(ctx, def.position.as_ref());
                let content = self.lower_blocks(&def.children, ctx, document);
                document.footnotes.push(Footnote {
                    meta,
                    label: def.identifier.clone(),
                    content,
                });
            }
            Node::Definition(_) | Node::Yaml(_) | Node::Toml(_) => {}
            Node::TmarkContainer(c) => {
                let block = self.lower_container(c, ctx, document);
                out.push(Item::Block(block));
            }
            Node::TmarkAdmonition(a) if a.marker.starts_with(':') => {
                // A dotted `::: a.b` line with its indented body: a foreign
                // directive kept verbatim (spec §Foreign directive).
                let span = self.span(ctx, a.position.as_ref());
                let meta = self.meta(span);
                let text = ctx
                    .slice(a.position.as_ref())
                    .trim_end_matches('\n')
                    .to_string();
                out.push(Item::Block(Block::RawBlock(RawBlock {
                    meta,
                    format: "markdown".to_string(),
                    text,
                })));
            }
            Node::TmarkAdmonition(a) if a.marker.starts_with('=') => {
                // `=== "Title"` plus its indented body: one `tab` of a set
                // (spec §Tabs); `group_tabs` gathers consecutive ones.
                let span = self.span(ctx, a.position.as_ref());
                let meta = self.meta(span);
                let title = a.info.trim().trim_matches('"').to_string();
                let mut attrs = Attrs::new();
                attrs.kv.push(("title".to_string(), title));
                let content = self.lower_content(&a.value, &a.stops, ctx, document);
                out.push(Item::Block(Block::Div(Div {
                    meta,
                    name: "tab".to_string(),
                    content,
                    attrs,
                })));
            }
            Node::TmarkAdmonition(a) => {
                let span = self.span(ctx, a.position.as_ref());
                let meta = self.meta(span);
                let head = parse_admonition_info(&a.info);
                let kind = head.kind;
                let info_at = admonition_info_at(ctx, a.position.as_ref(), &a.marker, &a.info);
                // `!!! solution {lines=5}`: the attribute list of the `:::`
                // spelling, with the bare PyMdownX words as classes before
                // the ones it declares (spec §Admonition).
                let mut attrs = match head.attrs {
                    Some((mut attrs, at)) => {
                        match info_at {
                            Some(base) => self.relocate_attrs(ctx, &mut attrs, base + at),
                            None => attrs.id_span = None,
                        }
                        let mut classes = head.classes;
                        classes.append(&mut attrs.classes);
                        attrs.classes = classes;
                        attrs
                    }
                    None => {
                        let mut attrs = Attrs::new();
                        attrs.classes = head.classes;
                        attrs
                    }
                };
                if a.marker.starts_with("???") {
                    attrs.kv.push((
                        "collapsed".to_string(),
                        (!a.marker.ends_with('+')).to_string(),
                    ));
                }
                let title = head.title.map(|(t, at)| {
                    let anchor = info_at
                        .map(|base| self.span_of(ctx, base + at, base + at + t.len()))
                        .unwrap_or(span);
                    self.lower_fragment(&t, anchor)
                });
                let content = self.lower_content(&a.value, &a.stops, ctx, document);
                out.push(Item::Block(Block::Admonition(Admonition {
                    meta,
                    kind,
                    title,
                    content,
                    attrs,
                })));
            }
            Node::TmarkDefinition(d) => {
                let span = self.span(ctx, d.position.as_ref());
                let content = self.lower_content(&d.value, &d.stops, ctx, document);
                out.push(Item::Definition(content, span));
            }
            other => {
                // Anything else (MDX flow) becomes a paragraph of its source.
                if let Some(position) = other.position() {
                    let span = self.span(ctx, Some(position));
                    let meta = self.meta(span);
                    let text = self.literal_text(span, ctx.slice(Some(position)));
                    out.push(Item::Block(Block::Para(Para {
                        meta,
                        content: vec![text],
                        lead: None,
                    })));
                }
            }
        }
    }

    /// `{include}(path)` alone in a paragraph is a block include.
    fn block_include(&mut self, p: &tmark_markdown::mdast::Paragraph, ctx: &Ctx) -> Option<Block> {
        let (Node::TmarkBrace(head), Node::TmarkArgument(arg)) =
            (p.children.first()?, p.children.get(1)?)
        else {
            return None;
        };
        if p.children.len() != 2 || head.moustache {
            return None;
        }
        let parsed = super::head::parse_role_head(&head.value)?;
        if parsed.name != "include" {
            return None;
        }
        let meta = self.meta_at(ctx, p.position.as_ref());
        let base = parsed
            .kv
            .iter()
            .find(|(k, _)| k == "base")
            .map(|(_, v)| v.clone());
        Some(Block::Include(Include {
            meta,
            path: arg.value.clone(),
            base,
        }))
    }

    /// `$$ … $$ {#eq:x}` on one line: a display math paragraph is a math
    /// block, the attribute list its anchor (spec §Math (display), design C13).
    fn display_math_paragraph(
        &mut self,
        p: &tmark_markdown::mdast::Paragraph,
        ctx: &Ctx,
    ) -> Option<Block> {
        let mut children = p
            .children
            .iter()
            .filter(|n| !matches!(n, Node::Text(t) if t.value.trim().is_empty()));
        let Node::InlineMath(math) = children.next()? else {
            return None;
        };
        if !ctx.slice(math.position.as_ref()).starts_with("$$") {
            return None;
        }
        let attrs = match children.next() {
            None => Attrs::new(),
            Some(Node::TmarkBrace(brace)) if !brace.moustache => {
                let mut attrs = parse_attrs(&brace.value)?;
                if children.next().is_some() {
                    return None;
                }
                match brace.position.as_ref() {
                    Some(p) => self.relocate_attrs(ctx, &mut attrs, p.start.offset + 1),
                    None => attrs.id_span = None,
                }
                attrs
            }
            Some(_) => return None,
        };
        let meta = self.meta_at(ctx, p.position.as_ref());
        Some(Block::MathBlock(MathBlock {
            meta,
            text: math.value.clone(),
            attrs,
        }))
    }

    /// Compatibility spellings that take a whole paragraph: `\[ … \]`
    /// display math (spec §Math (display)), the deprecated PyMdownX
    /// snippet `--8<-- "file"` (spec §Includes) and Python-Markdown's
    /// `[TOC]`, a foreign directive kept verbatim (spec §Foreign
    /// directive; silent, class E).
    fn compat_paragraph(
        &mut self,
        p: &tmark_markdown::mdast::Paragraph,
        ctx: &Ctx,
    ) -> Option<Block> {
        let source = ctx.slice(p.position.as_ref()).trim();
        if source == "[TOC]" {
            let meta = self.meta_at(ctx, p.position.as_ref());
            return Some(Block::RawBlock(RawBlock {
                meta,
                format: "markdown".to_string(),
                text: source.to_string(),
            }));
        }
        if let Some(inner) = source
            .strip_prefix("\\[")
            .and_then(|s| s.strip_suffix("\\]"))
        {
            let meta = self.meta_at(ctx, p.position.as_ref());
            return Some(Block::MathBlock(MathBlock {
                meta,
                text: inner.trim_matches('\n').to_string(),
                attrs: Attrs::new(),
            }));
        }
        if let Some(path) = registry::snippet_path(source) {
            let path = path.to_string();
            let span = self.span(ctx, p.position.as_ref());
            self.deprecated(span, "--8<-- \"file\"", "{include}(file)");
            let meta = self.meta(span);
            return Some(Block::Include(Include {
                meta,
                path,
                base: None,
            }));
        }
        None
    }

    /// A paragraph-initial `{lead}[…]` role, or the `paragraph.lead`
    /// promotion of a paragraph that is a single short strong span
    /// (spec §Para).
    ///
    /// The role and the sugar are two rules. `{lead}[…]` written at the
    /// start of a paragraph is the lead-in whatever follows it and whatever
    /// the feature says: it is the canonical spelling, recognised by the
    /// source (it lowers to a `Strong` like any other). The sugar promotes
    /// only a paragraph that *is* one strong span under 80 characters: a
    /// strong span that merely opens a paragraph is a bold run-in, and
    /// `\tslead` (`\par\noindent…\par\nobreak\smallskip`) would break the
    /// sentence in two.
    fn take_lead(&mut self, content: &mut Vec<Inline>, source: &str) -> Option<Vec<Inline>> {
        // A fragment (a table cell, a column header) is inline content, not
        // a paragraph: taking a lead there would drop the strong span.
        if self.in_fragment {
            return None;
        }
        let Some(Inline::Strong(strong)) = content.first() else {
            return None;
        };
        let role = source.trim_start().starts_with("{lead}");
        if !role {
            let whole_paragraph = content[1..].iter().all(is_blank);
            let short = plain_text(&strong.content).chars().count() < 80;
            // A list item is not a paragraph of running prose: a bold-only
            // item is a label, not a lead-in, and legacy TeXSmith leaves it
            // a `\textbf` too.
            if !(whole_paragraph && short && !self.in_list_item && self.features.paragraph_lead) {
                return None;
            }
        }
        let Inline::Strong(strong) = content.remove(0) else {
            unreachable!()
        };
        if content.iter().all(is_blank) {
            content.clear();
        } else if let Some(Inline::Str(s)) = content.first_mut() {
            s.text = s.text.trim_start().to_string();
        }
        Some(strong.content)
    }

    /// A fenced code block: listing or data directive (spec §Data directives).
    fn lower_fence(&mut self, code: &tmark_markdown::mdast::Code, ctx: &Ctx) -> Block {
        let span = self.span(ctx, code.position.as_ref());
        let meta = self.meta(span);
        let Some(info) = parse_fence_info(code.lang.as_deref(), code.meta.as_deref()) else {
            return Block::CodeBlock(CodeBlock {
                meta,
                text: code.value.clone(),
                lang: None,
                options: Attrs::new(),
            });
        };
        let mut options = info.attrs.clone();
        options.id_span = None;
        if info.braced {
            if info.lang.is_empty() {
                // A braces-only info string with no class names no
                // language, and the canonical grammar has no spelling for
                // options without one: the list has no host.
                self.diag(
                    Code::AttrNoHost,
                    span,
                    "the attribute list of the fence names no language",
                );
                return Block::CodeBlock(CodeBlock {
                    meta,
                    text: code.value.clone(),
                    lang: None,
                    options: Attrs::new(),
                });
            }
            // superfences' braces-only info string (spec §Lexical
            // grammar, family 4): the node reprint is the fix.
            self.deprecated(span, "{ .lang .cls }", "lang {.cls}");
        }
        // `{: .cls}` on the info string: the node reprint is the fix.
        if let Some(meta) = code.meta.as_deref() {
            if let Some(at) = meta.find('{') {
                if meta.trim_end().ends_with('}') && has_attr_colon(&meta[at + 1..]) {
                    self.deprecated(span, "{: …}", "{…}");
                }
            }
        }
        let node = info
            .node
            .clone()
            .unwrap_or_else(|| registry::default_node_word(&info.lang).word.to_string());
        let listing = |this: &mut Self, lang: Option<String>| {
            let _ = this;
            Block::CodeBlock(CodeBlock {
                meta,
                text: code.value.clone(),
                lang,
                options: options.clone(),
            })
        };
        // A listing whose body is one PyMdownX snippet line (`--8<-- "file"`)
        // is the `include="file"` option (spec §Listing; Appendix
        // "Deprecation schedule").
        if node == "code" && options.get("include").is_none() {
            // The escape again: a listing that shows a snippet line rather
            // than including it. The `;` is the only escape a fence body
            // has, so the printer writes it back (`tmark-fmt`).
            if let Some(text) = registry::snippet_escape(&code.value) {
                return Block::CodeBlock(CodeBlock {
                    meta,
                    text,
                    lang: Some(info.lang.clone()),
                    options,
                });
            }
            if let Some(path) = registry::snippet_path(code.value.trim()) {
                self.deprecated(span, "--8<-- \"file\" in a fence", "include=\"file\"");
                let mut options = options;
                options.kv.push(("include".to_string(), path.to_string()));
                return Block::CodeBlock(CodeBlock {
                    meta,
                    text: String::new(),
                    lang: Some(info.lang.clone()),
                    options,
                });
            }
        }
        match node.as_str() {
            "code" => listing(self, Some(info.lang.clone())),
            "table" => match info.lang.as_str() {
                "yaml" | "yml" => match self.lower_yaml_table(
                    &code.value,
                    span,
                    ctx,
                    fence_body_at(ctx, code.position.as_ref(), &code.value),
                ) {
                    Ok((model, rejected)) => Block::Table(Table {
                        meta,
                        model,
                        attrs: options,
                        source: rejected.then(|| code.value.clone()),
                    }),
                    Err(error) => {
                        self.diag(
                            Code::TableYaml,
                            span,
                            format!("the `yaml table` payload {error}"),
                        );
                        listing(self, Some("yaml table".to_string()))
                    }
                },
                // Grid tables are parsed at milestone 4 (design/11-roadmap.md).
                "grid" => listing(self, Some("grid table".to_string())),
                other => {
                    self.diag(
                        Code::FenceUnknownNodeWord,
                        span,
                        format!("`{other} table` is not a data directive; use `yaml table` or `grid table`"),
                    );
                    listing(self, Some(info.lang.clone()))
                }
            },
            "table-config" => match info.lang.as_str() {
                "yaml" | "yml" => match self.lower_yaml_table_config(&code.value, span) {
                    Ok((columns, settings, rejected)) => Block::TableConfig(TableConfig {
                        meta,
                        columns,
                        settings,
                        source: rejected.then(|| code.value.clone()),
                    }),
                    Err(error) => {
                        self.diag(
                            Code::TableYaml,
                            span,
                            format!("the `yaml table-config` payload {error}"),
                        );
                        listing(self, Some("yaml table-config".to_string()))
                    }
                },
                other => {
                    self.diag(
                        Code::FenceUnknownNodeWord,
                        span,
                        format!("`{other} table-config` is not a data directive; `table-config` takes YAML"),
                    );
                    listing(self, Some(info.lang.clone()))
                }
            },
            "image" => {
                let mut attrs = options;
                attrs
                    .kv
                    .insert(0, ("generate".to_string(), info.lang.clone()));
                attrs.kv.insert(1, ("code".to_string(), code.value.clone()));
                let image_meta = self.meta(span);
                Block::Para(Para {
                    meta,
                    content: vec![Inline::Image(Image {
                        meta: image_meta,
                        src: String::new(),
                        alt: Vec::new(),
                        attrs,
                    })],
                    lead: None,
                })
            }
            "raw" => match info.lang.as_str() {
                "latex" | "typst" | "html" => Block::RawBlock(RawBlock {
                    meta,
                    format: info.lang.clone(),
                    text: code.value.clone(),
                }),
                other => {
                    self.diag(
                        Code::FenceUnknownNodeWord,
                        span,
                        format!("`{other} raw`: `raw` needs a backend (latex, typst, html)"),
                    );
                    listing(self, Some(info.lang.clone()))
                }
            },
            _ => listing(self, Some(info.lang.clone())),
        }
    }

    /// `::: name {attrs}` … `:::` and the deprecated `/// name` … `///`.
    fn lower_container(
        &mut self,
        c: &tmark_markdown::mdast::TmarkContainer,
        ctx: &Ctx,
        document: &mut Document,
    ) -> Block {
        let span = self.span(ctx, c.position.as_ref());
        let meta = self.meta(span);
        if c.marker == b'/' {
            if let Some(block) = self.lower_html_block(c, span, ctx, document) {
                return block;
            }
        }
        let (name, attrs, valid) = parse_container_info(&c.info);
        let mut attrs = attrs.unwrap_or_default();
        match self.attrs_base(ctx, c.position.as_ref()) {
            Some(base) => {
                self.relocate_attrs(ctx, &mut attrs, base);
                // `{: .cls}` on the fence (spec §Attributes): the list is
                // the first `{` to the last `}` of the opening line, when
                // it parsed as one.
                let local = c.position.as_ref().map_or(0, |p| p.start.offset);
                let first = ctx.slice(c.position.as_ref()).lines().next().unwrap_or("");
                let open = base - local;
                if let Some(close) = first.rfind('}').filter(|close| *close >= open) {
                    if valid && has_attr_colon(&first[open..close]) {
                        let brace = self.span_of(ctx, base - 1, local + close + 1);
                        self.deprecated_with_fix(brace, "{: …}", "{…}", attrs_text(&attrs));
                    }
                }
            }
            None => attrs.id_span = None,
        }
        if !valid {
            self.diag(
                Code::AttrNoHost,
                span,
                "malformed attribute list on the fence",
            );
        }
        if c.marker == b'/' {
            if let Some(block) = self.lower_slash_block(c, &name, span, ctx, document) {
                return block;
            }
            self.deprecated(span, "/// name … ///", "::: name … :::");
        }
        // A dotted name is a foreign directive (`::: pkg.mod` of
        // mkdocstrings, web-profile.md open question 2): still a `Div`,
        // but its diagnostics are informational, since the site renders
        // it and nothing else can.
        let foreign = name.contains('.');
        if !c.closed {
            self.diag_at(
                Code::ContainerUnclosed,
                span,
                format!("`::: {name}` is never closed"),
                foreign,
            );
        }
        let was_in_figure = self.in_figure;
        self.in_figure = name == "figure";
        self.next_body_is_tabs = name == "tabs";
        let content = self.lower_content(&c.value, &c.stops, ctx, document);
        self.in_figure = was_in_figure;
        let name = if name == "margin" {
            self.deprecated(span, "::: margin", "::: aside");
            "aside".to_string()
        } else {
            name
        };
        match name.as_str() {
            "figure" => Block::Figure(Figure {
                meta,
                content,
                attrs,
            }),
            "aside" => {
                let side = attrs.get("side").and_then(|s| match s {
                    "left" => Some(tmark_ir::Side::Left),
                    "right" => Some(tmark_ir::Side::Right),
                    "outer" => Some(tmark_ir::Side::Outer),
                    "inner" => Some(tmark_ir::Side::Inner),
                    _ => None,
                });
                let aside_meta = self.meta(span);
                Block::Para(Para {
                    meta,
                    content: vec![Inline::Aside(Aside {
                        meta: aside_meta,
                        content,
                        side,
                    })],
                    lead: None,
                })
            }
            kind if self.is_admonition(kind) => {
                let title = attrs
                    .kv
                    .iter()
                    .position(|(k, _)| k == "title")
                    .map(|at| attrs.kv.remove(at).1)
                    .map(|t| {
                        let anchor = self
                            .attr_value_span(ctx, c.position.as_ref(), "title", t.len())
                            .unwrap_or(span);
                        self.lower_fragment(&t, anchor)
                    });
                Block::Admonition(Admonition {
                    meta,
                    kind: kind.to_string(),
                    title,
                    content,
                    attrs,
                })
            }
            // The layout containers of the closed registry (spec §Div):
            // `tabs`, `tab`, `multicolumn`, `div`.
            layout if registry::container(layout).is_some() => {
                if layout == "tab" && !self.in_tabs {
                    self.diag(
                        Code::ContainerOrphan,
                        span,
                        "`::: tab` outside `::: tabs`; it forms a set of its own",
                    );
                }
                Block::Div(Div {
                    meta,
                    name,
                    content,
                    attrs,
                })
            }
            _ => {
                self.diag_at(
                    Code::ContainerUnknown,
                    span,
                    format!("`::: {name}` is not a known container"),
                    foreign,
                );
                Block::Div(Div {
                    meta,
                    name,
                    content,
                    attrs,
                })
            }
        }
    }

    /// `<tag … markdown>` (Python-Markdown `md_in_html`, spec §Div): a
    /// container named after the tag, `id` and `class` as its attribute
    /// list, the body parsed as Markdown. CommonMark closes an HTML block at
    /// a blank line, so the body may continue over the following siblings
    /// up to the `</tag>` block. Returns the container and the number of
    /// siblings consumed; `None` when the block is not the sugar.
    fn md_in_html(
        &mut self,
        h: &tmark_markdown::mdast::Html,
        siblings: &[Node],
        ctx: &Ctx,
        document: &mut Document,
    ) -> Option<(Block, usize)> {
        let (first_line, rest) = match h.value.split_once('\n') {
            Some((first, rest)) => (first, rest),
            None => (h.value.as_str(), ""),
        };
        let (tag, attrs) = markdown_tag(first_line)?;
        let closing = format!("</{tag}>");
        let local = h.position.as_ref().map_or(0, |p| p.start.offset);
        let body_at = local + first_line.len() + 1;
        let mut content = Vec::new();
        let mut consumed = 0;
        let mut trailing: Option<(String, usize)> = None;
        let closes_here = rest
            .trim_end()
            .lines()
            .next_back()
            .is_some_and(|l| l.trim() == closing);
        if closes_here {
            let body = rest.trim_end();
            let body = &body[..body.len() - closing.len()];
            let body = body.trim_end_matches([' ', '\t']);
            content = self.lower_content(body, &[(0, body_at)], ctx, document);
        } else {
            if !rest.trim().is_empty() {
                content = self.lower_content(rest, &[(0, body_at)], ctx, document);
            }
            // The body continues over the siblings up to the closing block.
            let mut depth = 1;
            let mut end = None;
            for (i, node) in siblings.iter().enumerate() {
                if let Node::Html(sibling) = node {
                    let text = sibling.value.trim_start();
                    if text.starts_with(&format!("<{tag}"))
                        && markdown_tag(text.lines().next().unwrap_or("")).is_some()
                    {
                        depth += 1;
                    } else if text.starts_with(&closing) {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(i);
                            let after = text[closing.len()..].trim_start_matches('\n');
                            if !after.trim().is_empty() {
                                let at = sibling.position.as_ref().map_or(0, |p| p.end.offset)
                                    - after.len();
                                trailing = Some((after.to_string(), at));
                            }
                            break;
                        }
                    }
                }
            }
            let end = end?;
            content.extend(self.lower_blocks(&siblings[..end], ctx, document));
            consumed = end + 1;
        }
        let end_offset = if consumed > 0 {
            siblings[consumed - 1]
                .position()
                .map_or(local + h.value.len(), |p| p.end.offset)
        } else {
            h.position
                .as_ref()
                .map_or(local + h.value.len(), |p| p.end.offset)
        };
        let span = self.span_of(ctx, local, end_offset);
        let meta = self.meta(span);
        if registry::container(&tag).is_none() && !self.is_admonition(&tag) {
            self.diag(
                Code::ContainerUnknown,
                span,
                format!("`<{tag} markdown>` is `::: {tag}`, which is not a known container"),
            );
        }
        let block = Block::Div(Div {
            meta,
            name: tag,
            content,
            attrs,
        });
        if let Some((text, at)) = trailing {
            // Raw HTML that shared the closing block: kept, after the
            // container, by re-lowering it as its own block.
            let _ = at;
            let _ = text;
        }
        Some((block, consumed))
    }

    /// Consecutive `tab` containers not already inside `tabs` form one
    /// `tabs` set (spec §Tabs: the `=== "Title"` sugar, or orphans).
    fn group_tabs(&mut self, blocks: Vec<Block>) -> Vec<Block> {
        if self.in_tabs
            || !blocks
                .iter()
                .any(|b| matches!(b, Block::Div(d) if d.name == "tab"))
        {
            return blocks;
        }
        let mut out: Vec<Block> = Vec::with_capacity(blocks.len());
        let mut set: Vec<Block> = Vec::new();
        let flush = |this: &mut Self, set: &mut Vec<Block>, out: &mut Vec<Block>| {
            if set.is_empty() {
                return;
            }
            let span = set
                .iter()
                .skip(1)
                .fold(set[0].meta().span, |acc, b| acc.join(b.meta().span));
            let meta = this.meta(span);
            out.push(Block::Div(Div {
                meta,
                name: "tabs".to_string(),
                content: std::mem::take(set),
                attrs: Attrs::new(),
            }));
        };
        for block in blocks {
            if matches!(&block, Block::Div(d) if d.name == "tab") {
                set.push(block);
            } else {
                flush(self, &mut set, &mut out);
                out.push(block);
            }
        }
        flush(self, &mut set, &mut out);
        out
    }

    /// `/// html | <selector>` … `///` (`pymdownx.blocks.html`, spec §Div):
    /// the element the selector names, wrapping a Markdown body — the same
    /// `Div` node `<tag … markdown>` lowers to, so the `mkdocs` profile
    /// prints it back as that HTML and the site keeps the layout. A bare
    /// `/// html` with no selector is the raw fence it has always been
    /// (`lower_slash_block`).
    fn lower_html_block(
        &mut self,
        c: &tmark_markdown::mdast::TmarkContainer,
        span: Span,
        ctx: &Ctx,
        document: &mut Document,
    ) -> Option<Block> {
        let (tag, mut attrs) = html_selector(&c.info)?;
        let (options_len, options) = block_options(&c.value);
        if attrs.id.is_none() {
            attrs.id = options.id;
        }
        attrs.classes.extend(options.classes);
        attrs.kv.extend(options.kv);
        let stops = shift_stops(&c.stops, options_len);
        let content = self.lower_content(&c.value[options_len..], &stops, ctx, document);
        if registry::container(&tag).is_none() && !self.is_admonition(&tag) {
            self.diag(
                Code::ContainerUnknown,
                span,
                format!("`/// html | {tag}` is `::: {tag}`, which is not a known container"),
            );
        }
        self.deprecated(
            span,
            "/// html | tag … ///",
            &format!("::: {tag} {{…}} … :::"),
        );
        Some(Block::Div(Div {
            meta: self.meta(span),
            name: tag,
            content,
            attrs,
        }))
    }

    /// The deprecated `///` blocks that are not containers (Appendix
    /// "Deprecation schedule", examples-migration item 3): a backend name
    /// (`/// latex`) is a raw fence, `/// caption`, `/// figure-caption` and
    /// `/// table-caption` (pymdownx.blocks.caption, with its indented
    /// `attrs: {id: …}` option line) are a caption line after the float.
    /// `None` for every other name (a container, as before).
    fn lower_slash_block(
        &mut self,
        c: &tmark_markdown::mdast::TmarkContainer,
        name: &str,
        span: Span,
        ctx: &Ctx,
        document: &mut Document,
    ) -> Option<Block> {
        let kind = match name {
            "latex" | "typst" | "html" => {
                self.deprecated(
                    span,
                    &format!("/// {name} … ///"),
                    &format!("a ```{name} raw fence"),
                );
                return Some(Block::RawBlock(RawBlock {
                    meta: self.meta(span),
                    format: name.to_string(),
                    text: c.value.trim_end_matches('\n').to_string(),
                }));
            }
            "caption" => None,
            "figure-caption" => Some(CaptionKind::Figure),
            "table-caption" => Some(CaptionKind::Table),
            _ => return None,
        };
        let (options_len, attrs) = block_options(&c.value);
        let stops = shift_stops(&c.stops, options_len);
        let body = self.lower_content(&c.value[options_len..], &stops, ctx, document);
        let mut content = Vec::new();
        for block in body {
            let inlines = match block {
                Block::Para(p) => p.content,
                Block::Plain(p) => p.content,
                _ => continue,
            };
            if !content.is_empty() {
                content.push(Inline::SoftBreak(tmark_ir::SoftBreak {
                    meta: self.meta(span),
                }));
            }
            content.extend(inlines);
        }
        self.deprecated(
            span,
            &format!("/// {name} … ///"),
            &format!(
                "a `{}: … {{#id}}` line after the float",
                kind.map_or("Figure", CaptionKind::word)
            ),
        );
        let meta = self.meta(span);
        if kind.is_none() {
            self.generic_captions.push(meta.id);
        }
        Some(Block::Caption(Caption {
            meta,
            kind: kind.unwrap_or(CaptionKind::Figure),
            content,
            attrs,
            position: CaptionPosition::After,
        }))
    }

    /// Pair `:   definition` items with the paragraph before them; a run of
    /// terms and definitions is one list.
    fn pair_definitions(&mut self, items: Vec<Item>) -> Vec<Block> {
        let mut out: Vec<Block> = Vec::new();
        for item in items {
            match item {
                Item::Block(block) => out.push(block),
                Item::Definition(content, span) => {
                    let term = match out.last() {
                        Some(Block::Para(_)) => match out.pop() {
                            Some(Block::Para(para)) => Some(para),
                            _ => None,
                        },
                        _ => None,
                    };
                    match (out.last_mut(), term) {
                        // Another definition of the previous term.
                        (Some(Block::DefinitionList(dl)), None) => {
                            if let Some(last) = dl.items.last_mut() {
                                last.1.push(content);
                            }
                            dl.meta.span = dl.meta.span.join(span);
                        }
                        // A new term of the same list.
                        (Some(Block::DefinitionList(dl)), Some(para)) => {
                            dl.items.push((para.content, vec![content]));
                            dl.meta.span = dl.meta.span.join(span);
                        }
                        (_, term) => {
                            let start = term.as_ref().map_or(span, |p| p.meta.span);
                            let meta = self.meta(start.join(span));
                            let term_content = term.map(|p| p.content).unwrap_or_default();
                            out.push(Block::DefinitionList(DefinitionList {
                                meta,
                                items: vec![(term_content, vec![content])],
                            }));
                        }
                    }
                }
            }
        }
        out
    }

    /// Turn `Kind: text {#id}` paragraphs into captions attached to a
    /// neighbouring float (design C7).
    fn attach_captions(&mut self, blocks: Vec<Block>) -> Vec<Block> {
        let mut out: Vec<Block> = Vec::with_capacity(blocks.len());
        let mut iter = blocks.into_iter().peekable();
        while let Some(block) = iter.next() {
            let candidate = match &block {
                Block::Para(para) => is_caption(&para.content).is_some(),
                Block::Caption(_) => true,
                _ => false,
            };
            if !candidate {
                out.push(block);
                continue;
            }
            let previous_is_host =
                out.last().is_some_and(is_float) && !matches!(out.last(), Some(Block::Caption(_)));
            let next_is_host = iter.peek().is_some_and(is_float);
            let mut caption = match block {
                Block::Caption(c) => c,
                Block::Para(para) => match self.caption_from(para, None) {
                    Some(c) => c,
                    None => unreachable!("checked by is_caption"),
                },
                _ => unreachable!(),
            };
            // A generic `/// caption` takes the kind of its float.
            if self.generic_captions.contains(&caption.meta.id) {
                let host = if previous_is_host {
                    out.last()
                } else if next_is_host {
                    iter.peek()
                } else {
                    None
                };
                caption.kind = match host {
                    Some(Block::Table(_) | Block::TableConfig(_)) => CaptionKind::Table,
                    Some(Block::CodeBlock(_)) => CaptionKind::Listing,
                    _ => CaptionKind::Figure,
                };
            }
            if previous_is_host {
                caption.position = CaptionPosition::After;
                // A `yaml table-config` fence belongs right after its table,
                // before the caption (canonical order: table, config, caption).
                if matches!(out.last(), Some(Block::Table(_)))
                    && matches!(iter.peek(), Some(Block::TableConfig(_)))
                {
                    let config = iter.next().expect("peeked");
                    out.push(config);
                }
                out.push(Block::Caption(caption));
            } else if next_is_host {
                // The IR keeps the caption after its host whatever the
                // source order; `position` records the spelling.
                caption.position = CaptionPosition::Before;
                let host = iter.next().expect("peeked");
                let table = matches!(host, Block::Table(_));
                out.push(host);
                if table && matches!(iter.peek(), Some(Block::TableConfig(_))) {
                    let config = iter.next().expect("peeked");
                    out.push(config);
                }
                out.push(Block::Caption(caption));
            } else {
                // Inside `::: figure` a free caption captions the figure itself.
                if !self.in_figure {
                    self.diag(
                        Code::CaptionNoHost,
                        caption.meta.span,
                        "caption line with no table, figure or listing next to it",
                    );
                }
                out.push(Block::Caption(caption));
            }
        }
        out
    }

    /// Build a caption from a `Kind: …` paragraph.
    fn caption_from(&mut self, para: Para, attrs: Option<Attrs>) -> Option<Caption> {
        let (kind, skip) = is_caption(&para.content)?;
        let mut content = para.content;
        if let Some(Inline::Str(first)) = content.first_mut() {
            let rest = first.text[skip..].trim_start().to_string();
            // The span starts after `Kind:` and the space, as the text does.
            let removed = (first.text.len() - rest.len()) as u32;
            if first.meta.span.len() >= removed {
                first.meta.span.start += removed;
            }
            first.text = rest;
            if first.text.is_empty() {
                content.remove(0);
            }
        }
        let attrs = match attrs {
            Some(a) => a,
            None => take_trailing_attrs(&mut content).unwrap_or_default(),
        };
        trim_trailing_space(&mut content);
        Some(Caption {
            meta: para.meta,
            kind,
            content,
            attrs,
            position: CaptionPosition::After,
        })
    }
}

/// Local offset, in `ctx`'s text, of the body of a fence, when that body
/// is a verbatim slice of it. `None` for an indented fence, whose body the
/// tokenizer dedents line by line: nothing parsed out of it can be given
/// a span of the source.
fn fence_body_at(
    ctx: &Ctx,
    position: Option<&tmark_markdown::unist::Position>,
    body: &str,
) -> Option<usize> {
    if body.is_empty() {
        return None;
    }
    Some(position?.start.offset + ctx.slice(position).find(body)?)
}

/// Local offset, in `ctx`'s text, of the info string of a `!!!` line: the
/// marker, then the blanks after it. The tokenizer keeps the info as a
/// verbatim slice of that line (only its trailing blanks are dropped), so
/// a title parsed out of it can be given spans of the file rather than
/// spans of the title alone (spec §Round-trip and source spans).
fn admonition_info_at(
    ctx: &Ctx,
    position: Option<&tmark_markdown::unist::Position>,
    marker: &str,
    info: &str,
) -> Option<usize> {
    if info.is_empty() {
        return None;
    }
    let start = position?.start.offset;
    let line = ctx.slice(position).lines().next()?;
    let rest = line.get(marker.len()..)?;
    Some(start + marker.len() + rest.find(info)?)
}

/// `Kind:` at the start of a paragraph: the kind and the bytes to skip.
fn is_caption(content: &[Inline]) -> Option<(CaptionKind, usize)> {
    let Some(Inline::Str(first)) = content.first() else {
        return None;
    };
    for (word, kind) in [
        ("Table:", CaptionKind::Table),
        ("Figure:", CaptionKind::Figure),
        ("Listing:", CaptionKind::Listing),
    ] {
        if let Some(rest) = first.text.strip_prefix(word) {
            if rest.starts_with(' ') || rest.starts_with('\t') {
                return Some((kind, word.len()));
            }
        }
    }
    None
}

/// Blocks a caption can attach to (`Block::is_float`, spec §Caption).
fn is_float(block: &Block) -> bool {
    block.is_float()
}

/// The math a display fence carries on its own line: what follows the
/// opening `$$` in `source`, the whole block's text (spec §Math (display)).
/// Empty when the fence line holds nothing else, or when what it holds is
/// the attribute list (`$$ {#eq:x}`), which the fence meta already parsed.
fn math_head(source: &str) -> &str {
    let head = source.trim_start_matches([' ', '\t']);
    let head = head.trim_start_matches('$');
    let head = match head.find('\n') {
        Some(at) => &head[..at],
        None => head,
    };
    let head = head.trim();
    let attributes = looks_like_attributes(head.as_bytes(), 0) && head.ends_with('}');
    match attributes {
        true => "",
        false => head,
    }
}

/// An attribute list left as a trailing literal `{…}` by the inline lowering
/// (block quotes take `{.epigraph}` this way).
fn take_trailing_attrs(content: &mut Vec<Inline>) -> Option<Attrs> {
    let Some(Inline::Str(last)) = content.last() else {
        return None;
    };
    let text = last.text.trim_end();
    let inner = text.strip_prefix('{')?.strip_suffix('}')?;
    // The text is decoded, so the id's position is not known.
    let mut attrs = parse_attrs(inner)?;
    attrs.id_span = None;
    content.pop();
    trim_trailing_space(content);
    Some(attrs)
}

/// `*[KEY]: expansion` lines (spec §Glossary and acronyms), each with the
/// byte offset and length of its line in `text`.
fn abbreviations(text: &str) -> Option<Vec<(usize, usize, String, String)>> {
    let mut out = Vec::new();
    let mut at = 0;
    for raw in text.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\n', '\r']);
        let start = at + (line.len() - line.trim_start().len());
        at += raw.len();
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix("*[")?;
        let (key, expansion) = rest.split_once("]:")?;
        if key.is_empty() || key.contains(']') {
            return None;
        }
        out.push((
            start,
            trimmed.len(),
            key.to_string(),
            expansion.trim().to_string(),
        ));
    }
    (!out.is_empty()).then_some(out)
}

/// `- [.] item` partial tasks (feature `tasklist.partial`).
fn take_partial_task(content: &mut [Block]) -> Option<Task> {
    let first = content.first_mut()?;
    let inlines = match first {
        Block::Para(p) => &mut p.content,
        Block::Plain(p) => &mut p.content,
        _ => return None,
    };
    let Some(Inline::Str(s)) = inlines.first_mut() else {
        return None;
    };
    let rest = s.text.strip_prefix("[.] ")?;
    s.text = rest.to_string();
    Some(Task::Partial)
}

/// Drop the `;` that disabled a snippet line, once the line has lowered
/// to inlines: it is the first character of the first text run.
fn drop_snippet_escape(content: &mut [Inline]) {
    if let Some(Inline::Str(s)) = content.first_mut() {
        let indent = s.text.len() - s.text.trim_start().len();
        if s.text[indent..].starts_with(';') {
            s.text.remove(indent);
        }
    }
}

/// The `stops` of a collected body whose first `skip` bytes are dropped.
fn shift_stops(stops: &[(usize, usize)], skip: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, (local, source)) in stops.iter().enumerate() {
        if *local >= skip {
            out.push((local - skip, *source));
        } else if !stops.get(i + 1).is_some_and(|next| next.0 <= skip) {
            out.push((0, source + (skip - local)));
        }
    }
    out
}

/// The option lines of a `pymdownx.blocks` fence: indented `key: value`
/// YAML right after the opening line. Returns their byte length in the
/// body and the attribute list the `attrs:` mapping carries.
fn block_options(value: &str) -> (usize, Attrs) {
    let mut options_len = 0;
    let mut yaml = String::new();
    for line in value.split_inclusive('\n') {
        let is_option = (line.starts_with("    ") || line.starts_with('\t'))
            && line.trim_start().split_once(':').is_some_and(|(k, _)| {
                !k.is_empty()
                    && k.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            });
        if !is_option {
            break;
        }
        yaml.push_str(line.trim_start());
        options_len += line.len();
    }
    let mut attrs = Attrs::new();
    if let Ok(serde_yaml_ng::Value::Mapping(options)) =
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&yaml)
    {
        if let Some(serde_yaml_ng::Value::Mapping(list)) = options.get("attrs") {
            for (k, v) in list {
                let (Some(k), Some(v)) = (k.as_str(), v.as_str()) else {
                    continue;
                };
                match k {
                    "id" => attrs.id = Some(v.to_string()),
                    "class" => attrs
                        .classes
                        .extend(v.split_whitespace().map(str::to_string)),
                    _ => attrs.kv.push((k.to_string(), v.to_string())),
                }
            }
        }
    }
    (options_len, attrs)
}

/// The argument of a `pymdownx.blocks.html` fence, `/// html | <selector>`
/// (spec §Div, Appendix "PyMdownX compatibility profile"): a CSS-like
/// selector naming the element that wraps the body — a tag, then any
/// number of `#id`, `.class` and `[name]` / `[name=value]` groups, the
/// value bare, single- or double-quoted. `class` and `id` written as
/// attributes land where the dotted and hashed forms do. `None` when the
/// info string is not that form, so a bare `/// html` stays the raw
/// fence it has always been.
fn html_selector(info: &str) -> Option<(String, Attrs)> {
    let rest = info.trim().strip_prefix("html")?;
    let selector = rest.trim_start().strip_prefix('|')?.trim();
    let tag_len = selector
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    if tag_len == 0 || !selector.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let tag = selector[..tag_len].to_ascii_lowercase();
    let mut attrs = Attrs::new();
    let mut rest = &selector[tag_len..];
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[') {
            let (name, after) = ident(after.trim_start())?;
            let after = after.trim_start();
            let (value, after) = match after.strip_prefix('=') {
                Some(after) => quoted_or_ident(after.trim_start())?,
                None => (String::new(), after),
            };
            rest = after.trim_start().strip_prefix(']')?;
            put(&mut attrs, &name.to_ascii_lowercase(), value);
            continue;
        }
        let (sigil, after) = rest.split_at(1);
        let (name, after) = ident(after)?;
        match sigil {
            "#" => attrs.id = Some(name),
            "." => attrs.classes.push(name),
            _ => return None,
        }
        rest = after;
    }
    Some((tag, attrs))
}

/// One selector identifier: the characters `pymdownx.blocks.html` reads
/// in a tag, class, id or attribute name.
fn ident(s: &str) -> Option<(String, &str)> {
    let len = s
        .bytes()
        .take_while(|b| !b.is_ascii() || b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        .count();
    (len > 0).then(|| (s[..len].to_string(), &s[len..]))
}

/// An attribute value: `"…"`, `'…'` or a bare identifier.
fn quoted_or_ident(s: &str) -> Option<(String, &str)> {
    for quote in ['"', '\''] {
        if let Some(after) = s.strip_prefix(quote) {
            let end = after.find(quote)?;
            return Some((after[..end].to_string(), &after[end + 1..]));
        }
    }
    ident(s)
}

/// `class` and `id` written as attributes go where `.cls` and `#id` do.
fn put(attrs: &mut Attrs, name: &str, value: String) {
    match name {
        "id" => attrs.id = Some(value),
        "class" => attrs
            .classes
            .extend(value.split_whitespace().map(str::to_string)),
        _ => attrs.kv.push((name.to_string(), value)),
    }
}

/// The opening tag of an `md_in_html` block: `<tag attr… markdown>` on
/// one line, the `markdown` attribute bare or valued (`markdown="1"`,
/// `"block"`, `"span"`). Returns the tag name and the attribute list built
/// from `id` and `class`; other attributes are kept as keys.
fn markdown_tag(line: &str) -> Option<(String, Attrs)> {
    let line = line.trim();
    let inner = line.strip_prefix('<')?.strip_suffix('>')?;
    let inner = inner.strip_suffix('/').unwrap_or(inner);
    let name_len = inner
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'-')
        .count();
    if name_len == 0 || !inner.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }
    let tag = inner[..name_len].to_ascii_lowercase();
    let mut attrs = Attrs::new();
    let mut markdown = false;
    let mut rest = inner[name_len..].trim_start();
    while !rest.is_empty() {
        let name_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b':'))
            .count();
        if name_len == 0 {
            return None;
        }
        let name = rest[..name_len].to_ascii_lowercase();
        rest = &rest[name_len..];
        let mut value = None;
        if let Some(after) = rest.strip_prefix('=') {
            let (v, tail) = if let Some(quoted) = after.strip_prefix('"') {
                let end = quoted.find('"')?;
                (&quoted[..end], &quoted[end + 1..])
            } else if let Some(quoted) = after.strip_prefix('\'') {
                let end = quoted.find('\'')?;
                (&quoted[..end], &quoted[end + 1..])
            } else {
                let end = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..end], &after[end..])
            };
            value = Some(v.to_string());
            rest = tail;
        }
        match name.as_str() {
            "markdown" => markdown = true,
            "id" => attrs.id = value,
            "class" => attrs.classes.extend(
                value
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_string),
            ),
            _ => attrs.kv.push((name, value.unwrap_or_default())),
        }
        rest = rest.trim_start();
    }
    markdown.then_some((tag, attrs))
}

/// An inline that typesets nothing: the trailing whitespace a paragraph
/// whose whole content is one strong span may still carry.
fn is_blank(inline: &Inline) -> bool {
    match inline {
        Inline::Str(s) => s.text.trim().is_empty(),
        Inline::Space(_) | Inline::SoftBreak(_) => true,
        _ => false,
    }
}
