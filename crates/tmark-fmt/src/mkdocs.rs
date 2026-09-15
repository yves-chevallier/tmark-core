//! The MkDocs profile: the spelling table of design `04-printer.md`
//! §Profiles, made of one function per construct whose PyMdownX or
//! TeXSmith 0.6 spelling differs from the canonical one (spec Appendix
//! "PyMdownX compatibility profile", Appendix "Deprecation schedule").
//!
//! Every function returns `true` when it wrote the construct and `false`
//! when the sugar cannot spell this node (a title with a quote, a raw text
//! with brackets, an include with a base): the caller then prints the
//! canonical form, which a site shows literally (the degradation contract
//! of spec §Conformance). The text a function writes re-parses to the same
//! IR as the canonical form, modulo `deprecated` diagnostics; the gate test
//! in `tests/mkdocs.rs` checks that over the fixture corpus.

use std::collections::BTreeSet;

use tmark_ir::{
    registry, walk, Admonition, Aside, Block, Caption, CaptionKind, CounterItem, Div, Document,
    Include, IndexEntry, Inline, NodeRef, RawBlock, RawInline, RefItem,
};

use crate::escape::Context;
use crate::inline::{code_span, inlines, side_name};
use crate::out::Out;

/// What the profile must know about the document to choose between a
/// label reference and a citation (spec §Registries: "a reference `@a:b`
/// whose head `a` is a declared counter prefix is a label …; any other
/// `@key` is a bibliography key"). Built once per `format`; empty for a
/// node printed on its own.
#[derive(Clone, Debug)]
pub struct Lookup {
    /// Heads that make `@head:key` a label: the predeclared counter
    /// prefixes, `declare.counters` and the `sources.crossrefs` aliases
    /// (spec §Cross-document references), lowercase.
    heads: BTreeSet<String>,
    /// Ids defined by an attribute list anywhere in the document.
    labels: BTreeSet<String>,
    /// Footnote definition labels: `[^key]` with a definition is a
    /// footnote, so a citation of that key keeps its `@` spelling.
    footnotes: BTreeSet<String>,
}

impl Default for Lookup {
    /// What a node printed on its own knows: the predeclared prefixes.
    fn default() -> Self {
        Self {
            heads: registry::PREFIXES
                .iter()
                .map(|p| p.name.to_string())
                .collect(),
            labels: BTreeSet::new(),
            footnotes: BTreeSet::new(),
        }
    }
}

impl Lookup {
    pub fn of(doc: &Document) -> Self {
        let mut lookup = Self::default();
        let press = &doc.front_matter.keys.press;
        lookup.heads.extend(
            press
                .declare
                .counters
                .keys()
                .chain(press.sources.crossrefs.keys())
                .map(|k| k.to_ascii_lowercase()),
        );
        lookup
            .footnotes
            .extend(doc.footnotes.iter().map(|f| f.label.clone()));
        walk(doc, &mut |node: NodeRef| {
            let id = match node {
                NodeRef::Block(b) => match b {
                    Block::Header(h) => h.attrs.id.as_deref(),
                    Block::BlockQuote(q) => q.attrs.id.as_deref(),
                    Block::Table(t) => t.attrs.id.as_deref(),
                    Block::Caption(c) => c.attrs.id.as_deref(),
                    Block::Figure(f) => f.attrs.id.as_deref(),
                    Block::Admonition(a) => a.attrs.id.as_deref(),
                    Block::Div(d) => d.attrs.id.as_deref(),
                    Block::MathBlock(m) => m.attrs.id.as_deref(),
                    _ => None,
                },
                NodeRef::Inline(i) => match i {
                    Inline::Image(img) => img.attrs.id.as_deref(),
                    Inline::Span(s) => s.attrs.id.as_deref(),
                    _ => None,
                },
            };
            if let Some(id) = id {
                lookup.labels.insert(id.to_string());
            }
        });
        lookup
    }

    fn is_head(&self, prefix: &str) -> bool {
        self.heads.contains(&prefix.to_ascii_lowercase())
    }

