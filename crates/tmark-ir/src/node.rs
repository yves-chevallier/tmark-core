//! The node types: `Document`, `Block`, `Inline` and their supporting
//! structs, one per entry of the spec's node catalogue.
//!
//! Design: `design/03-ir.md` §Shape, §Node catalogue, §Serialization.
//! Spec §Node catalogue. Every node starts with a flattened [`Meta`]
//! (`id`, `span`); equality ignores it (design rule 3), [`eq_with_spans`]
//! compares it too.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::attrs::Attrs;
use crate::frontmatter::FrontMatter;
use crate::span::{FileId, NodeId, Span, SubSpan};
pub use crate::table::{
    Align, Cell, Column, ColumnConfig, ColumnGroup, DataRow, LeafColumn, Row, Separator,
    TableModel, TableSettings,
};
use crate::walk::{walk, NodeRef};

fn is_false(b: &bool) -> bool {
    !*b
}

/// Identity and source span, present on every node (design §Identity and
/// spans). Serialised flat into the node as `id` and `span`.
///
/// `PartialEq` on `Meta` is always `true`, which is what makes the derived
/// equality of every node structural (design rule 3: "by value excluding
/// spans and ids"). Use [`Meta::same`] or [`eq_with_spans`] to compare
/// identities.
#[derive(Copy, Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Meta {
    pub id: NodeId,
    pub span: Span,
}

impl Meta {
    pub fn new(id: NodeId, span: Span) -> Self {
        Meta { id, span }
    }

    /// Identity comparison: same id and same span.
    pub fn same(&self, other: &Meta) -> bool {
        self.id == other.id && self.span == other.span
    }
}

impl PartialEq for Meta {
    fn eq(&self, _: &Meta) -> bool {
        true
    }
}

/// A parsed file. Design §Shape.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Document {
    #[serde(default)]
    pub file: FileId,
    #[serde(default)]
    pub front_matter: FrontMatter,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// `*[HTML]: …` definitions, document-level (spec §Glossary and acronyms).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abbreviations: Vec<AbbrDef>,
    /// `[^1]: …` definitions, document-level (spec §Note).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub footnotes: Vec<Footnote>,
}

/// Structural equality plus identity: `a == b` and every node of `a` has the
/// id and span of its counterpart in `b` (pre-order). For tests.
pub fn eq_with_spans(a: &Document, b: &Document) -> bool {
    if a != b || !a.front_matter.meta.same(&b.front_matter.meta) {
        return false;
    }
    let mut metas_a = Vec::new();
    walk(a, &mut |n: NodeRef| metas_a.push(*meta_of(n)));
    let mut metas_b = Vec::new();
    walk(b, &mut |n: NodeRef| metas_b.push(*meta_of(n)));
    metas_a.len() == metas_b.len() && metas_a.iter().zip(&metas_b).all(|(x, y)| x.same(y))
}

fn meta_of(n: NodeRef<'_>) -> &Meta {
    match n {
        NodeRef::Block(b) => b.meta(),
        NodeRef::Inline(i) => i.meta(),
    }
}

/// Fields that record which spelling was written rather than what the node
/// is (`Caption.position`, `Ref.bracketed`). Every input of a conformance
/// fixture must agree on everything else (design 10 §Conformance fixtures).
pub const SUGAR_FIELDS: &[&str] = &["position", "bracketed"];

/// The document as JSON without identity and spelling: the file id, node
/// ids, spans (`span` and every `*_span` field), the [`SUGAR_FIELDS`], and
/// absent-equivalent values (`null`, empty strings, arrays and objects;
/// booleans and numbers stay). What conformance fixtures store and what the
/// round-trip tests compare (spec §Round-trip and source spans: "modulo
/// source spans"); the JSON twin of [`eq_with_spans`]'s complement.
pub fn structural_json(doc: &Document) -> serde_json::Value {
    let mut value = serde_json::to_value(doc).expect("the IR serialises");
    if let serde_json::Value::Object(map) = &mut value {
        // Not `remove`: with `preserve_order` that swaps the last key in.
        map.shift_remove("file");
    }
    strip_identity(value)
}

