//! Inline lowering: mdast phrasing nodes to `Inline`, including the brace
//! groups (roles, attribute lists, moustaches), references, definitions and
//! the attention sugar. Spec §Inline text, §Roles, §Attributes, §Two sigils.

use tmark_ir::{
    critic,
    registry::{self, ArgStyle},
    Aside, Attrs, Block, Code, CodeInline, Comment, CounterItem, Emph, Highlight, Image,
    IndexEntry, Inline, Keystroke, LineBreak, Link, Math, Note, Plain, ProgressBar, QuoteKind,
    Quoted, RawInline, Ref, Side, SmallCaps, SoftBreak, Span, SpanNode, Str, Strikeout, Strong,
    SubSpan, Subscript, Superscript, Target, Underline, Var,
};
use tmark_markdown::mdast::{Node, TmarkMarkKind};
use tmark_markdown::tmark::looks_like_attributes;

use super::head::{
    attrs_text, has_attr_colon, parse_attrs, parse_ref_items, parse_role_head, RoleHead,
};
use super::sugar::{self, Shortcode};
use super::{decode_escapes, plain_text, Ctx, Lowerer};

/// What a brace group is, decided by its text (spec §Roles: "a parser
/// decides after reading one token").
enum BraceKind {
    Moustache(Vec<String>),
    Attrs(Attrs),
    Role(RoleHead),
    Literal,
}

fn classify(node: &tmark_markdown::mdast::TmarkBrace) -> BraceKind {
    if node.moustache {
        let path = node.value.trim();
        let ok = !path.is_empty()
            && path.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            });
        return if ok {
            BraceKind::Moustache(path.split('.').map(str::to_string).collect())
        } else {
            BraceKind::Literal
        };
    }
    let mut probe = String::with_capacity(node.value.len() + 2);
    probe.push('{');
    probe.push_str(&node.value);
    probe.push('}');
    if looks_like_attributes(probe.as_bytes(), 0) {
        return parse_attrs(&node.value).map_or(BraceKind::Literal, BraceKind::Attrs);
    }
    parse_role_head(&node.value).map_or(BraceKind::Literal, BraceKind::Role)
}

/// Inline lowering result: the inlines and, when the last node was an
/// attribute list that no inline could host, that list for the block to take
/// (headings, captions, block quotes).
pub(crate) struct Lowered {
    pub inlines: Vec<Inline>,
    pub tail_attrs: Option<(Attrs, Span)>,
}