    /// The registry a bare `@key` resolves against, by the spec's lookup
    /// rule applied to what the document declares.
    fn registry(&self, key: &str) -> Registry {
        match key.split_once(':') {
            Some(("gls", _)) => Registry::Glossary,
            Some((head, _)) if self.is_head(head) => Registry::Label,
            Some(_) => Registry::Bibliography,
            None if self.labels.contains(key) => Registry::Label,
            None => Registry::Bibliography,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Registry {
    Label,
    Glossary,
    Bibliography,
}

/// A name a sugar spelling can carry bare: a class, a registry, a fence
/// language, an identifier.
fn bare(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '+' | '#'))
}

fn plain_item(item: &RefItem) -> bool {
    item.prefix.is_none()
        && item.suffix.is_none()
        && !item.suppress_author
        && !item.narrative
        && !item.key.is_empty()
        && !item
            .key
            .contains(|c: char| c.is_whitespace() || matches!(c, ']' | '[' | ',' | ';' | '^'))
}

/// Content a delimiter pair can wrap: not empty, no whitespace or line
/// break at either end (the delimiter would not flank), and no node of the
/// same delimiter family inside (`~~a ~b~~` is ambiguous).
fn wrappable(content: &[Inline], same_family: fn(&Inline) -> bool) -> bool {
    fn edge_ws(inline: &Inline, last: bool) -> bool {
        match inline {
            Inline::Str(s) => {
                let c = if last {
                    s.text.chars().next_back()
                } else {
                    s.text.chars().next()
                };
                c.map_or(true, char::is_whitespace)
            }
            Inline::Space(_) | Inline::SoftBreak(_) | Inline::LineBreak(_) => true,
            _ => false,
        }
    }
    fn contains(content: &[Inline], pred: fn(&Inline) -> bool) -> bool {
        let mut found = false;
        tmark_ir::walk_inlines(content, &mut |n: NodeRef| {
            if let NodeRef::Inline(i) = n {
                found |= pred(i);
            }
        });
        found
    }
    let (Some(first), Some(last)) = (content.first(), content.last()) else {
        return false;
    };
    !edge_ws(first, false) && !edge_ws(last, true) && !contains(content, same_family)
}

/// `==x==`, `~~x~~`, `~x~`, `^x^` (spec Appendix PyMdownX, Table
/// "PyMdownX sugar"). `same_family` names the nodes that share the
/// delimiter character.
pub fn delimited(
    out: &mut Out,
    delimiter: &str,
    content: &[Inline],
    ctx: Context,
    same_family: fn(&Inline) -> bool,
) -> bool {
    let marker = delimiter.chars().next().expect("a delimiter");
    if !wrappable(content, same_family) || out.last_char() == Some(marker) {
        return false;
    }
    out.push(delimiter);
    inlines(
        out,
        content,
        Context {
            block_start: false,
            ..ctx
        },
    );
    out.push(delimiter);
    true
}

pub fn tilde_family(i: &Inline) -> bool {
    matches!(i, Inline::Subscript(_) | Inline::Strikeout(_))
}

pub fn caret_family(i: &Inline) -> bool {
    matches!(i, Inline::Superscript(_))
}

pub fn equals_family(i: &Inline) -> bool {
    matches!(i, Inline::Highlight(_))
}

/// `__x__` small caps (spec deviation X1). Emphasis flanking: the run must
/// not touch an alphanumeric or an underscore on either side.
pub fn smallcaps(out: &mut Out, content: &[Inline], ctx: Context, next: Option<char>) -> bool {
    let touches = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    if !wrappable(content, |i| matches!(i, Inline::SmallCaps(_))) {
        return false;
    }
    if touches(out.last_char()) || touches(next) {
        return false;
    }
    out.push("__");
    inlines(
        out,
        content,
        Context {
            block_start: false,
            ..ctx
        },
    );
    out.push("__");
    true
}

/// `++ctrl+alt+s++`. The parser reads the run as Markdown before it
/// splits on `+`, so a key may only hold letters, digits, `-`, spaces and
/// quotes (PyMdownX's literal-key form).
pub fn keys(out: &mut Out, keys: &[String]) -> bool {
    let safe = |k: &String| {
        !k.is_empty()
            && k.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | ' ' | '"'))
    };
    if keys.is_empty() || !keys.iter().all(safe) || out.last_char() == Some('+') {
        return false;
    }
    out.push("++");
    out.push(&keys.join("+"));
    out.push("++");
    true
}

