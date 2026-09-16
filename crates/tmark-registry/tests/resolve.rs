//! Resolution against the registries (design 06).
use std::path::PathBuf;

use tmark_ir::{Code, FileId};
use tmark_registry::{resolve, MemoryLoader, Resolution, ResolveOptions};
use tmark_syntax::parse;

fn codes(diags: &[tmark_ir::Diagnostic]) -> Vec<&str> {
    let mut v: Vec<&str> = diags.iter().map(|d| d.code.id()).collect();
    v.sort();
    v
}

fn run(text: &str) -> tmark_registry::Resolved {
    let doc = parse(text, FileId::default()).document;
    resolve(&doc, &MemoryLoader::new(), &ResolveOptions::default())
}

#[test]
fn labels_resolve_by_host_and_prefix() {
    let r = run("## Intro {#sec:intro}\n\nSee @sec:intro, @Sec:intro and [here](#sec:intro), not @sec:nope.\n");
    assert_eq!(codes(&r.diagnostics), vec!["ref-unresolved"]);
    assert_eq!(r.refs.len(), 4);
    assert!(
        matches!(&r.refs[0].resolution, Resolution::Label { prefix: Some(p), number: None, .. } if p == "sec")
    );
    assert!(
        matches!(&r.refs[1].resolution, Resolution::Label { .. }),
        "prefixes are case-insensitive"
    );
    assert!(
        matches!(&r.refs[2].resolution, Resolution::Label { .. }),
        "anchor links resolve too"
    );
    assert_eq!(r.refs[3].resolution, Resolution::Unresolved);
}

#[test]
fn user_counters_are_numbered_in_document_order() {
    let text = "---\npress:\n  declare:\n    counters:\n      fw: {name: Finding, format: \"FW-{n:02d}\"}\n---\n\n#(fw:a) First. #(fw:b) Second.\n\n## Loop {#fw:c}\n\nSee @fw:b and @fw:c and #(rq:x).\n";
    let r = run(text);
    assert_eq!(codes(&r.diagnostics), vec!["prefix-unknown"]);
    let fw = r.counters.get("fw").expect("declared");
    assert_eq!(fw.label("a").as_deref(), Some("FW-01"));
    assert_eq!(fw.label("c").as_deref(), Some("FW-03"));
    assert!(
        matches!(&r.refs[0].resolution, Resolution::Label { number: Some(n), .. } if n == "FW-02")
    );
}

#[test]
fn host_mismatch_and_duplicates_are_reported() {
    let r = run("![x](a.png){#tbl:one}\n\n## A {#sec:a}\n\n## B {#sec:a}\n");
    assert_eq!(
        codes(&r.diagnostics),
        vec!["label-duplicate", "prefix-host-mismatch"]
    );
}

#[test]
fn citations_glossary_doi_and_ambiguity() {
    let text = "---\npress:\n  declare:\n    glossary:\n      solid: Five principles\n  sources:\n    bibliography:\n      ein05: https://doi.org/10.1002/andp.19053221004\n      stock: {type: misc, title: Stock}\n---\n\nSee @ein05, @gls:solid, @gls:nope, @doi:10.1/x and [span]{#stock} @stock.\n";
    let r = run(text);
    assert_eq!(
        codes(&r.diagnostics),
        vec!["ref-ambiguous", "ref-unresolved"]
    );
    assert!(matches!(&r.refs[0].resolution, Resolution::Citation { key } if key == "ein05"));
    assert!(matches!(&r.refs[1].resolution, Resolution::Glossary { term } if term == "solid"));
    assert_eq!(r.refs[2].resolution, Resolution::Unresolved);
    assert!(matches!(&r.refs[3].resolution, Resolution::Doi { doi } if doi == "10.1/x"));
    assert_eq!(r.refs[4].resolution, Resolution::Ambiguous);
}