impl Lowerer {
    pub fn lower_inlines(&mut self, nodes: &[Node], ctx: &Ctx) -> Lowered {
        let mut out: Vec<Inline> = Vec::new();
        let mut tail_attrs = None;
        let mut index = 0;
        while index < nodes.len() {
            let node = &nodes[index];
            index += 1;
            match node {
                Node::Text(text) => {
                    self.lower_text(&text.value, text.position.as_ref(), ctx, &mut out)
                }
                Node::Emphasis(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    out.push(Inline::Emph(Emph { meta, content }));
                }
                Node::Strong(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    let underscore = ctx.slice(n.position.as_ref()).starts_with("__");
                    if underscore && !self.options.strict {
                        out.push(Inline::SmallCaps(SmallCaps { meta, content }));
                    } else {
                        out.push(Inline::Strong(Strong { meta, content }));
                    }
                }
                Node::Delete(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    out.push(Inline::Strikeout(Strikeout { meta, content }));
                }
                Node::InlineCode(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    let (lang, text) = match n.value.strip_prefix("#!") {
                        Some(rest) => match rest.split_once(' ') {
                            Some((lang, code)) if !lang.is_empty() => {
                                (Some(lang.to_string()), code.to_string())
                            }
                            _ => (None, n.value.clone()),
                        },
                        None => (None, n.value.clone()),
                    };
                    out.push(Inline::Code(CodeInline { meta, text, lang }));
                }
                Node::InlineMath(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    let display = ctx.slice(n.position.as_ref()).starts_with("$$");
                    out.push(Inline::Math(Math {
                        meta,
                        text: n.value.clone(),
                        display,
                    }));
                }
                Node::Break(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    out.push(Inline::LineBreak(LineBreak { meta }));
                }
                Node::Link(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    let meta = self.meta(span);
                    // Deprecated `[](gls:term)` glossary reference (Appendix
                    // "Deprecation schedule", C20): a bare `Ref`.
                    if n.children.is_empty() && n.url.starts_with("gls:") && n.title.is_none() {
                        let at = n.position.as_ref().map_or(0, |p| p.start.offset) + 3;
                        self.deprecated(span, "[](gls:term)", "@gls:term");
                        out.push(Inline::Ref(Ref {
                            meta,
                            items: vec![tmark_ir::RefItem {
                                key: n.url.clone(),
                                key_span: SubSpan(self.span_of(ctx, at, at + n.url.len())),
                                ..Default::default()
                            }],
                            bracketed: false,
                        }));
                        continue;
                    }
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    out.push(Inline::Link(Link {
                        meta,
                        content,
                        target: target(&n.url),
                        title: n.title.clone(),
                    }));
                }
                Node::LinkReference(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    let (url, title) = self
                        .definitions
                        .get(&n.identifier)
                        .cloned()
                        .unwrap_or_default();
                    out.push(Inline::Link(Link {
                        meta,
                        content,
                        target: target(&url),
                        title,
                    }));
                }
                Node::Image(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    let (attrs, attrs_end) =
                        self.take_adjacent_attrs(nodes, &mut index, n.position.as_ref(), ctx);
                    let meta = self.meta(self.host_span(ctx, span, attrs_end));
                    let alt = self.lower_fragment(&n.alt, span);
                    out.push(Inline::Image(Image {
                        meta,
                        src: n.url.clone(),
                        alt,
                        attrs,
                    }));
                }
                Node::ImageReference(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    let (attrs, attrs_end) =
                        self.take_adjacent_attrs(nodes, &mut index, n.position.as_ref(), ctx);
                    let meta = self.meta(self.host_span(ctx, span, attrs_end));
                    let alt = self.lower_fragment(&n.alt, span);
                    let (url, _) = self
                        .definitions
                        .get(&n.identifier)
                        .cloned()
                        .unwrap_or_default();
                    out.push(Inline::Image(Image {
                        meta,
                        src: url,
                        alt,
                        attrs,
                    }));
                }
                Node::Html(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    out.push(html_inline(meta, &n.value));
                }
                Node::FootnoteReference(n) => {
                    let meta = self.meta_at(ctx, n.position.as_ref());
                    out.push(Inline::Note(Note {
                        meta,
                        label: Some(n.identifier.clone()),
                        content: Vec::new(),
                    }));
                }
                Node::TmarkMark(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    let meta = self.meta(span);
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    match n.kind {
                        TmarkMarkKind::Highlight => {
                            out.push(Inline::Highlight(Highlight { meta, content }));
                        }
                        TmarkMarkKind::Superscript => {
                            out.push(Inline::Superscript(Superscript { meta, content }));
                        }
                        TmarkMarkKind::Subscript if !self.options.strict => {
                            out.push(Inline::Subscript(Subscript { meta, content }));
                        }
                        TmarkMarkKind::Insert if self.features.inline_insert => {
                            out.push(Inline::Underline(Underline { meta, content }));
                        }
                        TmarkMarkKind::Keystroke => {
                            let keys = plain_text(&content)
                                .split('+')
                                .map(str::to_string)
                                .collect();
                            out.push(Inline::Keystroke(Keystroke { meta, keys }));
                        }
                        // Off: the markers are literal text around the content
                        // (`^^x^^` without `inline.insert` is the lint hint
                        // `feature-off`, spec §Inline text).
                        TmarkMarkKind::Subscript | TmarkMarkKind::Insert => {
                            let marker = if n.kind == TmarkMarkKind::Subscript {
                                "~"
                            } else {
                                "^^"
                            };
                            out.push(self.literal_text(span, marker));
                            out.extend(content);
                            out.push(self.literal_text(span, marker));
                        }
                    }
                }
                Node::TmarkReference(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    let meta = self.meta(span);
                    let local = n.position.as_ref().map_or(0, |p| p.start.offset);
                    let (items, bracketed) = match n.value.strip_prefix('[') {
                        // Deprecated `[^key]` citation (decision X7): one
                        // item, bare — the sugar was the short form
                        // (spec §Cite, C51) — so the fix prints `@key`.
                        Some(inner) if inner.starts_with('^') => {
                            let key = inner[1..].trim_end_matches(']');
                            let at = local + 2;
                            let item = tmark_ir::RefItem {
                                key: doi_key(key),
                                key_span: SubSpan(self.span_of(ctx, at, at + key.len())),
                                ..Default::default()
                            };
                            self.deprecated(span, "[^key]", "@key");
                            (vec![item], false)
                        }
                        Some(inner) => {
                            // Pandoc's `[@key, …]` import form carries `@` before
                            // each key; the item grammar is the same. The inner
                            // text starts after the `[` of the source.
                            let open = ctx.slice(n.position.as_ref()).find('[').unwrap_or(0);
                            let base = local + open + 1;
                            let mut items = parse_ref_items(inner.trim_end_matches(']'));
                            for item in &mut items {
                                let (s, e) =
                                    (item.key_span.0.start as usize, item.key_span.0.end as usize);
                                item.key_span = SubSpan(self.span_of(ctx, base + s, base + e));
                            }
                            (items, true)
                        }
                        // Deprecated `^[k1,k2]` citation group: one item per
                        // comma, bare like `[^key]`; the fix prints
                        // `@[k1; k2]` (several items) or `@k1`.
                        None if n.value.starts_with("^[") => {
                            let inner = n.value[2..].trim_end_matches(']');
                            let mut at = local + 2;
                            let mut items = Vec::new();
                            for key in inner.split(',') {
                                items.push(tmark_ir::RefItem {
                                    key: doi_key(key),
                                    key_span: SubSpan(self.span_of(ctx, at, at + key.len())),
                                    ..Default::default()
                                });
                                at += key.len() + 1;
                            }
                            self.deprecated(span, "^[k1,k2]", "@[k1; k2]");
                            (items, false)
                        }
                        None => (
                            vec![tmark_ir::RefItem {
                                key: doi_key(&n.value),
                                key_span: SubSpan(self.span_of(
                                    ctx,
                                    local + 1,
                                    local + 1 + n.value.len(),
                                )),
                                ..Default::default()
                            }],
                            false,
                        ),
                    };
                    out.push(Inline::Ref(Ref {
                        meta,
                        items,
                        bracketed,
                    }));
                }
                Node::TmarkSpan(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    // `[=45% "x"]{.thin}`: the tokenizer reads a bracket group
                    // hugging an attribute list as a span; the bar wins when
                    // the group is exactly its spelling (spec §ProgressBar).
                    let source = ctx.slice(n.position.as_ref());
                    let bar = sugar::progress_bar(source).filter(|p| p.len == source.len());
                    let (attrs, attrs_end) =
                        self.take_adjacent_attrs(nodes, &mut index, n.position.as_ref(), ctx);
                    let meta = self.meta(self.host_span(ctx, span, attrs_end));
                    if let Some(bar) = bar {
                        out.push(self.progress_bar(bar, span, meta, attrs));
                        continue;
                    }
                    let content = self.lower_inlines(&n.children, ctx).inlines;
                    out.push(Inline::Span(SpanNode {
                        meta,
                        content,
                        attrs,
                    }));
                }
                Node::TmarkDefine(n) => self.lower_define(n, nodes, &mut index, ctx, &mut out),
                Node::TmarkBrace(n) => {
                    let last = index == nodes.len();
                    if let Some(attrs) = self.lower_brace(n, nodes, &mut index, ctx, &mut out, last)
                    {
                        tail_attrs = Some(attrs);
                    }
                }
                Node::TmarkGroup(n) => {
                    // A group nothing claimed: literal brackets around its content.
                    let span = self.span(ctx, n.position.as_ref());
                    out.push(self.literal_text(span, "["));
                    out.extend(self.lower_inlines(&n.children, ctx).inlines);
                    out.push(self.literal_text(span, "]"));
                }
                Node::TmarkArgument(n) => {
                    let span = self.span(ctx, n.position.as_ref());
                    out.push(
                        self.literal_text(span, decode_escapes(ctx.slice(n.position.as_ref()))),
                    );
                }
                other => {
                    // MDX and anything unexpected: the source text, literally.
                    let position = other.position();
                    if position.is_some() {
                        let span = self.span(ctx, position);
                        out.push(self.literal_text(span, decode_escapes(ctx.slice(position))));
                    }
                }
            }
        }
        Lowered {
            inlines: merge_strs(out),
            tail_attrs,
        }
    }