fn strip_identity(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            // `id` next to a `span` is a node identity; `attrs.id` is content.
            let node = map.contains_key("span");
            Value::Object(
                map.into_iter()
                    .filter(|(k, _)| {
                        k != "span"
                            && !(node && k == "id")
                            && !k.ends_with("_span")
                            && !SUGAR_FIELDS.contains(&k.as_str())
                    })
                    .map(|(k, v)| (k, strip_identity(v)))
                    .filter(|(_, v)| !is_absent(v))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(strip_identity).collect()),
        other => other,
    }
}

fn is_absent(value: &serde_json::Value) -> bool {
    use serde_json::Value;
    match value {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        Value::String(s) => s.is_empty(),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

/// A `*[HTML]: HyperText Markup Language` definition. Spec §Glossary and
/// acronyms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AbbrDef {
    #[serde(flatten)]
    pub meta: Meta,
    pub key: String,
    pub expansion: String,
}

/// A `[^label]: …` definition. Spec §Note.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Footnote {
    #[serde(flatten)]
    pub meta: Meta,
    pub label: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

/// Where a link points. Spec §Ref (textual references) and §Includes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "value")]
pub enum Target {
    /// An external URL.
    Url(String),
    /// `#id` in this document.
    Anchor(String),
    /// `[text][id]`, a reference-style link no definition matches: a
    /// textual reference when `id` is a label of the document or of the
    /// book, and the literal text CommonMark makes of it otherwise.
    /// Spec §Ref.
    Reference(String),
    /// Another document, `[](other.md)`.
    Document(String),
}

/// One item of a reference or citation group. Spec §Ref, §Cite (Pandoc's
/// item grammar).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RefItem {
    /// Text before the key (`see`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// `@[-key]`: the year alone.
    #[serde(default, skip_serializing_if = "is_false")]
    pub suppress_author: bool,
    /// `@[+key]`: the narrative citation, whatever the document default
    /// (spec §Cite, C51).
    #[serde(default, skip_serializing_if = "is_false")]
    pub narrative: bool,
    /// The key as written, prefix included (`fig:boot`, `ein05`, `Fig:x`).
    pub key: String,
    /// Source range of `key` alone: no `@`, `-`, prefix or suffix (spec
    /// §Round-trip and source spans; design 03 §Identity and spans).
    /// `Span::default()` when the item was built from JSON.
    #[serde(default)]
    pub key_span: SubSpan,
    /// Locator or suffix text (`p. 33`, `column 3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

/// A list item. Spec §BulletList, OrderedList.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<Task>,
}

/// Task state of `- [ ]`, `- [x]` and `- [.]` (feature `tasklist.partial`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    Open,
    Done,
    Partial,
}

/// Numbering style of an ordered list (`pymdownx.fancylists` markers, spec
/// Appendix "PyMdownX compatibility profile").
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListStyle {
    #[default]
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
    /// `#.`: numbered by the backend.
    Generic,
}

/// Layout hint of an aside. Spec §Aside.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Left,
    Right,
    Outer,
    Inner,
}

/// Spec §Caption: `Table:`, `Figure:`, `Listing:`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionKind {
    Table,
    Figure,
    Listing,
}

impl CaptionKind {
    /// The caption word as written (`Table:`). Spec §Caption.
    pub fn word(self) -> &'static str {
        match self {
            CaptionKind::Table => "Table",
            CaptionKind::Figure => "Figure",
            CaptionKind::Listing => "Listing",
        }
    }

    /// The conventional counter prefix of the float (spec §Anchor: `tbl:`,
    /// `fig:`, `lst:`), an entry of `registry::PREFIXES`.
    pub fn prefix(self) -> &'static str {
        match self {
            CaptionKind::Table => "tbl",
            CaptionKind::Figure => "fig",
            CaptionKind::Listing => "lst",
        }
    }
}

/// Source position of a caption line relative to its float. Spec §Caption:
/// after is canonical, before is accepted sugar.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionPosition {
    Before,
    #[default]
    After,
}

/// Spec §Inline text: `"x"` or `'x'`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QuoteKind {
    #[default]
    Double,
    Single,
}

// ---------------------------------------------------------------------------
// Inline nodes
// ---------------------------------------------------------------------------

