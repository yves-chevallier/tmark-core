//! TMark constructs: roles, attributes, references, definitions, inline
//! sugar, containers, admonitions, definition lists.
//!
//! Positions are checked by `tmark-syntax`'s conformance tests; here the
//! shape of the tree is what matters.
use pretty_assertions::assert_eq;
use tmark_markdown::{
    mdast::{Node, TmarkMarkKind},
    message, to_mdast, ParseOptions,
};

fn parse(value: &str) -> Result<Node, message::Message> {
    to_mdast(value, &ParseOptions::tmark())
}

/// Children of the first paragraph of the document.
fn inlines(value: &str) -> Vec<Node> {
    let tree = parse(value).expect("parses");
    match tree.children().and_then(|c| c.first()) {
        Some(Node::Paragraph(p)) => p.children.clone(),
        other => panic!("expected a paragraph, got {:?}", other),
    }
}

/// Compact description of an inline tree, for assertions.
fn describe(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(x) => out.push_str(&format!("text({:?})", x.value)),
            Node::Strong(x) => out.push_str(&format!("strong[{}]", describe(&x.children))),
            Node::Emphasis(x) => out.push_str(&format!("emph[{}]", describe(&x.children))),
            Node::Delete(x) => out.push_str(&format!("delete[{}]", describe(&x.children))),
            Node::InlineMath(x) => out.push_str(&format!("math({:?})", x.value)),
            Node::Link(x) => out.push_str(&format!("link({:?})", x.url)),
            Node::TmarkBrace(x) => out.push_str(&format!(
                "{}({:?})",
                if x.moustache { "moustache" } else { "brace" },
                x.value
            )),
            Node::TmarkArgument(x) => {
                out.push_str(&format!("arg({}{:?})", x.marker as char, x.value));
            }
            Node::TmarkGroup(x) => out.push_str(&format!("group[{}]", describe(&x.children))),
            Node::TmarkSpan(x) => out.push_str(&format!("span[{}]", describe(&x.children))),
            Node::TmarkReference(x) => out.push_str(&format!("ref({:?})", x.value)),
            Node::TmarkDefine(x) => out.push_str(&format!("define[{}]", describe(&x.children))),
            Node::TmarkMark(x) => out.push_str(&format!(
                "{}[{}]",
                match x.kind {
                    TmarkMarkKind::Highlight => "mark",
                    TmarkMarkKind::Superscript => "sup",
                    TmarkMarkKind::Insert => "ins",
                    TmarkMarkKind::Keystroke => "keys",
                    TmarkMarkKind::Subscript => "sub",
                },
                describe(&x.children)
            )),
            other => out.push_str(&format!("{:?}", other)),
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

#[test]
fn roles() {
    assert_eq!(
        describe(&inlines("a {aside side=left}[see **Prandtl**] b")),
        r##"text("a ") brace("aside side=left") group[text("see ") strong[text("Prandtl")]] text(" b")"##,
        "should tokenise a role head and its bracket group as text"
    );
    assert_eq!(
        describe(&inlines("{index main=true}[byte order][endianness].")),
        r##"brace("index main=true") group[text("byte order")] group[text("endianness")] text(".")"##,
        "should chain bracket groups"
    );
    assert_eq!(
        describe(&inlines("{raw latex}(\\clearpage) and {include}(a/(b).md)")),
        r##"brace("raw latex") arg(("\\clearpage") text(" and ") brace("include") arg(("a/(b).md")"##,
        "should take a balanced parenthesised argument"
    );
    assert_eq!(
        describe(&inlines("{aside}[see [x](y) and \\] here]")),
        r##"brace("aside") group[text("see ") link("y") text(" and ] here")]"##,
        "should allow links and escaped brackets inside a group"
    );
    assert_eq!(
        describe(&inlines("[see {aside}[x] here](y)")),
        r##"link("y")"##,
        "should allow a group inside a link"
    );
    assert_eq!(
        describe(&inlines("{foo} alone, {aside} bare, and {unclosed")),
        r##"brace("foo") text(" alone, ") brace("aside") text(" bare, and {unclosed")"##,
        "should keep an unclosed brace literal"
    );
    assert_eq!(
        describe(&inlines("{raw latex}(never closed")),
        r##"brace("raw latex") text("(never closed")"##,
        "should keep an unclosed argument literal"
    );
}

#[test]
fn attributes_and_spans() {
    assert_eq!(
        describe(&inlines(
            "![Alt](f.png){width=60% #fig:a} and [this claim]{#claim:one}."
        )),
        r##"Image { position: Some(1:1-1:14 (0-13)), alt: "Alt", url: "f.png", title: None } brace("width=60% #fig:a") text(" and ") span[text("this claim")] brace("#claim:one") text(".")"##,
        "should tokenise attribute lists after images and anonymous spans"
    );
    assert_eq!(
        describe(&inlines("[not a span] and [text]{ not attrs }")),
        r##"text("[not a span] and [text]") brace(" not attrs ")"##,
        "should keep brackets literal without an attribute list"
    );
    assert_eq!(
        describe(&inlines("[a [x](y) b]{#id}")),
        r##"span[text("a ") link("y") text(" b")] brace("#id")"##,
        "should allow a link inside a span"
    );
    assert_eq!(
        describe(&inlines("{#id}[link](url)")),
        r##"brace("#id") link("url")"##,
        "should not take a group after an attribute list"
    );
    assert_eq!(
        describe(&inlines(r##"{k="a } b"} c"##)),
        r##"brace("k=\"a } b\"") text(" c")"##,
        "should honour quoted values"
    );
}

#[test]
fn moustaches() {
    assert_eq!(
        describe(&inlines("{{ title }} and {{press.template}}")),
        r##"moustache(" title ") text(" and ") moustache("press.template")"##,
        "should tokenise moustaches"
    );
}

#[test]
fn references() {
    assert_eq!(
        describe(&inlines(
            "See @sec:intro, @Fig:x and @[see ein05, p. 33; -AI2027]."
        )),
        r##"text("See ") ref("sec:intro") text(", ") ref("Fig:x") text(" and ") ref("[see ein05, p. 33; -AI2027]") text(".")"##,
        "should tokenise bare and bracketed references"
    );
    assert_eq!(
        describe(&inlines("me@example.com, a@b, @a, path/@x, \\@handle")),
        r##"link("mailto:me@example.com") text(", a@b, @a, path/@x, @handle")"##,
        "should apply the X4 guard and escapes"
    );
    assert_eq!(
        describe(&inlines("§@[sec:model] and «@key» but é@key")),
        r##"text("§") ref("[sec:model]") text(" and «") ref("key") text("» but é@key")"##,
        "should guard on word characters, not on non-ASCII bytes"
    );
    assert_eq!(
        describe(&inlines(
            "@doi:10.1002/andp.19053221004. @https://doi.org/10.1/x, done"
        )),
        r##"ref("doi:10.1002/andp.19053221004") text(". ") ref("https://doi.org/10.1/x") text(", done")"##,
        "should accept slashes in doi and URL keys and drop trailing punctuation"
    );
}

#[test]
fn defines() {
    assert_eq!(
        describe(&inlines(
            "#[endianness] #[byte order][sub] #(fw:boot-loop) #{fw:old}"
        )),
        r##"define[] group[text("endianness")] text(" ") define[] group[text("byte order")] group[text("sub")] text(" ") define[arg(("fw:boot-loop")] text(" ") define[arg({"fw:old")]"##,
        "should tokenise index entries and counter items"
    );
    assert_eq!(
        describe(&inlines("\\#tag C# #1 #(nope) #[ spaced] #hashtag")),
        r##"text("#tag C# #1 #(nope) #[ spaced] #hashtag")"##,
        "should keep escaped, mid-word and malformed sigils literal"
    );
}

#[test]
fn inline_sugar() {
    assert_eq!(
        describe(&inlines(
            "==x== ~~x~~ H~2~O E=mc^2^ ++ctrl+alt+s++ ^^new^^ a=b x+y 2^3 ~/home"
        )),
        r##"mark[text("x")] text(" ") delete[text("x")] text(" H") sub[text("2")] text("O E=mc") sup[text("2")] text(" ") keys[text("ctrl+alt+s")] text(" ") ins[text("new")] text(" a=b x+y 2^3 ~/home")"##,
        "should form highlight, strikethrough, subscript, superscript, keystroke and insert"
    );
    assert_eq!(
        describe(&inlines("math $a^2$ and \\(b^2\\) and \\(x")),
        r##"text("math ") math("a^2") text(" and ") math("b^2") text(" and (x")"##,
        "should accept the LaTeX-habit inline math delimiters"
    );
}

#[test]
fn containers() -> Result<(), message::Message> {
    let tree = parse(
        "::: warning {title=\"LaTeX toolchain\"}\nInstall TeX Live.\n\n  indented\n:::\n\nafter",
    )?;
    match &tree.children().unwrap()[0] {
        Node::TmarkContainer(node) => {
            assert_eq!(node.marker, b':');
            assert_eq!(node.info, "warning {title=\"LaTeX toolchain\"}");
            assert_eq!(node.value, "Install TeX Live.\n\n  indented");
            assert_eq!(node.stops, vec![(0, 38), (19, 57)]);
            assert!(node.closed);
        }
        other => panic!("expected a container, got {:?}", other),
    }
    assert!(matches!(&tree.children().unwrap()[1], Node::Paragraph(_)));

    let tree = parse("para\n::: note\nbody\n\nmore")?;
    match &tree.children().unwrap()[1] {
        Node::TmarkContainer(node) => {
            assert_eq!(
                node.value, "body\n\nmore",
                "should run to the end when unclosed"
            );
            assert!(!node.closed);
        }
        other => panic!("expected a container, got {:?}", other),
    }

    let tree = parse(":::: outer\n::: inner\nx\n:::\n::::")?;
    match &tree.children().unwrap()[0] {
        Node::TmarkContainer(node) => {
            assert_eq!(
                node.value, "::: inner\nx\n:::",
                "should nest by fence length"
            );
            assert!(node.closed);
        }
        other => panic!("expected a container, got {:?}", other),
    }

    let tree = parse("/// caption\nOld.\n///")?;
    match &tree.children().unwrap()[0] {
        Node::TmarkContainer(node) => {
            assert_eq!(node.marker, b'/');
            assert_eq!(node.info, "caption");
        }
        other => panic!("expected a container, got {:?}", other),
    }

    let tree = parse(":::\nnot a container\n:::")?;
    assert!(
        matches!(&tree.children().unwrap()[0], Node::Paragraph(_)),
        "should need a name on the opening fence"
    );
    Ok(())
}

#[test]
fn admonitions() -> Result<(), message::Message> {
    let tree = parse(
        "!!! warning \"Title\"\n    First.\n\n    Second.\nafter\n\n???+ note inline\n    Open.",
    )?;
    let children = tree.children().unwrap();
    match &children[0] {
        Node::TmarkAdmonition(node) => {
            assert_eq!(node.marker, "!!!");
            assert_eq!(node.info, "warning \"Title\"");
            assert_eq!(node.value, "First.\n\nSecond.");
            assert_eq!(node.stops, vec![(0, 24), (8, 36)]);
        }
        other => panic!("expected an admonition, got {:?}", other),
    }
    assert!(matches!(&children[1], Node::Paragraph(_)));
    match &children[2] {
        Node::TmarkAdmonition(node) => {
            assert_eq!(node.marker, "???+");
            assert_eq!(node.info, "note inline");
            assert_eq!(node.value, "Open.");
        }
        other => panic!("expected an admonition, got {:?}", other),
    }
    assert!(
        matches!(
            &parse("!!!\n    no type")?.children().unwrap()[0],
            Node::Paragraph(_)
        ),
        "should need a type"
    );
    // A marker line that closes a container is a lazy line; its body is
    // indented all the same and belongs to the callout.
    for source in [
        "- [x] a\n\n!!! note\n\n    Body.\n\nEnd.\n",
        "- [x] a\n!!! note\n    Body.\n\nEnd.\n",
    ] {
        let tree = parse(source)?;
        let children = tree.children().unwrap();
        match &children[1] {
            Node::TmarkAdmonition(node) => assert_eq!(node.value, "Body.", "{:?}", source),
            other => panic!("expected an admonition, got {:?}", other),
        }
        assert!(matches!(&children[2], Node::Paragraph(_)), "{:?}", source);
    }
    // A body line that is itself lazy, or that opens a container, is not
    // the callout's.
    let tree = parse(
        "> !!! note
    Body.
",
    )?;
    match &tree.children().unwrap()[0] {
        Node::Blockquote(quote) => match &quote.children[0] {
            Node::TmarkAdmonition(node) => assert_eq!(node.value, ""),
            other => panic!("expected an admonition, got {:?}", other),
        },
        other => panic!("expected a block quote, got {:?}", other),
    }
    Ok(())
}

#[test]
fn definitions() -> Result<(), message::Message> {
    let tree = parse("Term\n:   One.\n    continued\n\n    second para\n:   Two.\n\nNext")?;
    let children = tree.children().unwrap();
    assert!(matches!(&children[0], Node::Paragraph(_)));
    match &children[1] {
        Node::TmarkDefinition(node) => {
            assert_eq!(node.value, "One.\ncontinued\n\nsecond para");
            assert_eq!(node.stops, vec![(0, 9), (5, 18), (16, 33)]);
        }
        other => panic!("expected a definition, got {:?}", other),
    }
    match &children[2] {
        Node::TmarkDefinition(node) => assert_eq!(node.value, "Two."),
        other => panic!("expected a definition, got {:?}", other),
    }
    assert!(matches!(&children[3], Node::Paragraph(_)));
    assert!(
        matches!(
            &parse(":no space")?.children().unwrap()[0],
            Node::Paragraph(_)
        ),
        "should need a space after the colon"
    );
    Ok(())
}

#[test]
fn nesting() -> Result<(), message::Message> {
    let tree = parse("> ::: note\n> quoted\n> :::")?;
    match &tree.children().unwrap()[0] {
        Node::Blockquote(quote) => match &quote.children[0] {
            Node::TmarkContainer(node) => assert_eq!(node.value, "quoted"),
            other => panic!("expected a container, got {:?}", other),
        },
        other => panic!("expected a block quote, got {:?}", other),
    }
    let tree = parse("| a |\n| - |\n| #(n:joy) @sec:boot |")?;
    let cell = match &tree.children().unwrap()[0] {
        Node::Table(table) => match &table.children[1] {
            Node::TableRow(row) => match &row.children[0] {
                Node::TableCell(cell) => cell.children.clone(),
                other => panic!("expected a cell, got {:?}", other),
            },
            other => panic!("expected a row, got {:?}", other),
        },
        other => panic!("expected a table, got {:?}", other),
    };
    assert_eq!(
        describe(&cell),
        r##"define[arg(("n:joy")] text(" ") ref("sec:boot")"##,
        "should work inside table cells"
    );
    Ok(())
}

#[test]
fn off_by_default() {
    let tree =
        to_mdast("{aside}[x] @ref #[t] ==x== ::: note", &ParseOptions::gfm()).expect("parses");
    match tree.children().and_then(|c| c.first()) {
        Some(Node::Paragraph(p)) => assert_eq!(
            describe(&p.children),
            r##"text("{aside}[x] @ref #[t] ==x== ::: note")"##,
            "should leave everything literal without the tmark constructs"
        ),
        other => panic!("expected a paragraph, got {:?}", other),
    }
}
