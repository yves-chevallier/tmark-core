# 07 — Writers and source maps

Crate: `tmark-writers`. Input: `&Document`, `&Resolved`, `&WriterOptions`.
Output: `Body { text: String, map: SourceMap, requires: Requires }`.
Spec: the backend columns of the node catalogue, §Raw passthrough, P5.

## What a body is

The part of a target document that a template wraps: for LaTeX, what goes
between `\begin{document}` and `\end{document}` (or into a fragment file to
`\input`); for Typst, the content after the template's `#show` rules; for
HTML, the `<article>` innerHTML; for CommonMark, a full Markdown document in
the chosen profile (this one *is* a document, since Markdown has no
template).

A body never contains a preamble, a package load, a font selection or a
page geometry. Instead it reports what it **requires**:

```rust
pub struct Requires {
    pub packages: BTreeSet<String>,     // LaTeX: "tcolorbox", "siunitx"; Typst: "ctheorems"
    pub fragments: BTreeSet<String>,    // TeXSmith fragment names: "ts-callouts", "ts-code"
    pub shell_escape: bool,             // minted
    pub assets: Vec<AssetRef>,          // images referenced, with the attrs that affect conversion
    pub bibliography: bool,
    pub index: BTreeSet<String>,        // index registries used
    pub counters: BTreeSet<String>,     // user series used
}
```

TeXSmith turns `Requires` into a preamble. The writer does not know what a
fragment contains; it only names the contract (`ts-callouts` provides
`tscallout`). The list of contracts is the `FRAGMENTS` table in
`tmark_ir::registry`, shared with TeXSmith's fragment loader: one
`Fragment { name, provides, packages, shell_escape, description }` per
bundled `ts-*` fragment, `provides` listing macros (`\tskeys`) and
environments (`tscode`) by their contract name. A writer that emits a
contract macro adds the row's `name` to `Requires.fragments` and the row's
`packages` to `Requires.packages`; `shell_escape` is `false` on every
bundled row (minted is decided from `code.engine`). Keystroke labels come
from `registry::KEY_LABELS` (`03-ir.md` §Closed registries), so LaTeX,
Typst and HTML spell `ctrl` the same way.

## The trait

```rust
pub trait Writer {
    fn backend(&self) -> Backend;
    fn write(&self, doc: &Document, res: &Resolved, opts: &WriterOptions) -> Body;
}
```

Four implementations, one module each: `commonmark`, `html`, `latex`,
`typst`. Inside a module, one function per node (`fn header(&mut self, n:
&Header)`), dispatch by `match`. Shared helpers (escaping tables, attribute
formatting, label formatting from counter `ref` templates) live in
`common.rs`. No inheritance between writers: LaTeX and Typst share helpers,
not a base class.

`WriterOptions` is a plain struct: `media: Print | Web`, `profile` (for
CommonMark), `code: { engine, inline_breaks }`, `refs.textual` templates per
medium, `lang`, `numbering: Backend | Tmark` per series, and
`citations.narrative` (an override of the front matter's feature, the
precedence of `lang`). Anything requiring knowledge of a template is not
an option; it is a fragment contract.

## Mapping rules that are not obvious

- **Media.** A node with `media=print` is skipped by the HTML writer;
  `media=web` by LaTeX and Typst. Zero width, whitespace collapse as for
  comments.
- **Raw.** `RawInline`/`RawBlock` of another format are dropped silently.
- **References.** `Label` resolutions render the counter's `ref` template
  (`{name} {number}`) as a hyperlink where the backend numbers, and as
  `\ref`/`@label` where the backend numbers; unresolved ones render
  `[?key]` literally in every backend. Textual references (`[text](#id)`)
  apply `refs.textual.print` or `.web`; `{page}` is expanded by the backend
  (`\pageref`), never here.