/// A run of text. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Str {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
}

/// Inter-word space.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Space {
    #[serde(flatten)]
    pub meta: Meta,
}

/// A newline inside a paragraph, kept so prose is not re-wrapped
/// (design §Node catalogue).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SoftBreak {
    #[serde(flatten)]
    pub meta: Meta,
}

/// A hard line break (trailing `\`). Spec §HorizontalRule.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineBreak {
    #[serde(flatten)]
    pub meta: Meta,
}

/// `*x*`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Emph {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `**x**`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Strong {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `{del}[x]`, `~~x~~`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Strikeout {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `{underline}[x]`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Underline {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `{mark}[x]`, `==x==`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Highlight {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `{sub}[x]`, `~x~`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Subscript {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `{sup}[x]`, `^x^`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Superscript {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `{sc}[x]`, `__x__`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SmallCaps {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `"x"`, `'x'`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Quoted {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default)]
    pub kind: QuoteKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `` `x` ``, `{code py}[…]`, `` `#!py …` ``. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Code {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

/// `$…$`. Spec §Math (inline). Display math is [`MathBlock`]; `display` is
/// kept for `\[…\]` inside a paragraph.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Math {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub display: bool,
}

/// `[text](target "title")`. Spec §Ref (textual references).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Link {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
    pub target: Target,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// `@key`, `@[key, locator; key2]`. Spec §Ref, §Cite. Resolution decides
/// label versus citation versus glossary.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Ref {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<RefItem>,
    /// Written `@[…]` rather than bare.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bracketed: bool,
}

/// A footnote: `[^label]` refers to a [`Footnote`] definition, `^[text]` is
/// inline and carries its `content`. Exactly one of the two is set. Spec
/// §Note.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
}

/// `![alt](src){attrs}`. Spec §Image, Figure. `width`, `align`, `media` are
/// attributes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Image {
    #[serde(flatten)]
    pub meta: Meta,
    pub src: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alt: Vec<Inline>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// `{index}[a][b]`, `#[a][b]`. Spec §IndexEntry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IndexEntry {
    #[serde(flatten)]
    pub meta: Meta,
    /// One to three levels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<Vec<Inline>>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub main: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
}

/// `{counter}(fw:x)`, `#(fw:x)`. Spec §CounterItem.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CounterItem {
    #[serde(flatten)]
    pub meta: Meta,
    pub prefix: String,
    pub key: String,
    /// Source range of `key` alone (design 03 §Identity and spans).
    #[serde(default)]
    pub key_span: SubSpan,
}

/// `{keys}[ctrl+s]`, `++ctrl+s++`. Spec §Inline text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Keystroke {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
}

/// `{aside}[…]`, `::: aside`. Spec §Aside (the IR name of the spec's
/// `MarginNote`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Aside {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
}

/// `[x]{attrs}`: the anonymous span, a host for `#id`, `lang`, `media`.
/// Spec §Roles. Named `SpanNode` in Rust because [`Span`] is the source
/// range; its JSON `type` is `"Span"`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpanNode {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// `{{ press.template }}`. Spec §Front matter (moustaches).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Var {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
}

/// A substituted acronym, resolved from `Document::abbreviations`. Spec
/// §Glossary and acronyms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Abbr {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
}

/// `<!-- … -->`, inline or block. Spec §Comment. Zero width in the flow.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Comment {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
}

/// `[=45% "Review"]{.thin}`. Spec §ProgressBar: an inline bar with a
/// `value` from 0 to 100 (clamped) and an optional `label` (the
/// percentage when absent); `.thin` and the other classes are attributes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProgressBar {
    #[serde(flatten)]
    pub meta: Meta,
    /// Percentage, 0 to 100.
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

impl ProgressBar {
    /// The canonical spelling without the attribute list, `[=45% "label"]`
    /// (spec §ProgressBar: PyMdownX's percentage form). Shared by the
    /// printer and the parser's fix for the fraction form; a `"` inside
    /// the label, which the recogniser cannot hold, is dropped.
    pub fn head_text(&self) -> String {
        let mut out = format!("[={}%", self.value_text());
        if let Some(label) = &self.label {
            out.push_str(&format!(" \"{}\"", label.replace('"', "")));
        }
        out.push(']');
        out
    }

