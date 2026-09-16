//! Smoke test: the extension's sample and the spec parse without panicking,
//! and a few constructs land in the expected nodes.
use tmark_ir::{Block, FileId, Inline};
use tmark_syntax::parse;

fn blocks(text: &str) -> Vec<Block> {
    parse(text, FileId::default()).document.blocks
}

#[test]
fn sample_parses() {
    let sample = include_str!("../../../editors/vscode/test/sample.md");
    let parsed = parse(sample, FileId::default());
    assert!(!parsed.document.blocks.is_empty());
    let json = serde_json::to_string_pretty(&parsed.document).unwrap();
    assert!(json.contains("\"type\": \"Aside\""));
    for d in &parsed.diagnostics {
        eprintln!("{}: {} @ {:?}", d.code.id(), d.message, d.span);
    }
}

#[test]
fn spec_parses() {
    let spec = include_str!("../../../spec/tmark.md");
    let parsed = parse(spec, FileId::default());
    assert!(parsed.document.blocks.len() > 50);
}

#[test]
fn roles_and_attrs() {
    let blocks = blocks("## Title {#sec:a .x k=v}\n\nSee @sec:a and {aside side=left}[x] here.\n");
    let Block::Header(h) = &blocks[0] else {
        panic!("{blocks:?}")
    };
    assert_eq!(h.attrs.id.as_deref(), Some("sec:a"));
    assert_eq!(h.attrs.classes, vec!["x"]);
    assert_eq!(tmark_syntax_test_helpers::plain(&h.content), "Title");
    let Block::Para(p) = &blocks[1] else {
        panic!("{blocks:?}")
    };
    assert!(matches!(&p.content[1], Inline::Ref(r) if r.items[0].key == "sec:a"));
    assert!(matches!(&p.content[3], Inline::Aside(a) if a.side == Some(tmark_ir::Side::Left)));
}

#[test]
fn containers_and_captions() {
    let blocks = blocks("::: warning {title=\"T\"}\nBody @xy.\n:::\n\n| a |\n| - |\n| 1 |\n\nTable: Cap. {#tbl:x}\n");
    let Block::Admonition(a) = &blocks[0] else {
        panic!("{blocks:?}")
    };
    assert_eq!(a.kind, "warning");
    assert_eq!(
        a.title
            .as_ref()
            .map(|t| tmark_syntax_test_helpers::plain(t))
            .as_deref(),
        Some("T")
    );
    assert!(matches!(&a.content[0], Block::Para(p) if matches!(&p.content[1], Inline::Ref(_))));
    assert!(matches!(&blocks[1], Block::Table(_)));
    let Block::Caption(c) = &blocks[2] else {
        panic!("{blocks:?}")
    };
    assert_eq!(c.attrs.id.as_deref(), Some("tbl:x"));
    assert_eq!(tmark_syntax_test_helpers::plain(&c.content), "Cap.");
}

mod tmark_syntax_test_helpers {
    use tmark_ir::Inline;
    pub fn plain(inlines: &[Inline]) -> String {
        inlines
            .iter()
            .map(|i| match i {
                Inline::Str(s) => s.text.clone(),
                Inline::SoftBreak(_) => " ".into(),
                _ => String::new(),
            })
            .collect()
    }
}

#[test]
fn compat_spellings() {
    let blocks = blocks(
        "Pandoc [see @ein05, p. 33; -@AI2027].\n\n\\[\nx^2\n\\]\n\n--8<-- \"snippets/file.md\"\n",
    );
    let Block::Para(p) = &blocks[0] else {
        panic!("{blocks:?}")
    };
    let Inline::Ref(r) = &p.content[1] else {
        panic!("{:?}", p.content)
    };
    assert!(r.bracketed);
    assert_eq!(r.items[0].prefix.as_deref(), Some("see"));
    assert_eq!(r.items[0].key, "ein05");
    assert!(r.items[1].suppress_author);
    let Block::MathBlock(m) = &blocks[1] else {
        panic!("{blocks:?}")
    };
    assert_eq!(m.text, "x^2");
    let Block::Include(i) = &blocks[2] else {
        panic!("{blocks:?}")
    };
    assert_eq!(i.path, "snippets/file.md");
}