#[test]
fn glossary_declaration_accepts_both_spellings() {
    // Flat: every key is a term, the definition a string or an object.
    let flat = "---\npress:\n  declare:\n    glossary:\n      api: An interface\n      solid: {name: SOLID, description: Five principles}\n---\n\nSee @gls:api, @gls:solid and @gls:nope.\n";
    let r = run(flat);
    assert_eq!(codes(&r.diagnostics), vec!["ref-unresolved"]);
    assert!(matches!(&r.refs[0].resolution, Resolution::Glossary { term } if term == "api"));
    assert!(matches!(&r.refs[1].resolution, Resolution::Glossary { term } if term == "solid"));
    assert_eq!(r.refs[2].resolution, Resolution::Unresolved);
    assert_eq!(r.glossary["api"], "An interface");
    assert_eq!(r.glossary["solid"], "SOLID");

    // Structured: the terms live under `entries`, and `style` and `groups`
    // are form rather than terms.
    let structured = "---\npress:\n  declare:\n    glossary:\n      style: long\n      groups:\n        core: Core terms\n      entries:\n        api:\n          group: core\n          description: An interface\n---\n\nSee @gls:api, @gls:style and @gls:groups.\n";
    let r = run(structured);
    assert_eq!(
        codes(&r.diagnostics),
        vec!["ref-unresolved", "ref-unresolved"]
    );
    assert!(matches!(&r.refs[0].resolution, Resolution::Glossary { term } if term == "api"));
    assert_eq!(r.refs[1].resolution, Resolution::Unresolved);
    assert_eq!(r.refs[2].resolution, Resolution::Unresolved);
    assert_eq!(r.glossary.keys().collect::<Vec<_>>(), vec!["api"]);

    // The style and the groups stay in the front matter for the consumer
    // that renders the per-group tables.
    let doc = parse(structured, FileId::default()).document;
    let glossary = &doc.front_matter.keys.press.declare.glossary;
    assert_eq!(glossary.style.as_deref(), Some("long"));
    assert_eq!(glossary.groups["core"].title, "Core terms");
    assert_eq!(glossary.entries["api"].group.as_deref(), Some("core"));

    // Mixed: a flat term beside the structured keys, and a flat key named
    // like a structural one, which stays structural.
    let mixed = "---\npress:\n  declare:\n    glossary:\n      style: long\n      entries:\n        api: An interface\n      doi: A digital object identifier\n---\n\nSee @gls:api and @gls:doi.\n";
    let r = run(mixed);
    assert!(r.diagnostics.is_empty());
    assert_eq!(r.glossary["doi"], "A digital object identifier");
    assert_eq!(r.glossary["api"], "An interface");
}

#[test]
fn includes_and_inventories_load_through_the_loader() {
    let main = "---\npress:\n  sources:\n    crossrefs: {fwrev: build/fw.refs.json, gone: nope.json}\n---\n\n{include}(chapters/boot.md)\n\n{include}(chapters/missing.md)\n\nSee @sec:boot, @fwrev:fw:x and @fwrev:fw:y.\n";
    let loader = MemoryLoader::new()
        .with(
            "docs/chapters/boot.md",
            "## Boot {#sec:boot}\n\n{include}(../main.md)\n",
        )
        .with(
            "docs/build/fw.refs.json",
            r#"{"document": {"id": "RHE"}, "refs": {"fw:x": {"label": "FW-10", "page": 14}}}"#,
        );
    let doc = parse(main, FileId::default()).document;
    let options = ResolveOptions {
        path: PathBuf::from("docs/main.md"),
        ..Default::default()
    };
    let r = resolve(&doc, &loader, &options);
    assert_eq!(
        codes(&r.diagnostics),
        vec![
            "crossref-inventory-missing",
            "include-missing",
            "ref-unresolved"
        ]
    );
    assert_eq!(
        r.files.len(),
        1,
        "the include is parsed once despite the cycle"
    );
    assert!(
        matches!(&r.refs[0].resolution, Resolution::Label { .. }),
        "labels of includes count"
    );
    assert!(
        matches!(&r.refs[1].resolution, Resolution::External { label, page: Some(14), .. } if label == "FW-10")
    );
    assert_eq!(r.refs[2].resolution, Resolution::Unresolved);
    assert!(r.diagnostics.iter().all(|d| d.code != Code::LabelDuplicate));
}

#[test]
fn reference_style_links_resolve_against_the_labels_alone() {
    // Spec §Ref: `[text][id]` is a textual reference when `id` is a label
    // of the document or of the book, the literal brackets otherwise, and
    // it never reaches the bibliography or the glossary.
    let doc = parse(
        "[]{#claim}\n\nSee [the claim][claim], [the finding][fw:boot], [a review][knuth:1984] and [none][nope].\n",
        FileId::default(),
    )
    .document;
    let options = ResolveOptions {
        book: vec![tmark_registry::BookLabel {
            key: "fw:boot".to_string(),
            prefix: Some("fw".to_string()),
            number: Some("FW-10".to_string()),
            kind: tmark_registry::Host::Header,
            title: Some("Boot loop".to_string()),
            location: "findings.md#fw:boot".to_string(),
        }],
        ..ResolveOptions::default()
    };
    let r = resolve(&doc, &MemoryLoader::new(), &options);
    assert_eq!(r.refs.len(), 4);
    assert!(matches!(&r.refs[0].resolution, Resolution::Label { .. }));
    assert!(
        matches!(&r.refs[1].resolution, Resolution::Sibling { label, .. } if label == "FW-10"),
        "a label of the book stands in for a missing local one"
    );
    assert_eq!(
        r.refs[2].resolution,
        Resolution::Unresolved,
        "a bibliography key is not a label"
    );
    assert_eq!(r.refs[3].resolution, Resolution::Unresolved);
    // The spelling is a compatibility form: where it refers, it is
    // deprecated with the canonical `[text](#id)` as its fix; where it
    // names no label it is CommonMark's literal text and says nothing,
    // `deprecated` included.
    assert_eq!(codes(&r.diagnostics), ["deprecated", "deprecated"]);
    let fixes: Vec<&str> = r
        .diagnostics
        .iter()
        .filter_map(|d| d.fix.as_ref())
        .map(|f| f.replacement.as_str())
        .collect();
    assert_eq!(fixes, ["[the claim](#claim)", "[the finding](#fw:boot)"]);
}