- **Label words.** The `{name}` of a predeclared series is the word
  `tmark_ir::registry::PREFIX_NAMES` holds for the language
  (`refs::label_word`), which is `WriterOptions::lang`, else the
  resolution's — itself `ResolveOptions::lang` else the front matter's
  `lang`, so a site-wide language keeps beating a page's — else the front
  matter's, else English (`refs::language`). A `declare.counters` `name`
  is kept as written — on a predeclared prefix too, and whatever language
  the writer renders — by re-localising only a word the resolution had
  left at the registry's default. The same word labels HTML captions
  (`refs::caption_word`) and the web lowering's `[Figure 3](#id)` link
  text. The words follow babel's `\figurename`, `\tablename` and friends,
  which is what keeps a French reference (`Table 1`) agreeing with the
  caption babel prints; LaTeX's own `\caption` numbering stays babel's
  business and the writer never translates it.
- **Citations.** A bare `@key` is the short citation the style gives
  (`\cite{key}`, `#cite(<key>)`), the same as `@[key]`; `\textcite` /
  `form: "prose"` are written for a `+key` item and, under the feature
  `citations.narrative`, for a bare key (`refs::narrative`:
  `WriterOptions::citations.narrative`, else the front matter's feature,
  else the registry default — the precedence of `lang` without the
  resolution level, which does not read the switch). Locators as
  `\cite[pre][post]{k}` / `supplement:`, `-key` as `\citeyear` /
  `form: "year"`. HTML: `<a>` plus a bibliography list rendered from the
  entry fields with a minimal built-in style (author-year), one
  parenthetical form that reads neither switch nor flag; CSL is
  TeXSmith's. Spec §Cite, C51.
- **Zero-width nodes.** Comments, index entries, anchors, counter
  definitions that print nothing, asides: the writer collapses surrounding
  whitespace and removes it before punctuation (spec §Attributes). This is a
  post-pass over the inline sequence, shared by all writers.
- **Language.** `lang` on a span or the document: `\foreignlanguage`,
  `#text(lang:)`, `lang=`. Typographic spacing (French `;` `:`) is applied
  by the backend engines (`babel`, Typst), never by writers.
- **Tables.** The writer computes the column preamble from the semantic
  model (`X` columns → `tabularx`, spans → `multirow`/`multicolumn`, `long`
  → `longtable`); decimal alignment when the feature is on. A header cell
  is a cell: the writers render `Column::title` (the header as inline
  Markdown) when it is set and escape `Column::name` otherwise, so a link
  or a code span in a header survives (fixture `table-header-markup`).
- **Code.** `pygments` engine is TeXSmith's (it needs Python); the LaTeX
  writer emits an engine-neutral `\begin{tscode}[lang, title, linenums,
  hl_lines]` contract from the `ts-code` fragment and lets the fragment
  choose the engine. The HTML writer emits `<pre><code class="language-x">`.
- **Math.** Passed through verbatim to LaTeX; the Typst writer converts
  LaTeX math with a small translator (milestone 4; until then `raw` with a
  diagnostic).
- **Includes.** An `Include` node that survived to the writer (TeXSmith did
  not splice it) renders as `\input{stem}` / `#include "stem"` and reports
  the path in `Requires.assets`.

## Source maps

```rust
pub struct SourceMap { pub entries: Vec<(Range<u32>, NodeId)> }   // output byte range → node
```

Every writer pushes an entry when it starts and ends a block node and for
inline nodes that matter to navigation (references, images, captions). The
map plus `Document` spans give output range → source span. Consumers:

- TeXSmith's LaTeX build converts it to `%` line markers or to a SyncTeX
  sidecar so a PDF viewer can jump to the `.md` line.
- The Typst path uses `typst-ide` span→source, then this map (milestone 4).
- The HTML writer additionally emits `data-src="file:start-end"` on block
  elements so a web preview can scroll-sync.

## Tests

- Snapshot per fixture and per backend (`insta`).
- A "requires" assertion per fixture: the set of fragments and packages a
  construct needs is part of its conformance.
- Round-trip of the CommonMark writer through the parser: the `canonical`
  profile must be the fixed point of `04-printer.md` (the CommonMark writer
  *is* the printer; `tmark-writers::commonmark` re-exports `tmark-fmt`).

## Web lowering