/// Decision X7: the deprecated citation forms, and what stays literal.
#[test]
fn deprecated_citations() {
    let parsed = parse(
        "See [^1] and [^ein05] and ^[ab,cd] but ^[not keys] and x^2^ here.\n",
        FileId::default(),
    );
    let Block::Para(p) = &parsed.document.blocks[0] else {
        panic!("{:?}", parsed.document.blocks)
    };
    let kinds: Vec<&str> = p
        .content
        .iter()
        .map(|i| match i {
            Inline::Str(s) => s.text.as_str(),
            Inline::Ref(r) => {
                // The sugars were the short citation, hence bare (C51).
                assert!(!r.bracketed);
                "Ref"
            }
            Inline::Superscript(_) => "Sup",
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "See [^1] and ",
            "Ref",
            " and ",
            "Ref",
            " but ^[not keys] and x",
            "Sup",
            " here."
        ]
    );
    assert_eq!(
        parsed
            .diagnostics
            .iter()
            .filter(|d| d.code == tmark_ir::Code::Deprecated)
            .count(),
        2
    );
}

#[test]
fn table_header_keeps_its_markup() {
    // Spec §Table: inline Markdown survives in every cell, the header
    // included; `name` stays the plain text (the named-row key).
    let pipe = blocks("| **Bold** | [L](https://e.org) |\n| - | - |\n| a | b |\n");
    let Block::Table(t) = &pipe[0] else {
        panic!("{pipe:?}")
    };
    let names: Vec<Option<&str>> = t.model.columns.iter().map(|c| c.name()).collect();
    assert_eq!(names, [Some("Bold"), Some("L")]);
    assert!(matches!(t.model.columns[0].title(), [Inline::Strong(_)]));
    assert!(matches!(t.model.columns[1].title(), [Inline::Link(_)]));

    // A plain header carries no `title`: the IR of an ordinary table is
    // unchanged.
    let plain = blocks("| A |\n| - |\n| 1 |\n");
    let Block::Table(t) = &plain[0] else {
        panic!("{plain:?}")
    };
    assert!(t.model.columns[0].title().is_empty());

    // A `yaml table` column name is read the same way, and a cell that
    // starts with a strong span keeps it (no lead promotion in a fragment).
    let yaml = blocks(
        "```yaml table\ncolumns: [\"**Bold** name\"]\nrows:\n  - [\"**Lead** cell\"]\n```\n",
    );
    let Block::Table(t) = &yaml[0] else {
        panic!("{yaml:?}")
    };
    assert_eq!(t.model.columns[0].name(), Some("**Bold** name"));
    assert!(matches!(
        t.model.columns[0].title(),
        [Inline::Strong(_), Inline::Str(_)]
    ));
    let tmark_ir::Row::Data(row) = &t.model.rows[0] else {
        panic!("{:?}", t.model.rows)
    };
    assert!(matches!(
        row.cells[0].content.as_slice(),
        [Inline::Strong(_), Inline::Str(_)]
    ));
}

#[test]
fn lead_promotion_is_the_whole_paragraph() {
    // Spec §Para: the sugar promotes a paragraph that *is* one short strong
    // span; a strong span that opens a paragraph stays a bold run-in, and a
    // list item is never promoted. The role takes what follows it.
    let out = blocks(
        "**A whole paragraph in bold.**\n\n\
         **Leading bold:** followed by text.\n\n\
         - **macOS:** install it\n- **macOS**\n\n\
         {lead}[Explicit.] Text after.\n\n\
         {lead}[Alone.]\n",
    );
    let lead = |b: &Block| match b {
        Block::Para(p) => p.lead.is_some(),
        _ => false,
    };
    assert!(lead(&out[0]));
    let Block::Para(p) = &out[0] else {
        unreachable!()
    };
    assert!(p.content.is_empty());
    assert!(!lead(&out[1]));
    let Block::BulletList(list) = &out[2] else {
        panic!("{:?}", out[2])
    };
    assert!(list.items.iter().all(|i| !lead(&i.content[0])));
    assert!(lead(&out[3]));
    let Block::Para(p) = &out[3] else {
        unreachable!()
    };
    assert_eq!(p.content.len(), 1);
    assert!(lead(&out[4]));
}

