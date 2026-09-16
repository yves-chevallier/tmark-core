//! The web lowering (`tmark_writers::lower_web`, web-profile.md): one
//! snapshot per conformance fixture, a document exercising every row of
//! the per-construct table, and the mkdocstrings case (an unclosed
//! `::: pkg.mod` followed by prose with `@` references).
//!
//! Review a change with `cargo insta review` (or `INSTA_UPDATE=always
//! cargo test -p tmark-writers --test web` and read the diff).

use std::fs;
use std::path::{Path, PathBuf};

use tmark_registry::{resolve, BookLabel, Host, MemoryLoader, Numbering, ResolveOptions};
use tmark_writers::{lower_web, Citations, Lowered, SectionRefs, WebOptions};

/// The fenced block under `## canonical`, else the first under `## input`.
fn source(text: &str) -> Option<String> {
    let mut section = "";
    let mut fence: Option<(usize, String)> = None;
    let mut input = None;
    let mut canonical = None;
    for line in text.lines() {
        if let Some((len, content)) = fence.as_mut() {
            let trimmed = line.trim_end();
            if trimmed.starts_with('`')
                && trimmed.chars().all(|c| c == '`')
                && trimmed.len() >= *len
            {
                let (_, content) = fence.take().unwrap();
                match section {
                    "input" if input.is_none() => input = Some(content),
                    "canonical" if canonical.is_none() => canonical = Some(content),
                    _ => {}
                }
            } else {
                content.push_str(line);
                content.push('\n');
            }
            continue;
        }
        if let Some(title) = line.strip_prefix("## ") {
            section = match title.trim() {
                "input" => "input",
                "canonical" => "canonical",
                _ => "",
            };
        } else if line.starts_with("```") && !section.is_empty() {
            let len = line.chars().take_while(|c| *c == '`').count();
            fence = Some((len, String::new()));
        }
    }
    canonical.or(input)
}

fn lower_with(
    text: &str,
    loader: &MemoryLoader,
    options: ResolveOptions,
    web: &WebOptions,
) -> Lowered {
    let parsed = tmark_syntax::parse(text, tmark_ir::FileId::default());
    let resolved = resolve(&parsed.document, loader, &options);
    lower_web(text, &parsed.document, &resolved, loader, web)
}

fn site_options(path: &str) -> ResolveOptions {
    ResolveOptions {
        path: PathBuf::from(path),
        numbering: Numbering::All,
        bibliography: vec![PathBuf::from("refs.bib")],
        ..ResolveOptions::default()
    }
}

fn lower(text: &str) -> Lowered {
    lower_with(
        text,
        &MemoryLoader::new(),
        site_options("page.md"),
        &WebOptions::default(),
    )
}

#[test]
fn fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../spec/conformance");
    let mut paths: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "md") && p.file_name().unwrap() != "README.md"
        })
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    for path in paths {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        let text = fs::read_to_string(&path).unwrap();
        let Some(source) = source(&text) else {
            continue;
        };
        let lowered = lower(&source);
        let mut snapshot = lowered.text.clone();
        if !lowered.diagnostics.is_empty() {
            snapshot.push_str("---diagnostics---\n");
            for d in &lowered.diagnostics {
                snapshot.push_str(&format!(
                    "{} {}: {}\n",
                    d.severity.as_str(),
                    d.code.id(),
                    d.message
                ));
            }
        }
        insta::assert_snapshot!(format!("web__{name}"), snapshot);
    }
}

