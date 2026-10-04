# 06 — Registries and resolution

Crate: `tmark-registry`. Input: `&Document`, `&dyn Loader`. Output:
`Resolved` plus diagnostics. Spec: §Registries, §Anchors, references,
citations, §Cross-document references, §Counters, §Bibliography, §Glossary and
acronyms, §Index.

## The Loader seam

```rust
pub trait Loader {
    /// Text of a path relative to `from`; `None` if it does not exist.
    fn load(&self, from: &Path, rel: &str) -> Option<String>;
}
```

The only I/O abstraction in the core. `MemoryLoader` (a map, for tests and
WASM) and `FsLoader` (CLI, LSP, PyO3). No caching in the trait: callers cache.
No async: the LSP calls it on its worker thread.

What goes through it: included TMark files (to collect their labels and
counter items for numbering), `.bib` files, `refs.json` inventories, glossary
files if the front matter points to one. What never goes through it: DOI
resolution, image files, executed fences. Those are TeXSmith passes that run
*before* resolution and hand back a document with the results spliced in.

## Registries

```rust
pub struct Resolved {
    pub counters: Counters,       // prefix → declaration + allocated numbers
    pub labels: Labels,           // id → (NodeId, prefix, number, kind)
    pub bibliography: Bib,        // key → entry (fields as parsed; no formatting here)
    pub glossary: Glossary,       // term → entry
    pub index: IndexTable,        // registry name → entries with NodeIds
    pub crossrefs: CrossRefs,     // alias → inventory
    pub refs: Vec<RefResolution>, // for every Ref node: what it resolved to, or why not
    pub diagnostics: Vec<Diagnostic>,
}
```

Each registry is a plain struct with a `HashMap`; each is built by one
function in its own module. `Resolved` is immutable once built.

## Resolution algorithm

Spec §Registries (lookup): a `@key` whose head is a declared prefix is a
label or counter reference; `gls:` is a glossary reference; `doi:` a DOI
citation; anything else a bibliography key. A key present in two registries
is `ref-ambiguous`.

1. **Declare.** Merge the predeclared prefixes (`tmark_ir::registry::PREFIXES`)
   with `declare.counters` from the front matter; a user entry overrides
   fields of a predeclared one but cannot shadow a role name. Same for
   admonition types (theorem kinds carry a counter).
2. **Collect definitions**, in document order, across the document and its
   includes (through `Loader`, depth-first, cycle-guarded): every `#id`
   attribute on a host, every `CounterItem`, every `Caption` with an id, every
   `IndexEntry`, every footnote definition. The host decides the counter
   (heading → section, `Table:` → `tbl`, image → `fig`, code block → `lst`,
   `MathBlock` → `eq`, theorem admonition → its declared counter); an explicit
   prefix must agree with the host or `prefix-host-mismatch` is raised.
   Duplicates raise `label-duplicate` with the other span as `related`.
   An image inside a `::: figure` made of image paragraphs is a
   *sub-figure* (spec §Image, Figure): host `Subfigure`, and
   `Label::subfigure` records the container's label id and the letter.
   Which images those are needs the block structure, which the flat walk
   does not have, so the collector maps them per file before walking
   (`collect::figure_images`, the same list the writers lay out and letter).
3. **Allocate numbers** for TeXSmith-numbered series only (user counters,
   `thm` when TeXSmith numbers it). Backend-numbered series (`sec`, `fig`,
   `tbl`, `lst`, `eq`) get *no* number here: the backend numbers them. The
   registry records the kind and the order, which is enough for labels,
   diagnostics, and the `mkdocs` companion that numbers on the web.
   Multi-document numbering (a series continuing across files of a book) is
   an input: `ResolveOptions { start: HashMap<Prefix, u32> }`, set by
   TeXSmith from the previous document's `next_start`. A medium with no
   backend to number the rest asks for it with `numbering: All`
   (§Site-wide resolution below). A sub-figure is skipped: it takes no
   number of the series, and `Resolved::formatted` composes its
   container's number with its letter (`2a`), so `next_start` counts one
   figure per container.
4. **Load sources.** `.bib` files named in `sources.bibliography` (parsed
   with the `biblatex` crate; entries kept as fields, no CSL here), inline
   pybtex-shaped entries, DOI shorthands recorded as *pending* (TeXSmith
   fetches them). `sources.crossrefs` inventories (`refs.json`, format below).
   Glossary and acronym declarations.
5. **Resolve every `Ref`** by the lookup rule, honouring the X4 guard already
   applied by the parser and the capitalised-prefix convention (`@Fig:x`).
   Result per ref: `Label { node, prefix, number? }`, `Citation { key }`,
   `Glossary { term }`, `Doi { doi }`, `External { alias, label, page? }`, or
   `Unresolved { reason }`. Unresolved refs render visibly as `[?key]`
   downstream (spec P4); the registry only records the fact.
