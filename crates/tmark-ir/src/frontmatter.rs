//! The YAML front matter: raw text, the typed keys TMark reads, and
//! everything else preserved as JSON.
//!
//! Spec §Front matter. Design: `design/03-ir.md` §Front matter. Any key may
//! sit at the root or under `press`; `press` wins. Keys TMark does not own
//! (`template`, `callouts`, `code`, `slots`, `refs`, `details`, …) stay in
//! `extra` untouched.

use std::collections::BTreeMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::node::Meta;
use crate::registry::Scope;

/// The front matter of a document. `raw` is the YAML text between the
/// fences, copied byte for byte by the printer (ADR 0004).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FrontMatter {
    /// Span of the whole island, fences included; default when absent.
    #[serde(flatten)]
    pub meta: Meta,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw: String,
    #[serde(default)]
    pub keys: Keys,
    /// Root keys TMark does not own, plus `press` keys it does not own under
    /// `extra.press`. An object, empty when there is nothing.
    #[serde(default = "empty_object", skip_serializing_if = "is_empty_object")]
    pub extra: Value,
    /// Deprecated top-level spellings met while parsing (`bibliography`,
    /// `counters`, …), for the caller to report as `deprecated-frontmatter-key`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deprecated: Vec<String>,
}

fn empty_object() -> Value {
    Value::Object(Map::new())
}

fn is_empty_object(v: &Value) -> bool {
    matches!(v, Value::Object(m) if m.is_empty()) || v.is_null()
}

impl Default for FrontMatter {
    fn default() -> Self {
        FrontMatter {
            meta: Meta::default(),
            raw: String::new(),
            keys: Keys::default(),
            extra: empty_object(),
            deprecated: Vec::new(),
        }
    }
}

/// The typed subset of the front matter (spec §Front matter). Serialises to
/// the canonical layout: metadata at the root, the rest under `press`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Keys {
    /// `None` when absent (the first heading is promoted), `Some(None)` for
    /// `title: null` (spec §Header: opt out of promotion).
    #[serde(
        deserialize_with = "double_option",
        skip_serializing_if = "Option::is_none"
    )]
    #[schemars(with = "Option<String>")]
    pub title: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<Author>,
    /// ISO date, free text, or `commit`; kept as written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// Document identifier for cross-document references.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Document language (`fr`, `en-GB`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub epigraph: Option<Epigraph>,
    #[serde(skip_serializing_if = "Press::is_empty")]
    pub press: Press,
}

/// Distinguishes a missing field from an explicit `null`.
fn double_option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

/// Spec §Front matter: `authors: [{name, affiliation}]`. A bare string
/// item (`authors: [Ada Lovelace]`) is the name alone (C9: TMark tolerates
/// what TeXSmith accepts).
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct Author {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affiliation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl<'de> Deserialize<'de> for Author {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Name(String),
            Full {
                name: String,
                #[serde(default)]
                affiliation: Option<String>,
                #[serde(default)]
                email: Option<String>,
            },
        }
        Ok(match Repr::deserialize(d)? {
            Repr::Name(name) => Author {
                name,
                ..Author::default()
            },
            Repr::Full {
                name,
                affiliation,
                email,
            } => Author {
                name,
                affiliation,
                email,
            },
        })
    }
}

/// Spec §BlockQuote: `epigraph: {quote, source}`, the document's
/// epigraph as plain text. The key says what the epigraph is, never
/// where it goes: no writer reads it, and a consumer that decides where
/// an epigraph belongs splices a `> {.epigraph}` quote there itself.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Epigraph {
    pub quote: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// The `press` namespace, restricted to the keys TMark reads.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Press {
    /// What a top-level `#` maps to (`chapter`, `section`, …). Spec §Header.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_level: Option<String>,
    #[serde(skip_serializing_if = "Declare::is_empty")]
    pub declare: Declare,
    #[serde(skip_serializing_if = "Sources::is_empty")]
    pub sources: Sources,
    /// Spec §Feature registry: dotted name to switch.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, bool>,
}

