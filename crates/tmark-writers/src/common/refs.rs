//! Reference text from a `Resolved` (design 07 §Mapping rules,
//! "References"): the counter's `ref` template, `[?key]` for an unresolved
//! key, the textual templates of `WriterOptions.refs`.

use tmark_ir::NodeId;
use tmark_registry::{RefResolution, Resolution, Resolved};

/// The resolution recorded for `key` on `node`, if any.
pub fn lookup<'a>(res: &'a Resolved, node: NodeId, key: &str) -> Option<&'a RefResolution> {
    res.refs.iter().find(|r| r.node == node && r.key == key)
}

/// Whether the reference-style link `[text][key]` on `node` refers (spec
/// §Ref): it does when `key` is a label of this document or of the book,
/// and not otherwise — a document written without a resolution included,
/// where the node keeps the literal brackets CommonMark gives it.
pub fn refers(res: &Resolved, node: NodeId, key: &str) -> bool {
    matches!(
        lookup(res, node, key).map(|r| &r.resolution),
        Some(Resolution::Label { .. } | Resolution::Sibling { .. })
    )
}

/// What an unresolved key renders as, in every backend (spec §Ref).
pub fn unresolved(key: &str) -> String {
    format!("[?{key}]")
}

/// The text a numeric reference to `key` shows when its anchor has no
/// number (spec §Anchor, `ref-unnumbered`): the anchor's text or title,
/// else its id. `None` for a numbered label.
pub fn unnumbered_text(res: &Resolved, key: &str) -> Option<String> {
    let label = res.labels.get(key).filter(|l| res.unnumbered(l))?;
    Some(
        label
            .title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .unwrap_or(&label.id)
            .to_string(),
    )
}

/// Renders `{name}` and `{number}` in a counter's `ref` template.
pub fn template(template: &str, name: &str, number: &str) -> String {
    template.replace("{name}", name).replace("{number}", number)
}

/// Renders a textual-reference template: `{text}`, `{number}`, `{page}`.
pub fn textual(template: &str, text: &str, number: &str, page: &str) -> String {
    template
        .replace("{text}", text)
        .replace("{number}", number)
        .replace("{page}", page)
}

/// The language label words are rendered in: `WriterOptions.lang`, else
/// the resolution's — itself `ResolveOptions.lang` else the front
/// matter's `lang`, so a site-wide language keeps beating a page's — else
/// the front matter's, for a document written without resolving one, else
/// English (`None`).
pub fn language(option: Option<&str>, doc: &tmark_ir::Document, res: &Resolved) -> Option<String> {
    option
        .map(str::to_string)
        .or_else(|| res.lang.clone())
        .or_else(|| doc.front_matter.keys.lang.clone())
}

/// Whether a bare `@key` is the narrative citation (spec §Cite, C51):
/// `WriterOptions::citations.narrative`, else the front matter's
/// `citations.narrative` feature, else the registry default (off) — the
/// precedence of [`language`], minus the resolution, which does not read
/// the switch.
pub fn narrative(option: Option<bool>, doc: &tmark_ir::Document) -> bool {
    option.unwrap_or_else(|| doc.front_matter.keys.press.feature("citations.narrative"))
}