    /// Text with soft breaks split out.
    ///
    /// The value of a text node is decoded (escapes and character references
    /// resolved, continuation indent stripped), so spans come from the
    /// source slice, line by line, not from the value's lengths.
    fn lower_text(
        &mut self,
        value: &str,
        position: Option<&tmark_markdown::unist::Position>,
        ctx: &Ctx,
        out: &mut Vec<Inline>,
    ) {
        let span = self.span(ctx, position);
        let source = ctx.slice(position);
        self.compat_scan_text(value, source, span);
        let pieces: Vec<&str> = value.split('\n').collect();
        if pieces.len() == 1 {
            if !value.is_empty() {
                self.text_pieces(value, span, source, out);
            }
            return;
        }
        // Source lines of the node, as byte ranges in the parsed text.
        let source = ctx.slice(position);
        let base = position.map_or(0, |p| p.start.offset);
        let mut lines: Vec<(usize, usize)> = Vec::new();
        let mut line_start = 0;
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                lines.push((line_start, i));
                line_start = i + 1;
            }
        }
        lines.push((line_start, source.len()));
        let aligned = lines.len() == pieces.len();
        for (i, piece) in pieces.iter().enumerate() {
            if i > 0 {
                let at = if aligned {
                    self.span_of(ctx, base + lines[i].0 - 1, base + lines[i].0)
                } else {
                    span
                };
                out.push(Inline::SoftBreak(SoftBreak {
                    meta: self.meta(at),
                }));
            }
            if piece.is_empty() {
                continue;
            }
            let (piece_span, piece_source) = if aligned {
                let (from, to) = lines[i];
                // Continuation indent is stripped from the value.
                let skip = source[from..to].len() - source[from..to].trim_start().len();
                (
                    self.span_of(ctx, base + from + skip, base + to),
                    &source[from + skip..to],
                )
            } else {
                (span, source)
            };
            self.text_pieces(piece, piece_span, piece_source, out);
        }
    }

    /// One line of text as `Str` runs with the inline sugar found in it
    /// turned into nodes: progress bars, emoji (a `Str` holding the
    /// character), icon shortcodes (`Span{.icon media=web}`), quoted
    /// phrases, smart symbols. `source` is the line's source (the whole
    /// node's when the lines could not be told apart); when the text is
    /// its source byte for byte, sub-spans are precise, otherwise every
    /// node takes `span`. A spelling with a backslash escape inside it in
    /// the source (`\:smile:`, `\[=1%]`, `\(c)`, `1\/2`, `\"x"`) is the
    /// author's literal text (spec §Round-trip and source spans: the
    /// printer writes that escape back).
    fn text_pieces(&mut self, text: &str, span: Span, source: &str, out: &mut Vec<Inline>) {
        let exact = text == source;
        // Where each byte of the decoded text sits in the source and
        // whether it came from a backslash escape; when the source cannot
        // be aligned (an entity the tokenizer decoded in a way this scan
        // does not know), every node takes `span` and any escaped
        // spelling of the character anywhere in the source counts.
        let aligned = if exact { None } else { align(text, source) };
        let sub = |this: &Self, start: usize, end: usize| {
            let (from, to) = if exact {
                (start, end)
            } else if let Some(aligned) = &aligned {
                (aligned.offsets[start], aligned.offsets[end])
            } else {
                return span;
            };
            Span::new(this.file, span.start + from as u32, span.start + to as u32)
        };
        let escaped = |from: usize, to: usize| -> bool {
            if exact {
                return false;
            }
            match &aligned {
                Some(aligned) => aligned.escaped[from..to].iter().any(|b| *b),
                None => text[from..to]
                    .chars()
                    .any(|c| c.is_ascii_punctuation() && source.contains(&format!("\\{c}"))),
            }
        };
        let mut buffer = String::new();
        let mut buffer_start = 0;
        let mut i = 0;
        let flush = |this: &mut Self,
                     buffer: &mut String,
                     start: usize,
                     end: usize,
                     out: &mut Vec<Inline>| {
            if !buffer.is_empty() {
                out.push(Inline::Str(Str {
                    meta: this.meta(sub(this, start, end)),
                    text: std::mem::take(buffer),
                }));
            }
        };
        while i < text.len() {
            let rest = &text[i..];
            if rest.starts_with("[=") {
                if let Some(bar) = sugar::progress_bar(rest) {
                    if escaped(i, i + 1) {
                        // The literal spelling, quotes included.
                        buffer.push_str(&text[i..i + bar.len]);
                        i += bar.len;
                        continue;
                    }
                    flush(self, &mut buffer, buffer_start, i, out);
                    let len = bar.len;
                    let at = sub(self, i, i + len);
                    let meta = self.meta(at);
                    let node = self.progress_bar(bar, at, meta, Attrs::new());
                    out.push(node);
                    i += len;
                    buffer_start = i;
                    continue;
                }
            }
            if rest.starts_with('[') {
                // `[text][id]`: the reference-style link of spec §Ref.
                // The tokenizer leaves the spelling as text when no
                // definition matches, so the scan sees only the form the
                // resolution decides — a reference to the label `id`, or
                // the literal brackets CommonMark makes of it. A
                // backslash anywhere in the spelling is the author's
                // literal text.
                if let Some(link) = sugar::reference_link(text, i) {
                    if !escaped(i, i + link.len) {
                        flush(self, &mut buffer, buffer_start, i, out);
                        let at = sub(self, i, i + link.len);
                        let label = sub(self, link.text.start, link.text.end);
                        let content = vec![Inline::Str(Str {
                            meta: self.meta(label),
                            text: text[link.text].to_string(),
                        })];
                        let meta = self.meta(at);
                        out.push(Inline::Link(Link {
                            meta,
                            content,
                            target: Target::Reference(link.id),
                            title: None,
                        }));
                        i += link.len;
                        buffer_start = i;
                        continue;
                    }
                }
            }
            if rest.starts_with(':') && !escaped(i, i + 1) {
                if let Some((len, kind)) = sugar::shortcode(text, i) {
                    match kind {
                        Shortcode::Emoji(character) => buffer.push_str(character),
                        Shortcode::Icon => {
                            flush(self, &mut buffer, buffer_start, i, out);
                            let at = sub(self, i, i + len);
                            let meta = self.meta(at);
                            let content = vec![self.literal_text(at, &text[i..i + len])];
                            let mut attrs = Attrs::new();
                            attrs.classes.push("icon".to_string());
                            attrs.kv.push(("media".to_string(), "web".to_string()));
                            out.push(Inline::Span(SpanNode {
                                meta,
                                content,
                                attrs,
                            }));
                            buffer_start = i + len;
                        }
                    }
                    i += len;
                    continue;
                }
            }
            if rest.starts_with('"') && !escaped(i, i + 1) {
                if let Some((start, end)) =
                    sugar::quoted(text, i).filter(|(_, end)| !escaped(*end, end + 1))
                {
                    flush(self, &mut buffer, buffer_start, i, out);
                    let at = sub(self, i, end + 1);
                    let meta = self.meta(at);
                    let content = vec![Inline::Str(Str {
                        meta: self.meta(sub(self, start, end)),
                        text: text[start..end].to_string(),
                    })];
                    out.push(Inline::Quoted(Quoted {
                        meta,
                        kind: QuoteKind::Double,
                        content,
                    }));
                    i = end + 1;
                    buffer_start = i;
                    continue;
                }
            }
            if let Some((len, symbol)) = sugar::smart_symbol(text, i) {
                // An escaped spelling is literal as a whole: `\<-->` is
                // not `<` and an arrow.
                buffer.push_str(if escaped(i, i + len) {
                    &text[i..i + len]
                } else {
                    symbol
                });
                i += len;
                continue;
            }
            let c = rest.chars().next().expect("in bounds");
            buffer.push(c);
            i += c.len_utf8();
        }
        flush(self, &mut buffer, buffer_start, text.len(), out);
    }

    /// A `ProgressBar` from a recognised spelling at `span`; the fraction
    /// form is deprecated, with the canonical head as its fix (spec
    /// §ProgressBar, Appendix "Deprecation schedule").
    fn progress_bar(
        &mut self,
        bar: sugar::Progress,
        span: Span,
        meta: tmark_ir::Meta,
        attrs: Attrs,
    ) -> Inline {
        let node = ProgressBar {
            meta,
            value: bar.value,
            label: bar.label,
            attrs,
        };
        if bar.fraction {
            let replacement = node.head_text();
            self.deprecated_with_fix(span, "[=a/b \"…\"]", "[=NN% \"…\"]", replacement);
        }
        Inline::ProgressBar(node)
    }

    /// The deprecated `{: …}` attribute colon (spec §Attributes): a
    /// `deprecated` diagnostic on the brace with the canonical list as its
    /// fix.
    pub(crate) fn attrs_colon(&mut self, value: &str, attrs: &Attrs, span: Span) {
        if has_attr_colon(value) {
            self.deprecated_with_fix(span, "{: …}", "{…}", attrs_text(attrs));
        }
    }

    /// If the next node is an attribute list right after `position`, take
    /// it and return the end offset of the list, so that the host's span
    /// covers its attributes.
    fn take_adjacent_attrs(
        &mut self,
        nodes: &[Node],
        index: &mut usize,
        position: Option<&tmark_markdown::unist::Position>,
        ctx: &Ctx,
    ) -> (Attrs, Option<usize>) {
        if let Some(Node::TmarkBrace(brace)) = nodes.get(*index) {
            let adjacent = match (position, brace.position.as_ref()) {
                (Some(host), Some(b)) => host.end.offset == b.start.offset,
                _ => false,
            };
            if adjacent && !brace.moustache {
                if let BraceKind::Attrs(mut attrs) = classify(brace) {
                    *index += 1;
                    if let Some(p) = brace.position.as_ref() {
                        self.relocate_attrs(ctx, &mut attrs, p.start.offset + 1);
                    }
                    let span = self.span(ctx, brace.position.as_ref());
                    self.attrs_colon(&brace.value, &attrs, span);
                    return (attrs, brace.position.as_ref().map(|p| p.end.offset));
                }
            }
        }
        (Attrs::new(), None)
    }

    /// The span of a host node extended over its attribute list.
    fn host_span(&self, ctx: &Ctx, span: Span, attrs_end: Option<usize>) -> Span {
        match attrs_end {
            Some(end) => Span::new(self.file, span.start, ctx.map.translate(end) as u32),
            None => span,
        }
    }

    /// A brace group: moustache, attribute list, role head, or literal.
    /// Returns an attribute list the block should host when the group is the
    /// last inline and no inline could host it.
    fn lower_brace(
        &mut self,
        node: &tmark_markdown::mdast::TmarkBrace,
        nodes: &[Node],
        index: &mut usize,
        ctx: &Ctx,
        out: &mut Vec<Inline>,
        last: bool,
    ) -> Option<(Attrs, Span)> {
        let span = self.span(ctx, node.position.as_ref());
        match classify(node) {
            BraceKind::Moustache(path) => {
                let meta = self.meta(span);
                out.push(Inline::Var(Var { meta, path }));
            }
            BraceKind::Attrs(mut attrs) => {
                // `{margin}[…]{l}`-style suffixes are handled by the role;
                // here an attribute list needs a host: the previous inline
                // when adjacent, else the block when last, else nowhere.
                if let Some(p) = node.position.as_ref() {
                    self.relocate_attrs(ctx, &mut attrs, p.start.offset + 1);
                }
                self.attrs_colon(&node.value, &attrs, span);
                // A progress bar the text scan produced, hugging the list:
                // the list is the bar's (spec §ProgressBar).
                if let Some(Inline::ProgressBar(bar)) = out.last_mut() {
                    if bar.meta.span.end == span.start {
                        bar.attrs = attrs;
                        bar.meta.span = bar.meta.span.join(span);
                        return None;
                    }
                }
                // `[](){#id}`: an empty link hugging an attribute list is
                // the MkDocs/autorefs anchor idiom. An empty link is no
                // link; what the author wrote is an anchor, which is the
                // zero-width span `[]{#id}` (spec §Attributes) — deprecated
                // in favour of that spelling.
                if let Some(Inline::Link(link)) = out.last() {
                    let empty = link.content.is_empty()
                        && matches!(&link.target, Target::Url(url) if url.is_empty())
                        && link.title.is_none();
                    if empty && link.meta.span.end == span.start {
                        let Some(Inline::Link(link)) = out.pop() else {
                            unreachable!()
                        };
                        let whole = link.meta.span.join(span);
                        self.deprecated_with_fix(
                            whole,
                            "[](){…}",
                            "[]{…}",
                            format!("[]{}", attrs_text(&attrs)),
                        );
                        let meta = self.meta(whole);
                        out.push(Inline::Span(SpanNode {
                            meta,
                            content: Vec::new(),
                            attrs,
                        }));
                        return None;
                    }
                }
                if last {
                    trim_trailing_space(out);
                    return Some((attrs, span));
                }
                self.diag(
                    Code::AttrNoHost,
                    span,
                    "attribute list with no host element",
                );
                out.push(
                    self.literal_text(span, decode_escapes(ctx.slice(node.position.as_ref()))),
                );
            }
            BraceKind::Role(head) => self.lower_role(node, head, nodes, index, ctx, out),
            BraceKind::Literal => {
                if !self.lower_critic(node, ctx, span, out) {
                    out.push(
                        self.literal_text(span, decode_escapes(ctx.slice(node.position.as_ref()))),
                    );
                }
            }
        }
        None
    }

    /// Critic markup (spec Appendix "PyMdownX compatibility profile",
    /// challenge C49), which the tokenizer hands over as a literal brace
    /// group: `{++ins++}`, `{--del--}`, `{~~old~>new~~}`, `{==mark==}` and
    /// `{>>note<<}`. The four annotations are a `Span{.critic}` around the
    /// node the appendix names — `Underline`, `Strikeout`, the pair of the
    /// two, `Comment` — so that a writer can tell a reviewer's mark from an
    /// author's own underline or note and reach the `ts-critic` contract;
    /// `{==x==}` is critic's spelling of `pymdownx.mark` and lowers to the
    /// plain `Highlight` that `==x==` produces. The inner text is re-parsed
    /// as inline content, so markup inside an annotation is markup. Returns
    /// whether the group was critic markup.
    ///
    /// Nothing fires inside a code span, a fence, math, a raw block or a
    /// link destination: the brace group is a text construct (C14), which
    /// is where TMark parts from PyMdownX. The strict profile rejects the
    /// appendix, so the group stays literal there.
    fn lower_critic(
        &mut self,
        node: &tmark_markdown::mdast::TmarkBrace,
        ctx: &Ctx,
        span: Span,
        out: &mut Vec<Inline>,
    ) -> bool {
        if self.options.strict {
            return false;
        }
        let (Some(spelling), Some(position)) =
            (critic::spelling(&node.value), node.position.as_ref())
        else {
            return false;
        };
        // The text between the braces starts one byte after `{`.
        let base = position.start.offset + 1;
        let meta = self.meta(span);
        let attrs = Attrs {
            classes: vec![critic::CLASS.to_string()],
            ..Attrs::new()
        };
        let content = match spelling {
            critic::Spelling::Highlight(start, end) => {
                let content = self.lower_inline_slice(&node.value[start..end], base + start, ctx);
                out.push(Inline::Highlight(Highlight { meta, content }));
                return true;
            }
            critic::Spelling::Insert(start, end) => {
                vec![self.critic_underline(node, ctx, base, start, end)]
            }
            critic::Spelling::Delete(start, end) => {
                vec![self.critic_strikeout(node, ctx, base, start, end)]
            }
            critic::Spelling::Substitute {
                old: (old_start, old_end),
                new: (new_start, new_end),
            } => vec![
                self.critic_strikeout(node, ctx, base, old_start, old_end),
                self.critic_underline(node, ctx, base, new_start, new_end),
            ],
            critic::Spelling::Comment(start, end) => {
                let text = &node.value[start..end];
                let inner = self.slice_span(ctx, base + start, text);
                let meta = self.meta(inner);
                vec![Inline::Comment(Comment {
                    meta,
                    text: text.to_string(),
                })]
            }
        };
        out.push(Inline::Span(SpanNode {
            meta,
            content,
            attrs,
        }));
        true
    }

    /// The source span of `text` at offset `at` in `ctx`.
    fn slice_span(&self, ctx: &Ctx, at: usize, text: &str) -> Span {
        Span::new(
            self.file,
            ctx.map.translate(at) as u32,
            ctx.map.translate(at + text.len()) as u32,
        )
    }

    fn critic_underline(
        &mut self,
        node: &tmark_markdown::mdast::TmarkBrace,
        ctx: &Ctx,
        base: usize,
        start: usize,
        end: usize,
    ) -> Inline {
        let text = &node.value[start..end];
        let inner = self.slice_span(ctx, base + start, text);
        let meta = self.meta(inner);
        let content = self.lower_inline_slice(text, base + start, ctx);
        Inline::Underline(Underline { meta, content })
    }

    fn critic_strikeout(
        &mut self,
        node: &tmark_markdown::mdast::TmarkBrace,
        ctx: &Ctx,
        base: usize,
        start: usize,
        end: usize,
    ) -> Inline {
        let text = &node.value[start..end];
        let inner = self.slice_span(ctx, base + start, text);
        let meta = self.meta(inner);
        let content = self.lower_inline_slice(text, base + start, ctx);
        Inline::Strikeout(Strikeout { meta, content })
    }

    fn lower_role(
        &mut self,
        node: &tmark_markdown::mdast::TmarkBrace,
        head: RoleHead,
        nodes: &[Node],
        index: &mut usize,
        ctx: &Ctx,
        out: &mut Vec<Inline>,
    ) {
        let head_span = self.span(ctx, node.position.as_ref());
        let Some(role) = registry::role(&head.name) else {
            // Unknown name: literal text (spec §Roles), a hint when it was
            // followed by a group or an argument (design C4).
            if matches!(
                nodes.get(*index),
                Some(Node::TmarkGroup(_) | Node::TmarkArgument(_))
            ) {
                self.diag(
                    Code::RoleUnknown,
                    head_span,
                    format!("`{}` is not a role; the brace group is literal", head.name),
                );
            }
            out.push(
                self.literal_text(head_span, decode_escapes(ctx.slice(node.position.as_ref()))),
            );
            return;
        };

        // Collect what follows according to the argument style.
        let mut groups: Vec<&tmark_markdown::mdast::TmarkGroup> = Vec::new();
        let mut argument: Option<&tmark_markdown::mdast::TmarkArgument> = None;
        match role.arg {
            ArgStyle::Content | ArgStyle::ContentMany => {
                let max = if role.arg == ArgStyle::Content { 1 } else { 3 };
                while groups.len() < max {
                    match nodes.get(*index) {
                        Some(Node::TmarkGroup(g)) => {
                            groups.push(g);
                            *index += 1;
                        }
                        _ => break,
                    }
                }
            }
            ArgStyle::Argument => {
                if let Some(Node::TmarkArgument(a)) = nodes.get(*index) {
                    argument = Some(a);
                    *index += 1;
                }
            }
        }
        if groups.is_empty() && argument.is_none() {
            let what = if role.arg == ArgStyle::Argument {
                "(argument)"
            } else {
                "[content]"
            };
            self.diag(
                Code::RoleDanglingHead,
                head_span,
                format!("role `{}` needs {what} right after its head", head.name),
            );
            out.push(
                self.literal_text(head_span, decode_escapes(ctx.slice(node.position.as_ref()))),
            );
            return;
        }

        // Deprecated `{margin}[…]{l}` suffix: consumed with the role.
        let mut suffix_side = None;
        if role.name == "margin" {
            if let Some(Node::TmarkBrace(suffix)) = nodes.get(*index) {
                if let Some(side) = parse_side(&suffix.value) {
                    suffix_side = Some((side, suffix.position.as_ref()));
                    *index += 1;
                }
            }
        }
        // Deprecated `{index}[…]{b}` (main entry) and `{i}` (italic) suffixes
        // (Appendix "Deprecation schedule", C20): consumed with the role.
        let mut index_suffix = None;
        if role.name == "index" {
            if let Some(Node::TmarkBrace(suffix)) = nodes.get(*index) {
                if !suffix.moustache && matches!(suffix.value.as_str(), "b" | "i") {
                    index_suffix = Some((suffix.value.clone(), suffix.position.as_ref()));
                    *index += 1;
                }
            }
        }
        // The span of the whole role: head to last group, argument or suffix.
        let end = suffix_side
            .as_ref()
            .and_then(|(_, p)| *p)
            .or_else(|| index_suffix.as_ref().and_then(|(_, p)| *p))
            .or_else(|| groups.last().and_then(|g| g.position.as_ref()))
            .or_else(|| argument.and_then(|a| a.position.as_ref()))
            .map_or(head_span.end, |p| ctx.map.translate(p.end.offset) as u32);
        let span = Span::new(self.file, head_span.start, end);
        let meta = self.meta(span);

        if let Some(replacement) = role.replaced_by {
            self.deprecated(
                span,
                &format!("{{{}}}", head.name),
                &format!("{{{replacement}}}"),
            );
        }
        if let Some(suffix) = &head.registry_suffix {
            // On the whole role, so that the fix reprints the node.
            self.deprecated(
                span,
                &format!("{{{}:{suffix}}}", head.name),
                &format!("{{{} registry={suffix}}}", head.name),
            );
        }
        if let Some((suffix, _)) = &index_suffix {
            let canonical = if suffix == "b" {
                "{index main=true}[…]"
            } else {
                "{index}[*…*]"
            };
            self.deprecated(span, &format!("{{index}}[…]{{{suffix}}}"), canonical);
        }
        let key = |name: &str| -> Option<String> {
            head.kv
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .or_else(|| {
                    if role.principal == Some(name) {
                        head.positional.clone()
                    } else {
                        None
                    }
                })
        };
        let group_content = |this: &mut Self, g: &tmark_markdown::mdast::TmarkGroup| {
            this.lower_inlines(&g.children, ctx).inlines
        };
        let group_source = |g: &tmark_markdown::mdast::TmarkGroup| -> String {
            let s = ctx.slice(g.position.as_ref());
            s[1..s.len() - 1].to_string()
        };

        let inline = match role.name {
            "lead" => {
                // A lead-in mid-paragraph has no meaning: keep it visible as
                // a bold run-in. Paragraph-initial lead-ins are taken by the
                // block lowering before we get here.
                let content = group_content(self, groups[0]);
                Inline::Strong(Strong { meta, content })
            }
            "sc" => Inline::SmallCaps(SmallCaps {
                meta,
                content: group_content(self, groups[0]),
            }),
            "del" => Inline::Strikeout(Strikeout {
                meta,
                content: group_content(self, groups[0]),
            }),
            "underline" => Inline::Underline(Underline {
                meta,
                content: group_content(self, groups[0]),
            }),
            "mark" => Inline::Highlight(Highlight {
                meta,
                content: group_content(self, groups[0]),
            }),
            "sub" => Inline::Subscript(Subscript {
                meta,
                content: group_content(self, groups[0]),
            }),
            "sup" => Inline::Superscript(Superscript {
                meta,
                content: group_content(self, groups[0]),
            }),
            "keys" => Inline::Keystroke(Keystroke {
                meta,
                keys: group_source(groups[0])
                    .split('+')
                    .map(str::to_string)
                    .collect(),
            }),
            "code" => Inline::Code(CodeInline {
                meta,
                text: group_source(groups[0]),
                lang: key("lang"),
            }),
            "aside" | "margin" => {
                let side = suffix_side
                    .map(|(side, _)| side)
                    .or_else(|| key("side").as_deref().and_then(parse_side));
                let content = group_content(self, groups[0]);
                let plain_meta = self.meta(span);
                Inline::Aside(Aside {
                    meta,
                    content: vec![Block::Plain(Plain {
                        meta: plain_meta,
                        content,
                    })],
                    side,
                })
            }
            "index" => {
                let mut path: Vec<Vec<Inline>> =
                    groups.iter().map(|g| group_content(self, g)).collect();
                let mut main = key("main").as_deref() == Some("true");
                match index_suffix.as_ref().map(|(s, _)| s.as_str()) {
                    Some("b") => main = true,
                    Some("i") => {
                        // The rendering hint becomes content markup.
                        if let Some(last) = path.last_mut() {
                            let content = std::mem::take(last);
                            let emph_meta = self.meta(span);
                            *last = vec![Inline::Emph(Emph {
                                meta: emph_meta,
                                content,
                            })];
                        }
                    }
                    _ => {}
                }
                Inline::IndexEntry(IndexEntry {
                    meta,
                    path,
                    main,
                    registry: key("registry").or(head.registry_suffix.clone()),
                })
            }
            "counter" => match parse_counter(&argument.unwrap().value) {
                Some((prefix, key)) => {
                    let key_span = self.counter_key_span(ctx, argument.unwrap(), &prefix, &key);
                    Inline::CounterItem(CounterItem {
                        meta,
                        prefix,
                        key,
                        key_span,
                    })
                }
                None => {
                    self.diag(
                        Code::RoleDanglingHead,
                        span,
                        "`{counter}` takes a `prefix:key` argument",
                    );
                    self.literal_text(span, ctx.slice_range(span, self))
                }
            },
            "raw" => match key("backend") {
                Some(format) => Inline::RawInline(RawInline {
                    meta,
                    format,
                    text: argument.unwrap().value.clone(),
                }),
                None => {
                    self.diag(
                        Code::RoleDanglingHead,
                        span,
                        "`{raw}` needs a backend: `{raw latex}(…)`",
                    );
                    self.literal_text(span, ctx.slice_range(span, self))
                }
            },
            "latex" | "typst" | "html" => Inline::RawInline(RawInline {
                meta,
                format: role.name.to_string(),
                text: group_source(groups[0]),
            }),
            "include" => {
                self.diag(
                    Code::IncludeInline,
                    span,
                    "`{include}` is a block: put it alone on its line",
                );
                self.literal_text(span, ctx.slice_range(span, self))
            }
            _ => self.literal_text(span, ctx.slice_range(span, self)),
        };
        out.push(inline);
    }

    /// The range of `key` inside a `(prefix:key)` argument.
    fn counter_key_span(
        &self,
        ctx: &Ctx,
        argument: &tmark_markdown::mdast::TmarkArgument,
        prefix: &str,
        key: &str,
    ) -> SubSpan {
        let Some(p) = argument.position.as_ref() else {
            return SubSpan::default();
        };
        let start = p.start.offset + 1 + prefix.len() + 1;
        SubSpan(self.span_of(ctx, start, start + key.len()))
    }

    /// `#[…]…` index entries and `#(prefix:key)` counter items.
    fn lower_define(
        &mut self,
        node: &tmark_markdown::mdast::TmarkDefine,
        nodes: &[Node],
        index: &mut usize,
        ctx: &Ctx,
        out: &mut Vec<Inline>,
    ) {
        let sigil_span = self.span(ctx, node.position.as_ref());
        if let Some(Node::TmarkArgument(argument)) = node.children.first() {
            let span = Span::new(self.file, sigil_span.start, sigil_span.end);
            match parse_counter(&argument.value) {
                Some((prefix, key)) => {
                    if argument.marker == b'{' {
                        // Deprecated `#{prefix:key}`: only for a declared
                        // prefix (design C16); otherwise literal text.
                        if !self.is_prefix(&prefix) {
                            out.push(self.literal_text(
                                span,
                                decode_escapes(ctx.slice(node.position.as_ref())),
                            ));
                            return;
                        }
                        self.deprecated(span, "#{prefix:key}", "#(prefix:key)");
                    }
                    let meta = self.meta(span);
                    let key_span = self.counter_key_span(ctx, argument, &prefix, &key);
                    out.push(Inline::CounterItem(CounterItem {
                        meta,
                        prefix,
                        key,
                        key_span,
                    }));
                }
                None => out.push(self.literal_text(span, ctx.slice(node.position.as_ref()))),
            }
            return;
        }
        // Index entry: one to three groups follow.
        let mut groups: Vec<&tmark_markdown::mdast::TmarkGroup> = Vec::new();
        while groups.len() < 3 {
            match nodes.get(*index) {
                Some(Node::TmarkGroup(g)) => {
                    groups.push(g);
                    *index += 1;
                }
                _ => break,
            }
        }
        if groups.is_empty() {
            out.push(self.literal_text(sigil_span, "#"));
            return;
        }
        let end = groups
            .last()
            .and_then(|g| g.position.as_ref())
            .map_or(sigil_span.end, |p| ctx.map.translate(p.end.offset) as u32);
        let span = Span::new(self.file, sigil_span.start, end);
        let meta = self.meta(span);
        let mut main = false;
        let path: Vec<Vec<Inline>> = groups
            .iter()
            .map(|g| {
                let mut content = self.lower_inlines(&g.children, ctx).inlines;
                // `#[**term**]` is sugar for `main=true` (spec §IndexEntry).
                if groups.len() == 1 && content.len() == 1 {
                    if let Inline::Strong(strong) = &content[0] {
                        main = true;
                        content = strong.content.clone();
                    }
                }
                content
            })
            .collect();
        out.push(Inline::IndexEntry(IndexEntry {
            meta,
            path,
            main,
            registry: None,
        }));
    }
}