#[test]
fn empty_link_with_attributes_is_an_anchor() {
    // Spec §Attributes: `[](){#id}` is the anchor `[]{#id}`, deprecated.
    let parsed = parse("[](){ #myanchor }\n\n[label](){#a}\n", FileId::default());
    let Block::Para(p) = &parsed.document.blocks[0] else {
        panic!("{:?}", parsed.document.blocks)
    };
    let [Inline::Span(span)] = p.content.as_slice() else {
        panic!("{:?}", p.content)
    };
    assert_eq!(span.attrs.id.as_deref(), Some("myanchor"));
    assert!(span.content.is_empty());
    let deprecated: Vec<&str> = parsed
        .diagnostics
        .iter()
        .filter(|d| d.code == tmark_ir::Code::Deprecated)
        .filter_map(|d| d.fix.as_ref().map(|f| f.replacement.as_str()))
        .collect();
    assert_eq!(deprecated, ["[]{#myanchor}"]);

    // A link with a label is a link: the list still finds no host.
    let Block::Para(p) = &parsed.document.blocks[1] else {
        panic!("{:?}", parsed.document.blocks)
    };
    assert!(matches!(p.content.first(), Some(Inline::Link(_))));
    assert!(parsed
        .diagnostics
        .iter()
        .any(|d| d.code == tmark_ir::Code::AttrNoHost));
}

#[test]
fn reference_style_links_are_textual_references() {
    // Spec §Ref: `[text][id]` with no link definition is a `Link` to the
    // label `id`; a definition makes it an ordinary link; a backslash
    // anywhere in the spelling keeps it literal.
    let parsed = parse(
        "See [Plus haut][opengl-coordinates].\n\n[a][b]\n\n[b]: https://example.com\n\n\\[a\\][b2]\n",
        FileId::default(),
    );
    let Block::Para(p) = &parsed.document.blocks[0] else {
        panic!("{:?}", parsed.document.blocks)
    };
    let [Inline::Str(_), Inline::Link(link), Inline::Str(dot)] = p.content.as_slice() else {
        panic!("{:?}", p.content)
    };
    assert_eq!(
        link.target,
        tmark_ir::Target::Reference("opengl-coordinates".to_string())
    );
    assert_eq!(tmark_ir::plain_text(&link.content), "Plus haut");
    assert_eq!(dot.text, ".");

    let Block::Para(p) = &parsed.document.blocks[1] else {
        panic!("{:?}", parsed.document.blocks)
    };
    let [Inline::Link(link)] = p.content.as_slice() else {
        panic!("{:?}", p.content)
    };
    assert!(
        matches!(&link.target, tmark_ir::Target::Url(u) if u == "https://example.com"),
        "a definition makes it a link: {:?}",
        link.target
    );

    let Block::Para(p) = &parsed.document.blocks[2] else {
        panic!("{:?}", parsed.document.blocks)
    };
    assert!(
        p.content.iter().all(|i| !matches!(i, Inline::Link(_))),
        "an escaped spelling is literal text: {:?}",
        p.content
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

#[test]
fn a_reference_style_link_reads_a_text_that_carries_markup() {
    // Spec §Ref: the text may carry markup, as it may in `[text](#id)`.
    // The tokenizer leaves the brackets in the text nodes around it, so
    // the lowering cuts the spelling out of them — and only where it is
    // verbatim in the source, on one line, with no nearer `[`, no image
    // and no link inside.
    let reference = |md: &str| {
        let parsed = parse(md, FileId::default());
        let Block::Para(p) = &parsed.document.blocks[0] else {
            panic!("{:?}", parsed.document.blocks)
        };
        p.content.iter().find_map(|i| match i {
            Inline::Link(l) if matches!(l.target, tmark_ir::Target::Reference(_)) => {
                Some((l.target.clone(), tmark_ir::plain_text(&l.content)))
            }
            _ => None,
        })
    };
    let target = |id: &str| tmark_ir::Target::Reference(id.to_string());
    assert_eq!(
        reference("The [`#include`][cpp:include] directive.\n"),
        Some((target("cpp:include"), "#include".to_string()))
    );
    assert_eq!(
        reference("See [the *bold* one][opengl] here.\n"),
        Some((target("opengl"), "the bold one".to_string())),
        "the text before and after the markup is the link's too"
    );
    assert_eq!(
        reference("Two: [`a`][x] then [`b`][y].\n").map(|(t, _)| t),
        Some(target("x")),
        "what closes one spelling opens the next"
    );
    // CommonMark's reading stands: a nearer opener wins, and an image
    // reference, an escaped bracket or a line end is not a reference.
    assert_eq!(
        reference("[a [`b`][inner] c][outer]\n"),
        Some((target("inner"), "b".to_string()))
    );
    assert_eq!(reference("![`alt`][img]\n"), None);
    assert_eq!(reference("[`x` \\][id]\n"), None);
    assert_eq!(reference("[a\n`b`][id]\n"), None);
    assert_eq!(reference("[a [b](u) c][id]\n"), None);
}
