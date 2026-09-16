# 02 — Syntax: the parser

Crate: `tmark-syntax`. Input: `&str` and a `FileId`. Output: `Document` plus
`Vec<Diagnostic>`. It never fails and never performs I/O.

## Decision: vendor the micromark architecture (ADR 0002)

TMark is CommonMark plus constructs. The parser is built by vendoring
`markdown-rs` (MIT, a faithful port of micromark) into `tmark-syntax` and
adding constructs to it, rather than by wrapping it as a dependency or by
writing a CommonMark parser from scratch.

Why: micromark's design is *exactly* "a state-machine tokenizer where each
construct is a module" (`construct/*.rs`), positions are attached to every
event, GFM tables, footnotes, task lists, math and front matter already exist
as constructs, and CommonMark conformance comes with the vendored spec tests.
Post-processing a third-party AST is not an option: role content
(`{aside}[see **x**]`) interleaves with emphasis and links, and must be
tokenised in the same inline pass.

Cost accepted: a fork. CommonMark changes at glacial pace; upstream merges are
rare and mechanical. The vendored tree is its own workspace crate,
`crates/tmark-markdown` (lib `tmark_markdown`), so that its internal `crate::`
paths and its test suite stay untouched; `VENDORED.md` names the upstream
commit and lists every changed file. `tmark-syntax` depends on it and owns the
lowering.

## Two passes, as in micromark

1. **Block pass** (document → containers and flow): headings, paragraphs,
   lists, block quotes, fenced code, thematic breaks, HTML blocks, tables,
   footnote definitions, front matter, plus the TMark block constructs below.
2. **Inline pass** (per text chunk): attention (emphasis, strong), code spans,
   links, images, autolinks, escapes, entities, math, plus the TMark inline
   constructs below.

Events carry byte offsets. A third step, **lowering** (`lower.rs`), turns the
event stream into the IR of `03-ir.md`. Lowering is where most of TMark
happens: a great deal of the dialect is *reinterpretation* of CommonMark
events, not new tokenisation.

## TMark constructs, and where each is handled

Reference: spec §Four syntactic families, §Two sigils, §Lexical grammar.