6. **Footnote-versus-citation shadowing** (deprecated `[^key]` citations): a
   real footnote definition wins; a `[^key]` without definition whose key is
   in the bibliography becomes a citation with a `deprecated` diagnostic.

## Inventory format (`refs.json`)

Published by TeXSmith after a build (page numbers need the backend), read
here for cross-document references:

```json
{ "document": { "id": "RHE-423", "title": "Firmware review", "source": "review.md", "hash": "…" },
  "refs": { "fw:pas-de-temps": { "label": "FW-10", "page": 14, "kind": "counter", "prefix": "fw" } } }
```

`tmark-registry` owns the *reader* and the type; TeXSmith owns the writer,
generated from the same `schemars` schema. A stale `hash` raises
`crossref-inventory-stale`.

`refs` is required: an inventory without it is `crossref-inventory-missing`
("not valid: missing field `refs`") rather than an empty inventory, since
serde ignores unknown keys and a map published under another name would
otherwise unresolve every citation silently. `External.label` is the label a
citing document prints: `document.id` joined to the entry's label with a
hyphen (`RHE-423-FW-10`), or the bare label when the target has no `id`.

## Site-wide resolution

The web profile (TeXSmith `specs/migration/web-profile.md`) renders a book
page by page with no backend to number `fig`, `tbl`, `lst`, `eq`, `sec` or
the theorem kinds, and needs a reference on one page to reach a label on
another. Both are resolution inputs, so that the registry stays the one
numbering authority for print and web (decision D4 of the migration).

### Numbering every series

`ResolveOptions::numbering` is `Backend` (default: the TeXSmith-numbered
series only, as step 3 above) or `All`. Under `All` every predeclared
series with a scope (`part`, `chap`, `sec`, `app`, `fig`, `tbl`, `lst`,
`eq`, `thm`, `note`; `gls` and `doi` number nothing) becomes
tmark-numbered for this resolution: `Counter::tmark_numbered` is true,
its labels get `number`s in document order, `Counter::label` and
`Resolution::Label.number` format them and `Resolved::next_start` lists
the series, so a site chains `start` from page to page exactly as it does
for user counters. The `format`, `start` and `ref` fields of a
predeclared entry apply (`fig: {format: "F{n}"}` in the front matter).

Scope under `All` is always `document`, continuous across the chain:
`Scope::Chapter` and `Scope::Section` never reset here. The web has no
chapters (web-profile open question 1); a `chapter` mode keyed on the nav
would couple numbers to the nav shape and waits for a real site. Print
keeps the backend's scoped numbering: `All` is never passed on the LaTeX
path, so `Figure 3.2` in the PDF and `Figure 12` on the site are the same
label with two spellings, which is the accepted cost.

What is counted: labels, that is items with an id, minus the sub-figures
of a `::: figure`, which number under their container (step 3 above): a
page with a plain figure and a container of two labelled images counts two
figures, and `@fig:left` resolves to `2a`. A captioned figure
without `{#fig:x}` is not a label and takes no number, whereas LaTeX would
number it. The web lowering either gives such floats a synthetic id or
leaves them unnumbered; it is its decision, not the registry's.

`Counter::reference_text(key)` renders the `ref` template
(`"{name} {number}"` → `Figure 3`); `ResolveOptions::lang` picks the label
word of the predeclared series from `tmark_ir::registry::PREFIX_NAMES`
(French and German; English is `Prefix::label`; the primary subtag of a
BCP 47 tag decides, so `fr-CH` is `fr`; an unknown language is English).
It defaults to the front matter's `lang`. A `name` declared in the front
matter always wins, user series are never translated, and the LaTeX
writer keeps relying on babel. `Resolved::lang` and `ResolvedView::lang`
record the language used.

### Sibling documents

`ResolveOptions::book` is the list of the other documents' labels, each a
`BookLabel { key, prefix, number, kind: Host, title, location }`;
`Resolved::book_labels(location)` produces one document's contribution
(one entry per label in document order, `location` being the given string
plus `#key`; the view uses the document's path, and the caller relativises
the part before `#` for each referring page). `title` is the heading text
or the caption text (`Label::title`), what a page shows for a section it
links to.