impl<'a> Ctx<'a> {
    /// Source text of a span already translated to the file (only valid for
    /// the identity map, where offsets are file offsets; for re-parsed text
    /// the caller passes the local text). Used for literal fallbacks.
    fn slice_range(&self, span: Span, lowerer: &Lowerer) -> String {
        let _ = lowerer;
        // Translate back: find the local range whose translation is `span`.
        // Only the identity map can do that exactly; approximate with the
        // whole text otherwise (the text is short: a role).
        let start = span.start as usize;
        let end = span.end as usize;
        if self.map.translate(0) == 0 && end <= self.text.len() {
            self.text[start..end].to_string()
        } else {
            self.text.to_string()
        }
    }
}

fn target(url: &str) -> Target {
    if let Some(anchor) = url.strip_prefix('#') {
        Target::Anchor(anchor.to_string())
    } else if url.ends_with(".md") && !url.contains("://") {
        Target::Document(url.to_string())
    } else {
        Target::Url(url.to_string())
    }
}

fn html_inline(meta: tmark_ir::Meta, value: &str) -> Inline {
    match value
        .strip_prefix("<!--")
        .and_then(|rest| rest.strip_suffix("-->"))
    {
        Some(text) => Inline::Comment(Comment {
            meta,
            text: text.to_string(),
        }),
        None => Inline::RawInline(RawInline {
            meta,
            format: "html".to_string(),
            text: value.to_string(),
        }),
    }
}