    /// The value as the canonical spelling writes it: an integer when it
    /// is one, else at most two decimals (`45`, `33.33`).
    pub fn value_text(&self) -> String {
        let value = self.value.clamp(0.0, 100.0);
        let rounded = (value * 100.0).round() / 100.0;
        if rounded.fract() == 0.0 {
            format!("{}", rounded as u32)
        } else {
            let text = format!("{rounded:.2}");
            text.trim_end_matches('0').to_string()
        }
    }
}

/// `{raw latex}(…)`. Spec §Raw passthrough. `format=html` is a tag or an
/// HTML block kept as typed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RawInline {
    #[serde(flatten)]
    pub meta: Meta,
    pub format: String,
    pub text: String,
}

/// Spec §Node catalogue, inline entries. Serialised with a `type` tag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Inline {
    Str(Str),
    Space(Space),
    SoftBreak(SoftBreak),
    LineBreak(LineBreak),
    Emph(Emph),
    Strong(Strong),
    Strikeout(Strikeout),
    Underline(Underline),
    Highlight(Highlight),
    Subscript(Subscript),
    Superscript(Superscript),
    SmallCaps(SmallCaps),
    Quoted(Quoted),
    Code(Code),
    Math(Math),
    Link(Link),
    Ref(Ref),
    Note(Note),
    Image(Image),
    IndexEntry(IndexEntry),
    CounterItem(CounterItem),
    Keystroke(Keystroke),
    Aside(Aside),
    Span(SpanNode),
    Var(Var),
    Abbr(Abbr),
    Comment(Comment),
    RawInline(RawInline),
    ProgressBar(ProgressBar),
}

impl Inline {
    pub fn meta(&self) -> &Meta {
        match self {
            Inline::Str(n) => &n.meta,
            Inline::Space(n) => &n.meta,
            Inline::SoftBreak(n) => &n.meta,
            Inline::LineBreak(n) => &n.meta,
            Inline::Emph(n) => &n.meta,
            Inline::Strong(n) => &n.meta,
            Inline::Strikeout(n) => &n.meta,
            Inline::Underline(n) => &n.meta,
            Inline::Highlight(n) => &n.meta,
            Inline::Subscript(n) => &n.meta,
            Inline::Superscript(n) => &n.meta,
            Inline::SmallCaps(n) => &n.meta,
            Inline::Quoted(n) => &n.meta,
            Inline::Code(n) => &n.meta,
            Inline::Math(n) => &n.meta,
            Inline::Link(n) => &n.meta,
            Inline::Ref(n) => &n.meta,
            Inline::Note(n) => &n.meta,
            Inline::Image(n) => &n.meta,
            Inline::IndexEntry(n) => &n.meta,
            Inline::CounterItem(n) => &n.meta,
            Inline::Keystroke(n) => &n.meta,
            Inline::Aside(n) => &n.meta,
            Inline::Span(n) => &n.meta,
            Inline::Var(n) => &n.meta,
            Inline::Abbr(n) => &n.meta,
            Inline::Comment(n) => &n.meta,
            Inline::RawInline(n) => &n.meta,
            Inline::ProgressBar(n) => &n.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut Meta {
        match self {
            Inline::Str(n) => &mut n.meta,
            Inline::Space(n) => &mut n.meta,
            Inline::SoftBreak(n) => &mut n.meta,
            Inline::LineBreak(n) => &mut n.meta,
            Inline::Emph(n) => &mut n.meta,
            Inline::Strong(n) => &mut n.meta,
            Inline::Strikeout(n) => &mut n.meta,
            Inline::Underline(n) => &mut n.meta,
            Inline::Highlight(n) => &mut n.meta,
            Inline::Subscript(n) => &mut n.meta,
            Inline::Superscript(n) => &mut n.meta,
            Inline::SmallCaps(n) => &mut n.meta,
            Inline::Quoted(n) => &mut n.meta,
            Inline::Code(n) => &mut n.meta,
            Inline::Math(n) => &mut n.meta,
            Inline::Link(n) => &mut n.meta,
            Inline::Ref(n) => &mut n.meta,
            Inline::Note(n) => &mut n.meta,
            Inline::Image(n) => &mut n.meta,
            Inline::IndexEntry(n) => &mut n.meta,
            Inline::CounterItem(n) => &mut n.meta,
            Inline::Keystroke(n) => &mut n.meta,
            Inline::Aside(n) => &mut n.meta,
            Inline::Span(n) => &mut n.meta,
            Inline::Var(n) => &mut n.meta,
            Inline::Abbr(n) => &mut n.meta,
            Inline::Comment(n) => &mut n.meta,
            Inline::RawInline(n) => &mut n.meta,
            Inline::ProgressBar(n) => &mut n.meta,
        }
    }
}

// ---------------------------------------------------------------------------
// Block nodes
// ---------------------------------------------------------------------------

/// A paragraph, with an optional `{lead}[…]` run-in. Spec §Para.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Para {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<Vec<Inline>>,
}

/// Inline content without paragraph semantics (tight list items).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Plain {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
}