const BIB: &str = "@article{ein05,
  author = {Einstein, Albert},
  title = {Zur Elektrodynamik bewegter K{\\\"o}rper},
  journal = {Annalen der Physik},
  year = {1905},
  doi = {10.1002/andp.19053221004}
}
@book{ko20, author = {Ada Knuth and Bob Ritchie}, title = {Systems}, year = {2020}}
";

/// One construct per row of the per-construct table of web-profile.md.
const ROWS: &str = r##"---
title: Firmware review
lang: en
press:
  declare:
    counters:
      fw: {name: Finding, format: "FW-{n:02d}"}
    admonitions:
      theorem: {name: Theorem, counter: thm}
    glossary:
      hal: Hardware abstraction layer
---

# Findings {#sec:findings}

A finding #(fw:watchdog) and a silent one below. See @fw:watchdog, @Fw:watchdog,
@[fw:watchdog; fw:log-wrap], @sec:findings, @fig:boot, @[fig:a; fig:b], @fig:b,
@tbl:stock, @lst:hello, @eq:mass, @thm:pyth, @[see fig:boot, left panel],
[the trace](#fig:boot), @gls:hal, @nope and @doi:10.1000/x.

Cite @ein05, @[ein05, p. 33; see ko20] and @[-ein05].

## Log buffer wiped {#fw:log-wrap}

{{ title }} by {{ nobody }}.

![The boot](boot.png){#fig:boot width=50%}

Figure: The *boot* sequence, see @fw:watchdog. {#fig:boot}

::: figure {#fig:pair cols=2}
![Left](a.png){#fig:a}
![Right](b.png){#fig:b}

Figure: Two panels.
:::

| Id | Item |
| --- | ---- |
| #(fw:ota) | A cell with @fw:watchdog |

```yaml table-config
columns:
  - {width: 30%}
```

Table: Stock. {#tbl:stock}

```yaml table
columns: [A, {name: G, columns: [B, C]}]
rows:
  - ["*one*", {value: "==two== @fw:ota", cols: 2}]
  - [~, [x, y]]
```

Table: A model. {#tbl:model}

```py title="hello.py"
print("hi")
```

Listing: Hello. {#lst:hello}

$$
E = mc^2
$$ {#eq:mass}

!!! note "As written"
    Body with @fw:watchdog stays a `!!!` block.

:::: warning {title="A title" .wide}
Body with @fw:ota and a nested callout.

::: tip {collapsed=true}
Folded.
:::
::::

::: theorem {#thm:pyth title=Pythagoras}
In a right triangle, $a^2 + b^2 = c^2$.
:::

::: theorem {#thm:nest title=Nesting}
<div class="three-column-list" markdown>

- one

</div>

??? tip
    Folded, around

    <div class="two-column-list" markdown>

    1. un

    </div>
:::

::: aside {side=left}
A margin *note* with @fw:ota.
:::

::: div {#layout .two-column-list}
- one
- two
:::

/// html | div[class='wrapper']
Written the pymdownx way.
///

<div class="kept" markdown>
Already HTML.
</div>

Inline {aside side=right}[margin note] and {index}[hal][layer], #[**boot**],
{sc}[nasa], __caps__, {keys}[ctrl+alt+s], {mark}[x], {del}[y], H{sub}[2]O,
E=mc{sup}[2], {code py}[print(1)], {underline}[u], [x]{#sp .c lang=fr},
{raw html}(<b>raw</b>), {raw latex}(\clearpage), [print only]{media=print}
and [web only]{media=web}.

```latex raw
\clearpage
```

```html raw
<hr class="raw" />
```

![Print](p.png){media=print}

{include}(parts/extra.md)

--8<-- "snippets/x.md"

```mermaid
graph TD; A-->B;
```

```c title="iota.c" include="parts/iota.c"
```

```c include="parts/absent.c"
```

- A list item with @fw:ota

  ::: note {title=Nested}
  In a list, with @fw:watchdog.
  :::

> A quote with #(fw:quoted) and @fw:quoted.

Term
:   A definition with @fw:ota.

Footnote[^1] and *[HAL]: Hardware abstraction layer.

[^1]: A note with @fw:ota.

::: gadget {x=1}
Unknown container with @fw:ota.
:::

::: tip {#tip:markup title="A *folded* `title`" collapsed=true}
Markup in a `<summary>`.
:::

!!! note "#(fw:titled) in the title"
    A `!!!` line cannot carry a `<span>`.

---
"##;

fn rows_loader() -> MemoryLoader {
    MemoryLoader::new()
        .with("refs.bib", BIB)
        .with(
            "parts/extra.md",
            "## Included {#sec:extra}\n\nIncluded #(fw:extra), see @fw:watchdog and @sec:extra.\n",
        )
        .with("parts/iota.c", "int main(void) { return 0; }\n")
}

#[test]
fn every_row_of_the_table() {
    let lowered = lower_with(
        ROWS,
        &rows_loader(),
        site_options("page.md"),
        &WebOptions::default(),
    );
    let text = &lowered.text;
    let snapshot = format!(
        "{text}---bibliography---\n{}\n---diagnostics---\n{}",
        lowered.bibliography.as_deref().unwrap_or("(none)"),
        lowered
            .diagnostics
            .iter()
            .map(|d| format!("{} {}: {}\n", d.severity.as_str(), d.code.id(), d.message))
            .collect::<String>()
    );
    insta::assert_snapshot!("web__rows", snapshot);
    let has = |s: &str| assert!(text.contains(s), "missing {s:?} in\n{text}");
    // Counters, silent items, references.
    has(
        r##"<span class="ts-counter" id="fw:watchdog" data-counter="fw" data-key="watchdog">FW-01</span>"##,
    );
    has("## Log buffer wiped {#fw:log-wrap}");
    has("[FW-01](#fw:watchdog), [FW-01](#fw:watchdog),\n[FW-01 and FW-02](#fw:watchdog), [Findings](#sec:findings), [Figure 1](#fig:boot), [Figures 2a and 2b](#fig:a), [Figure 2b](#fig:b),\n[Table 1](#tbl:stock), [Listing 1](#lst:hello), [Equation 1](#eq:mass), [Theorem 1](#thm:pyth), see [Figure 1](#fig:boot), left panel,\n[the trace](#fig:boot), <abbr title=\"Hardware abstraction layer\">hal</abbr>, [?nope] and [doi:10.1000/x](https://doi.org/10.1000/x).");
    // Citations, built-in author-year, and the appended list.
    has(
        r##"Cite (<a class="ts-cite" href="#ref-ein05">Einstein 1905</a>), (<a class="ts-cite" href="#ref-ein05">Einstein 1905</a>, p. 33; see <a class="ts-cite" href="#ref-ko20">Knuth 2020</a>) and (<a class="ts-cite" href="#ref-ein05">1905</a>)."##,
    );
    has("\n## References\n\n<ol class=\"ts-bibliography\">\n<li id=\"ref-ein05\">Einstein, Albert. (1905). <em>Zur Elektrodynamik");
    assert!(lowered
        .bibliography
        .as_deref()
        .unwrap()
        .starts_with("<ol class=\"ts-bibliography\">"));
    // Moustaches: a front-matter value, an unknown key verbatim.
    has("Firmware review by {{ nobody }}.");
    // Figures, sub-figures.
    has("<figure markdown=\"span\" id=\"fig:boot\">\n![The boot](boot.png){width=50%}\n<figcaption markdown=\"span\"><span class=\"ts-caption-label\">Figure 1:</span> The *boot* sequence, see [FW-01](#fw:watchdog).</figcaption>\n</figure>");
    has("<figure markdown=\"span\" id=\"fig:pair\" class=\"ts-subfigures\" style=\"--ts-cols:2\">\n![Left](a.png){#fig:a}\n<span class=\"ts-subcaption\">(a)</span>\n![Right](b.png){#fig:b}\n<span class=\"ts-subcaption\">(b)</span>\n<figcaption markdown=\"span\"><span class=\"ts-caption-label\">Figure 2:</span> Two panels.</figcaption>\n</figure>");
    // Tables: the pipe table as written inside the wrapper, the config dropped.
    has("<figure markdown=\"1\" class=\"ts-table\" id=\"tbl:stock\">\n<figcaption markdown=\"span\"><span class=\"ts-caption-label\">Table 1:</span> Stock.</figcaption>\n\n| Id | Item |\n| --- | ---- |\n| <span class=\"ts-counter\" id=\"fw:ota\" data-counter=\"fw\" data-key=\"ota\">FW-03</span> | A cell with [FW-01](#fw:watchdog) |\n\n</figure>");
    assert!(!text.contains("table-config"));
    has("<table class=\"ts-table\" data-ts-table=\"1\" markdown=\"block\">\n<thead markdown=\"block\">\n<tr markdown=\"block\">\n<th markdown=\"span\" rowspan=\"2\">A</th>\n<th markdown=\"span\" colspan=\"2\">G</th>\n</tr>");
    has("<td markdown=\"span\" colspan=\"2\">==two== [FW-03](#fw:ota)</td>");
    // Listing, equation.
    has("<figure markdown=\"1\" class=\"ts-listing\" id=\"lst:hello\">\n\n```py title=\"hello.py\"\nprint(\"hi\")\n```\n\n<figcaption markdown=\"span\"><span class=\"ts-caption-label\">Listing 1:</span> Hello.</figcaption>\n</figure>");
    has("<div id=\"eq:mass\" class=\"ts-equation\" markdown=\"1\">\n$$\nE = mc^2\n$$\n</div>");
    // Callouts.
    has("!!! note \"As written\"\n    Body with [FW-01](#fw:watchdog) stays a `!!!` block.");
    has("!!! warning wide \"A title\"\n    Body with [FW-03](#fw:ota) and a nested callout.\n\n    ??? tip\n        Folded.");
    has("<div class=\"admonition theorem\" id=\"thm:pyth\" markdown=\"1\">\n<p class=\"admonition-title\" markdown=\"span\">Theorem 1 (Pythagoras)</p>\n\nIn a right triangle, $a^2 + b^2 = c^2$.\n\n</div>");
    // A callout inside an HTML wrapper is an HTML wrapper too, so the
    // `<div markdown>` its body holds is written at column zero:
    // indented, its `</div>` would close the wrapper (C68).
    has("<div class=\"admonition theorem\" id=\"thm:nest\" markdown=\"1\">\n<p class=\"admonition-title\" markdown=\"span\">Theorem 2 (Nesting)</p>\n\n<div class=\"three-column-list\" markdown>\n\n- one\n\n</div>\n\n<details class=\"tip\" markdown=\"1\">\n<summary class=\"admonition-title\" markdown=\"span\">Tip</summary>\n\nFolded, around\n\n<div class=\"two-column-list\" markdown>\n\n1. un\n\n</div>\n\n</details>\n\n</div>");
    // Asides, index, inline sugar, spans, raw, media.
    has("<aside class=\"ts-aside\" data-side=\"left\" markdown=\"1\">\n\nA margin *note* with [FW-03](#fw:ota).\n\n</aside>");
    has("<div id=\"layout\" class=\"two-column-list\" markdown=\"1\">\n\n- one\n- two\n\n</div>");
    has("<div class=\"wrapper\" markdown=\"1\">\n\nWritten the pymdownx way.\n\n</div>");
    // A container already written as HTML keeps its bytes.
    has("<div class=\"kept\" markdown>\nAlready HTML.\n</div>");
    // `include=` is spliced from the loader; `title=` survives, the
    // attribute superfences would refuse does not.
    has("```c title=\"iota.c\"\nint main(void) { return 0; }\n```");
    // A file the loader cannot serve is reported, and the fence is
    // reprinted empty: `include=` must not reach superfences, which reads
    // the whole fence as an inline code span and eats what follows it.
    has("```c\n```");
    assert!(
        !text.contains("include="),
        "an `include=` survived in\n{text}"
    );
    has("Inline <span class=\"ts-aside\" data-side=\"right\">margin note</span> and <span class=\"ts-index\" data-tag=\"hal\" data-tag1=\"layer\"></span>, <span class=\"ts-index\" data-tag=\"boot\" data-main></span>,\n<span class=\"ts-smallcaps\">nasa</span>, <span class=\"ts-smallcaps\">caps</span>, ++ctrl+alt+s++, ==x==, ~~y~~, H~2~O,\nE=mc^2^, `#!py print(1)`, <u>u</u>, <span id=\"sp\" class=\"c\" lang=\"fr\">x</span>,\n<b>raw</b>,,\nand web only.");
    assert!(!text.contains("\\clearpage"));
    has("<hr class=\"raw\" />");
    assert!(!text.contains("p.png"));
    // Includes: lowered with their own labels; snippets untouched.
    has("## Included {#sec:extra}\n\nIncluded <span class=\"ts-counter\" id=\"fw:extra\" data-counter=\"fw\" data-key=\"extra\">FW-06</span>, see [FW-01](#fw:watchdog) and [Included](#sec:extra).");
    has("--8<-- \"snippets/x.md\"");
    // Verbatim rows.
    has("```mermaid\ngraph TD; A-->B;\n```");
    has("- A list item with [FW-03](#fw:ota)\n\n  !!! note \"Nested\"\n      In a list, with [FW-01](#fw:watchdog).");
    has("> A quote with <span class=\"ts-counter\" id=\"fw:quoted\" data-counter=\"fw\" data-key=\"quoted\">FW-04</span> and [FW-04](#fw:quoted).");
    has("Term\n:   A definition with [FW-03](#fw:ota).");
    has("Footnote[^1] and *[HAL]: Hardware abstraction layer.\n\n[^1]: A note with [FW-03](#fw:ota).");
    has("::: gadget {x=1}\nUnknown container with [FW-03](#fw:ota).\n:::");
    // The title of a wrapper is `markdown="span"`, so `md_in_html`
    // renders its inlines; the marker forms above need no attribute,
    // Python-Markdown parsing their title as inline Markdown already.
    has("<details class=\"tip\" id=\"tip:markup\" markdown=\"1\">\n<summary class=\"admonition-title\" markdown=\"span\">Tip 1 (A *folded* `title`)</summary>\n\nMarkup in a `<summary>`.\n\n</details>");
    // A `!!!` source stays as written — unless its title lowers to
    // something the marker line cannot carry (C66): PyMdownX reads the
    // title up to the next `"`, so the wrapper takes over.
    has("<div class=\"admonition note\" markdown=\"1\">\n<p class=\"admonition-title\" markdown=\"span\"><span class=\"ts-counter\" id=\"fw:titled\" data-counter=\"fw\" data-key=\"titled\">FW-05</span> in the title</p>\n\nA `!!!` line cannot carry a `<span>`.\n\n</div>\n\n---\n");
    // The one diagnostic the lowering itself finds: the fence whose
    // `include=` names a file the loader does not serve.
    let codes: Vec<_> = lowered
        .diagnostics
        .iter()
        .map(|d| d.code.id())
        .collect::<Vec<_>>();
    assert_eq!(codes, ["include-missing"], "{:?}", lowered.diagnostics);
}

/// A fence whose `include=` the loader refuses is reprinted **without**
/// the attribute and with an empty body, not kept as written: PyMdownX's
/// `superfences` does not parse `include=` in an info string, so the kept
/// bytes are no fence at all — `convert` gives `<p><code>c
/// include="missing.c"</code></p>` and the paragraph after it is eaten.
/// `title=` and the other options survive, so the page shows an empty
/// listing under its title, and `include-missing` is still reported.
#[test]
fn an_unresolved_fence_include_is_reprinted_without_the_option() {
    let text = "```c title=\"missing.c\" include=\"parts/missing.c\"\n```\n\nA paragraph after the fence.\n";
    let lowered = lower(text);
    assert!(
        !lowered.text.contains("include="),
        "an `include=` survived in\n{}",
        lowered.text
    );
    assert_eq!(
        lowered.text,
        "```c title=\"missing.c\"\n```\n\nA paragraph after the fence.\n"
    );
    let codes: Vec<_> = lowered.diagnostics.iter().map(|d| d.code.id()).collect();
    assert_eq!(codes, ["include-missing"], "{:?}", lowered.diagnostics);
    // A fence still written `--8<--` is `pymdownx.snippets`' to expand.
    let snippet = lower("```c\n--8<-- \"parts/missing.c\"\n```\n");
    assert_eq!(snippet.text, "```c\n--8<-- \"parts/missing.c\"\n```\n");
    assert!(snippet.diagnostics.is_empty(), "{:?}", snippet.diagnostics);
}

/// The mkdocstrings case: `::: pkg.mod` is a foreign directive (spec C40)
/// closed by the dedent; its bytes stay, the prose after it is lowered.
#[test]
fn unclosed_foreign_container_keeps_its_bytes() {
    let text = "---\npress:\n  declare:\n    counters:\n      fw: {name: Finding, format: \"FW-{n:02d}\"}\n---\n\n#(fw:boot) A finding.\n\n::: texsmith.core.counters\n    options:\n      show_source: false\n\nProse after the directive still refers to @fw:boot and @sec:nope.\n";
    let parsed = tmark_syntax::parse(text, tmark_ir::FileId::default());
    let severities: Vec<_> = parsed
        .diagnostics
        .iter()
        .map(|d| (d.code.id(), d.severity.as_str()))
        .collect();
    // Spec C40: a dotted name is a foreign directive (`RawBlock{markdown}`),
    // parsed without a diagnostic; `directive-foreign` is a lint hint.
    assert_eq!(severities, []);
    let lowered = lower(text);
    assert_eq!(
        lowered.text,
        "---\npress:\n  declare:\n    counters:\n      fw: {name: Finding, format: \"FW-{n:02d}\"}\n---\n\n<span class=\"ts-counter\" id=\"fw:boot\" data-counter=\"fw\" data-key=\"boot\">FW-01</span> A finding.\n\n::: texsmith.core.counters\n    options:\n      show_source: false\n\nProse after the directive still refers to [FW-01](#fw:boot) and [?sec:nope].\n"
    );
    assert!(lowered.diagnostics.is_empty());
    assert!(lowered.bibliography.is_none());
}

#[test]
fn plain_commonmark_is_left_alone() {
    let text = "# Title\n\nSome *prose* with a [link](https://x.y) and `code`.\n\n- a\n- b\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\n```py\nx = 1\n```\n\n> quote\n\n!!! note\n    body\n\n=== \"Tab\"\n    content\n\nText with an email me@x.y and {{ macro }}.\n";
    let lowered = lower(text);
    assert_eq!(lowered.text, text);
    assert!(lowered.diagnostics.is_empty());
}

#[test]
fn sibling_labels_and_section_numbers() {
    let text = "# Intro {#sec:intro}\n\nSee @fw:x on another page, @fig:far and @sec:intro; @sec:other too.\n";
    let book = vec![
        BookLabel {
            key: "fw:x".into(),
            prefix: Some("fw".into()),
            number: Some("FW-10".into()),
            kind: Host::CounterItem,
            title: None,
            location: "findings.md#fw:x".into(),
        },
        BookLabel {
            key: "fig:far".into(),
            prefix: Some("fig".into()),
            number: Some("12".into()),
            kind: Host::Figure,
            title: Some("Far".into()),
            location: "other/page.md#fig:far".into(),
        },
        BookLabel {
            key: "sec:other".into(),
            prefix: Some("sec".into()),
            number: None,
            kind: Host::Header,
            title: Some("The other page".into()),
            location: "other.md#sec:other".into(),
        },
    ];
    let options = ResolveOptions {
        book: book.clone(),
        ..site_options("index.md")
    };
    let lowered = lower_with(
        text,
        &MemoryLoader::new(),
        options.clone(),
        &WebOptions::default(),
    );
    assert_eq!(
        lowered.text,
        "# Intro {#sec:intro}\n\nSee [FW-10](findings.md#fw:x) on another page, [Figure 12](other/page.md#fig:far) and [Intro](#sec:intro); [The other page](other.md#sec:other) too.\n"
    );
    let numbered = lower_with(
        text,
        &MemoryLoader::new(),
        options,
        &WebOptions {
            sections: SectionRefs::Number,
            ..WebOptions::default()
        },
    );
    assert!(
        numbered.text.contains("[Section 1](#sec:intro)"),
        "{}",
        numbered.text
    );
}

#[test]
fn citations_passthrough_for_mkdocs_bibtex() {
    let text = "Cite @ein05, @[ein05, p. 33; see -@ko20] and @nope.\n";
    let lowered = lower_with(
        text,
        &rows_loader(),
        site_options("page.md"),
        &WebOptions {
            citations: Citations::Passthrough,
            ..WebOptions::default()
        },
    );
    // Pandoc's bare `@key` is narrative; TMark's is the short form.
    assert!(
        lowered
            .text
            .ends_with("Cite [@ein05], [@ein05, p. 33; see -@ko20] and [?nope].\n"),
        "{}",
        lowered.text
    );
    assert!(lowered.bibliography.is_none());
    assert!(!lowered.text.contains("References"));
    // Under `citations.narrative` the bare key is Pandoc's bare key.
    let narrative = lower_with(
        &format!("---\npress:\n  features:\n    citations.narrative: true\n---\n\n{text}"),
        &rows_loader(),
        site_options("page.md"),
        &WebOptions {
            citations: Citations::Passthrough,
            ..WebOptions::default()
        },
    );
    assert!(
        narrative
            .text
            .ends_with("Cite @ein05, [@ein05, p. 33; see -@ko20] and [?nope].\n"),
        "{}",
        narrative.text
    );
}

#[test]
fn localised_words_and_css_prefix() {
    let text = "---\nlang: fr\n---\n\n![a](a.png)\n\nFigure: Un. {#fig:a}\n\n![b](b.png)\n\nFigure: Deux. {#fig:b}\n\nVoir @[fig:a; fig:b] et #(fw:x).\n";
    let lowered = lower_with(
        text,
        &MemoryLoader::new(),
        site_options("page.md"),
        &WebOptions {
            css_prefix: "x-".into(),
            ..WebOptions::default()
        },
    );
    assert!(
        lowered.text.contains("[Figures 1 et 2](#fig:a)"),
        "{}",
        lowered.text
    );
    assert!(
        lowered
            .text
            .contains("<span class=\"x-caption-label\">Figure 1:</span> Un."),
        "{}",
        lowered.text
    );
    assert!(
        lowered
            .text
            .contains("<span class=\"x-counter\" id=\"fw:x\""),
        "{}",
        lowered.text
    );
}

/// `[text](#id)`, the canonical textual reference, and the deprecated
/// `[text][id]` (spec §Ref). A same-page anchor keeps its bytes: `#id` is
/// what the rendered page answers to. A label of a sibling document is
/// spliced with that page's location, the way `@id` is, so the site
/// resolves the cross-page link from the site map and needs no
/// `mkdocs-autorefs`. The deprecated spelling is written canonically in
/// both cases — a plain CommonMark parser reads brackets there, not a
/// link — and a key that is no label keeps its bytes, being literal text.
#[test]
fn an_anchor_link_to_a_sibling_label_is_spliced() {
    let text = "# Intro {#sec:intro}\n\nSee [here](#sec:intro), [there](#sec:other), [here too][sec:intro],\n[there too][sec:other], [](#sec:other) and [prose][nothing].\n";
    let options = ResolveOptions {
        book: vec![BookLabel {
            key: "sec:other".into(),
            prefix: Some("sec".into()),
            number: None,
            kind: Host::Header,
            title: Some("The other page".into()),
            location: "other.md#sec:other".into(),
        }],
        ..site_options("index.md")
    };
    let lowered = lower_with(text, &MemoryLoader::new(), options, &WebOptions::default());
    assert_eq!(
        lowered.text,
        "# Intro {#sec:intro}\n\nSee [here](#sec:intro), [there](other.md#sec:other), [here too](#sec:intro),\n[there too](other.md#sec:other), [The other page](other.md#sec:other) and [prose][nothing].\n"
    );
}

/// TMark matches a label key case-insensitively; an HTML `id` is not.
/// The reference-style spelling therefore addresses the label as it is
/// *declared*, the way `@key` is lowered.
#[test]
fn a_reference_style_link_addresses_the_label_as_declared() {
    let md = "[](){#Claim}\n\nSee [the claim][claim].\n";
    assert!(
        lower(md).text.contains("See [the claim](#Claim)."),
        "{}",
        lower(md).text
    );
}

/// A code span is read before a link is, so a bracket inside one closes
/// no link and must not be escaped: a backslash there is a backslash.
#[test]
fn a_bracket_inside_a_code_span_of_a_link_text_is_not_escaped() {
    let md = "[](){#Idx}\n\nSee [the `array[i]` form][idx].\n";
    let text = lower(md).text;
    assert!(text.contains("[the `array[i]` form](#Idx)"), "{text}");
    // Outside one it still is: the text may not close the link early.
    let md = "[](){#idx}\n\nSee [a *[x]* b][idx].\n";
    let text = lower(md).text;
    assert!(text.contains("[a *\\[x\\]* b](#idx)"), "{text}");
}