Module `mkdocs.rs`, entry point `lower_web(text, doc, res, loader, opts)
-> Lowered { text, diagnostics, bibliography }`, re-exported by the facade.
Specification: TeXSmith `specs/migration/web-profile.md` (§Recommendation,
the per-construct table). A MkDocs page is rendered by Python-Markdown
with Material's extension set, which knows nothing of `@fig:x`,
`#(fw:x)`, `::: figure` or `Figure:` lines; the plugin hands the page to
`lower_web` in `on_page_markdown`, resolved with `Numbering::All` and the
site's `book`, and gets back Markdown Material renders.

It is not a writer: there is no body, no `Requires`, no source map. The
lowering walks the tree and emits one `NodeEdit` per construct of the
table, applied through `tmark_fmt::edit_many`, so **every byte outside a
recognised construct is untouched** (mkdocstrings, tabs, icons, `!!!`
callouts, custom fences, `{{ macros }}` pass through; critic markup is a
construct since C49 and is re-emitted in its own spelling, which
`pymdownx.critic` renders). A construct
kept as written still gets the splices of its children (a `@` reference
inside an unclosed `::: pkg.mod`, a `#(fw:x)` in a pipe-table cell). The
replacement text is:

- the `Mkdocs` profile's spelling (`tmark_fmt::print_node_with`) for a
  PyMdownX construct (`==x==`, `++ctrl+s++`, `` `#!py x` ``), and the
  HTML element when the profile had to keep the role;
- an HTML wrapper the standard extensions read (`md_in_html`,
  `attr_list`): `<figure markdown="span">` around an image as written and
  its `<figcaption>` with a `ts-caption-label`, `<figure markdown="1"
  class="ts-table">` around a pipe table as written, `<table
  data-ts-table="1" markdown="block">` with `<td markdown="span">` cells
  for a `yaml table` (verified: `md_in_html` reads cells only when
  `table`, `thead`/`tbody` and `tr` carry `markdown="block"`, which
  settles web-profile open question 3), `<div class="ts-equation"
  markdown="1">` around display math, `<div class="admonition …"
  markdown="1">` or `<details>` for a numbered or labelled callout,
  `<aside class="ts-aside" markdown="1">` for a block aside and `<span
  class="ts-aside">` for an inline one (an `<aside>` at the start of a
  paragraph would open an HTML block), `<span class="ts-counter" id=…
  data-counter data-key>`, `<span class="ts-index" data-tag…>`,
  `<span class="ts-smallcaps">`, `<u>`, `<span id class lang data-*>`,
  `<abbr title>` for a glossary term — but an anchor on its own
  (`[]{#id}`) keeps a *Markdown* spelling, `[](){#id}`: raw HTML is
  stashed out of Python-Markdown's element tree and `mkdocs-autorefs`
  registers the anchors it finds in that tree, so a `<span id>` would
  be an id no page of the site could point at;
- Markdown for references: `[FW-10](#fw:x)`, `[Figure 3](#fig:x)`,
  `[Figures 3 and 4](#fig:a)` for a group of one series, `[title](#sec:x)`
  (or `[Section 2]` with `sections: Number`), `[label](location)` for a
  `Resolution::Sibling`, `[?key]` unresolved, `[doi:…](https://doi.org/…)`;
  citations as `(<a class="ts-cite" href="#ref-key">Author Year</a>)` in
  the HTML writer's author-year style (`common::refs::author_year`,
  `html::bibliography_entry`) with a `## References` list appended to the
  page and returned alone in `Lowered.bibliography`, or Pandoc `[@key]`
  with `citations: Passthrough` — `@key` (Pandoc's narrative form) for a
  `+key` item and, under the front matter's `citations.narrative`, for a
  bare key; `WebOptions` has no override. The reference-style
  `[text][id]` (spec §Ref) is the one reference the lowering leaves
  byte for byte: it is already Markdown a site resolves, and
  `mkdocs-autorefs` knows which page holds the anchor, which this
  page does not;
- `!!! type cls "title"` (`???`, `???+` when `collapsed`) for a `:::`
  callout the `!!!` line can carry, its body re-indented by `Out`;
- the lowered text of the included file for `{include}(f)` (its
  `Document` from `Resolved.included`, its text from the `Loader`, its own
  labels and numbers from the same `Resolved`); `--8<--` stays for
  `snippets`;
- the empty string for `media=print`, raw LaTeX/Typst, a consumed caption
  line or `yaml table-config`; a removed inline takes one adjacent space
  with it (the zero-width collapse). `media=web` unwraps.

Numbers are the resolution's (`html::number_labels`, shared with the HTML
writer: a document-order count stands in for a series the resolution did
not number), sub-figures take the figure's number and a letter (`3b`).
The label word is the registry's for the language (§Mapping rules, "Label
words"): `[Équation 1](#eq:m)` under `lang: fr`. The lowering's own words
(`and`, `References`) follow that same language, so a page says `and` or
`und` where its labels say `Figure` or `Abbildung`.

