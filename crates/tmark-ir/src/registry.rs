//! The closed registries: roles, node words, predeclared counter prefixes,
//! admonition types, container names, features, deprecated spellings,
//! fragment contracts, keystroke labels and the TeX logo words.
//!
//! Design: `design/03-ir.md` §Closed registries. Each table is a `const`
//! slice of a small struct; grammars, completion lists, lint messages and
//! documentation are generated from them and no other crate hard-codes a
//! role name (AGENTS.md, SSOT).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// How a role takes its payload. Spec §Roles: "brackets hold content,
/// parentheses hold a verbatim argument; each role accepts one form or the
/// other, never both".
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum ArgStyle {
    /// One bracket group of Markdown content: `{aside}[…]`.
    Content,
    /// One or more bracket groups: `{index}[a][b]`.
    ContentMany,
    /// One parenthesised verbatim argument: `{raw latex}(…)`.
    Argument,
}

/// One entry of the role registry. Spec §Roles, §Node catalogue.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Role {
    /// The bare identifier after `{`.
    pub name: &'static str,
    pub arg: ArgStyle,
    /// The key the positional argument stands for: `{code py}` is
    /// `{code lang=py}`. `None` when the role takes no positional argument.
    pub principal: Option<&'static str>,
    /// Accepted `key=value` keys, the principal one included.
    pub keys: &'static [&'static str],
    /// The IR node the role produces (a variant name of `Inline` or `Block`).
    pub node: &'static str,
    /// The canonical role that replaces a deprecated one (Appendix
    /// "Deprecation schedule"); `None` for a canonical role.
    pub replaced_by: Option<&'static str>,
}

const fn mk_role(
    name: &'static str,
    arg: ArgStyle,
    principal: Option<&'static str>,
    keys: &'static [&'static str],
    node: &'static str,
) -> Role {
    Role {
        name,
        arg,
        principal,
        keys,
        node,
        replaced_by: None,
    }
}

const fn deprecated_role(
    name: &'static str,
    arg: ArgStyle,
    node: &'static str,
    replaced_by: &'static str,
) -> Role {
    Role {
        name,
        arg,
        principal: None,
        keys: &[],
        node,
        replaced_by: Some(replaced_by),
    }
}

/// Spec §Roles, §Inline text, §Notes, §Anchors, references, citations, §Raw
/// passthrough, §Includes.
pub const ROLES: &[Role] = &[
    mk_role("lead", ArgStyle::Content, None, &[], "Para"),
    mk_role("sc", ArgStyle::Content, None, &[], "SmallCaps"),
    mk_role("del", ArgStyle::Content, None, &[], "Strikeout"),
    mk_role("underline", ArgStyle::Content, None, &[], "Underline"),
    mk_role("mark", ArgStyle::Content, None, &[], "Highlight"),
    mk_role("sub", ArgStyle::Content, None, &[], "Subscript"),
    mk_role("sup", ArgStyle::Content, None, &[], "Superscript"),
    mk_role("keys", ArgStyle::Content, None, &[], "Keystroke"),
    mk_role("code", ArgStyle::Content, Some("lang"), &["lang"], "Code"),
    mk_role("aside", ArgStyle::Content, Some("side"), &["side"], "Aside"),
    mk_role(
        "index",
        ArgStyle::ContentMany,
        None,
        &["main", "registry"],
        "IndexEntry",
    ),
    mk_role("counter", ArgStyle::Argument, None, &[], "CounterItem"),
    mk_role(
        "raw",
        ArgStyle::Argument,
        Some("backend"),
        &["backend"],
        "RawInline",
    ),
    mk_role("include", ArgStyle::Argument, None, &["base"], "Include"),
    // Deprecated spellings, still parsed (Appendix "Deprecation schedule").
    deprecated_role("margin", ArgStyle::Content, "Aside", "aside"),
    deprecated_role("latex", ArgStyle::Content, "RawInline", "raw"),
    deprecated_role("typst", ArgStyle::Content, "RawInline", "raw"),
    deprecated_role("html", ArgStyle::Content, "RawInline", "raw"),
];

/// Looks a role up by its exact name.
pub fn role(name: &str) -> Option<&'static Role> {
    ROLES.iter().find(|r| r.name == name)
}

// ---------------------------------------------------------------------------
// Node words
// ---------------------------------------------------------------------------

/// The second word of a data directive's info string. Spec §Data directives.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodeWord {
    pub word: &'static str,
    /// The block node produced.
    pub node: &'static str,
}