fn parse_side(value: &str) -> Option<Side> {
    match value {
        "left" | "l" => Some(Side::Left),
        "right" | "r" => Some(Side::Right),
        "outer" | "o" => Some(Side::Outer),
        "inner" | "i" => Some(Side::Inner),
        _ => None,
    }
}

/// `prefix:key` of a counter item.
pub(crate) fn parse_counter(value: &str) -> Option<(String, String)> {
    let (prefix, key) = value.split_once(':')?;
    let prefix_ok = prefix
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic())
        && prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    let key_ok = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    (prefix_ok && key_ok).then(|| (prefix.to_string(), key.to_string()))
}

/// Drop trailing whitespace before an attribute list taken by the block.
pub(crate) fn trim_trailing_space(inlines: &mut Vec<Inline>) {
    if let Some(Inline::Str(s)) = inlines.last_mut() {
        let trimmed = s.text.trim_end().len();
        if trimmed == 0 {
            inlines.pop();
        } else {
            // Whitespace is never escaped: the span shrinks by as much as
            // the text (design 03 §Identity and spans).
            let removed = (s.text.len() - trimmed) as u32;
            if s.meta.span.len() >= removed {
                s.meta.span.end -= removed;
            }
            s.text.truncate(trimmed);
        }
    }
}