Nesting: a whole-block replacement is re-indented for the line it is
spliced on (the text before the block on its line, list markers turned
into spaces, `>` kept), and a block kept as written inside a lowered
wrapper has that prefix removed from its continuation lines before `Out`
adds the wrapper's own. Inline spans that are not the text's (a title
parsed from an attribute value, a `yaml table` cell) are detected — every
`Str` must read back from its span — and printed instead of sliced.

Deviations from the per-construct table, taken here: the inline aside is
a `<span class="ts-aside">` (see above); the `data-ts-table` attribute
carries the value `1` (`md_in_html` re-serialises a bare attribute as
`data-ts-table="data-ts-table"`); the figure wrapper carries the id and
the image is reprinted without it, so a page has one element per id; a
captioned float without a label stays unnumbered (design 06 §Site-wide
resolution leaves the choice to the lowering: a synthetic id would drift
from the print numbering). Not lowered: `media=web` on a block other than
a heading (the attribute reaches `attr_list`, harmless); the blank lines
left where a caption or configuration was emptied stay (Markdown ignores
them). `Lowered.diagnostics` carries only what the lowering itself found
(an include the resolution did not parse); `ref-unresolved` and
`include-missing` are the resolution's.

Tests: `tests/web.rs` snapshots every conformance fixture (`web__*`), a
document exercising every row of the table with one assertion per row,
the mkdocstrings case (`spec/conformance/container-dotted.md`: an
unclosed `::: pkg.mod` whose bytes stay while the prose after it is
lowered; its `container-unknown` and `container-unclosed` are `info` for
a dotted name), sibling labels, `sections: Number`, `citations:
Passthrough`, a French `and`, a custom `css_prefix`, and that plain
CommonMark with `!!!`, tabs and macros comes back byte for byte.

## Implementation notes (milestone 4)

State of `crates/tmark-writers` at the end of the first M4 writers pass
(branch `wt/writers`), for whoever continues. Specification of the
behaviour: TeXSmith's `specs/migration/writers-and-passes.md` (§1 layout
and options, §2 the LaTeX catalogue, §4 Typst math), the contract names of
`specs/migration/fragment-contracts.md` (§1, §3, §5) and `decisions.md`
(X1–X4). Read those before this section.

### What exists

- `lib.rs`: `Writer`, `Backend` (`html`, `latex`, `typst`), `Media`,
  `WriterOptions { media, lang, code {engine, inline_plain,
  inline_breaks}, latex {legacy_accents}, headings {base_level, numbered},
  refs {textual_print, textual_web}, numbering, typst {math}, citations
  {narrative}, source_map }`, `Body { text, map, requires }`, `Requires`
  (design fields plus
  `citations`, `acronyms`), `SourceMap` (serialises as `[[start, end,
  node], …]`), `write(doc, res, backend, opts)`, `writer(backend)`. All
  serde-derived: `tmark-py` and `tmark write --map` hand the JSON over.
- `common/`: `out.rs` (the printer's `Out` plus `begin`/`end` map entries,
  `blank_line` never doubles), `zero.rs` (the zero-width collapse, shared),
  `media.rs`, `refs.rs` (template rendering, `[?key]`, label word
  capitalisation, `Resolved` lookups), `text.rs` (dash/quote/script tables,
  `key_label` over `tmark_ir::registry::KEY_LABELS`, `slugify`,
  `acronym_key`, ASCII fold), `abbr.rs` (whole-word acronym substitution
  in `Str`, see below). Fragment names are written as the registry
  spells them; `Requires::fragment` asserts the row exists in debug
  builds, and `Requires::close` (called once per body) merges the
  `packages` and `shell_escape` of every named row of
  `tmark_ir::registry::FRAGMENTS` into `Requires.packages`.
