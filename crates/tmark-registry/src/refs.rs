//! Step 5: what every reference refers to (spec §Registries (lookup),
//! §Ref, §Cite, §Glossary reference, §Cross-document references).

use schemars::JsonSchema;
use serde::Serialize;
use tmark_ir::{walk, Code, Diagnostic, Inline, NodeId, NodeRef, Span, Target};

use crate::Resolved;

/// What a key refers to. Serialises with a `kind` tag (`label`, `sibling`,
/// `citation`, `glossary`, `doi`, `external`, `ambiguous`, `unresolved`).
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resolution {
    /// A label or counter item of this document (or an include).
    Label {
        target: NodeId,
        prefix: Option<String>,
        /// Formatted number, for tmark-numbered series.
        number: Option<String>,
    },
    /// A label of another document of the book (`ResolveOptions::book`);
    /// a local definition always wins over it.
    Sibling {
        /// What to show: the formatted number, else the title, else the
        /// key as written in the sibling.
        label: String,
        /// The sibling's `BookLabel::location` (`findings.md#fw:boot`).
        location: String,
    },
    Citation {
        key: String,
    },
    Glossary {
        term: String,
    },
    Doi {
        doi: String,
    },
    External {
        alias: String,
        label: String,
        page: Option<u32>,
    },
    /// A key present in two registries (`ref-ambiguous`).
    Ambiguous,
    Unresolved,
}

#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
pub struct RefResolution {
    pub node: NodeId,
    /// The key token (the whole node for an anchor link, or when the item
    /// came from JSON without a key span).
    pub span: Span,
    /// The key as written.
    pub key: String,
    pub resolution: Resolution,
}

pub fn resolve_all(doc: &tmark_ir::Document, resolved: &mut Resolved) {
    // (node, key span, node span, key, numeric: `@key` or `[](#key)`,
    // which shows a number, as opposed to `[text](#key)`; labels_only:
    // the reference-style `[text][id]`, which is a reference only when it
    // names a label and stays literal text otherwise)
    let mut found: Vec<(NodeId, Span, Span, String, bool, bool)> = Vec::new();
    walk(doc, &mut |node: NodeRef| {
        if let NodeRef::Inline(inline) = node {
            match inline {
                Inline::Ref(r) => {
                    for item in &r.items {
                        let span = if item.key_span.0.is_empty() {
                            r.meta.span
                        } else {
                            item.key_span.0
                        };
                        found.push((r.meta.id, span, r.meta.span, item.key.clone(), true, false));
                    }
                }
                Inline::Link(l) => match &l.target {
                    Target::Anchor(id) => found.push((
                        l.meta.id,
                        l.meta.span,
                        l.meta.span,
                        id.clone(),
                        l.content.is_empty(),
                        false,
                    )),
                    Target::Reference(id) => {
                        found.push((l.meta.id, l.meta.span, l.meta.span, id.clone(), false, true))
                    }
                    Target::Url(_) | Target::Document(_) => {}
                },
                _ => {}
            }
        }
    });
    for (node, span, node_span, key, numeric, labels_only) in found {
        let resolution = if labels_only {
            resolve_label(&key, resolved)
        } else {
            resolve_one(&key, span, resolved)
        };
        // Spec §Ref: a reference-style link that names no label is the
        // literal text CommonMark makes of it, and says nothing.
        if resolution == Resolution::Unresolved && !labels_only {
            resolved.diagnostics.push(Diagnostic::new(
                Code::RefUnresolved,
                span,
                format!("`@{key}` does not resolve to a label, a citation key, a glossary term or an inventory"),
            ));
        }
        if let Resolution::Label { target, .. } = &resolution {
            let label = resolved
                .labels
                .get(&key)
                .filter(|l| l.node == *target)
                .cloned();
            // Spec §Header: an implicit id changes with the title; suggest
            // an explicit one.
            if label.as_ref().is_some_and(|l| l.implicit) {
                resolved.diagnostics.push(Diagnostic::new(
                    Code::RefImplicitId,
                    node_span,
                    format!("`{key}` is the heading's implicit id, which changes with its title; give the heading `{{#{key}}}`"),
                ));
            }
            // Spec §Anchor: a number is asked of a host that has none.
            if numeric && label.as_ref().is_some_and(|l| resolved.unnumbered(l)) {
                resolved.diagnostics.push(Diagnostic::new(
                    Code::RefUnnumbered,
                    node_span,
                    format!("`{key}` is an anchor with no number; the reference shows its text. Write `[text](#{key})` for a textual reference"),
                ));
            }
        }
        resolved.refs.push(RefResolution {
            node,
            span,
            key,
            resolution,
        });
    }
}