impl Press {
    pub fn is_empty(&self) -> bool {
        self.base_level.is_none()
            && self.declare.is_empty()
            && self.sources.is_empty()
            && self.features.is_empty()
    }

    /// The switch `name` (spec §Feature registry): the front matter's
    /// value, else the registry default; off for a name the registry does
    /// not know.
    pub fn feature(&self, name: &str) -> bool {
        self.features
            .get(name)
            .copied()
            .unwrap_or_else(|| crate::registry::feature(name).is_some_and(|f| f.default))
    }
}

/// `declare`: what things *are*. Spec §Front matter, §Counters, §Admonition,
/// §Glossary and acronyms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Declare {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub counters: BTreeMap<String, CounterDecl>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub admonitions: BTreeMap<String, AdmonitionDecl>,
    /// Spec §Glossary and acronyms: both accepted spellings of the
    /// declaration land in the same struct.
    #[serde(skip_serializing_if = "GlossaryDecl::is_empty")]
    pub glossary: GlossaryDecl,
    /// Structure owned by TeXSmith for now; kept as JSON.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub acronyms: Value,
}

impl Declare {
    pub fn is_empty(&self) -> bool {
        self.counters.is_empty()
            && self.admonitions.is_empty()
            && self.glossary.is_empty()
            && self.acronyms.is_null()
    }
}

/// A user-declared (or overridden) counter. Spec §Counters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CounterDecl {
    /// Label word, used in references and diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Python format string over `n`, `prefix`, `key`; default `"{n}"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// Template a reference renders, with `{name}` and `{number}` fields.
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// A declared admonition type. Spec §Admonition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AdmonitionDecl {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Section title under the `reference` strategy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Text of the "See page N" link, with a `{page}` field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// Counter prefix of a theorem-type admonition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counter: Option<String>,
}

/// The glossary declaration, `declare.glossary`. Spec §Glossary and
/// acronyms: two spellings reach this one struct.
///
/// The *flat* spelling is a mapping of term to definition
/// (`api: An interface`, or `api: {name, description}`); the *structured*
/// one names its parts (`style`, `groups`, `entries`) and keeps the terms
/// under `entries`. The two may be mixed: `style`, `groups` and `entries`
/// are structural wherever they appear at the top level, every other key
/// there is a term. A term of one of those three names is therefore only
/// writable under `entries` — where it wins over a flat key of the same
/// name. `glossary: long` (TeXSmith 0.6) names a style and no term.
///
/// `style` and `groups` are not terms and never reach the glossary
/// registry: they are form, read from here by the consumer that renders
/// the per-group tables (TeXSmith's `ts-glossary` fragment).
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(default)]
pub struct GlossaryDecl {
    /// A `glossaries`-package style name (`long`, `altlist`, …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Group key to its heading; a template prints one table per group.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub groups: BTreeMap<String, GlossaryGroup>,
    /// The terms, whichever spelling declared them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub entries: BTreeMap<String, GlossaryEntry>,
}

impl GlossaryDecl {
    pub fn is_empty(&self) -> bool {
        self.style.is_none() && self.groups.is_empty() && self.entries.is_empty()
    }

    /// Reads either spelling out of a YAML value. Anything that is neither
    /// a mapping nor a style string declares nothing: the front matter of
    /// a document is never rejected over the shape of this key.
    fn from_value(value: &Value) -> Self {
        let mut out = GlossaryDecl::default();
        match value {
            Value::String(style) => out.style = non_empty(style),
            Value::Object(map) => {
                if let Some(Value::String(style)) = map.get("style") {
                    out.style = non_empty(style);
                }
                if let Some(Value::Object(groups)) = map.get("groups") {
                    for (key, value) in groups {
                        out.groups
                            .insert(key.clone(), GlossaryGroup::from_value(value));
                    }
                }
                if let Some(Value::Object(entries)) = map.get("entries") {
                    for (key, value) in entries {
                        out.entries
                            .insert(key.clone(), GlossaryEntry::from_value(value));
                    }
                }
                for (key, value) in map {
                    if GLOSSARY_STRUCTURAL_KEYS.contains(&key.as_str()) {
                        continue;
                    }
                    out.entries
                        .entry(key.clone())
                        .or_insert_with(|| GlossaryEntry::from_value(value));
                }
            }
            _ => {}
        }
        out
    }
}