- `assets/texsmith.typ`, exposed as `tmark_writers::TEXSMITH_TYP`: a
  default definition for every `#ts-…` function the Typst writer emits
  (`ts-lead`, `ts-divider`, `ts-rule`, `ts-epigraph`, `ts-aside`, `ts-div`,
  `ts-callout`, `ts-code`, `ts-task`, `ts-keys`, `ts-gls`, `ts-acr`,
  `ts-index`, `ts-script`, `ts-emoji`, `ts-page`). TeXSmith writes it next
  to the `.typ`; a body compiles with `#import "texsmith.typ": *` (plus
  the `mitex` import when `Requires.packages` names it, and `#set
  math.equation(numbering: …)` when `ts-equations` is required) in front
  of it. Checked with typst 0.15.1 on the examples: 46 of 52 bodies
  compile; the six that do not fail on remote or unconverted images
  (`book`, `markdown/features`, `diagrams`, `mermaid`: the assets pass),
  on a `#cite` without a `#bibliography` (`paper`: the template) and on
  `mitex` rejecting `\imath` (`math`: pre-existing, `baseline.md`).
- `html/`: the `<article>` innerHTML, one function per node, `data-src`
  only when `source_map` is on, labels numbered in document order (the
  web has no backend counter; sub-figure images take no number),
  footnotes and an author-year bibliography appended, `<abbr>` for
  acronyms. 564 of the 652 CommonMark examples render byte for byte
  (`tests/commonmark.rs`, golden list asserted).
- `latex/`: `mod.rs` (blocks), `inline.rs`, `figure.rs`, `table.rs`,
  `escape.rs` (the `escaper.py` tables and order; no math scan of `Str`).
- `typst/`: `mod.rs`, `inline.rs`, `table.rs`, `math.rs` (mitex),
  `escape.rs`.
- Facade `tmark::write` and re-exports; CLI `tmark write FILE --to
  latex|typst|html [--media print|web] [--map]` (`--map` prints the whole
  `Body` as JSON).
- Tests: `tests/fixtures.rs` snapshots every `spec/conformance` fixture
  per backend, text plus `Requires`, under `tests/snapshots/`
  (`INSTA_UPDATE=always cargo test -p tmark-writers` regenerates; read
  the diff). Unit tests for the escapers, tables layout, widths, math,
  zero-width, slugs.

### Construct coverage

| Construct | LaTeX | Typst | HTML |
| --- | --- | --- | --- |
| Paragraphs, inline text, `\tslead` | done | done | done |
| Headings (levels, `*`, `\label`, slug) | done | done | done (id only when explicit) |
| Lists, tasks (`tstasklist`), definition lists | done | done | done (tightness approximated) |
| Code (`tscode`, `\tscodeinline`, `Div{code}` X3) | done | done (`#ts-code`, `#raw`) | done |
| Pipe and model tables | done | done | done |
| Figures, sub-figures, captions, `\captionof` in a box | done | done | done |
| Footnotes | done (`\par` joins paragraphs) | done | done |
| References, citations, glossary, DOI, external | done | done | done |
| Index (`\tsindex`) | done | done (`#ts-index`, no-op in `texsmith.typ`) | dropped (zero-width) |
| Acronyms (`\tsacr`, key rule, `Requires.acronyms`) | done | done | done |
| Counters | done | done | done |
| Asides, admonitions, keystrokes | done | done | done |
| Math (verbatim; `equation` + `\label` for an anchored block) | done | done (mitex) | done (MathJax delimiters) |
| Links, anchors, textual template | done | done (`{page}` → `#ts-page`, not in the contract yet) | done |
| Raw, comments, zero-width collapse, media | done | done | done |
| `Div` dispatch (`epigraph`, `code`, `tsdiv`) | done | done | done |
| `Include`, `\tsdivider` (top level) / `\tsrule` (in a container) | done | done | done (`<hr>` / `<hr class="rule">`) |
| Scripts, emoji (`\tsscript`, `\tsemoji`) | done, untested on a corpus | done | plain spans |
| Progress bars (`\tsprogress[thin]{0.45}{label}`) | done | done (`#ts-progress`) | done (`<progress>` in a `.progress` span) |
| `multicolumn`/`div` containers | via `tsdiv` | via `#ts-div` | `<div class="multicolumn">`, `<div class="…">` |
| Tabs (`tsdiv{tab}[title=…]` in sequence) | done | done | done (`tabbed-set` / `tabbed-labels` / `tabbed-block`) |
| TeX logos (`typography.tex-logos`) | done (`\LaTeX{}`, `\tslogo{…}`) | done (`#ts-logo`) | done (`<span class="tex-logo">`) |
| `.unnumbered` / `.unlisted` headings | done (`\section*`, `\addcontentsline` kept for unnumbered only) | done (`numbering: none`, `outlined: false`) | classes |
| Implicit heading ids | `\label` only when referenced | same | none (the site slugs) |
| Foreign directives, icon spans | dropped | dropped | dropped / `<span class="icon">` |