Lookup: a key that is a local label resolves locally, whatever the book
says (a local definition always wins, so a page may redefine `sec:intro`).
A key that is no local label but is in `book` resolves to
`Resolution::Sibling { label, location }`, `label` being the formatted
number, else the title, else the key. The bibliography rule is unchanged:
a sibling key that is also a bibliography key is `Ambiguous` with
`ref-ambiguous` on the reference (a local one reports it on the label, as
before). Sibling keys match case-insensitively, the first entry of a key
wins, and the prefix routing rule of the spec (`@a:b` with an undeclared
head is a bibliography key) does not apply to them: the book entry carries
its prefix, and the site's counter declarations may live in `mkdocs.yml`
rather than in every page. `@alias:prefix:key` inventories are the other,
explicit mechanism and stay as they are. The writers render a `Sibling`
as the HTML writer renders any reference, a link to `location` with
`label` as text; LaTeX and Typst print `label` as text, since a sibling
outside the build has no `\label` to point to (the web lowering is where
`Sibling` matters).

The plugin's two passes are then: pre-pass every page with `numbering:
All` and the chained `start`, collect `book_labels`; lower every page with
`book` = the site's labels minus its own (`Resolution::Sibling` gives the
link text and target). Both are covered by
`crates/tmark-registry/tests/book.rs`, since the conformance fixtures
cannot express options.

## What the LSP gets from this

Completion after `@` (every label, key and term with its kind), go-to-definition
from a `Ref` to its host node, hover with the label word and the number when
known, rename of a label (a `NodeEdit` on the definition plus one per
reference), document links for includes.

## Non-goals

- Formatting citations (CSL) or labels: writers and TeXSmith.
- Fetching anything.
- Persisting registries between runs: the LSP keeps `Resolved` in memory per
  document; TeXSmith keeps inventories on disk.

## Implementation notes (milestone 2)

- `tmark-registry` exposes `resolve(doc, loader, options) -> Resolved`;
  `ResolveOptions` carries the document path (includes and sources resolve
  against its directory), the `.bib` paths and the first values of series
  that continue across documents.
- `FsLoader` lives in this crate behind the default `fs` feature (off in
  WASM) rather than in each edge crate: one implementation, three users.
- Hosts and prefixes: a heading accepts any heading-class prefix (`part`,
  `chap`, `sec`, `app`); a predeclared prefix on the wrong host is
  `prefix-host-mismatch`, a user series on any host numbers it in that
  series (spec §Anchor). Image and span nodes span their attribute list, so
  a label diagnostic covers `![…](…){#id}` as a whole.
- Numbers are allocated for the TeXSmith-numbered series only (declared
  counters and theorem kinds with a counter of their own); `Counter::label`
  applies the Python-style `format` (`{n:02d}`, `{prefix}`, `{key}`).
- A key present in a label registry and in the bibliography resolves to
  `Resolution::Ambiguous` with a `ref-ambiguous` diagnostic, not to
  `Unresolved`, so that it is not reported twice.
- Anchor links (`[text](#id)`) are resolved like references and appear in
  `Resolved.refs`.
- Inventories are read from `sources.crossrefs`; a missing or invalid file
  is `crossref-inventory-missing`. The staleness check (`hash`) waits for
  the writer side in TeXSmith, which fixes the hash algorithm.
- Footnote-versus-citation shadowing (decision X7, examples-migration
  item 2): the tokenizer rule in `tmark_reference.rs` turns a `[^key]`
  whose label has no `[^key]:` definition (and `^[k1,k2]` groups) into a
  `Ref`, keyed on the *missing definition*, not on the bibliography (a
  `.bib` given on the CLI is invisible to the parser). A defined label
  wins and stays a `Note`, whatever the bibliography holds; the
  `citation-shadowed-by-footnote` diagnostic for that case is still not
  emitted. A `[^key]` that resolves nowhere is `ref-unresolved`, which is
  the loud outcome the spec wants (P4).
- Glossary terms come from `declare.glossary`, `declare.acronyms` and the
  `*[KEY]: …` lines. `declare.glossary` is typed
  (`tmark_ir::frontmatter::GlossaryDecl`) and both spellings of spec
  §Glossary and acronyms land in it: the flat mapping's keys and the keys
  under `entries` are its `entries`, `style` and `groups` are form and stay
  in the front matter for the template that prints the per-group tables
  (C50). `collect::glossary` reads `entries` alone, case-folded, each term
  mapping to its `name`, else its `description`, else its `long` form;
  `declare.acronyms` (still a loose JSON mapping, term to string or object
  with `name`/`description`) then the `*[KEY]: …` abbreviations are merged
  over it, in that order.

## Implementation notes (milestone 3)

- References inside included files resolve too: the collector keeps the
  parsed included documents (`Resolved.included`) and `resolve_all` runs
  on each after the main document, so `ref-unresolved` is reported with
  the included file's `FileId` and the language server publishes it under
  that file. `RefResolution.span` is the key token (`RefItem.key_span`);
  `Label.id_span` is the id token.
- Heading-class prefixes carry `Prefix::heading` in the registry instead
  of a list in the collector.
