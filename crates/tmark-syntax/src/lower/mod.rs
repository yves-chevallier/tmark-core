//! Lowering: the mdast of `tmark-markdown` to the TMark IR.
//!
//! Most of TMark is decided here, not in the tokenizer: what a brace group
//! is, which role a head names, where an attribute list attaches, what a
//! fence produces, which paragraph is a caption. Every function here is
//! total: unrecognised input lowers to literal text and a diagnostic.

mod block;
mod compat;
mod inline;
mod sugar;
mod table;
mod table_yaml;

pub(crate) mod head;

use std::collections::HashMap;
use std::rc::Rc;

use tmark_ir::{
    frontmatter, registry, Attrs, Code, Diagnostic, Document, FileId, FrontMatter, Inline, Meta,
    NodeId, Severity, Span, Str, SubSpan,
};
use tmark_markdown::{mdast::Node, to_mdast, unist::Position, ParseOptions};

use crate::offset::OffsetMap;
use crate::Parsed;

/// Parse options.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// The strict profile: X1 (`__x__` small caps) and X3 (`~x~` subscript)
    /// are off (spec §Conformance).
    pub strict: bool,
}

/// The text a tree was parsed from, and how its offsets map to the file.
#[derive(Clone)]
pub(crate) struct Ctx<'a> {
    pub text: &'a str,
    pub map: Rc<OffsetMap>,
}

impl<'a> Ctx<'a> {
    /// The source text of a node.
    pub fn slice(&self, position: Option<&Position>) -> &'a str {
        match position {
            Some(p) => &self.text[p.start.offset..p.end.offset],
            None => "",
        }
    }
}

/// Feature switches read from the front matter (spec §Feature registry).
#[derive(Clone, Debug)]
pub(crate) struct Features {
    pub paragraph_lead: bool,
    pub tasklist_partial: bool,
    pub inline_insert: bool,
}

impl Features {
    fn from_front_matter(fm: &FrontMatter) -> Self {
        let get = |name: &str| fm.keys.press.feature(name);
        Features {
            paragraph_lead: get("paragraph.lead"),
            tasklist_partial: get("tasklist.partial"),
            inline_insert: get("inline.insert"),
        }
    }
}

pub(crate) struct Lowerer {
    pub file: FileId,
    pub options: Options,
    pub features: Features,
    next_id: u32,
    pub diagnostics: Vec<Diagnostic>,
    /// Counter prefixes: predeclared plus `declare.counters`.
    pub prefixes: Vec<String>,
    /// Admonition kinds: built-in plus `declare.admonitions`.
    pub admonitions: Vec<String>,
    /// Link reference definitions of the current parse (`[id]: url "title"`).
    pub definitions: HashMap<String, (String, Option<String>)>,
    /// Lowering the body of a `::: figure`: a free caption is the figure's.
    pub in_figure: bool,
    /// Captions from a generic `/// caption` block, whose kind the
    /// neighbouring float decides (`attach_captions`).
    pub generic_captions: Vec<NodeId>,
    /// Lowering the direct body of a `::: tabs`: its `tab` containers are
    /// not regrouped (`group_tabs`).
    pub in_tabs: bool,
    /// Set by `lower_container` before the body of a `tabs`; consumed by
    /// `lower_content`.
    pub next_body_is_tabs: bool,
    /// Lowering an inline fragment (`lower_fragment`: a table cell, a
    /// column header, an attribute value). The text is inline content, not
    /// a paragraph, so the `paragraph.lead` promotion does not apply —
    /// taking a lead there would drop the strong span from the fragment.
    pub in_fragment: bool,
    /// Lowering the blocks of a list item: the `paragraph.lead` sugar does
    /// not fire there (spec §Para).
    pub in_list_item: bool,
}

pub fn parse(text: &str, file: FileId) -> Parsed {
    parse_with(text, file, Options::default())
}