### Decisions taken here (not in the notes)

- Blocks are separated by exactly one blank line in every backend; the
  legacy `\n` join with per-emitter trailing newlines is not reproduced
  (parity normalisation collapses blank runs anyway).
- `\ref` and `\hyperref` use the label *as defined* (`Labels::get(key).id`),
  because TMark matches keys case-insensitively and LaTeX does not.
- A backend-numbered reference renders the `ref` template with
  `{number}` → `\ref{key}` and a `~` between name and number
  (`Figure~\ref{fig:x}`); a TMark-numbered one renders the text inside
  `\hyperref[key]{…}`. A capitalised prefix capitalises the label word;
  a lower-case one keeps the declared word.
- Citations: `\cite{k1,k2}` for consecutive plain items of one form
  (locators as `\cite[pre][post]{k}`), `\textcite` for a `+key` item and
  for a bare `@key` under `citations.narrative` (then `ts-bibliography`
  in `Requires.fragments` for the fallback), `-key` → `\citeyear`.
  Typst: `#cite(<k>)`, `form: "prose"` and `form: "year"` on the same
  rule. Before C51 a bare key was `\textcite` unconditionally, which
  turned the migrated `[^key]` ("[3]") into a narrative citation.
- An `Image` alone in a paragraph is a figure; its alt is the caption when
  no caption line follows (legacy `render_images`), and the short caption
  when one does and the alt is not longer. Inside `tscallout`/`tscode`
  the figure is `center` + `\captionof{figure}`.
- `::: figure` with several images → `subfigure` (package `subcaption`),
  `cols` per row; Typst uses a `grid`.
- The acronym substitution lives in the writers (`common/abbr.rs`): the
  parser fills `Document.abbreviations` but emits no `Abbr` node, so
  `Str` runs are split at whole-word keys (`*[X]:` and
  `press.declare.acronyms`). When the parser emits `Abbr`, the helper
  finds nothing and can be deleted. `examples/abbr` and
  `examples/glossary` render `\tsacr` with this.
- An `Image` with the `icon` class (the emoji pass in artifact mode) is
  inline in every backend, never a figure: `\tsicon{path}`
  (ts-typesetting), `#box(image(..), height: 1em)`, `<img class="icon">`.
  A converted diagram (`Image` with `generate=<lang>` and a `src`) is
  wrapped in `\adjustbox{max width=\textwidth}` (`media.py:213`,
  package `adjustbox`); `tests/images.rs` covers both.
- A `Listing: …` caption line on a fence is passed to `tscode` as
  `caption={…}` (and to `#ts-code` as `caption: […]`); the key is in
  fragment-contracts.md §5. The Typst writer writes the label after the
  call (`#ts-code(…)[…] <lst:x>`), never as an argument, so it attaches
  to the figure the function returns.
- `ts-equations` is a row of `FRAGMENTS` with no macro and no package:
  the Typst writer names it when an equation label was emitted so the
  template numbers equations (writers-and-passes.md §4).
- `WriterOptions.latex.legacy_accents` is accepted and ignored (the
  `pylatexenc` path has no Rust twin; engines read UTF-8).