| Construct | Spec regex | Pass | How |
| --------- | ---------- | ---- | --- |
| Attribute list `{#id .cls k=v}` | family 1 | inline | New construct `attributes`, recognised when `{` is followed by `#`, `.` or `ident=`. Attaches to the *preceding* host in lowering: heading (end of line), image, link, span `[…]`, or the caption line. Unattached list: literal text + diagnostic `attr-no-host`. |
| Role `{name …}[content]` / `(argument)` | family 2 | inline | New construct `role`: head, then one or more bracket groups tokenised as inline content, or one parenthesised verbatim argument with balanced parens. Name not in the registry: literal text (spec). |
| Anonymous span `[text]{attrs}` | family 1 | inline | No new tokenisation: a `[…]` that is not a link and is immediately followed by an attribute list lowers to `Span`. |
| Container `::: name {attrs}` … `:::` | family 3 | block | New container construct; nesting by fence length; content is flow. Unknown name: `Div` with `attrs.name`, diagnostic `container-unknown` (spec §Div: class D error). |
| Data directive ```` ```lang node opts ```` | family 4 | lowering | Fenced code exists; lowering parses the info string with the family-4 regex and produces `CodeBlock`, `Table`, `TableConfig`, `Image` or `RawBlock`. Bare `mermaid` → `Image` (spec exception). A trailing `{…}` attribute list (C28) adds classes and an id to the options; the printer keeps the braces only then. superfences' braces-only spelling (`{ .c .annotate }`, the whole info string in one attribute list, split by CommonMark into a `lang` of `{` or `{.c`) is recombined: the first class is the language, the rest are options, `deprecated` with the reprint as its fix. A group with no class names no language: `attr-no-host`, plain listing. |
| Bare reference `@key` | sigil | inline | New construct `reference` with the X4 look-behind guard. Key grammar from the spec; `doi:` keys additionally accept `/` (spec challenge C3). |
| Bracketed reference `@[…]` | sigil | inline | Same construct; item grammar (prefix, `-`, key, suffix, `;`) parsed in lowering, not in the tokenizer. |
| Index entry `#[a][b]` | sigil | inline | New construct `define`, bracket groups are inline content. |
| Counter item `#(prefix:key)` | sigil | inline | Same construct, parenthesised argument. |
| Caption line `Kind: text {#id}` | §Caption | lowering | A paragraph whose text matches the caption regex lowers to `Caption`, attached to the adjacent float (after it canonically; before it as accepted sugar). |
| Lead-in `{lead}[…]` | §Para | inline/lowering | Role. The `paragraph.lead` feature promotes a leading short `Strong` in lowering. |
| Small caps `__x__` | X1 | lowering | Attention already tokenises `__`; lowering maps `__` strong to `SmallCaps` unless the profile disables X1. |
| `==x==`, `~x~`, `^x^`, `++k++`, `~~x~~` | §Inline | inline | New attention-like constructs with the PyMdownX rules (no space inside the delimiters). `~~` is Strikeout, `~` Subscript. |
| Definition list | §DefinitionList | block | New construct (PHP-Markdown-Extra rules). |
| Abbreviation `*[HTML]: …` | §Glossary | block | New construct, definition-only line; lowering records it in the document's abbreviation table and substitutes `Abbr` inlines. |
| `!!! type "Title"`, `??? type` | §Admonition | block | New construct; body is the following indented block. Lowers to `Admonition` (same node as `::: type`). |
| `/// name … ///` | deprecated | block | Same construct as `:::`; lowers like a container with a `deprecated` fix. `/// html | <selector>` is `pymdownx.blocks.html`: the selector's tag is the container name and its `#id`, `.class`, `[k=v]` groups the attribute list, so it is the `Div` of `<tag … markdown>` (`html_selector`). Otherwise, the pymdownx.blocks names that are not containers: `/// latex` / `typst` / `html` → `RawBlock` (fix: a `latex raw` fence); `/// caption`, `/// figure-caption`, `/// table-caption` → `Caption` after the float, the id read from the indented `attrs: {id: …}` option line, the kind from the name or (generic `caption`) from the float (fix: the `Kind: … {#id}` line). |
| Footnote `[^1]`, `[^1]:` | §Note | block+inline | GFM footnotes construct (vendored). |
| Citation sugar `[^key]`, `^[k1,k2]` | deprecated (X7) | inline | `tmark_reference`: `[^key]` whose key is a citation key with no `[^key]:` definition, and `^[…]` holding comma-separated keys, tokenise as references; lowering gives a bare `Ref` (the sugar was the short citation, spec §Cite, C51) and a `deprecated` fix (`@key`, `@[k1; k2]`; a bare DOI gets `doi:`). `^[` never opens a caret superscript. A numeric label (`[^1]`) or a defined one stays a footnote. |
| Math `$…$`, `$$…$$`, `\(…\)`, `\[…\]` | §Math | inline/block | Vendored math construct plus the two LaTeX-habit delimiters. |
| Moustache `{{ key }}` | §Front matter | lowering | Text scan in lowering; produces `Var` inline. Not inside code. |
| Comment `<!-- -->` | §Comment | block/inline | HTML construct; lowering produces `Comment` for the comment form only, `RawInline`/`RawBlock` with `format = "html"` for other HTML. |
| Critic markup | Appendix "Critic markup" | lowering | Recognised from the literal brace group the tokenizer already hands over (`tmark_ir::critic::spelling`, `lower/inline.rs`): `{++x++}`, `{--x--}`, `{~~old~>new~~}` and `{>>note<<}` are a `Span{.critic}` around `Underline` / `Strikeout` / the pair / `Comment`, `{==x==}` a plain `Highlight`. The inner text is re-parsed as inline content (`Lowerer::lower_inline_slice`). Challenge C49. |
| Escapes `\@`, `\#` | §Lexical grammar | inline | Added to the escape construct's character set. |
| Tabs `=== "Title"` | §Tabs | block | New construct: the line plus its four-space-indented body; consecutive tab lines lower to one `Div{name=tabs}` of `Div{name=tab, title}`, the same nodes as `:::: tabs` / `::: tab`. |
| Layout containers `multicolumn`, `div`, `tabs`, `tab` | §Div | block | Names of the container registry: no `container-unknown`. An HTML block whose opening tag carries `markdown` lowers to `Div{name=tag}` with `id`/`class` as attrs and its body parsed as Markdown (`markdown="span"`: inlines). |
| Foreign directive: `[TOC]`, dotted `::: a.b` | §Foreign directive | block/lowering | New block construct for the dotted line plus its indented continuation (closed by dedent); `[TOC]` is recognised from a finished paragraph in lowering. Both `RawBlock{format=markdown}`. |
| HTML other than comments | §Raw | block/inline | Kept as typed: `RawInline`/`RawBlock{format=html}`. The printer prints HTML-shaped text as typed, any other payload as `{raw html}(…)` / `html raw`. |
| Progress bar `[=45% "x"]` | §ProgressBar | inline | New construct with the PyMdownX recogniser; fraction sugar normalised to a percentage (`deprecated`). |
| Emoji `:smile:`, icons `:material-…:` | §Emoji and icon shortcodes | lowering | Text scan of `Str` against the bundled `gemoji` table: the character; the four Material icon prefixes: `Span{.icon media=web}`. |
| `{: .cls}` attribute list | §Attributes | inline | The attribute construct with an optional colon after the brace; diagnostic `deprecated`. |
| Heading `.unnumbered` / `.unlisted`, implicit id | §Header | resolution | No parse change: classes stay attrs and writers branch; the resolver registers the GitHub slug as a label when `attrs.id` is absent (hint `ref-implicit-id` on use). |
| `^^x^^` | §Inline text | inline | Attention-like construct gated on `inline.insert`; off: literal text plus lint `feature-off`. |
| TeX logos | §TeX logos | writer | No construct: writers apply `typography.tex-logos` to `Str` text. |

### Rule of thumb

Add a tokenizer construct only when the syntax must interleave with other
inline or block tokenisation. If the construct can be recognised from a
finished paragraph, an info string or a text node, recognise it in lowering.
This keeps the fork small.

## Spans, files, identity

- Offsets are byte offsets into the file's text; line/column are computed on
  demand from a `LineIndex` (`tmark-ir::span`).
- Each node gets a `NodeId` (u32, dense, per document) assigned in lowering in
  document order. Ids are stable for a given text; a re-parse of an edited
  file renumbers. The LSP maps old→new ids by span when it needs continuity.
- A node produced from an included file (TeXSmith's include pass, not this
  crate) keeps the included file's `FileId`. The parser only ever sees one
  file.

## Error handling

The parser never returns an error. Three outcomes for unrecognised input:

1. Literal text (the CommonMark way): `{foo}` alone, an unknown role name, an
   `@` inside a word.
2. Literal text plus a diagnostic when the author clearly meant a construct:
   an attribute list with no host, `#[` with an unbalanced bracket, a role head
   followed by nothing, a container never closed (closed at end of document,
   diagnostic `container-unclosed`).
3. A node plus a diagnostic for accepted-but-deprecated spellings
   (`deprecated`, with the canonical replacement in the message).

Diagnostics from the parser are syntactic only; anything that needs a
registry (unknown prefix, unresolved key) is `tmark-registry`'s or
`tmark-lint`'s job.

## Front matter

The YAML island is tokenised by the vendored construct and handed *as text*
to `tmark-ir::frontmatter::parse`, which produces a `FrontMatter` value:
known keys typed, unknown keys kept as a JSON value under `extra` (TeXSmith
validates its own `press` keys). Parsing keeps the raw text so the printer can
copy it byte for byte (spec §Round-trip). A YAML error is a diagnostic; the
document still parses with an empty front matter.

## Performance targets

- 1 MB of prose: under 50 ms on a laptop core, single-threaded.
  **Measured at milestone 3** (`reviews/06-performance.md`): the cost is
  superlinear in the file size and identical to upstream markdown-rs 1.0.0
  (no fork overhead): 12.6 ms for the 75 KB spec (167 ms/MB), 553 ms for a
  1 MB file (523 ms/MB); lowering is about 6 % of it, resolve and lint
  2 %. A chapter-sized file parses in 15–20 ms, which the language server
  lives with. The 1 MB target is unmet and is an upstream matter (the
  tokenizer's resolver passes are the suspect); the target stands.
- No allocation per character; events are a `Vec<Event>` reused across
  parses where the caller keeps the parser.
- The LSP re-parses whole files. Incremental parsing is out of scope until a
  measurement says otherwise (YAGNI).

## What the conformance fixtures cover

Every row of the table above has at least one fixture under
`spec/conformance/` showing sugar → canonical → IR (format in
`spec/conformance/README.md`). The vendored CommonMark spec tests run
unchanged, except for the documented X-class deviations, which are listed by
example number in `crates/tmark-syntax/tests/commonmark_exceptions.rs`.

## Implementation notes (milestone 1)

- The tokenizer additions live in `tmark-markdown/src/construct/tmark_*.rs`;
  `VENDORED.md` lists every touched upstream file. The lowering lives in
  `tmark-syntax/src/lower/`: `head.rs` (attribute lists, role heads,
  reference items, info strings), `inline.rs`, `block.rs`, `table.rs`.
- Bracket groups after a role head or `#` ride on the label machinery of
  the tokenizer (`LabelKind::TmarkGroup`), so their content is tokenised as
  text like a link label; `][` chains groups. A `[text]` label followed by an
  attribute list is retyped `TmarkSpan` at its `]`, so a span may wrap a
  link and a link may wrap a span or a group.
- Container, admonition and definition bodies are collected raw with source
  stops and re-parsed as documents (`OffsetMap` in `offset.rs` maps every
  span back). Consequence: link reference definitions and footnote
  definitions written *inside* a body are local to it, and a `[text][id]`
  inside a body cannot see a definition outside. Accepted; documented here
  rather than worked around.
- Equal-length fences do not nest (`:::` inside `:::` closes the outer one),
  as the spec's "nesting by fence length" implies and as Pandoc does.
- One-line display math (`$$x$$ {#eq:a}`) becomes a `MathBlock` when it is
  the whole paragraph; a `\[ … \]` paragraph likewise; `$$ … $$ {#eq:a}` with
  the attribute list on the closing fence is accepted by the tokenizer.
- Smart symbols and straight quotes are read in `text_pieces`, the scan of
  a decoded text run that already expands emoji shortcuts and progress bars
  (`lower/sugar.rs`): a symbol becomes the character in the `Str` buffer, a
  quoted phrase a `Quoted` node. Code, math, raw spans, destinations and
  attribute values are other nodes and never reach the scan, and the
  printer escapes a `"` that would pair when read back. The quote pattern
  is TeXSmith's (`extensions/quotes.py`): non-greedy, no newline inside, a
  backslash declines it; a pair that straddles inline markup is left
  literal (C45).
- An attribute list hugging an *empty* link (`[](){#id}`) takes the link:
  the pair lowers to the zero-width `Span` of `[]{#id}` with a `deprecated`
  fix, like the progress-bar case just above it in `lower_brace` (spec
  §Attributes, parity finding F5).
- Pandoc's `[@key, locator; -@key2]` is tokenised as a reference when the
  first item holds an `@`; the lowering strips the `@`s.
- The `paragraph.lead` promotion applies to a paragraph whose whole content
  is one `Strong` under 80 characters, outside a list item (parity finding
  F4: a `Strong` that merely opens a paragraph is a bold run-in, and
  `\tslead` would break the sentence in two). `{lead}[…]` at the start of a
  paragraph reaches the same field because the role lowers to a `Strong`
  first; the lowering tells the two apart by the source (`{lead}` opens the
  paragraph), and the role is taken whatever follows it and whatever the
  feature says — it is canonical syntax, not sugar. A fragment (a table
  cell, a column header) is inline content, not a paragraph: no promotion
  there, or the `Strong` would be dropped from the cell.
- Implemented by the C31–C42 wave (one fixture each): tabs (`=== "Title"`
  and `::: tab` are the admonition-shaped construct plus `group_tabs`;
  the direct body of `::: tabs` is not regrouped, `in_tabs`), the layout
  containers (`registry::CONTAINERS`), foreign directives (the dotted
  `:::` head is read by the admonition construct, guarded by
  `util::tmark::is_foreign_directive`, before the container fence; `[TOC]`
  in `compat_paragraph`), HTML as typed and `<tag markdown>`
  (`md_in_html`: the body runs over the siblings up to the `</tag>` block,
  since CommonMark ends an HTML block at a blank line), progress bars and
  shortcodes (`lower/sugar.rs`, a scan of each `Str` line; `[=…]{…}`
  arrives as a `TmarkSpan` and is taken there), the `{: ` colon
  (`looks_like_attributes` and `parse_attrs`; every host reports
  `deprecated` with the canonical list as a text fix), heading classes
  (attrs only) and `^^x^^` (literal when off; the lint hints). TeX logos
  and implicit ids are not the parser's.
- Not implemented yet, deliberately: grid tables (listing with lang
  `grid table`), wiki links, inline footnotes
  `^[…]` (the spelling is still the deprecated citation group; a `^[…]`
  that is not a key list is literal text), fancy list styles (milestone
  5), and `Space` nodes (see 03).
- The printer puts a space before a `Ref` that would otherwise follow a
  word character or one of `@/:.-` (the X4 guard), so `tutor.^[key]`
  prints as `tutor. @key`.
- Strict profile: `__x__` is bold and `~x~` is literal; the rest of the
  Appendix-PyMdownX sugar is still accepted (milestone 5 completes the
  profile).