/// The labels alone, the document's own before the book's (spec §Ref,
/// "reference-style form"): `[text][id]` is a reference only where `id`
/// is a label, so it never reaches the bibliography, the glossary or an
/// inventory, and a key it does not find says nothing — the node keeps
/// the meaning CommonMark gives it.
fn resolve_label(key: &str, resolved: &Resolved) -> Resolution {
    let lower = key.to_ascii_lowercase();
    let head = lower.split_once(':').map(|(h, _)| h);
    let local = match head {
        Some(prefix) if resolved.counters.is_declared(prefix) => resolved.labels.get(&lower),
        Some(_) => None,
        None => resolved.labels.get(&lower),
    };
    match local {
        Some(label) => Resolution::Label {
            target: label.node,
            prefix: label.prefix.clone(),
            number: resolved.formatted(label),
        },
        None => match resolved.sibling(&lower) {
            Some(sibling) => Resolution::Sibling {
                label: sibling
                    .number
                    .clone()
                    .or_else(|| sibling.title.clone())
                    .unwrap_or_else(|| sibling.key.clone()),
                location: sibling.location.clone(),
            },
            None => Resolution::Unresolved,
        },
    }
}

/// Resolution order (spec §Registries): declared prefix (`doi` and `gls`
/// included), then bibliography; a key in two registries is ambiguous.
/// A sibling document's label (design 06 §Site-wide resolution) stands in
/// for a missing local one: it never shadows a local label and is as
/// ambiguous with a bibliography key as a local label would be.
fn resolve_one(key: &str, span: Span, resolved: &mut Resolved) -> Resolution {
    let lower = key.to_ascii_lowercase();
    // Cross-document: `alias:prefix:key` with a declared alias.
    if let Some((alias, rest)) = lower.split_once(':') {
        if let Some(inventory) = resolved.crossrefs.by_alias.get(alias) {
            return match inventory.refs.get(rest) {
                Some(entry) => Resolution::External {
                    alias: alias.to_string(),
                    label: entry.label.clone(),
                    page: entry.page,
                },
                None => Resolution::Unresolved,
            };
        }
    }
    if let Some(doi) = lower.strip_prefix("doi:") {
        return Resolution::Doi {
            doi: key[key.len() - doi.len()..].to_string(),
        };
    }
    if let Some(term) = lower.strip_prefix("gls:") {
        return if resolved.glossary.contains_key(term) {
            Resolution::Glossary {
                term: term.to_string(),
            }
        } else {
            Resolution::Unresolved
        };
    }
    let head = lower.split_once(':').map(|(h, _)| h);
    let label = match head {
        Some(prefix) if resolved.counters.is_declared(prefix) => resolved.labels.get(&lower),
        Some(_) => None,
        None => resolved.labels.get(&lower),
    };
    let citation = resolved.bibliography.contains(key);
    match (label, citation) {
        (Some(label), true) => {
            let span = label.span;
            resolved.diagnostics.push(Diagnostic::new(
                Code::RefAmbiguous,
                span,
                format!("`{key}` is both a label and a bibliography key"),
            ));
            Resolution::Ambiguous
        }
        (Some(label), false) => Resolution::Label {
            target: label.node,
            prefix: label.prefix.clone(),
            number: resolved.formatted(label),
        },
        (None, citation) => match (resolved.sibling(&lower), citation) {
            (Some(sibling), false) => Resolution::Sibling {
                label: sibling
                    .number
                    .clone()
                    .or_else(|| sibling.title.clone())
                    .unwrap_or_else(|| sibling.key.clone()),
                location: sibling.location.clone(),
            },
            (Some(_), true) => {
                resolved.diagnostics.push(Diagnostic::new(
                    Code::RefAmbiguous,
                    span,
                    format!("`{key}` is both a label of another document and a bibliography key"),
                ));
                Resolution::Ambiguous
            }
            (None, true) => Resolution::Citation {
                key: key.to_string(),
            },
            (None, false) => Resolution::Unresolved,
        },
    }
}
