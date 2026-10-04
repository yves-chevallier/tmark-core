//! Cross-document inventories (`refs.json`), spec §Cross-document
//! references. This crate owns the reader and the type; TeXSmith writes
//! them from the same schema.

use std::collections::BTreeMap;
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tmark_ir::{Code, Diagnostic, Document};

use crate::loader::Loader;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InventoryDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InventoryEntry {
    /// The formatted label (`FW-10`, `3.2`).
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

/// A published inventory. Design 06 §Inventory format.
///
/// `refs` is required: serde ignores unknown keys, so a defaulted map would
/// read an inventory whose entries sit under another name (TeXSmith's schema
/// 1 wrote `anchors`) as an empty one, and every citation through it would go
/// unresolved with no word on why.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Inventory {
    #[serde(default)]
    pub document: InventoryDocument,
    pub refs: BTreeMap<String, InventoryEntry>,
}

impl Inventory {
    /// What a citing document prints for `entry`: the label prefixed with the
    /// target's `document.id` (`RHE-423` + `FW-10` → `RHE-423-FW-10`), which
    /// is what keeps it unambiguous outside the document that defines it.
    pub fn qualified_label(&self, entry: &InventoryEntry) -> String {
        match self.document.id.as_deref().map(str::trim) {
            Some(id) if !id.is_empty() => format!("{id}-{}", entry.label),
            _ => entry.label.clone(),
        }
    }
}

/// Inventories by alias, as declared in `sources.crossrefs`.
#[derive(Debug, Default)]
pub struct CrossRefs {
    pub by_alias: BTreeMap<String, Inventory>,
}

pub fn load(
    doc: &Document,
    loader: &dyn Loader,
    base: &Path,
    diagnostics: &mut Vec<Diagnostic>,
) -> CrossRefs {
    let mut out = CrossRefs::default();
    let span = doc.front_matter.meta.span;
    for (alias, path) in &doc.front_matter.keys.press.sources.crossrefs {
        match loader.load(base, path) {
            Some(text) => match serde_json::from_str::<Inventory>(&text) {
                Ok(inventory) => {
                    out.by_alias.insert(alias.clone(), inventory);
                }
                Err(error) => diagnostics.push(Diagnostic::new(
                    Code::CrossrefInventoryMissing,
                    span,
                    format!("inventory `{path}` of `{alias}` is not valid: {error}"),
                )),
            },
            None => diagnostics.push(Diagnostic::new(
                Code::CrossrefInventoryMissing,
                span,
                format!("inventory `{path}` of `{alias}` not found"),
            )),
        }
    }
    out
}