pub fn parse_with(text: &str, file: FileId, options: Options) -> Parsed {
    let mut lowerer = Lowerer {
        file,
        options,
        features: Features {
            paragraph_lead: true,
            tasklist_partial: false,
            inline_insert: false,
        },
        next_id: 0,
        diagnostics: Vec::new(),
        prefixes: registry::PREFIXES
            .iter()
            .map(|p| p.name.to_string())
            .collect(),
        admonitions: registry::ADMONITIONS
            .iter()
            .map(|a| a.name.to_string())
            .collect(),
        definitions: HashMap::new(),
        in_figure: false,
        generic_captions: Vec::new(),
        in_tabs: false,
        next_body_is_tabs: false,
        in_fragment: false,
        in_list_item: false,
    };
    let ctx = Ctx {
        text,
        map: OffsetMap::identity(),
    };
    let document = match lowerer.tree(text) {
        Ok(tree) => lowerer.lower_root(&tree, &ctx),
        // A tokenizer error or panic: the text as one paragraph, plus the
        // diagnostic that names the bug (fixture `diag-parse-internal`).
        Err(message) => {
            let span = Span::new(file, 0, text.len() as u32);
            lowerer.diag(
                Code::ParseInternal,
                span,
                format!("the tokenizer failed on this file ({message}); shown as plain text"),
            );
            let meta = lowerer.meta(span);
            let inline = Inline::Str(Str {
                meta: lowerer.meta(span),
                text: text.to_string(),
            });
            Document {
                file,
                blocks: vec![tmark_ir::Block::Para(tmark_ir::Para {
                    meta,
                    content: vec![inline],
                    lead: None,
                })],
                ..Document::default()
            }
        }
    };
    Parsed {
        document,
        diagnostics: lowerer.diagnostics,
    }
}