/// Spec §Data directives: "Node words: `code` (default), `table`,
/// `table-config`, `image`, `raw`."
pub const NODE_WORDS: &[NodeWord] = &[
    NodeWord {
        word: "code",
        node: "CodeBlock",
    },
    NodeWord {
        word: "table",
        node: "Table",
    },
    NodeWord {
        word: "table-config",
        node: "TableConfig",
    },
    NodeWord {
        word: "image",
        node: "Image",
    },
    NodeWord {
        word: "raw",
        node: "RawBlock",
    },
];

/// Languages whose bare fence does not produce `code`. Spec §Data
/// directives: "a bare `mermaid` fence produces an image".
pub const LANG_DEFAULT_NODE_WORDS: &[(&str, &str)] = &[("mermaid", "image")];

pub fn node_word(word: &str) -> Option<&'static NodeWord> {
    NODE_WORDS.iter().find(|n| n.word == word)
}

/// The node word a fence with only a language produces.
pub fn default_node_word(lang: &str) -> &'static NodeWord {
    let word = LANG_DEFAULT_NODE_WORDS
        .iter()
        .find(|(l, _)| *l == lang)
        .map_or("code", |(_, w)| *w);
    node_word(word).expect("default node words are registered")
}

// ---------------------------------------------------------------------------
// Counter prefixes
// ---------------------------------------------------------------------------

/// Numbering scope of a counter. Spec §Counters (`scope`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Document,
    Chapter,
    Section,
}

/// A predeclared entry of the counter registry. Spec §Counters, Table
/// "Predeclared counter prefixes".
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Prefix {
    pub name: &'static str,
    /// The label word ("Figure"); `None` for `gls` and `doi`, which number
    /// nothing.
    pub label: Option<&'static str>,
    /// `None` for `gls` and `doi`.
    pub scope: Option<Scope>,
    /// The `ref` template with `{name}` and `{number}` fields.
    pub reference: Option<&'static str>,
    /// A heading-class prefix (`part`, `chap`, `sec`, `app`): any of them
    /// may label a heading (spec Table "Predeclared counter prefixes").
    pub heading: bool,
}

const fn mk_prefix(name: &'static str, label: &'static str, scope: Scope) -> Prefix {
    Prefix {
        name,
        label: Some(label),
        scope: Some(scope),
        reference: Some("{name} {number}"),
        heading: false,
    }
}

const fn mk_heading(name: &'static str, label: &'static str) -> Prefix {
    Prefix {
        heading: true,
        ..mk_prefix(name, label, Scope::Document)
    }
}

/// Spec §Counters.
pub const PREFIXES: &[Prefix] = &[
    mk_heading("part", "Part"),
    mk_heading("chap", "Chapter"),
    mk_heading("sec", "Section"),
    mk_heading("app", "Appendix"),
    mk_prefix("fig", "Figure", Scope::Chapter),
    mk_prefix("tbl", "Table", Scope::Chapter),
    mk_prefix("lst", "Listing", Scope::Chapter),
    mk_prefix("eq", "Equation", Scope::Chapter),
    mk_prefix("thm", "Theorem", Scope::Chapter),
    mk_prefix("note", "Note", Scope::Document),
    Prefix {
        name: "gls",
        label: None,
        scope: None,
        reference: None,
        heading: false,
    },
    Prefix {
        name: "doi",
        label: None,
        scope: None,
        reference: None,
        heading: false,
    },
];

/// Looks a predeclared prefix up, case-insensitively (spec §Counters:
/// "matched case-insensitively").
pub fn prefix(name: &str) -> Option<&'static Prefix> {
    PREFIXES.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

/// The label word of a predeclared prefix in one language: the "Name
/// (localised)" column of spec Table "Predeclared counter prefixes".
/// English is `Prefix::label`; this table carries the other languages
/// (design 06 §Site-wide resolution: the web has no babel to localise
/// `Figure 3` for it). Words follow babel's `\figurename`, `\tablename`,
/// `\chaptername`, `\partname`, `\appendixname`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrefixName {
    /// The primary language subtag, lowercase (`fr`).
    pub lang: &'static str,
    pub prefix: &'static str,
    pub name: &'static str,
}

const fn name(lang: &'static str, prefix: &'static str, name: &'static str) -> PrefixName {
    PrefixName { lang, prefix, name }
}

pub const PREFIX_NAMES: &[PrefixName] = &[
    name("fr", "part", "Partie"),
    name("fr", "chap", "Chapitre"),
    name("fr", "sec", "Section"),
    name("fr", "app", "Annexe"),
    name("fr", "fig", "Figure"),
    name("fr", "tbl", "Table"),
    name("fr", "lst", "Listing"),
    name("fr", "eq", "Équation"),
    name("fr", "thm", "Théorème"),
    name("fr", "note", "Note"),
    name("de", "part", "Teil"),
    name("de", "chap", "Kapitel"),
    name("de", "sec", "Abschnitt"),
    name("de", "app", "Anhang"),
    name("de", "fig", "Abbildung"),
    name("de", "tbl", "Tabelle"),
    name("de", "lst", "Listing"),
    name("de", "eq", "Gleichung"),
    name("de", "thm", "Satz"),
    name("de", "note", "Anmerkung"),
];