/// `@https://doi.org/…` is sugar for `@doi:…` (spec §Cite); a bare DOI
/// (`10.<digits>/…`, only reachable through the deprecated citation forms)
/// takes the `doi:` prefix too.
fn doi_key(key: &str) -> String {
    for prefix in ["https://doi.org/", "http://doi.org/", "https://dx.doi.org/"] {
        if let Some(doi) = key.strip_prefix(prefix) {
            return format!("doi:{doi}");
        }
    }
    if key.starts_with("10.") && key.contains('/') {
        return format!("doi:{key}");
    }
    key.to_string()
}

/// Adjacent `Str` nodes become one: the IR does not record how text was
/// split by the tokenizer or by literal fallbacks (design 03 §Shape).
/// The decoded text of a run aligned with its source.
struct Aligned {
    /// The source byte offset each decoded byte starts at, plus one entry
    /// for the end of the text.
    offsets: Vec<usize>,
    /// Whether the source spelled the decoded byte with a backslash escape.
    escaped: Vec<bool>,
}

/// Aligns `text` (decoded) with `source`; `None` when they cannot be
/// aligned, and the caller falls back to coarser rules. Handles the two
/// decodings the tokenizer applies to a text run, backslash escapes
/// (`\\` + ASCII punctuation) and character references (`&amp;`,
/// `&#169;`), which decode to one character each.
fn align(text: &str, source: &str) -> Option<Aligned> {
    let t = text.as_bytes();
    let s = source.as_bytes();
    let mut offsets = Vec::with_capacity(t.len() + 1);
    let mut escaped = vec![false; t.len()];
    let (mut i, mut j) = (0, 0);
    while i < t.len() {
        let &sj = s.get(j)?;
        if sj == b'\\'
            && s.get(j + 1)
                .is_some_and(|b| b.is_ascii_punctuation() && *b == t[i])
        {
            offsets.push(j);
            escaped[i] = true;
            i += 1;
            j += 2;
            continue;
        }
        if sj == b'&' {
            let entity = s[j + 1..]
                .iter()
                .take(40)
                .position(|b| *b == b';')
                .filter(|end| {
                    *end > 0
                        && s[j + 1..j + 1 + end]
                            .iter()
                            .all(|b| b.is_ascii_alphanumeric() || *b == b'#')
                });
            // A reference decodes to one character; `&` itself only from
            // `&amp;`. Anything else spelled `&…;` is literal text.
            if let Some(end) = entity {
                if t[i] != b'&' || s[j..j + end + 2].eq_ignore_ascii_case(b"&amp;") {
                    let len = text[i..].chars().next()?.len_utf8();
                    offsets.extend(std::iter::repeat(j).take(len));
                    i += len;
                    j += end + 2;
                    continue;
                }
            }
        }
        if sj != t[i] {
            return None;
        }
        offsets.push(j);
        i += 1;
        j += 1;
    }
    offsets.push(j);
    Some(Aligned { offsets, escaped })
}