impl Lowerer {
    /// The mdast tree of `text`. A panic inside the vendored tokenizer
    /// (markdown-rs 1.0.0 has at least one: an unclosed fence in a list
    /// item followed by a list of another kind) is caught and reported as
    /// an error, so that parsing never fails (AGENTS.md).
    pub fn tree(&self, text: &str) -> Result<Node, String> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            to_mdast(text, &ParseOptions::tmark())
        })) {
            Ok(Ok(tree)) => Ok(tree),
            Ok(Err(message)) => Err(message.reason.to_string()),
            Err(panic) => Err(panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "tokenizer panic".to_string())),
        }
    }

    /// Allocate the next node id (dense, document order: call before
    /// lowering the children).
    pub fn meta(&mut self, span: Span) -> Meta {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        Meta::new(id, span)
    }

    pub fn span(&self, ctx: &Ctx, position: Option<&Position>) -> Span {
        match position {
            Some(p) => Span::new(
                self.file,
                ctx.map.translate(p.start.offset) as u32,
                ctx.map.translate(p.end.offset) as u32,
            ),
            None => Span::new(self.file, 0, 0),
        }
    }

    /// A span for a byte range of the current text.
    pub fn span_of(&self, ctx: &Ctx, start: usize, end: usize) -> Span {
        Span::new(
            self.file,
            ctx.map.translate(start) as u32,
            ctx.map.translate(end) as u32,
        )
    }

    /// Moves the sub-spans of a parsed attribute list from offsets
    /// relative to its text to file offsets, `base` being the local offset
    /// of that text.
    pub fn relocate_attrs(&self, ctx: &Ctx, attrs: &mut Attrs, base: usize) {
        if let Some(sub) = attrs.id_span {
            let (s, e) = (sub.0.start as usize, sub.0.end as usize);
            attrs.id_span = Some(SubSpan(self.span_of(ctx, base + s, base + e)));
        }
    }

    /// The local offset of the first `{` on the first line of `position`,
    /// plus one: where an attribute list on that line starts (containers).
    pub fn attrs_base(&self, ctx: &Ctx, position: Option<&Position>) -> Option<usize> {
        let position = position?;
        let first = ctx.slice(Some(position)).lines().next()?;
        Some(position.start.offset + first.find('{')? + 1)
    }

    /// The span of the value of `key` in the attribute list on the first
    /// line of `position`, `len` being the length of the decoded value.
    /// `None` when the list has no such key, or when the value is not a
    /// verbatim slice of the line: what is parsed out of it has then no
    /// source of its own (spec §Round-trip and source spans).
    pub fn attr_value_span(
        &self,
        ctx: &Ctx,
        position: Option<&Position>,
        key: &str,
        len: usize,
    ) -> Option<Span> {
        let base = self.attrs_base(ctx, position)?;
        let local = position?.start.offset;
        let first = ctx.slice(position).lines().next()?;
        let close = first.rfind('}')?;
        let body = first.get(base - local..close)?;
        let at = base + head::attr_value_offset(body, key)?;
        ctx.text.get(at..at + len)?;
        Some(self.span_of(ctx, at, at + len))
    }

    /// Same on the last line of `position` (display math: the list follows
    /// the closing `$$`).
    pub fn attrs_base_last_line(&self, ctx: &Ctx, position: Option<&Position>) -> Option<usize> {
        let position = position?;
        let src = ctx.slice(Some(position));
        let last_start = src.rfind('\n').map_or(0, |i| i + 1);
        Some(position.start.offset + last_start + src[last_start..].find('{')? + 1)
    }

    pub fn meta_at(&mut self, ctx: &Ctx, position: Option<&Position>) -> Meta {
        let span = self.span(ctx, position);
        self.meta(span)
    }

    pub fn diag(&mut self, code: Code, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic::new(code, span, message));
    }

    /// A diagnostic downgraded to `info` when `informational` (a foreign
    /// container, which the spec leaves to the site).
    pub fn diag_at(
        &mut self,
        code: Code,
        span: Span,
        message: impl Into<String>,
        informational: bool,
    ) {
        let mut d = Diagnostic::new(code, span, message);
        if informational {
            d.severity = Severity::Info;
        }
        self.diagnostics.push(d);
    }

    pub fn deprecated(&mut self, span: Span, spelling: &str, canonical: &str) {
        self.diag(
            Code::Deprecated,
            span,
            format!("`{spelling}` is deprecated, write `{canonical}`"),
        );
    }

    /// A `deprecated` diagnostic that carries its own text fix, for a
    /// spelling whose span is not a node's (an attribute list on a host,
    /// the head of a progress bar before its attributes).
    pub fn deprecated_with_fix(
        &mut self,
        span: Span,
        spelling: &str,
        canonical: &str,
        replacement: String,
    ) {
        self.diagnostics.push(Diagnostic {
            fix: Some(tmark_ir::Fix { span, replacement }),
            ..Diagnostic::new(
                Code::Deprecated,
                span,
                format!("`{spelling}` is deprecated, write `{canonical}`"),
            )
        });
    }

    pub fn is_prefix(&self, name: &str) -> bool {
        self.prefixes.iter().any(|p| p.eq_ignore_ascii_case(name))
    }

    pub fn is_admonition(&self, name: &str) -> bool {
        self.admonitions.iter().any(|a| a == name)
    }

    /// A literal `Str` with an explicit text (for synthesised pieces).
    pub fn literal_text(&mut self, span: Span, text: impl Into<String>) -> Inline {
        Inline::Str(Str {
            meta: self.meta(span),
            text: text.into(),
        })
    }

    fn lower_root(&mut self, root: &Node, ctx: &Ctx) -> Document {
        let children: &[Node] = root.children().map_or(&[], Vec::as_slice);
        let root_span = self.span(ctx, root.position());
        let doc_meta = self.meta(root_span);
        let _ = doc_meta;

        // Front matter.
        let mut front_matter = FrontMatter {
            meta: Meta::default(),
            raw: String::new(),
            keys: Default::default(),
            extra: serde_json::Value::Null,
            deprecated: Vec::new(),
        };
        let mut rest = children;
        if let Some(Node::Yaml(yaml)) = children.first() {
            let span = self.span(ctx, yaml.position.as_ref());
            let meta = self.meta(span);
            let raw = ctx.slice(yaml.position.as_ref()).to_string();
            match frontmatter::parse(&yaml.value) {
                Ok(mut fm) => {
                    fm.meta = meta;
                    fm.raw = raw;
                    // One fix moves every deprecated key at once; each
                    // diagnostic carries it (the CLI applies the first and
                    // skips the overlapping rest).
                    let fix = deprecated_keys_fix(&fm.raw, &fm.deprecated)
                        .map(|replacement| tmark_ir::Fix { span, replacement });
                    for key in &fm.deprecated {
                        let target = frontmatter::deprecated_key_target(key).unwrap_or_default();
                        self.diagnostics.push(Diagnostic {
                            fix: fix.clone(),
                            ..Diagnostic::new(
                                Code::DeprecatedFrontmatterKey,
                                span,
                                format!("`{key}` is deprecated, write `{target}`"),
                            )
                        });
                    }
                    front_matter = fm;
                }
                Err(error) => {
                    self.diag(Code::FrontmatterYaml, span, error.message.clone());
                    front_matter.meta = meta;
                    front_matter.raw = raw;
                }
            }
            rest = &children[1..];
        }
        self.features = Features::from_front_matter(&front_matter);
        self.prefixes
            .extend(front_matter.keys.press.declare.counters.keys().cloned());
        self.admonitions
            .extend(front_matter.keys.press.declare.admonitions.keys().cloned());

        self.collect_definitions(rest);
        let mut document = Document {
            file: self.file,
            front_matter,
            ..Document::default()
        };
        document.blocks = self.lower_blocks(rest, ctx, &mut document);
        document
    }

    /// Link reference definitions of a parse, for reference links.
    pub fn collect_definitions(&mut self, nodes: &[Node]) {
        self.definitions.clear();
        for node in nodes {
            if let Node::Definition(def) = node {
                self.definitions
                    .insert(def.identifier.clone(), (def.url.clone(), def.title.clone()));
            }
        }
    }

    /// A fragment the construct spells verbatim, lowered with the spans
    /// of that slice — an image's alternative text is the text between
    /// `![` and `]` — and anchored at the whole construct where it does
    /// not (spec §Round-trip and source spans).
    pub fn lower_fragment_in(
        &mut self,
        ctx: &Ctx,
        position: Option<&Position>,
        text: &str,
        construct: Span,
    ) -> Vec<Inline> {
        let anchor = position
            .filter(|_| !text.is_empty())
            .and_then(|p| {
                let at = p.start.offset + ctx.slice(Some(p)).find(text)?;
                Some(self.span_of(ctx, at, at + text.len()))
            })
            .unwrap_or(construct);
        self.lower_fragment(text, anchor)
    }

    /// Parse and lower a fragment of text, `anchor` being where it sits
    /// in the file — the slice it is, or the construct it came from when
    /// it is no slice of it (a YAML cell, a title, an image alt).
    pub fn lower_fragment(&mut self, text: &str, anchor: Span) -> Vec<Inline> {
        let Ok(tree) = self.tree(text) else {
            return vec![self.literal_text(anchor, text)];
        };
        let map = OffsetMap::nested(vec![(0, anchor.start as usize)], OffsetMap::identity());
        let ctx = Ctx { text, map };
        let mut document = Document::default();
        let saved = std::mem::take(&mut self.definitions);
        let was_fragment = std::mem::replace(&mut self.in_fragment, true);
        let blocks = self.lower_blocks(
            tree.children().map_or(&[], Vec::as_slice),
            &ctx,
            &mut document,
        );
        self.in_fragment = was_fragment;
        self.definitions = saved;
        match blocks.into_iter().next() {
            Some(tmark_ir::Block::Para(para)) => para.content,
            Some(tmark_ir::Block::Plain(plain)) => plain.content,
            _ => Vec::new(),
        }
    }

    /// Parse and lower a contiguous slice of `ctx`'s text as inline
    /// content: critic markup's inner text (spec Appendix "PyMdownX
    /// compatibility profile"), which the brace group hands over raw.
    /// `at` is the slice's offset in `ctx.text`, so every span maps back to
    /// the file. Content that is not one paragraph (a slice that reads as a
    /// list or a heading on its own) is literal text, never dropped.
    pub fn lower_inline_slice(&mut self, value: &str, at: usize, ctx: &Ctx) -> Vec<Inline> {
        let span = Span::new(
            self.file,
            ctx.map.translate(at) as u32,
            ctx.map.translate(at + value.len()) as u32,
        );
        if value.is_empty() {
            return Vec::new();
        }
        let Ok(tree) = self.tree(value) else {
            return vec![self.literal_text(span, value.to_string())];
        };
        let map = OffsetMap::nested(vec![(0, at)], ctx.map.clone());
        let inner = Ctx { text: value, map };
        let mut document = Document::default();
        let was_fragment = std::mem::replace(&mut self.in_fragment, true);
        let blocks = self.lower_blocks(
            tree.children().map_or(&[], Vec::as_slice),
            &inner,
            &mut document,
        );
        self.in_fragment = was_fragment;
        let mut blocks = blocks.into_iter();
        match (blocks.next(), blocks.next()) {
            (Some(tmark_ir::Block::Para(para)), None) => para.content,
            (Some(tmark_ir::Block::Plain(plain)), None) => plain.content,
            _ => vec![self.literal_text(span, value.to_string())],
        }
    }

    /// Parse and lower re-parsed content (a container or admonition body)
    /// whose `stops` map it back to `ctx`.
    pub fn lower_content(
        &mut self,
        value: &str,
        stops: &[(usize, usize)],
        ctx: &Ctx,
        document: &mut Document,
    ) -> Vec<tmark_ir::Block> {
        let Ok(tree) = self.tree(value) else {
            return Vec::new();
        };
        let map = OffsetMap::nested(stops.to_vec(), ctx.map.clone());
        let inner = Ctx { text: value, map };
        let saved = std::mem::take(&mut self.definitions);
        let in_tabs = std::mem::take(&mut self.next_body_is_tabs);
        let saved_tabs = std::mem::replace(&mut self.in_tabs, in_tabs);
        let children: &[Node] = tree.children().map_or(&[], Vec::as_slice);
        self.collect_definitions(children);
        let blocks = self.lower_blocks(children, &inner, document);
        self.definitions = saved;
        self.in_tabs = saved_tabs;
        blocks
    }
}

pub(crate) use tmark_ir::plain_text;

/// The front matter (`raw`, fences included) with every deprecated key
/// moved to its canonical place (`tmark_ir::yaml_edit::move_key`), or
/// `None` when one of the moves is not a safe line edit.
fn deprecated_keys_fix(raw: &str, deprecated: &[String]) -> Option<String> {
    let mut lines = raw.split_inclusive('\n');
    let open = lines.next()?;
    let close = raw.rsplit('\n').next()?;
    let inner_end = raw.len() - close.len();
    let mut inner = raw.get(open.len()..inner_end)?.to_string();
    for key in deprecated {
        let target = frontmatter::deprecated_key_target(key)?;
        inner = tmark_ir::yaml_edit::move_key(&inner, key, &target)?;
    }
    Some(format!("{open}{inner}{close}"))
}

/// Source text with its backslash escapes decoded (`\+` → `+`), for text
/// that the tokenizer took raw and the lowering gives back as literal: what
/// CommonMark would have produced had the construct not been recognised.
pub(crate) fn decode_escapes(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if next.is_ascii_punctuation() {
                    out.push(next);
                    chars.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}