- The Greek subscript entries map to `\beta` etc. as the legacy table did
  (`escaper.py:223`, latent bug reproduced for parity; fix both sides).
- HTML `id`s on headings only when written; auto-slugs are LaTeX/Typst
  labels only (open question b: `python-slugify` on the plain text).
- A `[` that opens a LaTeX table cell, a column name or a text run right
  after a hard line break is brace-protected (`{[}`, `latex/escape.rs`
  `guard_bracket`; fixture `table-cell-bracket`, `tests/brackets.rs`).
  Every command that can stand there takes an optional argument and looks
  for it with `\@ifnextchar[`, which skips spaces *and* line ends: the
  row break `\\[⟨dimen⟩]`, booktabs' `\toprule`/`\midrule`/`\bottomrule`
  `[⟨wd⟩]`, `\cmidrule[⟨wd⟩]`, `\addlinespace[⟨dimen⟩]`. An unresolved
  reference-style link (`[text][id]`, spec §Ref) or a literal `[note]`
  first in a cell was read as that argument and the run died on `Missing
  number, treated as zero`. The guard goes on the **content** because a
  guard on the command side is wrong twice over: `\tabularnewline` takes
  the same optional argument as `\\`, and `\\{}` puts a group between the
  row break and the next cell, which makes `\multicolumn` no longer the
  first token of its cell — exactly what a spanning cell of a `yaml
  table` opens with. Outside a table the guard is `{}` pushed after the
  `\\` instead, since a text run there is not a cell. The Typst writer
  escapes `[` unconditionally in markup (`typst/escape.rs::MARKUP`) and
  its `\` line break takes no argument, so it has no twin of this bug.
- `Requires.packages` lists what *structural* output needs (`ulem`,
  `csquotes`, `booktabs`, `tabularx`, `longtable`, `multirow`, `float`,
  `graphicx`, `caption`, `subcaption`, `enumitem`, `babel`, `glossaries`,
  `imakeidx`); contract packages come from the `FRAGMENTS` table on
  TeXSmith's side.

### What is next

1. Run the TeXSmith parity harness (`scripts/parity.py --reader tmark`)
   and triage: expected differences are `\item{}` → `\item`, the
   zero-width spacing, `\clearpage` → `\tsdivider`, `\index` →
   `\tsindex`, `\acrshort` → `\tsacr`, `\marginnote` → `\tsaside`,
   `\keystroke` → `\tskeys`, `callout` → `tscallout`, `code` → `tscode`,
   blank-line runs, heading slugs of headings containing inline markup.
3. `Requires.assets` for generated images (`Image` with empty `src` and
   `generate=`): the writers emit nothing for them today; decide with the
   assets pass whether the writer should still list them.
4. `texsmith.typ`: `ts-acr` and `ts-gls` show the key/term (no glossary
   table yet), `ts-index` is a no-op, `ts-aside` places the note in the
   margin with a fixed offset; a template restyles them. Nothing in this
   repository runs `typst` in CI; the check above was manual.
5. Typst preview in the LSP (ADR 0005) and the source-map consumers
   (`%` line markers, SyncTeX sidecar) are untouched.
6. The CommonMark failures are IR-level: list tightness is not in the IR
   (an `Item`/`List` `tight` flag would fix ~15 examples), URL
   percent-encoding and entity decoding, raw HTML blocks dropped, tabs
   in indented code. None is a writer bug.
7. `examples/tables` now renders through the model path (X9 merged);
   a fence the parser rejected keeps `Table.source`, which the writers
   ignore (the best-effort model is written).

### Pitfalls

- `Out::scratch()` for anything rendered aside (captions, titles, cell
  text): `render_inlines` swaps the buffer and loses map entries inside;
  push the enclosing node's `begin`/`end` at the outer level.
- `blank_line` is idempotent; `ensure_newline` is what a nested list
  wants after `\item text`.
- `zero::collapse` clones the inline sequence; it is applied at every
  `inlines()` call, so nested content is collapsed at its own level.
- `in_box` (LaTeX) is what decides `\captionof`; `in_cell` decides
  `\newline` for a hard break.
- `container` (all three writers) counts how deep the writer is inside a
  container — block quote, callout, figure, `:::` div or tab, list item,
  table cell, aside, footnote: anything that is not the document's
  top-level block sequence. Every recursion into contained blocks goes
  through `contained(|w| …)`, which bumps the counter and restores it,
  so a new container is one wrapped call and never a forgotten
  decrement. A `HorizontalRule` reads it: `\tsdivider` / `#ts-divider()`
  / `<hr>` at depth 0, `\tsrule` / `#ts-rule()` / `<hr class="rule">`
  deeper (spec §HorizontalRule, challenge C48). Typst *rejects* a page
  break inside a container, so this is a hard requirement there, not a
  matter of taste.
