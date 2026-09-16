//! `[text](#id)`, the canonical textual reference (spec §Ref), in the
//! print writers and the HTML writer, with and without a `book` map.
//!
//! A paged build has every page of the book in one document, so a label
//! that the resolution places on a sibling page is still a label LaTeX and
//! Typst will see: both name it (`\hyperref[id]{text}`, `#link(<id>)[text]`)
//! whatever the resolution says, which is why the writers read the target
//! and not the resolution here. The web lowering is the one backend that
//! has to splice the sibling's page (`tests/web.rs`).

use tmark_registry::{resolve, BookLabel, Host, MemoryLoader, ResolveOptions};
use tmark_writers::{write, Backend, Media, WriterOptions};

const DOC: &str = "# Intro {#sec:intro}\n\nSee [here](#sec:intro) and [there](#sec:other).\n";

fn body(md: &str, backend: Backend, book: Vec<BookLabel>) -> String {
    let parsed = tmark_syntax::parse(md, tmark_ir::FileId::default());
    let resolved = resolve(
        &parsed.document,
        &MemoryLoader::new(),
        &ResolveOptions {
            book,
            ..ResolveOptions::default()
        },
    );
    let opts = WriterOptions {
        media: match backend {
            Backend::Html => Media::Web,
            _ => Media::Print,
        },
        ..WriterOptions::default()
    };
    write(&parsed.document, &resolved, backend, &opts).text
}

fn sibling() -> Vec<BookLabel> {
    vec![BookLabel {
        key: "sec:other".into(),
        prefix: Some("sec".into()),
        number: None,
        kind: Host::Header,
        title: Some("The other page".into()),
        location: "other.md#sec:other".into(),
    }]
}

#[test]
fn latex_names_the_label_of_a_same_page_and_of_a_sibling_anchor() {
    let alone = body(DOC, Backend::Latex, Vec::new());
    assert!(
        alone.contains("\\hyperref[sec:intro]{here} and \\hyperref[sec:other]{there}"),
        "{alone}"
    );
    assert_eq!(alone, body(DOC, Backend::Latex, sibling()));
}

#[test]
fn typst_names_the_label_of_a_same_page_and_of_a_sibling_anchor() {
    let alone = body(DOC, Backend::Typst, Vec::new());
    assert!(
        alone.contains("#link(<sec:intro>)[here] and #link(<sec:other>)[there]"),
        "{alone}"
    );
    assert_eq!(alone, body(DOC, Backend::Typst, sibling()));
}

#[test]
fn html_addresses_the_rendered_page() {
    let html = body(DOC, Backend::Html, sibling());
    assert!(
        html.contains("<a href=\"#sec:intro\" class=\"reference\">here</a>"),
        "{html}"
    );
    assert!(
        html.contains("<a href=\"#sec:other\" class=\"reference\">there</a>"),
        "{html}"
    );
}

/// The deprecated `[text][id]` reaches the same place as `[text](#id)`
/// where it refers, and stays CommonMark's literal text where it does not.
#[test]
fn the_reference_style_spelling_refers_where_the_key_is_a_label() {
    let md =
        "# Intro {#sec:intro}\n\nSee [here][sec:intro], [there][sec:other] and [prose][nothing].\n";
    let latex = body(md, Backend::Latex, sibling());
    assert!(
        latex.contains(
            "\\hyperref[sec:intro]{here}, \\hyperref[sec:other]{there} and [prose][nothing]"
        ),
        "{latex}"
    );
    let typst = body(md, Backend::Typst, sibling());
    assert!(
        typst.contains(
            "#link(<sec:intro>)[here], #link(<sec:other>)[there] and \\[prose\\]\\[nothing\\]"
        ),
        "{typst}"
    );
}