/// `#` to `######`. Spec §Header. The id lives in `attrs`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Header {
    #[serde(flatten)]
    pub meta: Meta,
    /// 1 to 6.
    pub level: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// A fenced or indented code block. Spec §CodeBlock, listing. `options`
/// holds the info-string keys (`title`, `linenums`, `hl_lines`, `include`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CodeBlock {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub options: Attrs,
}

/// `>`. Spec §BlockQuote; `.epigraph` is a class.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BlockQuote {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// `-` list. Spec §BulletList, OrderedList.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BulletList {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ListItem>,
}

/// `1.` list. Spec §BulletList, OrderedList.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrderedList {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ListItem>,
    #[serde(default)]
    pub start: u32,
    #[serde(default)]
    pub style: ListStyle,
}

/// PHP-Markdown-Extra definition list. Spec §DefinitionList. Each item is a
/// term and its definitions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DefinitionList {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<(Vec<Inline>, Vec<Vec<Block>>)>,
}

/// `---`: a divider. Spec §HorizontalRule.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HorizontalRule {
    #[serde(flatten)]
    pub meta: Meta,
}

/// A table of any rung of the ladder. Spec §Table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Table {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default)]
    pub model: TableModel,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
    /// The body of a `yaml table` fence the parser reported a `table-*`
    /// diagnostic on: `model` is then a best effort and the printer writes
    /// this text back as typed (design 03 §Tables). `None` for a table the
    /// model holds faithfully.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A `yaml table-config` fence, attached to the preceding [`Table`] by a
/// pass; kept as a node so round-trip is exact. Spec §Table rung 3.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TableConfig {
    #[serde(flatten)]
    pub meta: Meta,
    /// Positional, matched against the table's leaf columns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<ColumnConfig>,
    #[serde(default)]
    pub settings: TableSettings,
    /// The fence body when the parser reported a `table-*` diagnostic on
    /// it; printed back as typed (see [`Table::source`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// `Kind: text {#id}` caption line. Spec §Caption. The anchor lives in
/// `attrs`; `position` is what the printer normalises.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Caption {
    #[serde(flatten)]
    pub meta: Meta,
    pub kind: CaptionKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Inline>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
    #[serde(default)]
    pub position: CaptionPosition,
}

/// `::: figure`. Spec §Image, Figure: the images inside are subfigures.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Figure {
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// `::: warning {title="…"}`, `!!! warning "…"`. Spec §Admonition.
/// `collapsed` is an attribute.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Admonition {
    #[serde(flatten)]
    pub meta: Meta,
    /// The type word: built-in, theorem, or declared.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<Vec<Inline>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// Any other `::: name` container. Spec §Div: the names of the closed
/// registry (`registry::CONTAINERS`: `tabs`, `tab`, `multicolumn`, `div`)
/// and, with a `container-unknown` diagnostic, any other name, kept so the
/// printer round-trips it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Div {
    #[serde(flatten)]
    pub meta: Meta,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// `$$ … $$ {#eq:x}`. Spec §Math (display), equation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MathBlock {
    #[serde(flatten)]
    pub meta: Meta,
    pub text: String,
    #[serde(default, skip_serializing_if = "Attrs::is_empty")]
    pub attrs: Attrs,
}