/// The label word of a predeclared prefix in `lang` (a BCP 47 tag, of
/// which the primary subtag decides: `fr-CH` is `fr`). English, an
/// unknown language and a prefix that numbers nothing give
/// `Prefix::label`; an unknown prefix gives `None`.
pub fn prefix_name(prefix_name: &str, lang: &str) -> Option<&'static str> {
    let p = prefix(prefix_name)?;
    let primary = lang.split(['-', '_']).next().unwrap_or(lang);
    PREFIX_NAMES
        .iter()
        .find(|n| n.prefix == p.name && n.lang.eq_ignore_ascii_case(primary))
        .map(|n| n.name)
        .or(p.label)
}

// ---------------------------------------------------------------------------
// Admonitions
// ---------------------------------------------------------------------------

/// A built-in admonition type. Spec §Admonition (callout).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Admonition {
    /// The type word after `:::` or `!!!`.
    pub name: &'static str,
    /// The default title word.
    pub label: &'static str,
    /// The counter prefix of a theorem-type admonition.
    pub counter: Option<&'static str>,
}

const fn mk_admonition(name: &'static str, label: &'static str) -> Admonition {
    Admonition {
        name,
        label,
        counter: None,
    }
}

const fn mk_theorem(name: &'static str, label: &'static str) -> Admonition {
    Admonition {
        name,
        label,
        counter: Some("thm"),
    }
}

/// Spec §Admonition: the built-in types and the predeclared theorem types
/// (`proof` has no counter).
pub const ADMONITIONS: &[Admonition] = &[
    mk_admonition("note", "Note"),
    mk_admonition("tip", "Tip"),
    mk_admonition("warning", "Warning"),
    mk_admonition("important", "Important"),
    mk_admonition("danger", "Danger"),
    mk_admonition("info", "Info"),
    mk_admonition("hint", "Hint"),
    mk_admonition("seealso", "See also"),
    mk_admonition("question", "Question"),
    mk_admonition("abstract", "Abstract"),
    mk_theorem("theorem", "Theorem"),
    mk_theorem("lemma", "Lemma"),
    mk_theorem("corollary", "Corollary"),
    mk_theorem("proposition", "Proposition"),
    mk_theorem("definition", "Definition"),
    mk_admonition("proof", "Proof"),
];

pub fn admonition(name: &str) -> Option<&'static Admonition> {
    ADMONITIONS.iter().find(|a| a.name == name)
}

// ---------------------------------------------------------------------------
// Containers
// ---------------------------------------------------------------------------

/// A name of the closed container registry (spec §Div): the `::: name`
/// fences that have a node or a layout meaning. Admonition types are the
/// other container names ([`ADMONITIONS`] plus `declare.admonitions`); any
/// other name is `container-unknown`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Container {
    /// The word after `:::`.
    pub name: &'static str,
    /// The block node produced (`Figure`, `Aside`, `Div`).
    pub node: &'static str,
    /// The attribute keys the container takes besides `#id`, `.class`,
    /// `lang` and `media`.
    pub keys: &'static [&'static str],
    /// The spec section.
    pub section: &'static str,
}

const fn container_row(
    name: &'static str,
    node: &'static str,
    keys: &'static [&'static str],
    section: &'static str,
) -> Container {
    Container {
        name,
        node,
        keys,
        section,
    }
}

/// Spec §Div: "The container names TMark knows form a closed registry".
/// A layout container (`multicolumn`, `landscape`, `div`, `tabs`, `tab`) renders through
/// the `tsdiv` / `#ts-div` / `<div class="name">` contract with its
/// attributes forwarded.
pub const CONTAINERS: &[Container] = &[
    container_row("figure", "Figure", &["cols"], "Image, Figure"),
    container_row("aside", "Aside", &["side"], "Aside"),
    container_row("tabs", "Div", &[], "Tabs"),
    container_row("tab", "Div", &["title"], "Tabs"),
    container_row("multicolumn", "Div", &["cols"], "Div"),
    container_row("landscape", "Div", &[], "Div"),
    container_row("div", "Div", &[], "Div"),
];

/// Looks a container name up (admonition types excluded: see
/// [`admonition`]).
pub fn container(name: &str) -> Option<&'static Container> {
    CONTAINERS.iter().find(|c| c.name == name)
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

/// A switchable behaviour. Spec §Feature registry and extensibility.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Feature {
    /// Dotted name, as written under `features:`.
    pub name: &'static str,
    pub default: bool,
    /// The spec section that defines the effect.
    pub section: &'static str,
    pub effect: &'static str,
}