/// `` `#!py code` `` (pymdownx.inlinehilite).
pub fn code(out: &mut Out, text: &str, lang: &str, in_cell: bool) -> bool {
    if !bare(lang)
        || text.is_empty()
        || text.starts_with(char::is_whitespace)
        || text.contains('\n')
    {
        return false;
    }
    code_span(out, &format!("#!{lang} {text}"), in_cell);
    true
}

/// `{index:registry}[a][b]{b}`: the deprecated registry suffix and the
/// `{b}` main-entry suffix of TeXSmith 0.6 (Appendix Deprecation schedule).
pub fn index(out: &mut Out, n: &IndexEntry, ctx: Context) -> bool {
    if !n.main && n.registry.is_none() {
        return false;
    }
    if n.registry.as_deref().is_some_and(|r| !bare(r)) {
        return false;
    }
    out.push("{index");
    if let Some(registry) = &n.registry {
        out.push(":");
        out.push(registry);
    }
    out.push("}");
    for group in &n.path {
        out.push("[");
        inlines(
            out,
            group,
            Context {
                in_group: true,
                block_start: false,
                ..ctx
            },
        );
        out.push("]");
    }
    if n.main {
        out.push("{b}");
    }
    true
}

/// `#{prefix:key}`, recognised by the parser only for a declared prefix
/// (design C16), so an unknown prefix keeps the role.
pub fn counter(out: &mut Out, n: &CounterItem) -> bool {
    let Some(lookup) = out.mkdocs() else {
        return false;
    };
    if !lookup.is_head(&n.prefix) || !bare(&n.key) || n.key.contains(':') {
        return false;
    }
    out.push("#{");
    out.push(&n.prefix);
    out.push(":");
    out.push(&n.key);
    out.push("}");
    true
}

/// `{margin}[…]{l}` (Appendix Deprecation schedule). Only an inline aside:
/// a multi-block one is a `::: aside` container in both profiles.
pub fn aside(out: &mut Out, n: &Aside, ctx: Context) -> bool {
    let [Block::Plain(plain)] = n.content.as_slice() else {
        return false;
    };
    out.push("{margin}[");
    inlines(
        out,
        &plain.content,
        Context {
            in_group: true,
            block_start: false,
            ..ctx
        },
    );
    out.push("]");
    if let Some(side) = n.side {
        out.push("{");
        out.push(&side_name(side)[..1]);
        out.push("}");
    }
    true
}

/// `[]{#id}` as `[](){#id}`, the empty-link anchor idiom (spec
/// Appendix "Deprecation schedule"). Python-Markdown's `attr_list` hangs
/// an attribute list on the element before it, and only the empty-link
/// spelling gives it an element with that id: `mkdocs-autorefs` scans the
/// element tree, where raw HTML is a stashed placeholder, so an anchor a
/// site must be able to point at is written this way. Only a span with no
/// content is an anchor; `[text]{#id}` is an attributed phrase.
pub fn anchor(out: &mut Out, n: &tmark_ir::SpanNode) -> bool {
    if !n.content.is_empty() || n.attrs.id.is_none() {
        return false;
    }
    out.push("[](){");
    out.push(&crate::attrs::items(&n.attrs));
    out.push("}");
    true
}

/// `{latex}[…]`, `{typst}[…]`, `{html}[…]`: the group is taken verbatim by
/// the parser, so the text may hold no bracket and may not end in a
/// backslash.
pub fn raw_inline(out: &mut Out, n: &RawInline) -> bool {
    if !matches!(n.format.as_str(), "latex" | "typst" | "html")
        || n.text.contains(['[', ']', '\n'])
        || n.text.ends_with('\\')
        // An HTML tag prints as typed in every profile (spec §Raw).
        || (n.format == "html" && crate::inline::is_html_tag(&n.text))
    {
        return false;
    }
    out.push("{");
    out.push(&n.format);
    out.push("}[");
    out.push(&n.text);
    out.push("]");
    true
}