fn merge_strs(inlines: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(inlines.len());
    for inline in inlines {
        if let (Some(Inline::Str(last)), Inline::Str(next)) = (out.last_mut(), &inline) {
            last.text.push_str(&next.text);
            last.meta.span = last.meta.span.join(next.meta.span);
            continue;
        }
        out.push(inline);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::align;

    #[test]
    fn alignment_finds_escapes_and_entities() {
        let map = |text: &str, source: &str| {
            align(text, source).map(|a| {
                a.escaped
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| **b)
                    .map(|(i, _)| i)
                    .collect::<Vec<_>>()
            })
        };
        assert_eq!(map("a (c) b", "a \\(c) b"), Some(vec![2]));
        assert_eq!(map("1/2", "1\\/2"), Some(vec![1]));
        assert_eq!(map("&(c) (c)", "&amp;(c) \\(c)"), Some(vec![5]));
        assert_eq!(map("© (c)", "&copy; \\(c)"), Some(vec![3]));
        assert_eq!(map("&b; (c)", "&b; \\(c)"), Some(vec![4]));
        assert_eq!(map("a b", "a b"), Some(vec![]));
        assert_eq!(map("x", "y"), None, "not the text's source");
        assert_eq!(map("ab", "a"), None, "source too short");
        let a = align("a © b", "a &copy; b").unwrap();
        assert_eq!(a.offsets, vec![0, 1, 2, 2, 8, 9, 10]);
        let a = align("(c) x", "\\(c) x").unwrap();
        assert_eq!(a.offsets, vec![0, 2, 3, 4, 5, 6]);
    }
}