/// `latex raw` fence. Spec §Raw passthrough. `format=html` is an HTML
/// block kept as typed; `format=markdown` is a foreign directive (spec
/// §Foreign directive: `[TOC]`, a dotted `::: a.b` line with its indented
/// continuation) kept verbatim, printed as typed and rendered by no other
/// writer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RawBlock {
    #[serde(flatten)]
    pub meta: Meta,
    pub format: String,
    pub text: String,
}

/// `{include}(file)` alone on its line. Spec §Includes. TeXSmith splices.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Include {
    #[serde(flatten)]
    pub meta: Meta,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
}

/// Spec §Node catalogue, block entries. Serialised with a `type` tag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum Block {
    Para(Para),
    Plain(Plain),
    Header(Header),
    CodeBlock(CodeBlock),
    BlockQuote(BlockQuote),
    BulletList(BulletList),
    OrderedList(OrderedList),
    DefinitionList(DefinitionList),
    HorizontalRule(HorizontalRule),
    Table(Table),
    TableConfig(TableConfig),
    Caption(Caption),
    Figure(Figure),
    Admonition(Admonition),
    Div(Div),
    MathBlock(MathBlock),
    RawBlock(RawBlock),
    Include(Include),
    Comment(Comment),
}

impl Block {
    pub fn meta(&self) -> &Meta {
        match self {
            Block::Para(n) => &n.meta,
            Block::Plain(n) => &n.meta,
            Block::Header(n) => &n.meta,
            Block::CodeBlock(n) => &n.meta,
            Block::BlockQuote(n) => &n.meta,
            Block::BulletList(n) => &n.meta,
            Block::OrderedList(n) => &n.meta,
            Block::DefinitionList(n) => &n.meta,
            Block::HorizontalRule(n) => &n.meta,
            Block::Table(n) => &n.meta,
            Block::TableConfig(n) => &n.meta,
            Block::Caption(n) => &n.meta,
            Block::Figure(n) => &n.meta,
            Block::Admonition(n) => &n.meta,
            Block::Div(n) => &n.meta,
            Block::MathBlock(n) => &n.meta,
            Block::RawBlock(n) => &n.meta,
            Block::Include(n) => &n.meta,
            Block::Comment(n) => &n.meta,
        }
    }

    pub fn meta_mut(&mut self) -> &mut Meta {
        match self {
            Block::Para(n) => &mut n.meta,
            Block::Plain(n) => &mut n.meta,
            Block::Header(n) => &mut n.meta,
            Block::CodeBlock(n) => &mut n.meta,
            Block::BlockQuote(n) => &mut n.meta,
            Block::BulletList(n) => &mut n.meta,
            Block::OrderedList(n) => &mut n.meta,
            Block::DefinitionList(n) => &mut n.meta,
            Block::HorizontalRule(n) => &mut n.meta,
            Block::Table(n) => &mut n.meta,
            Block::TableConfig(n) => &mut n.meta,
            Block::Caption(n) => &mut n.meta,
            Block::Figure(n) => &mut n.meta,
            Block::Admonition(n) => &mut n.meta,
            Block::Div(n) => &mut n.meta,
            Block::MathBlock(n) => &mut n.meta,
            Block::RawBlock(n) => &mut n.meta,
            Block::Include(n) => &mut n.meta,
            Block::Comment(n) => &mut n.meta,
        }
    }