/// `[^key]` and `^[k1,k2]` citations, `[](gls:term)` glossary references
/// (Appendix Deprecation schedule). Label references keep `@key`, which
/// TeXSmith 0.6 reads.
pub fn reference(out: &mut Out, items: &[RefItem]) -> bool {
    let Some(lookup) = out.mkdocs() else {
        return false;
    };
    if items.is_empty() || !items.iter().all(plain_item) {
        return false;
    }
    let registries: Vec<Registry> = items.iter().map(|i| lookup.registry(&i.key)).collect();
    match (items, registries.as_slice()) {
        ([item], [Registry::Glossary]) => {
            out.push("[](");
            out.push(&item.key);
            out.push(")");
            true
        }
        // The footnote spellings take the bare-key class of §Ref
        // (`[A-Za-z0-9_:.-]`); a key outside it (an implicit id of an
        // accented heading, which the printer cannot tell from a
        // bibliography key) keeps `@[key]`.
        _ if registries.iter().all(|r| *r == Registry::Bibliography)
            && !items.iter().any(|i| lookup.footnotes.contains(&i.key))
            && items.iter().all(|i| {
                i.key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-'))
            }) =>
        {
            if let [item] = items {
                out.push("[^");
                out.push(&item.key);
                out.push("]");
            } else {
                out.push("^[");
                let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
                out.push(&keys.join(","));
                out.push("]");
            }
            true
        }
        _ => false,
    }
}

/// `!!! type class "Title"`, `??? type` (collapsed), `???+ type` (open),
/// body indented by four spaces (spec Appendix PyMdownX). An id, an extra
/// attribute, a quote in the title or an empty body have no `!!!` spelling.
pub fn admonition(out: &mut Out, a: &Admonition) -> bool {
    let collapsed = a.attrs.kv.iter().find(|(k, _)| k == "collapsed");
    let marker = match collapsed.map(|(_, v)| v.as_str()) {
        None => "!!!",
        Some("true") => "???",
        Some("false") => "???+",
        Some(_) => return false,
    };
    if a.attrs.id.is_some()
        || a.attrs.kv.len() > usize::from(collapsed.is_some())
        || !bare(&a.kind)
        || !a.attrs.classes.iter().all(|c| bare(c))
        || a.content.is_empty()
    {
        return false;
    }
    let title = a.title.as_ref().map(|title| {
        let mut buf = Out::with_profile(out.profile());
        inlines(
            &mut buf,
            title,
            Context {
                in_group: true,
                ..Context::default()
            },
        );
        buf.finish().trim_end().to_string()
    });
    if title
        .as_deref()
        .is_some_and(|t| t.contains(['"', '\n']) || t != t.trim())
    {
        return false;
    }
    out.push(marker);
    out.push(" ");
    out.push(&a.kind);
    for class in &a.attrs.classes {
        out.push(" ");
        out.push(class);
    }
    if let Some(title) = title {
        out.push(" \"");
        out.push(&title);
        out.push("\"");
    }
    out.push("\n");
    out.push_prefix("    ");
    crate::block::blocks(out, &a.content);
    out.pop_prefix();
    out.ensure_newline();
    true
}

/// `/// caption` with `attrs: {id: …}` (pymdownx.blocks.caption, what
/// TeXSmith 0.6 renders under a figure or a listing). A table caption is
/// the `Table:` line before its table instead (`order`).
pub fn caption(out: &mut Out, c: &Caption) -> bool {
    if c.kind == CaptionKind::Table
        || !c.attrs.classes.is_empty()
        || !c.attrs.kv.is_empty()
        || c.attrs.id.as_deref().is_some_and(|id| !bare(id))
    {
        return false;
    }
    let mut buf = Out::with_profile(out.profile());
    inlines(&mut buf, &c.content, Context::default());
    let text = buf.finish();
    if text.lines().any(|l| l.trim() == "///") {
        return false;
    }
    out.push("/// caption\n");
    if let Some(id) = &c.attrs.id {
        out.push("    attrs: {id: ");
        out.push(id);
        out.push("}\n");
    }
    out.push(text.trim_end_matches('\n'));
    out.push("\n///\n");
    true
}