const fn mk_feature(
    name: &'static str,
    default: bool,
    section: &'static str,
    effect: &'static str,
) -> Feature {
    Feature {
        name,
        default,
        section,
        effect,
    }
}

/// Spec Table "The feature registry".
pub const FEATURES: &[Feature] = &[
    mk_feature(
        "paragraph.lead",
        true,
        "Para",
        "promote a paragraph that is one short strong span to {lead}[…]",
    ),
    mk_feature(
        "table.decimal-align",
        true,
        "Table",
        "align numeric right-aligned columns on the decimal point",
    ),
    mk_feature(
        "tasklist.partial",
        false,
        "BulletList, OrderedList",
        "`- [.]` partial task items",
    ),
    mk_feature(
        "figures.exec",
        false,
        "Image, Figure",
        "execute `python image` fences",
    ),
    mk_feature(
        "glossary.wikipedia",
        false,
        "Glossary and acronyms",
        "fetch glossary summaries from Wikipedia links",
    ),
    mk_feature(
        "inline.insert",
        false,
        "Inline text",
        "`^^x^^` as {underline}[x]; off: literal text and the hint `feature-off`",
    ),
    mk_feature(
        "typography.tex-logos",
        true,
        "TeX logos",
        "set the TeX logo words (`TEX_LOGOS`) as logos in the writers",
    ),
    mk_feature(
        "citations.narrative",
        false,
        "Cite",
        "render a bare `@key` as the narrative citation (`\\textcite`, `form: \"prose\"`); `@[key]` stays parenthetical",
    ),
    mk_feature(
        "compat.pymdownx",
        true,
        "PyMdownX compatibility profile",
        "accept the PyMdownX sugar; off under strict",
    ),
];

pub fn feature(name: &str) -> Option<&'static Feature> {
    FEATURES.iter().find(|f| f.name == name)
}

// ---------------------------------------------------------------------------
// Deprecations
// ---------------------------------------------------------------------------

/// When a deprecated spelling stops being accepted. Spec Appendix
/// "Deprecation schedule".
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub enum Horizon {
    /// Removed once `tmark fmt` ships and can rewrite it.
    Fmt,
    /// Part of the compatibility promise; not scheduled for removal.
    Indefinite,
    /// Never shipped in a release; accepted for draft-2 documents only.
    NeverShipped,
}

/// One row of the deprecation table.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deprecation {
    /// Stable identifier used by parsers and lint messages.
    pub id: &'static str,
    /// The deprecated spelling, as the table writes it.
    pub spelling: &'static str,
    /// The canonical replacement.
    pub canonical: &'static str,
    /// The draft that deprecated it (`"draft 2"`, `"draft 3"`, `"none"`).
    pub status: &'static str,
    pub horizon: Horizon,
}

const fn dep(
    id: &'static str,
    spelling: &'static str,
    canonical: &'static str,
    status: &'static str,
    horizon: Horizon,
) -> Deprecation {
    Deprecation {
        id,
        spelling,
        canonical,
        status,
        horizon,
    }
}