/// The keys of the structured spelling; never terms at the top level.
const GLOSSARY_STRUCTURAL_KEYS: [&str; 3] = ["style", "groups", "entries"];

impl<'de> Deserialize<'de> for GlossaryDecl {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(GlossaryDecl::from_value(&Value::deserialize(d)?))
    }
}

/// A glossary group: `core: Core terms` or `core: {title: Core terms}`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
pub struct GlossaryGroup {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
}

impl GlossaryGroup {
    fn from_value(value: &Value) -> Self {
        let title = match value {
            Value::String(title) => title.clone(),
            Value::Object(map) => map
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            _ => String::new(),
        };
        GlossaryGroup { title }
    }
}

/// A glossary term. `api: An interface` sets `description`; the object
/// form takes `name` (the short form), `description`, `long` and the
/// `group` the entry belongs to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(default)]
pub struct GlossaryEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The expanded form of an acronym, when it differs from the
    /// description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long: Option<String>,
    /// Key of the `groups` entry this term is listed under.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

impl GlossaryEntry {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => GlossaryEntry::default(),
            Value::String(text) => GlossaryEntry {
                description: non_empty(text),
                ..GlossaryEntry::default()
            },
            Value::Object(map) => {
                let field = |key: &str| map.get(key).and_then(Value::as_str).and_then(non_empty);
                GlossaryEntry {
                    name: field("name"),
                    description: field("description"),
                    long: field("long"),
                    group: field("group"),
                }
            }
            other => GlossaryEntry {
                description: Some(other.to_string()),
                ..GlossaryEntry::default()
            },
        }
    }

    /// What `@gls:term` resolves to: the short `name`, else the
    /// description, else the long form.
    pub fn definition(&self) -> &str {
        self.name
            .as_deref()
            .or(self.description.as_deref())
            .or(self.long.as_deref())
            .unwrap_or_default()
    }
}

fn non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// `sources`: where references resolve. Spec §Bibliography, §Cross-document
/// references.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Sources {
    /// A map of key to DOI URL or pybtex-shaped entry, or a list of `.bib`
    /// paths; kept as JSON, `tmark-registry` interprets it.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub bibliography: Value,
    /// Alias to inventory path.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub crossrefs: BTreeMap<String, String>,
}

impl Sources {
    pub fn is_empty(&self) -> bool {
        self.bibliography.is_null() && self.crossrefs.is_empty()
    }
}

/// Why the front matter could not be read. The caller reports it as
/// `frontmatter-yaml` and keeps `raw`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontMatterError {
    pub message: String,
    /// 1-based line and column inside `raw`, when the YAML parser knows them.
    pub line: Option<u32>,
    pub col: Option<u32>,
}