/// `/// latex … ///` (Appendix Deprecation schedule; the only backend the
/// slash block ever spelled).
pub fn raw_block(out: &mut Out, r: &RawBlock) -> bool {
    if r.format != "latex" || r.text.lines().any(|l| l.trim() == "///") {
        return false;
    }
    out.push("/// latex\n");
    if !r.text.is_empty() {
        out.push(&r.text);
        out.push("\n");
    }
    out.push("///\n");
    true
}

/// `=== "Title"` per tab, the body indented by four spaces (pymdownx.tabbed,
/// spec §Tabs). Every child must be a `tab` whose only attribute is a
/// title without a quote or a line break, and have content; anything else
/// (an id, a class, an empty tab) keeps the `:::: tabs` spelling.
pub fn tabs(out: &mut Out, d: &Div) -> bool {
    if !d.attrs.is_empty() || d.content.is_empty() {
        return false;
    }
    let mut titles = Vec::new();
    for block in &d.content {
        let Block::Div(tab) = block else { return false };
        let title = tab.attrs.get("title").unwrap_or_default();
        if tab.name != "tab"
            || tab.attrs.id.is_some()
            || !tab.attrs.classes.is_empty()
            || tab.attrs.kv.len() != 1
            || tab.content.is_empty()
            || title.is_empty()
            || title != title.trim()
            || title.contains(['"', '\n'])
        {
            return false;
        }
        titles.push(title);
    }
    for (i, block) in d.content.iter().enumerate() {
        let Block::Div(tab) = block else {
            unreachable!()
        };
        if i > 0 {
            out.blank_line();
        }
        out.push("=== \"");
        out.push(titles[i]);
        out.push("\"\n");
        out.blank_line();
        out.push_prefix("    ");
        crate::block::blocks(out, &tab.content);
        out.pop_prefix();
        out.ensure_newline();
    }
    true
}

/// `<div class="x" markdown>` … `</div>` (Python-Markdown `md_in_html`,
/// spec §Div: the only container spelling a Python-Markdown site renders).
/// An attribute other than the id and the classes has no HTML-safe
/// spelling here and keeps `::: div`.
pub fn div_markdown(out: &mut Out, d: &Div) -> bool {
    if !d.attrs.kv.is_empty()
        || d.attrs.id.as_deref().is_some_and(|id| !bare(id))
        || !d.attrs.classes.iter().all(|c| bare(c))
    {
        return false;
    }
    out.push("<div");
    if let Some(id) = &d.attrs.id {
        out.push(&format!(" id=\"{id}\""));
    }
    if !d.attrs.classes.is_empty() {
        out.push(&format!(" class=\"{}\"", d.attrs.classes.join(" ")));
    }
    out.push(" markdown>\n");
    crate::block::blocks(out, &d.content);
    out.ensure_newline();
    out.push("</div>\n");
    true
}

/// `--8<-- "file"` (pymdownx.snippets); a `base` has no snippet spelling.
pub fn include(out: &mut Out, i: &Include) -> bool {
    if i.base.is_some() || i.path.contains(['"', '\n']) || i.path.is_empty() {
        return false;
    }
    out.push("--8<-- \"");
    out.push(&i.path);
    out.push("\"\n");
    true
}

/// The blocks in printing order: a table caption moves before its table
/// (the `Table:` line TeXSmith 0.6 reads), unless the block printed before
/// it is a float, which the parser would attach the caption to first
/// (spec §Caption: the previous host wins).
pub fn order(blocks: &[Block]) -> Vec<&Block> {
    let mut out: Vec<&Block> = Vec::with_capacity(blocks.len());
    let mut i = 0;
    while i < blocks.len() {
        if let Block::Table(_) = &blocks[i] {
            let config = matches!(blocks.get(i + 1), Some(Block::TableConfig(_)));
            let at = i + 1 + usize::from(config);
            let caption = match blocks.get(at) {
                Some(Block::Caption(c)) if c.kind == CaptionKind::Table => Some(&blocks[at]),
                _ => None,
            };
            if let Some(caption) = caption {
                if !out.last().is_some_and(|b| b.is_float()) {
                    out.push(caption);
                    out.extend(&blocks[i..at]);
                    i = at + 1;
                    continue;
                }
            }
        }
        out.push(&blocks[i]);
        i += 1;
    }
    out
}