/// Spec Table "Deprecated spellings and their horizons", in table order.
pub const DEPRECATIONS: &[Deprecation] = &[
    dep(
        "counter-brace",
        "#{prefix:key}",
        "#(prefix:key)",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "pandoc-citation",
        "[@key, locator]",
        "@[key, locator]",
        "draft 3",
        Horizon::Indefinite,
    ),
    dep(
        "footnote-citation",
        "[^key], ^[k1,k2]",
        "@key, @[k1; k2]",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "backend-role",
        "{latex}[…], {typst}[…], {html}[…]",
        "{raw latex}(…)",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "slash-raw-block",
        "/// latex … ///",
        "latex raw fence",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "render-fence",
        "latex render fence",
        "latex raw",
        "draft 3",
        Horizon::NeverShipped,
    ),
    dep(
        "slash-caption",
        "/// caption, /// figure-caption",
        "Kind: … {#id} caption line",
        "draft 2",
        Horizon::Fmt,
    ),
    dep(
        "index-colon-registry",
        "{index:registry}[…]",
        "{index registry=…}[…]",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "index-suffix",
        "{index}[…]{b} / {i}",
        "{index main=true}[…] / content markup",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "margin-role",
        "{margin}[…], {margin}[…]{l} / {r} / {o} / {i}",
        "{aside}[…], {aside side=left}[…]",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "margin-container",
        "::: margin",
        "::: aside",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "snippet",
        "--8<-- \"file\"",
        "{include}(file)",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "doi-url",
        "@https://doi.org/…",
        "@doi:…",
        "draft 3",
        Horizon::Indefinite,
    ),
    dep(
        "gls-link",
        "[](gls:term)",
        "@gls:term",
        "draft 2",
        Horizon::Fmt,
    ),
    dep(
        "bare-mermaid",
        "bare mermaid fence",
        "mermaid image",
        "draft 3",
        Horizon::Indefinite,
    ),
    dep(
        "caption-before",
        "Table: line before the table",
        "Table: line after",
        "draft 3",
        Horizon::Indefinite,
    ),
    dep(
        "bang-callout",
        "!!! / ??? callouts",
        "::: type {…}",
        "draft 2",
        Horizon::Indefinite,
    ),
    dep(
        "frontmatter-sources",
        "top-level bibliography, crossrefs",
        "sources.*",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "frontmatter-declare",
        "top-level counters, admonitions, glossary, acronyms",
        "declare.*",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "admonition-style",
        "admonitions.<type>.icon / .color",
        "press.callouts.<type>",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "callout-style",
        "press.callout_style",
        "press.callouts.style",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "no-promote-title",
        "--no-promote-title CLI flag",
        "title: null",
        "none",
        Horizon::Indefinite,
    ),
    dep(
        "attr-colon",
        "{: .cls #id} attribute list",
        "{.cls #id}",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "progress-fraction",
        "[=a/b \"…\"] progress fraction",
        "[=NN% \"…\"]",
        "draft 3",
        Horizon::Fmt,
    ),
    dep(
        "tabbed",
        "=== \"Title\" tabs",
        "::: tabs / ::: tab {title=…}",
        "draft 3",
        Horizon::Indefinite,
    ),
    dep(
        "md-in-html",
        "<div markdown>",
        "::: div",
        "draft 3",
        Horizon::Indefinite,
    ),
];

pub fn deprecation(id: &str) -> Option<&'static Deprecation> {
    DEPRECATIONS.iter().find(|d| d.id == id)
}

/// The marker of a PyMdownX snippet line (deprecation `snippet`), and the
/// rest of `line` after it.
///
/// `pymdownx.snippets` writes the marker `-{2,}8<-{2,}`: two or more
/// dashes on each side, so `--8<--`, `---8<---` and the asymmetric
/// `--8<---` are one and the same spelling. Only the two-dash form was
/// ever recognised here, and every occurrence in TeXSmith's own corpus
/// uses three.
fn snippet_marker(line: &str) -> Option<&str> {
    let dashes = |s: &str| s.len() - s.trim_start_matches('-').len();
    let open = dashes(line);
    if open < 2 {
        return None;
    }
    let rest = line[open..].strip_prefix("8<")?;
    let close = dashes(rest);
    (close >= 2).then(|| &rest[close..])
}

/// The path of a PyMdownX snippet line (spec Appendix "Deprecation
/// schedule", row `snippet`), when `line` is exactly one: the marker, then
/// the path quoted, or bare without whitespace. The bare marker alone
/// (PyMdownX's block form) has no path and is not one.
pub fn snippet_path(line: &str) -> Option<&str> {
    let rest = snippet_marker(line.trim())?.trim();
    let path = rest.trim_matches('"');
    let quoted = rest.starts_with('"') && rest.ends_with('"') && rest.len() >= 2;
    (!path.is_empty() && !path.contains('\n') && (quoted || !rest.contains(char::is_whitespace)))
        .then_some(path)
}

/// A snippet line disabled by PyMdownX's escape: one or more `;` before
/// the marker. The line includes nothing; it reaches the document as text
/// with exactly one `;` dropped, which is what this returns.
pub fn snippet_escape(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let semicolons = trimmed.len() - trimmed.trim_start_matches(';').len();
    if semicolons == 0 || snippet_path(&trimmed[semicolons..]).is_none() {
        return None;
    }
    let indent = &line[..line.len() - trimmed.len()];
    Some(format!("{indent}{}", &trimmed[1..]))
}

#[cfg(test)]
mod snippet_tests {
    use super::{snippet_escape, snippet_path};

    #[test]
    fn dash_counts() {
        for line in [
            "--8<-- \"f.py\"",
            "---8<--- \"f.py\"",
            "--8<--- \"f.py\"",
            "-----8<-- \"f.py\"",
            "  --8<--\t\"f.py\"  ",
        ] {
            assert_eq!(snippet_path(line), Some("f.py"), "{line}");
        }
        assert_eq!(snippet_path("--8<- \"f.py\""), None);
        assert_eq!(snippet_path("-8<-- \"f.py\""), None);
        assert_eq!(snippet_path("--8< \"f.py\""), None);
        // The bare marker is PyMdownX's block form, not a file include.
        assert_eq!(snippet_path("--8<--"), None);
        assert_eq!(snippet_path("---8<---"), None);
        // A bare path may not hold whitespace; a quoted one may.
        assert_eq!(snippet_path("---8<--- a b.py"), None);
        assert_eq!(snippet_path("---8<--- \"a b.py\""), Some("a b.py"));
        assert_eq!(snippet_path("---8<--- f.py"), Some("f.py"));
    }