impl fmt::Display for FrontMatterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.col) {
            (Some(l), Some(c)) => write!(f, "{}:{}: {}", l, c, self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for FrontMatterError {}

impl From<serde_yaml_ng::Error> for FrontMatterError {
    fn from(err: serde_yaml_ng::Error) -> Self {
        let location = err.location();
        FrontMatterError {
            message: err.to_string(),
            line: location.as_ref().map(|l| l.line() as u32),
            col: location.map(|l| l.column() as u32),
        }
    }
}

fn error(message: impl Into<String>) -> FrontMatterError {
    FrontMatterError {
        message: message.into(),
        line: None,
        col: None,
    }
}

/// Root-level metadata keys TMark owns.
const ROOT_KEYS: &[&str] = &[
    "title", "subtitle", "authors", "date", "id", "lang", "epigraph",
];
/// `press` keys TMark owns.
const PRESS_KEYS: &[&str] = &["base_level", "declare", "sources", "features"];
/// Deprecated top-level spellings and the group they moved to (Appendix
/// "Deprecation schedule").
const DEPRECATED_KEYS: &[(&str, &str)] = &[
    ("bibliography", "sources"),
    ("crossrefs", "sources"),
    ("counters", "declare"),
    ("admonitions", "declare"),
    ("glossary", "declare"),
    ("acronyms", "declare"),
];

/// Deprecated spellings of `press.callouts.style` (Appendix "Deprecation
/// schedule": `callout_style`; TeXSmith's own `admonition_style`). The key
/// is TeXSmith's, so the value moves inside `extra`.
const DEPRECATED_CALLOUT_STYLE: &[&str] = &["callout_style", "admonition_style"];

/// Every deprecated key as a dotted path, with the path it moved to: the
/// one table the diagnostic message and the fix read (`yaml_edit::move_key`).
pub fn deprecated_key_target(key: &str) -> Option<String> {
    if let Some((_, group)) = DEPRECATED_KEYS.iter().find(|(k, _)| *k == key) {
        return Some(format!("press.{group}.{key}"));
    }
    key.strip_prefix("press.")
        .filter(|k| DEPRECATED_CALLOUT_STYLE.contains(k))
        .map(|_| "press.callouts.style".to_string())
}

/// Parses the YAML text between the `---` fences.
///
/// Every owned key is looked up under `press` first, then at the root; a
/// deprecated top-level spelling fills its canonical group when that group
/// does not already set it and is recorded in `deprecated`. Unknown keys are
/// kept in `extra`.
pub fn parse(raw: &str) -> Result<FrontMatter, FrontMatterError> {
    let value: Value = serde_yaml_ng::from_str(raw)?;
    let mut root = match value {
        Value::Null => Map::new(),
        Value::Object(map) => map,
        _ => return Err(error("front matter must be a YAML mapping")),
    };
    let mut press = match root.remove("press") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map,
        Some(_) => return Err(error("`press` must be a mapping")),
    };

    let mut keys = Map::new();
    let mut press_keys = Map::new();
    for key in ROOT_KEYS {
        if let Some(v) = take(&mut press, &mut root, key) {
            keys.insert((*key).into(), v);
        }
    }
    for key in PRESS_KEYS {
        if let Some(v) = take(&mut press, &mut root, key) {
            press_keys.insert((*key).into(), v);
        }
    }

    let mut deprecated = Vec::new();
    for (key, group) in DEPRECATED_KEYS {
        let Some(v) = take(&mut press, &mut root, key) else {
            continue;
        };
        deprecated.push((*key).to_string());
        let group_map = press_keys
            .entry(*group)
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(group_map) = group_map else {
            return Err(error(format!("`{group}` must be a mapping")));
        };
        group_map.entry(*key).or_insert(v);
    }

    for key in DEPRECATED_CALLOUT_STYLE {
        let Some(v) = press.remove(*key) else {
            continue;
        };
        deprecated.push(format!("press.{key}"));
        let callouts = press
            .entry("callouts")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Value::Object(callouts) = callouts {
            callouts.entry("style").or_insert(v);
        }
    }

    if let Some(date) = keys.get_mut("date") {
        // YAML may type a date-like scalar; the spec keeps it as text.
        if let Value::Number(n) = date {
            *date = Value::String(n.to_string());
        }
    }
    keys.insert("press".into(), Value::Object(press_keys));
    let keys: Keys = serde_json::from_value(Value::Object(keys))
        .map_err(|e| error(format!("invalid front matter: {e}")))?;

    if !press.is_empty() {
        root.insert("press".into(), Value::Object(press));
    }
    Ok(FrontMatter {
        meta: Meta::default(),
        raw: raw.to_string(),
        keys,
        extra: Value::Object(root),
        deprecated,
    })
}

/// `press` wins over the root; the loser is dropped.
fn take(press: &mut Map<String, Value>, root: &mut Map<String, Value>, key: &str) -> Option<Value> {
    let from_root = root.remove(key);
    press.remove(key).or(from_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_and_press_precedence() {
        let fm = parse(
            "title: Root title\ndate: 2025-03-15\nlang: fr\npress:\n  title: Press title\n  base_level: chapter\n  features: {figures.exec: true}\n",
        )
        .unwrap();
        assert_eq!(fm.keys.title, Some(Some("Press title".into())));
        assert_eq!(fm.keys.date.as_deref(), Some("2025-03-15"));
        assert_eq!(fm.keys.lang.as_deref(), Some("fr"));
        assert_eq!(fm.keys.press.base_level.as_deref(), Some("chapter"));
        assert_eq!(fm.keys.press.features.get("figures.exec"), Some(&true));
        assert!(fm.deprecated.is_empty());
        assert!(is_empty_object(&fm.extra));
    }

    #[test]
    fn title_null_is_distinct_from_absent() {
        assert_eq!(parse("title: null\n").unwrap().keys.title, Some(None));
        assert_eq!(parse("lang: en\n").unwrap().keys.title, None);
        assert_eq!(parse("").unwrap(), FrontMatter::default());
    }

    #[test]
    fn deprecated_keys_move_to_their_group() {
        let fm = parse(
            "counters:\n  fw: {name: Finding, format: \"FW-{n:02d}\", start: 1, scope: document, ref: \"{number}\"}\nbibliography:\n  ein05: https://doi.org/10.1002/andp.19053221004\npress:\n  crossrefs: {fwrev: build/fw.refs.json}\n  sources:\n    crossrefs: {other: x.json}\n",
        )
        .unwrap();
        assert_eq!(fm.deprecated, vec!["bibliography", "crossrefs", "counters"]);
        let fw = &fm.keys.press.declare.counters["fw"];
        assert_eq!(fw.name.as_deref(), Some("Finding"));
        assert_eq!(fw.scope, Some(Scope::Document));
        assert_eq!(fw.reference.as_deref(), Some("{number}"));
        assert_eq!(
            fm.keys.press.sources.bibliography["ein05"],
            "https://doi.org/10.1002/andp.19053221004"
        );
        // The canonical `sources.crossrefs` wins over the deprecated spelling.
        assert_eq!(fm.keys.press.sources.crossrefs["other"], "x.json");
        assert!(!fm.keys.press.sources.crossrefs.contains_key("fwrev"));
    }

    #[test]
    fn string_authors_and_callout_style_spellings() {
        let fm = parse(
            "authors: [TeXSmith, {name: Ada, email: a@b.c}]\npress:\n  admonition_style: classic\n",
        )
        .unwrap();
        assert_eq!(fm.keys.authors[0].name, "TeXSmith");
        assert_eq!(fm.keys.authors[1].email.as_deref(), Some("a@b.c"));
        assert_eq!(fm.deprecated, vec!["press.admonition_style"]);
        assert_eq!(fm.extra["press"]["callouts"]["style"], "classic");
        assert!(fm.extra["press"].get("admonition_style").is_none());
        assert_eq!(
            deprecated_key_target("press.callout_style").as_deref(),
            Some("press.callouts.style")
        );
        assert_eq!(
            deprecated_key_target("counters").as_deref(),
            Some("press.declare.counters")
        );
        assert!(deprecated_key_target("title").is_none());
        // The explicit `callouts.style` wins over a deprecated spelling.
        let fm = parse("press:\n  callout_style: a\n  callouts: {style: b}\n").unwrap();
        assert_eq!(fm.extra["press"]["callouts"]["style"], "b");
    }

    #[test]
    fn glossary_reads_both_spellings() {
        // Flat: every key is a term.
        let fm = parse("press:\n  declare:\n    glossary:\n      api: An interface\n      solid: {name: SOLID, description: Five principles}\n").unwrap();
        let g = &fm.keys.press.declare.glossary;
        assert!(g.style.is_none() && g.groups.is_empty());
        assert_eq!(g.entries["api"].definition(), "An interface");
        assert_eq!(g.entries["solid"].definition(), "SOLID");

        // Structured: the terms sit under `entries`, `style` and `groups`
        // are form and never terms.
        let fm = parse("press:\n  declare:\n    glossary:\n      style: long\n      groups:\n        core: {title: Core terms}\n      entries:\n        api: {group: core, description: An interface, long: Application Programming Interface}\n").unwrap();
        let g = &fm.keys.press.declare.glossary;
        assert_eq!(g.style.as_deref(), Some("long"));
        assert_eq!(g.groups["core"].title, "Core terms");
        assert_eq!(g.entries.keys().collect::<Vec<_>>(), vec!["api"]);
        assert_eq!(g.entries["api"].group.as_deref(), Some("core"));
        assert_eq!(
            g.entries["api"].long.as_deref(),
            Some("Application Programming Interface")
        );

        // Mixed: the structural keys win over a flat key of the same name.
        let fm = parse("glossary:\n  style: long\n  entries: {api: An interface}\n  doi: A digital object identifier\n").unwrap();
        let g = &fm.keys.press.declare.glossary;
        assert_eq!(fm.deprecated, vec!["glossary"]);
        assert_eq!(g.entries["api"].definition(), "An interface");
        assert_eq!(g.entries["doi"].definition(), "A digital object identifier");

        // A bare string names a style; anything else declares nothing and
        // never fails the front matter.
        assert_eq!(
            parse("press:\n  declare:\n    glossary: altlist\n")
                .unwrap()
                .keys
                .press
                .declare
                .glossary
                .style
                .as_deref(),
            Some("altlist")
        );
        assert!(parse("press:\n  declare:\n    glossary: [a, b]\n")
            .unwrap()
            .keys
            .press
            .declare
            .is_empty());

        // Both spellings serialise to the structured one and round trip.
        let json = serde_json::to_value(&fm).unwrap();
        assert_eq!(
            json["keys"]["press"]["declare"]["glossary"]["entries"]["api"]["description"],
            "An interface"
        );
        assert_eq!(serde_json::from_value::<FrontMatter>(json).unwrap(), fm);
    }

    #[test]
    fn unknown_keys_are_preserved() {
        let fm = parse(
            "title: T\nauthors: [{name: Ada, affiliation: AE}]\ntags: [a, b]\npress:\n  template: book\n  callouts: {style: fancy}\n  declare:\n    admonitions:\n      solution: {name: Solution, group: Solutions}\n",
        )
        .unwrap();
        assert_eq!(fm.keys.authors[0].name, "Ada");
        assert_eq!(fm.extra["tags"], serde_json::json!(["a", "b"]));
        assert_eq!(fm.extra["press"]["template"], "book");
        assert_eq!(fm.extra["press"]["callouts"]["style"], "fancy");
        assert!(fm.extra.get("title").is_none());
        assert_eq!(
            fm.keys.press.declare.admonitions["solution"]
                .group
                .as_deref(),
            Some("Solutions")
        );
        // Round trip through JSON keeps everything.
        let json = serde_json::to_value(&fm).unwrap();
        assert_eq!(
            json["keys"]["press"]["declare"]["admonitions"]["solution"]["name"],
            "Solution"
        );
        let back: FrontMatter = serde_json::from_value(json).unwrap();
        assert_eq!(back, fm);
    }

    #[test]
    fn errors_carry_a_location() {
        let err = parse("title: [unclosed\n").unwrap_err();
        assert!(err.line.is_some());
        assert!(parse("- a list\n").is_err());
        assert!(parse("press: 3\n").is_err());
        assert!(parse("authors: 3\n").is_err());
    }
}