    /// A block a caption line attaches to (spec §Caption): a table, a
    /// figure container, a code block, a table configuration, or a
    /// paragraph made of images only.
    pub fn is_float(&self) -> bool {
        match self {
            Block::Table(_) | Block::Figure(_) | Block::CodeBlock(_) | Block::TableConfig(_) => {
                true
            }
            Block::Para(para) => {
                para.content.iter().any(|i| matches!(i, Inline::Image(_)))
                    && para.content.iter().all(|i| match i {
                        Inline::Image(_) | Inline::SoftBreak(_) => true,
                        Inline::Str(s) => s.text.trim().is_empty(),
                        _ => false,
                    })
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: u32, start: u32, end: u32) -> Meta {
        Meta::new(NodeId(id), Span::new(FileId(0), start, end))
    }

    fn str_node(id: u32, start: u32, text: &str) -> Inline {
        Inline::Str(Str {
            meta: meta(id, start, start + text.len() as u32),
            text: text.into(),
        })
    }

    fn sample() -> Document {
        Document {
            file: FileId(0),
            front_matter: FrontMatter::default(),
            blocks: vec![
                Block::Header(Header {
                    meta: meta(1, 0, 20),
                    level: 2,
                    content: vec![str_node(2, 3, "Title")],
                    attrs: Attrs {
                        id: Some("sec:intro".into()),
                        ..Default::default()
                    },
                }),
                Block::Para(Para {
                    meta: meta(3, 22, 60),
                    content: vec![
                        str_node(4, 22, "See"),
                        Inline::Space(Space {
                            meta: meta(5, 25, 26),
                        }),
                        Inline::Emph(Emph {
                            meta: meta(6, 26, 31),
                            content: vec![str_node(7, 27, "fig")],
                        }),
                        Inline::Ref(Ref {
                            meta: meta(8, 32, 41),
                            items: vec![RefItem {
                                key: "fig:boot".into(),
                                ..Default::default()
                            }],
                            bracketed: false,
                        }),
                        Inline::Link(Link {
                            meta: meta(9, 42, 60),
                            content: vec![str_node(10, 43, "the trace")],
                            target: Target::Anchor("fig:trace".into()),
                            title: None,
                        }),
                    ],
                    lead: None,
                }),
            ],
            abbreviations: vec![],
            footnotes: vec![Footnote {
                meta: meta(11, 62, 70),
                label: "1".into(),
                content: vec![Block::Para(Para {
                    meta: meta(12, 67, 70),
                    content: vec![str_node(13, 67, "foo")],
                    lead: None,
                })],
            }],
        }
    }

    #[test]
    fn serde_round_trip() {
        let doc = sample();
        let json = serde_json::to_value(&doc).unwrap();
        assert_eq!(json["blocks"][0]["type"], "Header");
        assert_eq!(json["blocks"][0]["id"], 1);
        assert_eq!(json["blocks"][0]["span"], serde_json::json!([0, 0, 20]));
        assert_eq!(json["blocks"][1]["content"][2]["type"], "Emph");
        assert_eq!(
            json["blocks"][1]["content"][4]["target"],
            serde_json::json!({"type": "Anchor", "value": "fig:trace"})
        );
        // Options and empty vectors are skipped.
        assert!(json["blocks"][1].get("lead").is_none());
        assert!(json["blocks"][1]["content"][3].get("bracketed").is_none());
        assert!(json.get("abbreviations").is_none());
        let back: Document = serde_json::from_value(json).unwrap();
        assert_eq!(back, doc);
        assert!(eq_with_spans(&back, &doc));
    }

    #[test]
    fn deserialises_without_ids_and_spans() {
        let json = serde_json::json!({
            "blocks": [{"type": "Para", "content": [
                {"type": "Str", "text": "hi"},
                {"type": "Span", "content": [], "attrs": {"id": "x"}}
            ]}]
        });
        let doc: Document = serde_json::from_value(json).unwrap();
        assert_eq!(doc.blocks.len(), 1);
        assert_eq!(doc.blocks[0].meta().id, NodeId(0));
        match &doc.blocks[0] {
            Block::Para(p) => {
                assert!(matches!(&p.content[1], Inline::Span(s) if s.attrs.id() == Some("x")))
            }
            other => panic!("expected Para, got {other:?}"),
        }
    }

    #[test]
    fn equality_ignores_meta() {
        let a = sample();
        let mut b = sample();
        b.blocks[0].meta_mut().id = NodeId(99);
        b.footnotes[0].content[0].meta_mut().span = Span::new(FileId(0), 0, 1);
        assert_eq!(a, b);
        assert!(!eq_with_spans(&a, &b));
        let mut c = sample();
        if let Block::Header(h) = &mut c.blocks[0] {
            h.level = 3;
        }
        assert_ne!(a, c);
    }
}