    #[test]
    fn escapes() {
        assert_eq!(
            snippet_escape(";---8<--- \"f.py\"").as_deref(),
            Some("---8<--- \"f.py\"")
        );
        // PyMdownX drops exactly one `;` of `;*`.
        assert_eq!(
            snippet_escape(";;--8<-- \"f.py\"").as_deref(),
            Some(";--8<-- \"f.py\"")
        );
        assert_eq!(
            snippet_escape("  ;--8<-- \"f.py\"").as_deref(),
            Some("  --8<-- \"f.py\"")
        );
        // Only a snippet line is escapable: `;` before anything else is text.
        assert_eq!(snippet_escape(";[^1]: a note"), None);
        assert_eq!(snippet_escape("--8<-- \"f.py\""), None);
    }
}

// ---------------------------------------------------------------------------
// Fragment contracts
// ---------------------------------------------------------------------------

/// One row of the fragment-contract table. Design `07-writers.md`: the
/// writer names the contract in `Requires.fragments`; TeXSmith's fragment
/// defines the macros. Names are the `press.fragments` spellings.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    pub name: &'static str,
    /// Macros (`\` prefix) and environments (bare) the fragment must define.
    pub provides: &'static [&'static str],
    /// LaTeX packages the contract implies; merged into `Requires.packages`
    /// so `tlmgr` hints and `ts-extra` see them. What the fragment actually
    /// loads is its own business.
    pub packages: &'static [&'static str],
    /// The contract needs `-shell-escape` regardless of options.
    pub shell_escape: bool,
    pub description: &'static str,
}

const fn frag(
    name: &'static str,
    provides: &'static [&'static str],
    packages: &'static [&'static str],
    description: &'static str,
) -> Fragment {
    Fragment {
        name,
        provides,
        packages,
        shell_escape: false,
        description,
    }
}

/// The bundled contracts (TeXSmith `specs/migration/fragment-contracts.md`
/// §2). `shell_escape` is `false` on every bundled row: `ts-code` needs it
/// only for `minted`, which the writer decides from `code.engine`; the
/// field serves third-party contracts that always shell out.
pub const FRAGMENTS: &[Fragment] = &[
    frag(
        "ts-typesetting",
        &[
            "\\tslead",
            "\\tsmark",
            "\\tsdivider",
            "\\tsrule",
            "\\tsepigraph",
            "\\tsaside",
            "\\tsprogress",
            "\\tsicon",
            "\\tslogo",
            "tsdiv",
        ],
        &[
            "xcolor",
            "epigraph",
            "marginnote",
            "multicol",
            "pdflscape",
            "progressbar",
            "graphicx",
        ],
        "lead-ins, highlight, divider, in-container rule, epigraph, asides, progress bars, TeX logos, generic containers",
    ),
    frag(
        "ts-callouts",
        &["tscallout"],
        &["tcolorbox", "xcolor"],
        "admonitions and theorem boxes",
    ),
    frag(
        "ts-code",
        &["tscode", "\\tscodeinline"],
        &["tcolorbox", "fvextra"],
        "code listings; engine (minted/listings/verbatim/pygments) is the fragment's choice",
    ),
    frag("ts-keystrokes", &["\\tskeys"], &["tikz"], "keyboard keys"),
    frag(
        "ts-todolist",
        &["tstasklist", "\\tsdone", "\\tstodo", "\\tspartial"],
        &["enumitem", "amssymb", "pifont"],
        "task lists",
    ),
    frag(
        "ts-glossary",
        &["\\tsgls", "\\tsacr"],
        &["glossaries"],
        "glossary terms and acronyms",
    ),
    frag(
        "ts-index",
        &["\\tsindex"],
        &["imakeidx"],
        "index entries and registries",
    ),
    frag(
        "ts-bibliography",
        &["\\parencite", "\\textcite"],
        &[],
        "citation fallbacks without biblatex",
    ),
    frag(
        "ts-fonts",
        &["\\tsscript", "\\tsemoji"],
        &["fontspec"],
        "script and emoji font switches",
    ),
    frag(
        "ts-critic",
        &["\\tsins", "\\tsdel", "\\tssubst", "\\tscomment"],
        &["ulem", "xcolor"],
        "critic markup (tmark M5)",
    ),
    frag(
        "ts-equations",
        &[],
        &[],
        "Typst: an equation label was emitted, the template numbers equations (writers-and-passes.md §4)",
    ),
];