/// The label word of a series as the reference wants it: the counter's
/// own `name`, re-localised into `lang` when the resolution had left it at
/// the registry's word for the resolution's own language
/// (`tmark_ir::registry::PREFIX_NAMES`, predeclared prefixes only). A
/// `declare.counters` `name` is therefore kept as written, even on a
/// predeclared prefix and even when the writer renders another language.
/// A capitalised prefix (`@Fig:x`) capitalises the word (spec §Ref,
/// pandoc-crossref).
pub fn label_word(
    res: &Resolved,
    prefix: &str,
    key_as_written: &str,
    lang: Option<&str>,
) -> String {
    let declared = res.counters.get(prefix).and_then(|c| c.name.clone());
    // What the resolution would have put there on its own; `None` for
    // `res.lang` is English, which is the table's fallback too.
    let default = tmark_ir::registry::prefix_name(prefix, res.lang.as_deref().unwrap_or("en"));
    let name = if declared.is_some() && declared.as_deref() != default {
        declared
    } else {
        lang.and_then(|lang| tmark_ir::registry::prefix_name(prefix, lang))
            .map(str::to_string)
            .or(declared)
    };
    let name = name.unwrap_or_default();
    if key_as_written
        .chars()
        .next()
        .is_some_and(char::is_uppercase)
    {
        let mut chars = name.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().chain(chars).collect(),
            None => name,
        }
    } else {
        name
    }
}

/// The word of a caption kind (`Table`, `Tableau`, `Tabelle`) in `lang`.
pub fn caption_word(kind: tmark_ir::CaptionKind, lang: Option<&str>) -> String {
    lang.and_then(|lang| tmark_ir::registry::prefix_name(kind.prefix(), lang))
        .unwrap_or(kind.word())
        .to_string()
}

/// The `ref` template of a series (`{name} {number}` by default).
pub fn reference_template(res: &Resolved, prefix: &str) -> String {
    res.counters
        .get(prefix)
        .map(|c| c.reference.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "{name} {number}".to_string())
}

/// The formatted number of `key` in the series `prefix`, when TMark
/// numbers it.
pub fn number(res: &Resolved, prefix: &str, key: &str) -> Option<String> {
    res.counters.get(prefix).and_then(|c| c.label(key))
}

/// The minimal built-in citation style (design 07 §Mapping rules):
/// `Author Year`, the year alone for `-@key`, the key when the
/// bibliography has no record.
pub fn author_year(res: &Resolved, key: &str, suppress_author: bool) -> String {
    let Some(entry) = res.bibliography.get(key) else {
        return key.to_string();
    };
    let year = entry
        .fields
        .get("year")
        .or_else(|| entry.fields.get("date"))
        .map(|d| d.chars().take(4).collect::<String>());
    let author = entry.fields.get("author").map(|a| {
        let first = a.split(" and ").next().unwrap_or(a);
        match first.split_once(',') {
            Some((last, _)) => last.trim().to_string(),
            None => first.rsplit(' ').next().unwrap_or(first).to_string(),
        }
    });
    match (author, year, suppress_author) {
        (_, Some(year), true) => year,
        (Some(author), Some(year), false) => format!("{author} {year}"),
        (Some(author), None, false) => author,
        (None, Some(year), _) => year,
        _ => key.to_string(),
    }
}

/// The series numbers labels itself (declared counters); `false` for the
/// predeclared backend-numbered ones.
pub fn tmark_numbered(res: &Resolved, prefix: &str) -> bool {
    res.counters.get(prefix).is_some_and(|c| c.tmark_numbered)
}

/// The implicit id of a heading (spec §Header) when a reference of the
/// document targets it, else `None`: the paged writers write a `\label`
/// for an implicit id only when something points at it. An explicit id is
/// the node's own attribute and is not looked up here.
pub fn referenced_implicit_id(res: &Resolved, node: NodeId) -> Option<&str> {
    let label = res
        .labels
        .in_order
        .iter()
        .find(|l| l.node == node && l.implicit)?;
    res.refs
        .iter()
        .any(|r| matches!(&r.resolution, tmark_registry::Resolution::Label { target, .. } if *target == node))
        .then_some(label.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates() {
        assert_eq!(template("{name} {number}", "Figure", "3"), "Figure 3");
        assert_eq!(template("{number}", "Finding", "FW-01"), "FW-01");
        assert_eq!(
            textual("{text} (p. {page})", "the trace", "3", "12"),
            "the trace (p. 12)"
        );
        assert_eq!(unresolved("sec:nope"), "[?sec:nope]");
    }
}