- `render_blocks_inline` joins paragraphs with `\par ` (LaTeX) and
  `#parbreak()` (Typst); a footnote with a list inside renders the list
  environment inline, which LaTeX accepts.
- The fixture snapshot names are `<fixture>@<backend>`; a new fixture
  needs `INSTA_UPDATE=always` once, then review the three new files.

### Decisions of the C31–C42 wave

- `Div{tabs}` is transparent on the paged backends: its `tab` children
  render in sequence through the generic `tsdiv` / `#ts-div` contract,
  `title=` forwarded as a key (spec §Tabs: "the title in bold, then the
  content" is the fragment's default). The HTML writer mirrors Material's
  `tabbed` markup without the radio inputs.
- TeX logos are a split of `Str` runs like the acronym substitution
  (`common/logos.rs`), so code, math, raw text, destinations and attribute
  values never see them. `\TeX{}`, `\LaTeX{}` and `\LaTeXe{}` are the
  kernel's; the other words are `\tslogo{Name}`, a new macro of the
  `ts-typesetting` contract (`FRAGMENTS`), `#ts-logo("Name")` in Typst.
  The feature is read from the document's front matter, not from
  `WriterOptions`.
- A heading without `{#id}` gets a `\label` / `<label>` only when a
  reference of the document resolves to its implicit id
  (`common::refs::referenced_implicit_id`); the earlier unconditional
  `\label{slug}` is gone, so unreferenced headings carry no label. HTML
  keeps writing ids only when explicit: the site's slugifier owns the rest.
- `ProgressBar` values are fractions with at most four decimals
  (`text::trim_float`); the label defaults to the percentage.
- The Typst escaper (`typst/escape.rs::markup`) escapes more than
  TeXSmith's fixed `_ESCAPE_CHARS` table, because a plain `Str` can contain
  bytes that table never had to cover: `~` unconditionally (a non-breaking
  space in markup); a `/` immediately before another `/` or `*`, anywhere
  in the text, because `//`/`/*` open a comment at any position and would
  otherwise silently eat the rest of the line or a block; and `=`, `+`,
  `-` or `/` at the start of the text (position 0, and again right after
  an embedded `\n`) when they would be read as a heading, a list, an enum
  or a term-list marker — Typst's "start of line" also includes the start
  of any markup content block (`#footnote[= x]`, a table cell, a link's
  content), not only a paragraph or a soft/hard break, so escaping
  position 0 unconditionally is a safe superset rather than an attempt at
  exact context tracking. `=` is escaped whenever leading, since a heading
  marker is a run of one or more `=`; `+`, `-` and `/` only when followed
  by a space, so a leading `-5` keeps Typst's own minus-sign substitution
  and a leading `a--b` keeps its en-dash conversion. Verified against
  typst 0.15.1 (fixture `escape-typst-structural`).
- A Typst label (`typst/escape.rs::label`) keeps the characters Typst's
  lexer reads in one, XID_Continue plus `_ - : .`, so an accented or
  non-Latin id (`café-au-lait`, `日本語-見出し`) is a label as written;
  any other character becomes `-` and the label takes a six-hex-digit
  hash of the original id as a suffix, so two ids that differ only there
  never collide. LaTeX writes the id unchanged (`\label{café-au-lait}`,
  fine under UTF-8 engines), HTML uses it as the fragment. Verified
  against typst 0.15.1 (fixture `heading-implicit-id-unicode`).