/// Looks a fragment contract up by its exact name.
pub fn fragment(name: &str) -> Option<&'static Fragment> {
    FRAGMENTS.iter().find(|f| f.name == name)
}

// ---------------------------------------------------------------------------
// TeX logos
// ---------------------------------------------------------------------------

/// The words the feature `typography.tex-logos` sets as logos (spec §TeX
/// logos): whole words, case-sensitive, never inside code, math, raw
/// passthroughs, link destinations or attribute values. No node: the
/// words stay `Str` and the writers apply the rule.
pub const TEX_LOGOS: &[&str] = &[
    "TeX", "LaTeX", "LaTeX2e", "XeTeX", "XeLaTeX", "LuaTeX", "LuaLaTeX", "pdfTeX", "pdfLaTeX",
    "BibTeX", "BibLaTeX", "ConTeXt",
];

/// Whether `word` is a TeX logo word.
pub fn tex_logo(word: &str) -> bool {
    TEX_LOGOS.contains(&word)
}

// ---------------------------------------------------------------------------
// Keystroke labels
// ---------------------------------------------------------------------------

/// The label of a keystroke name (spec §Inline text, `{keys}[ctrl+s]`).
/// Names are PyMdownX `keys` names, lowercase; labels are backend-neutral
/// text (a writer maps `↑` to `\uparrow` if it must). A name with no row
/// is labelled by its uppercase spelling, as TeXSmith's partial did.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct KeyLabel {
    pub name: &'static str,
    pub label: &'static str,
}

const fn key(name: &'static str, label: &'static str) -> KeyLabel {
    KeyLabel { name, label }
}

/// Ported from TeXSmith's `keystroke.tex` partial, plus the PyMdownX
/// aliases authors type most (`ctrl`, `cmd`, `del`, `escape`, `return`).
pub const KEY_LABELS: &[KeyLabel] = &[
    key("control", "Ctrl"),
    key("ctrl", "Ctrl"),
    key("alt", "Alt"),
    key("delete", "Del"),
    key("del", "Del"),
    key("enter", "⏎ Enter"),
    key("return", "⏎ Enter"),
    key("shift", "⇧ Shift"),
    key("slash", "/"),
    key("comma", ","),
    key("period", "."),
    key("arrow-up", "↑"),
    key("arrow-down", "↓"),
    key("arrow-left", "←"),
    key("arrow-right", "→"),
    key("backslash", "\\"),
    key("double-quote", "\""),
    key("backspace", "⌫ Delete"),
    key("command", "⌘"),
    key("cmd", "⌘"),
    key("tab", "Tab"),
    key("esc", "Esc"),
    key("escape", "Esc"),
    key("insert", "Ins"),
    key("home", "Home"),
    key("end", "End"),
    key("page-up", "PgUp"),
    key("page-down", "PgDn"),
    key("space", "Space"),
    key("f1", "F1"),
    key("f2", "F2"),
    key("f3", "F3"),
    key("f4", "F4"),
    key("f5", "F5"),
    key("f6", "F6"),
    key("f7", "F7"),
    key("f8", "F8"),
    key("f9", "F9"),
    key("f10", "F10"),
    key("f11", "F11"),
    key("f12", "F12"),
];

/// Looks a keystroke label up, case-insensitively (`Ctrl` and `ctrl` are
/// the same key).
pub fn key_label(name: &str) -> Option<&'static KeyLabel> {
    KEY_LABELS
        .iter()
        .find(|k| k.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles() {
        let code = role("code").unwrap();
        assert_eq!(code.arg, ArgStyle::Content);
        assert_eq!(code.principal, Some("lang"));
        assert_eq!(role("index").unwrap().arg, ArgStyle::ContentMany);
        assert_eq!(role("raw").unwrap().arg, ArgStyle::Argument);
        assert_eq!(role("margin").unwrap().replaced_by, Some("aside"));
        assert!(role("foo").is_none());
        assert!(role("Code").is_none(), "role names are case-sensitive");
        for r in ROLES {
            if let Some(p) = r.principal {
                assert!(r.keys.contains(&p), "{}: principal key is accepted", r.name);
            }
        }
    }

    #[test]
    fn node_words() {
        assert_eq!(node_word("table-config").unwrap().node, "TableConfig");
        assert_eq!(default_node_word("python").word, "code");
        assert_eq!(default_node_word("mermaid").word, "image");
        assert!(node_word("render").is_none());
    }

    #[test]
    fn prefixes() {
        assert_eq!(prefix("fig").unwrap().label, Some("Figure"));
        assert_eq!(prefix("Fig").unwrap().name, "fig");
        assert_eq!(prefix("gls").unwrap().scope, None);
        assert!(prefix("fw").is_none());
        assert_eq!(prefix_name("fig", "de"), Some("Abbildung"));
        assert_eq!(prefix_name("Fig", "fr-CH"), Some("Figure"));
        assert_eq!(prefix_name("tbl", "en"), Some("Table"));
        assert_eq!(prefix_name("tbl", "xx"), Some("Table"));
        assert_eq!(prefix_name("gls", "fr"), None);
        assert_eq!(prefix_name("fw", "fr"), None);
        for n in PREFIX_NAMES {
            assert!(
                prefix(n.prefix).is_some(),
                "{}: a predeclared prefix",
                n.prefix
            );
        }
        for lang in ["fr", "de"] {
            let covered = PREFIX_NAMES.iter().filter(|n| n.lang == lang).count();
            let numbered = PREFIXES.iter().filter(|p| p.label.is_some()).count();
            assert_eq!(
                covered, numbered,
                "{lang}: every numbered prefix has a word"
            );
        }
        for p in PREFIXES {
            assert!(
                role(p.name).is_none(),
                "{}: a prefix never shadows a role",
                p.name
            );
        }
    }

    #[test]
    fn admonitions_features_deprecations() {
        assert_eq!(admonition("lemma").unwrap().counter, Some("thm"));
        assert_eq!(admonition("proof").unwrap().counter, None);
        assert!(admonition("solution").is_none());
        assert!(feature("paragraph.lead").unwrap().default);
        assert!(!feature("figures.exec").unwrap().default);
        assert_eq!(FEATURES.len(), 9);
        assert!(!feature("citations.narrative").unwrap().default);
        assert!(feature("typography.tex-logos").unwrap().default);
        assert_eq!(deprecation("margin-role").unwrap().horizon, Horizon::Fmt);
        assert_eq!(deprecation("tabbed").unwrap().horizon, Horizon::Indefinite);
        assert_eq!(DEPRECATIONS.len(), 26);
        let mut ids: Vec<_> = DEPRECATIONS.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), DEPRECATIONS.len(), "deprecation ids are unique");
    }

    #[test]
    fn containers_and_logos() {
        assert_eq!(container("multicolumn").unwrap().node, "Div");
        assert_eq!(container("landscape").unwrap().node, "Div");
        assert_eq!(container("tab").unwrap().keys, &["title"]);
        assert!(
            container("note").is_none(),
            "admonition types have their own table"
        );
        assert!(container("grid").is_none());
        for c in CONTAINERS {
            assert!(
                admonition(c.name).is_none(),
                "{}: not an admonition",
                c.name
            );
            assert!(role(c.name).is_none() || c.name == "aside");
        }
        assert!(tex_logo("LaTeX") && tex_logo("ConTeXt"));
        assert!(!tex_logo("latex") && !tex_logo("Tex"));
        let mut names: Vec<&str> = CONTAINERS.iter().map(|c| c.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), CONTAINERS.len());
    }

    #[test]
    fn fragments() {
        assert_eq!(
            fragment("ts-code").unwrap().packages,
            &["tcolorbox", "fvextra"]
        );
        assert!(
            fragment("ts-extra").is_none(),
            "ts-extra is config, not a contract"
        );
        assert!(FRAGMENTS.iter().all(|f| !f.shell_escape));
        let mut provides: Vec<&str> = FRAGMENTS.iter().flat_map(|f| f.provides).copied().collect();
        let count = provides.len();
        provides.sort_unstable();
        provides.dedup();
        assert_eq!(
            provides.len(),
            count,
            "provides entries are unique across rows"
        );
        let mut names: Vec<&str> = FRAGMENTS.iter().map(|f| f.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), FRAGMENTS.len(), "fragment names are unique");
        for f in FRAGMENTS {
            assert!(
                f.name.starts_with("ts-"),
                "{}: contract names are ts-*",
                f.name
            );
            for p in f.provides {
                let bare = p.strip_prefix('\\').unwrap_or(p);
                assert!(
                    bare.chars().all(|c| c.is_ascii_lowercase()),
                    "{p}: contract macros are lowercase letters"
                );
            }
        }
    }

    #[test]
    fn key_labels() {
        assert_eq!(key_label("ctrl").unwrap().label, "Ctrl");
        assert_eq!(key_label("Control").unwrap().label, "Ctrl");
        assert_eq!(key_label("arrow-up").unwrap().label, "↑");
        assert!(
            key_label("s").is_none(),
            "unknown keys are the writer's uppercase fallback"
        );
        let mut names: Vec<&str> = KEY_LABELS.iter().map(|k| k.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KEY_LABELS.len(), "key names are unique");
        assert!(KEY_LABELS
            .iter()
            .all(|k| k.name == k.name.to_ascii_lowercase()));
    }
}
