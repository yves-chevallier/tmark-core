# 13 — Handoff notes

Five handoffs, newest first. Read `AGENTS.md`, then this file, then
`11-roadmap.md`, then `design/reviews/`. Everything below is opinion from
the inside of the work: verify it, do not trust it.

## The fragment-spans round (2026-09-16, second Zensical wave)

Branch `autorefs-anchors`, on top of the round below. The handbook's
`NOTES-zensical.md` §8 lists nine defects of that wave; three were the
core's and are fixed here, one commit each.

- **A counter in a `!!!` title spliced at column 0** (`b9020ef`, C66).
  `!!! exercise "#(ex:un) : T"` lowered to `<span
  class="ts-counter">Exercice 1</span>cise "#(ex:un) : T"`. The title is
  parsed out of a string the tokenizer hands over whole, and
  `lower_fragment` anchored every node of it at the *block*, so the
  node's span was the marker line's first bytes. `parse_admonition_info`
  now returns the title with its offset in the info string,
  `head::attr_value_offset` locates an attribute value no escape
  decodes, and `Lowerer::attr_value_span` turns either into the span of
  the value itself. The `::: kind {title="…"}` form had the same wrong
  spans and only looked healthy because the writer's "are these the
  text's spans" test rejected them and printed each node instead.
  Second half of the same bug: PyMdownX reads a title up to the next
  `"`, so a `!!!` source whose title lowers to something holding a `"`
  or a newline — a counter, a reference — now gets the
  `<div class="admonition …">` wrapper the `:::` form already got,
  instead of a marker line no extension can parse. Fixture
  `container-admonition-title`, plus the `!!!` case in `tests/web.rs`
  (`ROWS`, which had to become a `r##"…"##` literal: `"#` ends `r#"`).
- **A code span in a `yaml table` cell rendered as the fence line**
  (`c9b81d8`, C66 as well). Same class: the cells were anchored at the
  fence, so `` "`+`" `` carried the span of ```` ``` ```` and the
  writer sliced that back. A cell is now located in the payload by its
  **single** occurrence there (`table::Payload::locate`) and carries the
  spans of that slice; a cell the payload does not spell verbatim (a
  YAML escape, a folded scalar, two cells reading the same) keeps the
  fence as its span. As a second line of defence `inlines_text` also
  checks that a `Code` and a `Math` read back from their span, so the
  fallback prints the node. Fixture `fence-yaml-table-markup`.
- **An `_` in a label reached `\hyperref[a\_b]`** (`39f4d9c`, C67). The
  LaTeX writer escaped a label *name* with the prose escaper.
  `escape::label` replaces `escape::escape` at every site that prints a
  name — `\label`, `\ref`, `\pageref`, `\hyperref`, the figure, table
  and equation labels, `\tsgls`, the `id=` key of `tscode` and
  `tscallout` — and maps only the nine characters that break the reading
  of a brace argument or of a `\csname` (`\ { } # % ~ ^ $` and a
  blank), each to `+` and a letter with `+` doubled, so the mapping is
  injective. Fixture `anchor-label-name`.

What the spec gained: §Round-trip and source spans now says what the
spans of a fragment are, and what they are when the fragment has no
source of its own (the construct's span, so a tool prints the node
rather than slicing the file). Design 07 gained the label-name rule.

Measured on this branch: `cargo fmt --all --check`, `cargo clippy
--workspace --all-targets -- -D warnings`, `cargo test --workspace`
clean; schema, registries and the Python stub regenerate with no drift;
`crates/tmark-py/tests` 50 passed after `maturin develop`. TeXSmith `uv
run pytest -q` 1504 passed; `scripts/parity.py baseline --check`
249/251 identical, the two that differ being another agent's prose edit
to `docs/syntax/code.md` in the working tree — **no writer output of the
corpus moved**, because no label of it holds an `_` (checked with a
`grep` over `tests/parity/baseline`). On the handbook, `texsmith site
build --no-pdf` and `zensical build -c` both report no issue, and the
built `.tex` holds **0** `\hyperref[…\_…]`, `\label{…\_…}` and
`id=…\_…` — it held 0 before this round too, because the handbook had
already renamed `twos_complement` → `twos-complement` to work around the
bug; on a copy of `docs/course-c/10-numeration/numbers.md` with the
underscore put back, the writer now emits `\label{twos_complement}`, so
those anchors can be renamed back.

### The cross-repository contract (additions this round)

- **Label names in LaTeX changed shape.** Anything on TeXSmith's side
  that builds a label name itself, or that matches the writer's output,
  must use the same rule: leave `_ - . : &` alone, map `\ { } # % ~ ^ $`
  and blanks to `+`+letter, `+` to `++`. The one visible move in the
  existing corpus is a *space* in an anchor: `#h i` was
  `\hyperref[h i]`, it is now `\hyperref[h+si]` (fixture
  `link-destination-escapes`). Nothing else in the parity baseline moved.
- **`\newacronym` is TeXSmith's and is not covered.** The core prints
  `\tsgls{key}` and `\tsacr{key}` through the new rule; the declaration
  that must match them is written by `ts-glossary` from the front
  matter. A glossary key that is not csname-safe (`k&r`, item 7 of the
  handbook notes) therefore stays TeXSmith's bug: `&` ends a key in the
  TMark grammar, so no document can *write* one — it can only be
  declared in the front matter, and the same mapping belongs in the
  Python that emits `\newacronym`. The core's `\tsacr` key is a slug
  (`text::acronym_key`) and needs nothing.
- **`lower_web` now writes a `<div class="admonition …" markdown="1">`
  where it used to leave a `!!!` line**, but only when the title lowers
  to something holding a `"` or a newline. A `!!!` callout with a plain
  title is still left byte for byte. Anything diffing lowered pages
  against a recorded artifact must re-record those pages.
- **Node spans moved for fragments.** An admonition title, an attribute
  value and a `yaml table` cell now carry spans of the file where they
  can. A consumer that assumed those spans were meaningless, or that
  they all pointed at the construct, sees real ones; a consumer that
  slices the file at a node's span gets the node, not the construct.
  The IR schema is unchanged.
- **An unresolved fence `include=` no longer keeps the fence bytes.**
  `lower_web` now reprints the fence *without* the option and with an
  empty body — `title=` and the rest kept — and still reports
  `include-missing` (the message reads "the fence is printed empty").
  `pymdownx.superfences` does not parse `include=` in an info string, so
  the kept bytes were no fence at all on the site: `convert` gave
  `<p><code>c include="missing.c"</code></p>` and the paragraph after it
  was swallowed. Verified with the installed extension set: an empty
  fence renders as `<div class="highlight">…` and `title=` still shows
  as the `<span class="filename">`. A fence still written `--8<--` is
  untouched, as before. Anything on TeXSmith's side diffing lowered
  pages against a recorded artifact must re-record a page whose include
  does not resolve.
- **A callout inside an HTML wrapper is an HTML wrapper too** (C68).
  `lower_web` used to leave a `??? solution` inside a
  `<div class="admonition exercise" markdown="1">` as its four indented
  source lines; it now writes `<details class="solution" markdown="1">`
  with the body at the wrapper's column. The reason is `md_in_html`: an
  indented HTML block inside a `markdown="1"` wrapper has its opening tag
  read as data and its closing tag matched against the stack of open
  blocks, so the `</div>` of a `<div markdown>` in the body closed the
  wrapper and the rest of the page rendered inside it. Anything on
  TeXSmith's side diffing lowered pages against a recorded artifact must
  re-record a page with a callout inside a numbered or labelled one.

### Known and left

- The `<p class="admonition-title">` the profile writes carries no
  `markdown` attribute, so Markdown *inside* a rewritten title (an
  `*emphasis*`, not a counter, which is already HTML) does not render on
  the site. `<figcaption markdown="span">` and `<th markdown="span">`
  in the same file suggest the fix is one attribute, but it was not
  verified against the installed extension set and would move every
  callout of the corpus, so it was left. Check it against Material
  before changing it.
- `Payload::locate` refuses a cell the payload spells twice, so two
  identical cells of a `yaml table` fall back to the fence span. The
  fallback is correct, not exact: a forward-scanning cursor would place
  them, at the cost of trusting YAML document order.

## The Zensical-corpus round (2026-09-16)

Branch `autorefs-anchors`. A 142-page French course site
(`/home/ycr/handbook`, MkDocs Material → Zensical, PDF books through
TeXSmith) was run against the core as a corpus; its migration notes
(`NOTES-zensical.md` there) named five defects. Four were core bugs, each
fixed at the layer that owned it and in its own commit; the fifth was not
ours.

- **A `[` that opens a LaTeX table cell** (`202354a`). `\\`, booktabs'
  rules and `\addlinespace` all take an optional `[⟨dimen⟩]` and scan for
  it past the line end, so a cell starting with `[` — typically an
  unresolved `[text][id]` reference-style link — was eaten as that
  argument (`Missing number, treated as zero`). The guard is `{[}` on the
  *content*, not on the command: `\tabularnewline` takes the same
  argument, and `\\{}` would stop `\multicolumn` being the first token of
  its cell. Typst has no twin (it escapes `[` in markup). Fixture
  `table-cell-bracket`, `tests/brackets.rs`.
- **The braces-only fence info string** (`efbff73`, challenge C63).
  Material's documented ```` ``` { .c .annotate } ```` gave a language of
  `{`. The first class is the language, as superfences reads it;
  deprecated, the fix is the reprint. Fixture `fence-info-braces`.
- **`/// html | div[…]` and `::: div` on the web** (`d1a832b`, C64). The
  `pymdownx.blocks.html` selector had no reading and the block became a
  `RawBlock{html}` holding *Markdown*; and a `Div` was not lowered for
  the web at all. Both halves now land on the spec's `<div … markdown>`
  row. Fixture `container-html-block`.
- **`include="file"` on a fence, on the web** (`f4b3648`, C65). The
  spec's own replacement for `--8<--` made superfences render the fence
  as an inline code span, so following the deprecation lost every
  listing. The `mkdocs` profile splices the body through the `Loader`.
- **Not ours: `\|` in a code span on the web.** Python-Markdown's
  `tables` matches `\|` in `TableProcessor.RE_CODE_PIPES` only to keep it
  from splitting the row and never removes it from the cell, and a code
  span is atomic for the inline pass — so `` `x \| y` `` renders
  `<code>x \| y</code>` with the backslash. Verified with the installed
  extension set (`markdown` 3.10, `pymdownx.superfences`,
  `inlinehilite`, `escapeall`). The lowering already leaves the row byte
  for byte, and the `\|` must stay: GFM and TMark both split on a bare
  `|`, code span or not (printer rule U2, fixture `table-pipe-in-code`).
  The backslash belongs to TeXSmith's HTML postprocessor to strip, not
  to the core.

Measured on this branch: `cargo fmt --check`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace` clean;
`crates/tmark-py/tests` 50 passed after `maturin develop`; TeXSmith
`uv run pytest -q` 1489 passed and `scripts/parity.py baseline --check`
249/249 identical (no writer output of the corpus moved — no cell of it
opens with `[`). On the handbook itself, `texsmith site build` now runs
the whole book through TeX: `Missing number, treated as zero` and `Extra
alignment tab` are both **0** (they were 104+ and the blocker of every
table), 519 pages of `.xdv` are written, and the first remaining error
is `LaTeX Error: Too deeply nested` on a fifth-level list
(`enumitem`'s `\setlistdepth`, a template matter), with the rest of the
tail being `hyperref` `\csname` trouble in `.aux` label entries. None of
those is a core defect.

### The cross-repository contract (additions this round)

- **`lower_web` emits two new wrappers.** A `Div{name="div"}` becomes
  `<div … markdown="1">` … `</div>`, and a fence with `include="file"`
  is reprinted with the file's body and without the attribute. Anything
  on TeXSmith's side that diffed lowered pages against a recorded
  artifact must re-record.
- **`Lowered.diagnostics` can now carry `include-missing`** for a fence
  whose `include=` the loader cannot serve (the resolution collects no
  fence include, so nothing else reports it). The severity is the code's
  default, warning — not the `Info` the block-include case uses.
- **Two new `deprecated` spellings** reach `tmark.lint` / `fixes`:
  `{ .lang .cls }` fence info and `/// html | <selector>`. Both carry
  the node reprint as their fix, so `tmark lint --fix` rewrites them.
- `/// html | <selector>` no longer produces `RawBlock{format=html}` nor
  a spurious `attr-no-host`: a consumer keyed on either changes.

## The review-and-fix round (2026-09-14)

Written by the agent that ran the spec reviews and the fix branches
listed below, for whoever takes `texsmith-migration` to `main`. Nothing
here supersedes the migration-waves handoff's architecture; it corrects
and extends it where the round changed something.

### State of the branch

- `texsmith-migration` at `9029359`, `git rev-list --count main..HEAD` =
  137 (`main` unmoved at `9017bda`) — see the correction inline below
  where the previous handoff undercounted this. Nine merges landed this
  round: `fix/typst-escape` (`0424387`), `fix/citations` (`4593a6b`),
  `fix/spec-text` (`c3bf402`), `fix/spec-code` (`d8041cf`),
  `chore/release-prep` (`76b2fd0`), `fix/keystroke` (`01682b1`),
  `fix/quote-boundaries` (`9029359`), plus two review commits written
  straight to the branch (`3d97493`, `8e8dbce`). Full list: `git log
  --oneline 353311f..HEAD`.
- Measured clean: `cargo test --workspace` — 340 tests, 0 failed;
  `cargo clippy --workspace --all-targets -- -D warnings` — clean;
  `cargo fmt --all --check` — clean; `cargo +1.80 check --workspace
  --all-targets` — clean (the MSRV job added to `.github/workflows/ci.yml`
  this round, `dad4f56`, also makes CI run on `v*` tags); generated
  artifacts current (`schema`, `registries` examples regenerate to no
  diff). Workspace version is now `0.1.0` (`4bd0d0b`), every internal
  path dependency version-pinned; `Cargo.lock` repointed three
  transitive deps to their MSRV-1.80-compatible versions (`4f22f38`).
  `tmark-writers` gained `unicode-ident = "1.0"` (Typst label escaping,
  below). No tag is pushed yet.

### What the round did

- **Review 07** (`3d97493`, spec-vs-code over C27–C50): 0 blocking, 11
  major, 14 minor. **Review 08** (`8e8dbce`, the spec's own internal
  consistency, a lens review 07 does not cover): 5 blocking, 15 major,
  19 minor. Every finding of both is dispositioned in their own
  §Triage — fixed, deferred to a new challenge row, or closed as spec.
- **fix/typst-escape**: the writer now escapes Typst's comment openers
  `//` and `/*` (either silently ate the rest of the line), `~` (always
  a non-break space), and `=`/`+`/`-`/`/` at a line start — verified
  against typst 0.15.1.
- **fix/citations** (closes C51): the citation model below.
- **fix/spec-text**: draft 4 of `spec/tmark.md` — one normative
  preamble, one registry lookup order, one identifier grammar, one
  citation rule, one state for `#{prefix:key}`, and the C27/C30 row
  grammars the code already implemented. Closes challenges through C60.
- **fix/spec-code**: the code side of review 07's F1, F2, F7, F8, F10,
  plus the digit-initial reference key (F3/C27), and drops
  `Code::StrictXConstruct` (`c90815e`) — a diagnostic nothing ever
  emitted, removed from `tmark.codes()` and `schema("diagnostic")`.
- **chore/release-prep**: version `0.1.0`, the MSRV CI job, `tmark-lint`
  tests raised 2 → 8 (`5cf6551`), TeXSmith's IR model header regenerated
  and the PyPI name conflict flagged (`ebeff13`).
- **fix/keystroke** (closes C61): `++…++` needed word boundaries and no
  internal whitespace — `C++03 … C++` in ordinary prose was read as one
  keystroke spanning the sentence.
- **fix/quote-boundaries** (a corpus regression found after fix/spec-code
  landed, fixed same day): a closing smart quote must also close before
  an em/en dash, not only before ASCII punctuation.

### The citation model, precisely (C51)

A bare `@key` is the short citation on every backend (`\cite{key}`,
`#cite(<key>)`) — the same rendering as bracketed `@[key]`. The feature
`citations.narrative` (a `press.features` entry, no front-matter schema
change) flips the bare form document-wide to narrative (`\textcite`,
`form: "prose"`); inside brackets, a leading `+` makes one item narrative
regardless of the document default (`RefItem.narrative`, a new IR
boolean, absent when false), mirroring `-key` for the year alone.
`tmark.write` accepts `options["citations"]["narrative"]` with the
precedence of `lang`; `tmark.registries()["features"]` lists the row. The
`[^key]` fix now inserts a space when the sugar hugs a preceding word.

### The cross-repository contract now in force (additions this round)

- `RefItem.narrative` is a new IR field: TeXSmith's `texsmith/ir/model.py`
  and the schema hash need regenerating.
- `Code::StrictXConstruct` is gone from `tmark.codes()` and
  `schema("diagnostic")` — anything on TeXSmith's side keyed on it breaks
  loudly rather than degrading. `Code::RefUnnumbered` (warning, added
  just before this round, F8) stays and is now spec-documented: a
  numeric reference to an anchor with no counter renders the anchor's
  text instead of `?` / `Figure ?`.
- The Typst writer's wider escaping (comments, `~`, line-start markers)
  can change bytes of already-generated `.typ` output; re-render rather
  than diff against an old artifact.
- The wheel is `0.1.0` and `from_json`'s minor-version refusal is live
  for the first time on a real version bump — see the pitfall below.
- `vendor/tmark` in TeXSmith is still the symlink to `../../tmark`
  (`pyproject.toml`'s `[tool.uv.sources]` path entry); unchanged, no
  PyPI release yet.

### Pitfalls learned this round

- **A version bump breaks fixtures, not just code.** Moving the
  workspace (and the wheel) to `0.1.0` made `tmark.ir.codec.from_json`'s
  minor-version refusal reject every one of TeXSmith's twelve
  `tests/passes/*.in.json` fixtures, recorded against the old dev
  version; they needed a one-line regeneration each (`d8fbdf6`) before
  the suite was green again. Expect this on every future version bump,
  not just this one. TeXSmith also hardened the failure mode
  (`953d46a`): a stale wheel now raises a named `RuntimeError` naming
  both schema hashes instead of a confusing `decode_document` field
  error.
- **A fixture-clean fix can still regress on the corpus.**
  `fix/spec-code`'s SmartyPants quote-boundary fix (review 07 F2) passed
  every fixture and broke on real prose: a closing quote followed by an
  em/en dash (`"…technique"—fitting`) is not
  `char::is_ascii_punctuation`. Run the corpus loop after any
  inline-boundary change, not only `cargo test`.
- **Reviews can be written straight onto the integration branch.**
  07 and 08 only read; committing them directly to `texsmith-migration`
  (no worktree) and citing their finding ids from the fix branches' commit
  bodies made each review's own §Triage table double as the round's task
  list — no separate tracking file was needed.
- The MSRV job is real now: `cargo +1.80 check` in CI will fail on a
  dependency that only builds with a newer resolver; `4f22f38` had to
  repoint three transitive versions down before this round could pass it.

### What is next

TeXSmith's `specs/merge-tasklist.md` is the live list; as of today it
still has open, in order: ~~pick the PyPI name~~ (decided 2026-09-14:
`tmark-core`, import name `tmark`, GitHub repository renamed
`yves-chevallier/tmark-core`), tag `v0.1.0`, then merge `texsmith-migration` into `main` with
green CI. After that, on TeXSmith's side: point `pyproject.toml` at
`tmark>=0.1,<0.2` from PyPI, drop `vendor/tmark` and its
`[tool.uv.sources]` entry, move the six `ref: texsmith-migration` lines
in `.github/workflows/*.yml` to the tag, get CI green, and cut `v0.7.0`.
Measured today: tmark 340 tests, clippy `-D warnings`, fmt, MSRV 1.80,
generated artifacts clean; TeXSmith 1347 tests, parity 196 identical / 0
differing.

## End of the TeXSmith-migration waves (2026-09-11)

Written by the agent that coordinated the waves of the TeXSmith migration
on branch `texsmith-migration`, for whoever takes over. The previous
handoff (end of M3 and migration wave 1, same date) follows below; its
pitfalls still apply.

### State of the branch

- Branch `texsmith-migration`, about sixty commits ahead of `main`, **not
  merged**. [Stale even at the time: `git rev-list --count main..HEAD`
  read 50 then, not "about sixty"; it is 137 after the round described
  at the top of this file. Do not trust a commit count in prose —
  recompute it.] `main` is at
  `9017bda` (end of the M3 round). The next step
  is a pull request from `texsmith-migration` to `main`, after the
  reviews listed at the end of this handoff. Every wave was developed in
  a worktree (`wt/fixes`, `wt/tables`, `wt/registry`, `wt/writers`,
  `wt/py`, `wt/webresolve`, `wt/spec`, `wt/web`) and merged here; the
  eleven merge commits are in the log.
- Workspace green on this branch: `cargo test --workspace`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  the generated artifacts current (`schema`, `registries` examples). New
  since `main`: `.github/workflows/ci.yml` (the three cargo checks, the
  generated-artifact diff, `maturin develop` plus the pytest suite of
  `tmark-py`; runs on pushes to `main` and `texsmith-migration` and on
  pull requests) and `.github/workflows/wheels.yml` (abi3 wheels on a
  `v*` tag). Neither has run on a tag yet; nothing is published.
- The companion branch is TeXSmith's `tmark-migration` (`~/texsmith`).
  Its plan is `specs/tmark-migration.md`, the settled choices
  `specs/migration/decisions.md` (X1–X12 and the wave list), the measured
  state `specs/migration/status.md`: the `docs/` corpus renders through
  `--reader tmark` on both backends; 31 / 32 LaTeX and 24 / 26 Typst
  examples build. Its CI checks this branch out at `vendor/tmark`.
- `11-roadmap.md` is the authority on what is done: M1 is closed, M4 is
  open on the Typst preview and the parity triage, M5 on the three
  compatibility spellings, the releases and TeXSmith's phases 4 and 5.

### What each wave did

**Registry** (`wt/registry`, first to land): the `FRAGMENTS` table
(fragment contracts: name, `provides`, `packages`, `shell_escape`) and
`KEY_LABELS` (keystroke labels) in `tmark_ir::registry`, serialisable
registry tables and `schema("diagnostic")`, every diagnostic code
documented and staged (`Code::doc()`, `Code::stage()`), the `Resolved`
view (`ResolvedView`, `next_start`, `schema("resolved")`), `edit_many` in
`tmark-fmt`, `lint --fix --stdout|--diff`, and `Profile::Mkdocs` as a
real spelling table (`04-printer.md`) with a snapshot test over the
fixtures and the D0 gate (the `Mkdocs` text re-parses to the canonical
IR). The facade gained versioned document JSON (`"tmark"` at the root),
`schema_hash`, `apply_fixes`.

**Fixes** (`wt/fixes`): TeXSmith's `examples-migration.md` items, so that
`tmark lint --fix` migrates a real document: `[^key]` / `^[k1,k2]`
citations to `Ref` with a fix (X7, tokenizer rule in
`tmark_reference.rs`), `/// latex` and `/// caption` blocks, attribute
lists on fence info strings (C28), string authors, the front-matter
line-edit fix (`yaml_edit::move_key`), the C20 rows, the `--8<--` fence
body, digit-initial keys printed bracketed (C27), and
`compat-unsupported` in place of silence. Details in the previous
handoff's wave-1 section.

**Tables** (`wt/tables`, decision X9): the `yaml table` model mirrors
TeXSmith's `schema.py` field for field (`tmark-ir/src/table.rs`),
lowering is a port of `parse_table` and `build_matrix`
(`lower/table_yaml.rs`) with the parse codes `table-yaml`,
`table-unknown-key`, `table-columns`, `table-align`, `table-shape`,
`table-row-width`, `table-span`, `table-column-unknown` and the lint
codes `table-placement`, `table-width`, `table-width-sum`; a rejected
fence keeps its `source` and prints back as typed; the printer writes the
Python row shapes; a proptest round-trips generated models and TeXSmith's
table corpus is a fixed point. Where the Python validator contradicts its
own documentation the documented rule won and the Python reading is a
fallback (C30).

**Writers** (`wt/writers`, three merges): `crates/tmark-writers` from the
skeleton (`Writer`, `WriterOptions`, `Body`, `Requires`, `SourceMap`,
escapers) to the HTML writer (CommonMark comparison with a golden list),
the LaTeX writer over the whole migration catalogue
(`writers-and-passes.md` §2), the Typst writer (`mitex` math, the
`texsmith.typ` contract names, `assets/texsmith.typ` shipped as
`TEXSMITH_TYP`), `tmark write`, fixture snapshots per backend, the
`Requires` read from `FRAGMENTS`, then the C31–C42 constructs (tabs,
progress bars, TeX logos, heading classes, referenced implicit labels),
inline icons (`\tsicon`) and `adjustbox` around converted diagrams. The
decisions taken without a note are listed in `07-writers.md`
§Implementation notes (milestone 4).

**Py** (`wt/py`): `crates/tmark-py` from the skeleton to the module
`tmark._tmark`, the package `tmark`, the generated stub, the pytest
suite, the `Loader` wrapper (GIL released around the Rust stages), the
`Resolved` handle shared between `resolve` and `write`, `edit_many`,
`fragments`, `key_labels` in `registries()`, and the wheel workflow. One
commit untracked the `.so` `maturin develop` drops into the package.

**Webresolve** (`wt/webresolve`): site-wide resolution in
`tmark-registry` (`06-registries.md` §Site-wide resolution):
`Numbering::All`, `ResolveOptions::book` and `Resolution::Sibling`,
`ResolveOptions::lang` with the localised label words of the predeclared
prefixes (`PREFIX_NAMES`), the options exposed to Python, challenge C29;
`tests/book.rs` covers what fixtures cannot express.

**Spec and parser** (`wt/spec`): the twelve open constructs of the
migration audit decided in the spec (C31–C42, each written into its
section in the same commit) and implemented end to end: the IR
(`ProgressBar`, `CONTAINERS`, `TEX_LOGOS`, the `gemoji` table), the
tokenizer (tab and foreign-directive heads, the attribute colon, exactly
three markers except the directive's colons), the lowering (tabs, layout
containers, foreign directives, `md_in_html`, progress bars, shortcodes,
`^^x^^`), the printer (HTML as typed, tabs and div sugar under `mkdocs`),
the lint hints `icon-web-only`, `directive-foreign`, `feature-off`,
implicit heading ids by GitHub's slug rule in the registry, the writers,
and the fixture IR blocks. `compat-unsupported` shrank to critic markup,
wiki links and fancy list markers.

**Web** (`wt/web`, last): `lower_web` in `tmark-writers/src/mkdocs.rs`
(`07-writers.md` §Web lowering, TeXSmith `web-profile.md`): the MkDocs
page lowering as local splices through `edit_many`, every byte outside a
recognised construct untouched; `tmark lower FILE --to web`; the Python
`lower_web`; then the foreign-directive rule (C40: an unclosed
`::: pkg.mod` keeps its bytes, the prose after it is lowered) after the
merge with the spec wave.

### The parity-triage round (2026-09-12, worktree `wt/parityfix`)

Four findings of TeXSmith's `specs/migration/parity-triage.md` whose cause
was in this core, one commit each:

- **F3** — inline markup flattened in a table header cell. `LeafColumn`
  and `ColumnGroup` gained `title`; the writers and `tmark fmt` render it.
  Also fixed the lead promotion firing inside `lower_fragment`, which was
  dropping a leading `Strong` from a cell.
- **F4** — the lead-paragraph heuristic. The sugar now promotes only a
  paragraph that *is* one short strong span, outside a list item; the
  `{lead}[…]` role takes what follows it whatever the feature says. Spec
  §Para was the thing at fault (C44).
- **F5** — `[](){#id}` is the anchor `[]{#id}`, deprecated with a fix
  (C45).
- **F6** — the smart symbols and the straight-quote pairing, which were
  never implemented (C46); this closes M5 item 2 except for dialect
  import.

Left to TeXSmith, with the reason: F1 (Typst document metadata), F2
(front-matter `glossary.entries`), F7 (the scripts pass, `fonts/`), F8
(snippet includes: I/O, and the stray `;` is in the source), F9 (same
`;`), F10 (choosing one of a light/dark image pair — tmark strips the
`#only-light` marker as the contract says; dropping the duplicate is an
assets-pass decision that would need a spec row, most naturally as a
`media=` restriction), F11 (the Typst equation numbering is the template's;
the emphasis nesting, the code-span padding and the `<code>` in a cell are
places where tmark is the faithful one and legacy the lossy one, so they
belong in the allow-list or §4).

### The cross-repository contract now in force

TeXSmith's `tmark-migration` branch depends on these; changing one is a
two-repository change (R14 of its plan). **ADR 0008 (proposed) would replace
the first of them**: TeXSmith would stop mirroring the IR and ask for
resolutions instead. It is not started, and must not land before
`texsmith-migration` merges.

- **The IR JSON and its schema hash.** Documents cross as JSON with
  `"tmark": "<version>"` first and `"diagnostics"` last; TeXSmith
  generates `texsmith/ir/model.py` from `tmark.schema("ir")`, records
  `tmark.schema_hash()` (FNV-1a of the schema, 16 hex digits) and its CI
  fails when the committed models drift. A field added to a node is a
  regeneration on their side; a renamed one breaks their passes. The
  parity-triage round added one: `LeafColumn.title` / `ColumnGroup.title`
  (the header of a table column as inline Markdown, finding F3), so
  `texsmith/ir/model.py` and the schema hash must be regenerated. C50
  changed one: `press.declare.glossary` is no longer a loose JSON value but
  `GlossaryDecl {style, groups, entries}`, whichever spelling the author
  used — so `passes/glossary.py`, which normalised the structured section
  into a flat mapping before resolution, is reading a typed object now and
  has nothing left to normalise. C51 added one: `RefItem.narrative`
  (`@[+key]`, the narrative citation whatever the document default), a
  boolean absent when false, so `model.py` and the schema hash are
  regenerated again; nothing in TeXSmith's passes reads it.
- **Citation forms (C51).** A bare `@key` is now the short citation on
  every backend (`\cite`, `#cite(<key>)`), the same as `@[key]`; the
  feature `citations.narrative` (a `press.features` entry, no schema
  change) makes the bare form `\textcite` / `form: "prose"`, and `+key`
  does so per item. `tmark.write` accepts `options["citations"]
  ["narrative"]` as an override (the precedence of `lang`); TeXSmith's
  `build_writer_options` may pass it from a CLI or template setting, or
  leave the front matter to decide. `tmark.registries()["features"]`
  lists the new row. TeXSmith's `docs/syntax/references.md` (line ~119)
  and `docs/guide/features/bibliography.md` (line ~91) still say "`@key`
  is the in-text (narrative) citation": they must state the new rule and
  the switch. The `[^key]` fix now also inserts a space when the sugar
  hugs a word (`sortie[^key]` → `sortie @key`), which the earlier fixer
  got wrong on a real corpus.
- **`Requires` and `FRAGMENTS`.** A writer names the contracts it used
  in `Requires.fragments` and the structural packages in
  `Requires.packages`; TeXSmith's fragment loader reads
  `tmark.fragments()` to activate the `ts-*` fragments and checks a
  replacement fragment against `provides`. The macro names (`\tskeys`,
  `tscode`, `\tsdivider`, `\tslogo`, `tsdiv`, `\tsicon`, …) are the API
  between the LaTeX writer and the fragments; `KEY_LABELS` spells the
  keystrokes on both sides. Critic markup (C49) made `ts-critic` a
  contract the writers actually name: a document with `{++x++}`,
  `{--x--}`, `{~~a~>b~~}` or `{>>note<<}` now carries `ts-critic` in
  `Requires.fragments`, so the fragment must load for LaTeX *and* Typst.
- **`texsmith.typ`.** The Typst writer emits `#ts-…` calls;
  `tmark_writers::TEXSMITH_TYP` is the default definition TeXSmith writes
  next to the `.typ`; a template redefines what it restyles. `ts-subfigure`
  and `ts-subnumber` (the images of a `::: figure`) joined the file with
  the sub-figure numbering, `ts-ins`, `ts-del`, `ts-subst` and
  `ts-comment` with critic markup (C49); a template that ships its own
  copy of `texsmith.typ` needs them.
- **The `Resolved` capsule.** `resolve()` returns the view of
  `schema("resolved")` plus an opaque handle; TeXSmith calls `resolve`
  once per document and `write` once per slot body with the same handle,
  chains `next_start` into the next document's `start`, and feeds
  `book` from the other pages' `book` lists. Numbers, label words and
  `Resolution` kinds are the registry's, never recomputed in Python (D4).
  A sub-figure (spec §Image, Figure) is a label with `host: "subfigure"`
  and a `subfigure: {parent, letter}` object; it has no number of its own,
  its formatted number is the container's plus the letter (`2a`), and
  `next_start` counts one figure per container. The new host value and
  field are a `model.py` regeneration on their side.
- **The `lower_web` shapes the MkDocs plugin relies on**: the return
  `{text, diagnostics, bibliography}`; the wrappers Material's extensions
  read (`<figure markdown="span">`, `<figure markdown="1"
  class="ts-table">`, `<table data-ts-table="1" markdown="block">`,
  `<div class="ts-equation" markdown="1">`, `<aside class="ts-aside">`,
  `<span class="ts-counter" id data-counter data-key>`, `<span
  class="ts-index">`, `<abbr>`), the reference spellings
  (`[FW-10](#fw:x)`, `[title](#sec:x)`, `[label](location)` for a
  sibling, `[?key]`), the `!!!`/`???` callouts, the `## References` list
  and the `ts-` prefix (`css_prefix`). The plugin's CSS and its
  `on_page_markdown` hook are written against these strings.
- **Diagnostics**: `{code, severity, span, message, fix, related}` plus
  `stage`, `path`, `line`, `col` (1-based, byte column, X10); the
  catalogue from `tmark.codes()`.
  Review 07 F8 added `ref-unnumbered` (resolve, warning: a numeric
  reference to an anchor with no counter); `tmark.codes()` and
  `schema("diagnostic")` list it, and the writers render the anchor's
  text in place of the number, so a TeXSmith template that keyed on `?`
  or `Figure ?` for such a reference sees prose now.

### How TeXSmith consumes the crate

- `~/texsmith/vendor/tmark` is a symlink to `../../tmark` (this
  checkout); `pyproject.toml` declares `tmark` as a dependency with a
  `[tool.uv.sources]` path entry `vendor/tmark/crates/tmark-py`
  (editable), so `uv sync` builds the wheel through maturin against
  whatever this working tree holds — including uncommitted changes and
  the branch that happens to be checked out. Switch branches here and
  TeXSmith's next `uv sync` follows.
- TeXSmith's CI (`.github/workflows/ci.yml`) checks this repository out
  at `ref: texsmith-migration`, `path: vendor/tmark`, installs Rust, and
  runs `uv sync --all-groups --frozen`. When this branch merges, that
  `ref` must move to `main` (or a tag) in the same change, or their CI
  builds a stale branch.
- The version handshake: `tmark.version()` is the workspace version; the
  IR root carries it; `resolve` and `edit` refuse a document from another
  version. There is no PyPI release; the compatible-release pin of their
  plan (D8) is not in force yet.

### Pitfalls learned this round

- **Merge conflicts between waves on `diagnostic.rs`.** Every wave that
  added a code touched the same three tables in
  `tmark-ir/src/diagnostic.rs` (the enum, `stage()`, `doc()`) and the
  `codes` fixture. Merging two waves conflicted there repeatedly;
  resolve by keeping both sides in the order the enum lists them, then
  regenerate `schema("diagnostic")` (`cargo run -p tmark-ir --example
  schema`) and run the `codes` tests. Landing the code-adding wave first
  and rebasing the others is cheaper than merging.
- **The `PENDING` list of `tmark-fmt/tests/mkdocs.rs`.** The D0 gate
  lists fixtures whose `Mkdocs` spelling the parser cannot read back yet
  and *requires them to fail*; when a parser wave lands the spelling,
  the test fails with "now passing, remove from PENDING". The list is
  empty today. Do not put a fixture there to silence a real regression.
- **MSRV 1.80.** `Cargo.toml` pins `rust-version = "1.80"`;
  `Option::is_none_or` (1.82) crept into the `Mkdocs` profile and was
  reverted (`34dec57`). Clippy on a newer toolchain suggests it; refuse
  the suggestion or raise the MSRV deliberately.
- **Snapshots after every parser change.** `tmark-writers/tests/` holds
  349 insta snapshots (`fixtures.rs`, `web.rs`), `tmark-fmt/tests/mkdocs.rs`
  more. Any lowering change moves several of them; review with `cargo
  insta review` (or read the diff of `INSTA_UPDATE=always`), never accept
  blindly. A new fixture needs one `INSTA_UPDATE=always` run, then a
  reading of the three new files. The fixture `ir` blocks are regenerated
  with `scripts/fixture-ir.py` after the `dump` example is built.
- **`__pycache__` files ended up tracked** by the first `py` commit and
  had to be untracked (`9bf13d1`), with the `.so` `maturin develop` drops
  into `python/tmark`. `.gitignore` now lists both; check `git status`
  after a `maturin develop` before committing anyway.
- **The snap `typst` binary cannot read `/tmp`.** A body written to
  `/tmp` and compiled with the snap `typst` fails on the file, not on
  the body; compile from a directory under `/home` (the scratchpad of
  an agent is under `/tmp`). TeXSmith's `status.md` notes the same for
  its examples.
- **The rate limit kills parallel agents.** The waves ran as parallel
  agents, one worktree each, and the session rate limit killed some of
  them mid-task (this handoff itself was restarted after one); a killed
  agent leaves a dirty worktree that the next one must inspect before
  building on it. Run at most two or three waves at a time, commit
  small, and write the design note of a wave before its last commit
  rather than after.
- The previous round's pitfalls (`rtk proxy`, `cargo` on `PATH`, `cargo
  fmt` moving patch anchors, `preserve_order`, `lsp_types::Uri`, node
  ids restarting per file) all held again.

### Review mandates before merging to `main`

**Done** (2026-09-14 round, see the new handoff at the top of this file):
mandate 1 became `design/reviews/07-spec-conformance-migration.md` (0
blocking, 11 major, 14 minor) plus `08-spec-consistency.md` (5 blocking,
15 major, 19 minor, a review of the spec's internal consistency that
mandate 1 did not ask for but the text needed); every finding is triaged
in both files' own §Triage. Mandates 2–4 (writers-versus-legacy, the
`tmark-py` API surface, the `Mkdocs` profile) are still open — nothing
below ran them.

Run these as separate reviewer agents (two at a time, see the rate-limit
pitfall), each writing `design/reviews/07-…` and following incrementally;
act on the findings before the pull request.

1. **Spec conformance pass over C27–C42**: the wording written into
   `spec/tmark.md` (§Tabs, §Div, §TeX logos, §Emoji and icon shortcodes,
   §ProgressBar, §HorizontalRule, §Raw, §Header, §Inline text, §Foreign
   directive, the lexical grammar additions, the C27/C28/C30 rows) against
   the code that implements it (`lower/sugar.rs`, `lower/table_yaml.rs`,
   `md_in_html`, `is_foreign_directive`, `Label::implicit`, the
   `deprecated` fixes) and the fixtures. The rows were written by the
   same agents that wrote the code, in the same commits.
2. **Writers review against TeXSmith's legacy `.tex`** on the parity
   corpus (`~/texsmith/tests/parity/corpus.yml`, `scripts/parity.py diff`):
   every difference is either on the allow-list with a reason
   (`\tsdivider`, contract macros, blank-line runs, zero-width collapse)
   or a bug in one of the two paths. The escapers, the table layout
   (`\tabcolsep` discount, `longtable`), captions (`\label` after
   `\caption`, short captions) and the Greek subscript table (a legacy
   bug reproduced on purpose) deserve a line each.
3. **API review of `tmark-py`**: the surface of `_tmark.pyi` against
   `09-bindings.md` (the `options` dicts, the `TypeError` / `ValueError`
   split, `Resolved` as a frozen handle, `lower_web`'s return, the
   `Loader` exception path, GIL release), what TeXSmith's passes call in
   practice, and what a second consumer (Zensical, the LSP preview) would
   need. Decide the console script and the PyPI name before the first
   tag.
4. Smaller, if capacity remains: the `Mkdocs` profile's fallback rules
   against a Material site (every "falls back to canonical" row of
   `04-printer.md` shows the canonical spelling literally on the web);
   the `lower_web` wrappers against the plugin's CSS; `03-ir.md`
   §Identity versus X6.

### Commands

```sh
export PATH=$HOME/.cargo/bin:$PATH
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
cargo run -q -p tmark-ir --example schema && cargo run -q -p tmark-ir --example registries && git diff --exit-code
cargo insta review                                                             # after a parser or writer change
cargo build -p tmark-syntax --example dump && python3 scripts/fixture-ir.py    # then review the diff
cargo run -q -p tmark-cli -- write FILE --to latex --map                       # Body as JSON
cargo run -q -p tmark-cli -- lower FILE --to web --bib refs.bib                # the MkDocs page
cargo run -q -p tmark-cli -- lint --fix --diff FILE
uv venv && . .venv/bin/activate && uv pip install maturin pytest && maturin develop -m crates/tmark-py/Cargo.toml && pytest crates/tmark-py/tests
python crates/tmark-py/scripts/gen_stubs.py                                    # after a signature or docstring change
(cd ~/texsmith && uv sync --all-groups && uv run scripts/parity.py render --reader tmark)
```

## Previous handoff: end of the milestone 3 round and migration wave 1 (2026-09-11)


Written by the agent that implemented most of M3 on top of M1–M2, for the
agent that takes over. Read `AGENTS.md`, then this file, then
`11-roadmap.md`, then `design/reviews/`. Everything below is opinion from
the inside of the work: verify it, do not trust it.

### State of the repository

- `main`, workspace green: `cargo test --workspace`, `cargo clippy
  --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `npm test` in `editors/vscode`. Pushed to `origin/main`.
- New since M2: `tmark-lsp` (server library plus binary, `crates/tmark-lsp`),
  the VS Code client (`editors/vscode/src/extension.js`, esbuild bundle,
  `bin/tmark-lsp` bundled by `npm run bundle:server`, `.vsix` verified
  with `vsce`), `tmark::Config` (`tmark.toml`), `tmark::parse_with`,
  `tmark::analyse`, `tmark::fixes`, `tmark lint --fix`, sub-spans in the IR
  (`SubSpan`: `RefItem.key_span`, `Attrs.id_span`, `CounterItem.key_span`),
  `tmark_ir::structural_json` (the one fixture normaliser), `find` and
  `nodes_at`, `plain_text` in `tmark-ir`, `Severity::as_str/parse`, the
  `fs` feature as a real opt-in, the grammar tables exported from
  `tmark-ir` (`examples/registries.rs` → `scripts/registries.json`).
- Reviews under `design/reviews/`: all six mandates have a report.
  `02-spec-conformance.md`, `04-ir-review.md`, `05-architecture.md` and
  `03-printer-critic.md` were written by reviewer agents;
  `01-parser-adversary.md` (a compact pass) and `06-performance.md` by
  the implementing agent after the reviewer agents died on usage limits
  (twice for the parser one). What was acted on is listed below.

### What the reviews said and what was done

From `04-ir-review.md` (all five changes landed): C1–C3 sub-spans, C4
`structural_json` + `SUGAR_FIELDS`, C5 `LineIndex` fixes (snap inside a
multibyte character, clamp before the line break; `line_end` added). Also
done from its side findings: each `AbbrDef` owns its line; `print_node`
exposed by `tmark-fmt`. Not done: node ids are not pre-order/dense
(`03-ir.md` §Identity overclaims; either fix the three allocation sites in
`tmark-syntax` or weaken the doc); `#id` on fence info strings is dropped
(spec question, `reviews/02` D2); `edit()` is single-shot and returns a
`String`.

From `05-architecture.md`: A1 (feature gate), A2 (documented in `01`), A5
(dependency comments), A6 (severity words), C3 (`analyse`, `parse_with`),
A4 (grammar tables from `tmark-ir`) done. A3 mostly done (`CaptionKind::prefix()`/`word()` used by the lint rule
and the outline, `Prefix::heading` replaces the collector's list; the
`Host` ↔ prefix mapping in `collect.rs` and the label words of
`hardcoded_number` remain). Not done: B4 (inventory schema, `tmark
schema inventory`, M4); C4 (drop `tmark-writers → tmark-fmt`, decide at
M4); the `ResolveOptions.bibliography` relativisation duplicated in the
CLI and `Config` (`pathdiff`/`relative_to`); `has_press_key` re-scans the
front matter in the LSP instead of reading `Document.front_matter`.

From `03-printer-critic.md` (11 under-escaping findings, over-escaping
measured as rare): U1–U8, U10 and U11 fixed with a fixture each
(`04-printer.md` §Implementation notes (milestone 3)); C25 decided in the
spec (`\"` and `\\` escape inside quoted values). U9 (unbalanced
parentheses in a raw argument) is challenge C26. The 93 TeXSmith pages,
the spec and the editor sample reach the fixed point and round-trip.

From `01-parser-adversary.md`: P1 is an upstream markdown-rs 1.0.0 panic
(unclosed fence in a list item followed by a list of another kind), now
caught in `Lowerer::tree` and reported as `parse-internal`; the tokenizer
itself is not fixed (`construct/document.rs` exit ordering; report it
upstream with `spec/conformance/diag-parse-internal.md`'s input). P2 and
P3 fixed. `reviews/06`: no fork overhead; the tokenizer is superlinear in
the number of blocks, in upstream too.

From `02-spec-conformance.md`: nothing fixed in code, by mandate. Its
ranking for M3 is the to-do list of the next pass; C18–C24 were added to
`12-spec-challenges.md`. The one I checked myself: D13 (anchors with an
undeclared prefix do not resolve) is the spec's lookup rule, not a bug —
now C24.

### What M3 still lacks (against `11-roadmap.md` §M3 and `08-lsp.md`)

Done since the first pass: the `press` schema merge, diagnostics of
included files under their own URI, references inside included files
resolved, outline and folding of asides and generated images, completion
of classes, front-matter paths and file paths, printer findings U1–U8,
U10, U11.

1. **A person installing the `.vsix` and trying it.** Everything is tested
   over the in-memory connection and the binary over stdio; nobody has
   opened VS Code. Expect small things: activation on `.md` files without
   `press` (the client sends them all; the server stays quiet — check
   that VS Code does not show "TMark" errors for a README), the output
   channel, the restart command.
2. **Printer U9** waits for C26 (`12-spec-challenges.md`); the parser-side
   observations at the end of `reviews/03` (`$5 and $6` is math,
   `x^2 and y^3` a superscript; the two-`Str` case is fixed) and the
   `reviews/02` ranking for M3 (anchors with undeclared prefixes, C24;
   `frontmatter-unknown-key` never fires; links and fences are not
   attribute hosts; deprecated front-matter groups dropped) are the
   parser's to-do list.
3. **The upstream tokenizer panic** (`reviews/01` P1) is guarded, not
   fixed: report it to markdown-rs with the input of
   `spec/conformance/diag-parse-internal.md`, or fix the exit ordering in
   `construct/document.rs` and drop the guard's fixture.
4. **Range formatting** (whole-document diff) and incremental text sync if
   the 1 MB case matters (`reviews/06`: it does not for chapters).
5. **Fixes beyond `deprecated`**: the audit's D5–D8 rows emit nothing;
   `caption-id-off-convention` could offer the conventional prefix;
   `deprecated-frontmatter-key` could move the key under `press`.
6. **Editor polish**: per-platform download of the binary (only the
   bundled or PATH binary today), a changelog entry when released, the
   `TMARK_DEV` variable in `launch.json` is unused.
7. **Included files in the editor**: an unsaved buffer of an included
   file is not seen by the analysis (it reads the disk); hover on a label
   of an included file shows its heading text, definition jumps there.

### Migration wave 1, worktree `fixes` (2026-09-11)

Implemented items 1–6, 8 and 9 of TeXSmith's
`specs/migration/examples-migration.md` §4 (decisions X7; challenges C27,
C28 filed, C29 closed). Where things live:

- `tmark_reference.rs` gained `footnote_start` (`[^key]` with no
  definition, keyed on `gfm_footnote_definitions`, never on the
  bibliography) and `caret_start` (`^[k1,k2]`); `attention.rs` refuses
  `^[` as an opener. Lowering in `lower/inline.rs` (`Node::TmarkReference`).
  The printer puts a space before a `Ref` that would fall under the X4
  guard (`tutor.^[key]` → `tutor. @key`) and prints digit-initial keys
  bracketed (C27).
- `lower/block.rs::lower_slash_block`: `/// latex` → `RawBlock`,
  `/// caption` family → `Caption` (options line parsed as YAML, body
  re-lowered with `shift_stops`); `Lowerer::generic_captions` lets
  `attach_captions` pick the kind from the float.
- `head.rs::parse_fence_info` returns `attrs: Attrs` (bare options plus a
  trailing `{…}` list); `fmt/block.rs::fence_attrs` prints braces only
  when there are classes or an id.
- `tmark_ir::yaml_edit::move_key` is the front-matter fix (line edit);
  `frontmatter::deprecated_key_target` the one table of moves.
- `lower/compat.rs`: the `compat-unsupported` scans. Narrow on purpose.

Not done, and why:

- `citation-shadowed-by-footnote` is still never emitted: a defined
  `[^key]` is a `Note` at tokenization time and the bibliography is
  unknown to the parser; the registry (worktree `registry`) can emit it
  by intersecting `Document.footnotes` labels with the bibliography keys.
- The `--diff` output has no `\ No newline at end of file` marker and
  compares lines (`str::lines`), which is enough for `lint --fix`.
- `paper/docs/cheese.md` keeps 3 `ref-unresolved`: its `.bib` lives one
  directory up and is named by `mkdocs.yml`, which tmark does not read
  (TeXSmith item 4).
- Item 7 (table model, X9) belongs to worktree `tables`.

Pitfalls met this round: `diff::lines` yields a trailing empty line for
text ending in `\n` (use `str::lines` + `diff::slice`); one-letter keys
are not keys (spec grammar), so `^[a,b]` is literal; `marker.split_at(len
- 1)` panics on a multibyte last character (the totality proptest caught
it); a fixture whose sugar hugs a word (`tutor.^[key]`) cannot share the
IR with its canonical (the printer inserts a space), so the fixture input
carries the space and a printer test covers the hugging case.

### Plan of attack for M4 (writers and preview)

Design: `07-writers.md`, with the architecture review's C4 amendment
(no CommonMark writer: `Profile::Mkdocs` in `tmark-fmt`; `tmark-writers`
holds `Writer`, `Body`, `Requires`, `SourceMap`, `html`, `latex`, `typst`).
Suggested order: HTML writer first (the CommonMark suite compares HTML,
`10-testing.md` §2, and the LSP preview can show it), then LaTeX against
TeXSmith's output on its docs, then Typst with the in-process preview
(ADR 0005). The printer is ready for TeXSmith's corpus (93 pages at the
fixed point, `04-printer.md`); the HTML writer can reuse the CommonMark
suite's expectations. Keep the corpus loop of §Commands as the M4 gate.

### Pitfalls learned this pass

- **The shell.** `cargo` is not on `PATH` in the agent's non-interactive
  shell: `export PATH=$HOME/.cargo/bin:$PATH`. zsh expands a bare `=====`
  as a command. The `rtk` hook filters command output aggressively (test
  results and clippy findings vanish); prefix with `rtk proxy` to see
  everything, and read `~/.local/share/rtk/tee/*.log` when in doubt.
- **`rustfmt` versus Python patch scripts, again.** Every `cargo fmt` moves
  the anchors; patch, then format, then verify with a build — never
  format between writing a patch script and running it. Two commits in
  this pass were amended because a patch silently failed after a format.
- **`serde_json` `preserve_order`.** `Value::Object` is an `IndexMap`;
  `Map::remove` swaps the last key into the hole, `shift_remove` keeps
  order. `structural_json` depends on this to leave the fixtures' key
  order untouched.
- **`lsp_types::Uri` has interior mutability**; clippy refuses it as a map
  key (`mutable_key_type`). Key maps by `uri.as_str()` and keep the `Uri`
  in the value; `WorkspaceEdit.changes` is built at the end by `collect`.
- **Node ids restart in every included file.** `Resolution::Label { target }`
  alone does not identify a label; look labels up by id (`Labels::get`).
- **`walk`'s `NodeRef` lifetime** is now tied to the document
  (`walk<'a>(doc: &'a Document, f: impl FnMut(NodeRef<'a>))`), which is what
  lets `find` and `nodes_at` return nodes. Closures that collected
  `NodeRef`s before could not.
- **The counter item deprecation message** says "write `#(prefix:key)`"
  while the canonical spelling (and the fix) is `{counter}(prefix:key)`.
  Harmless, but pick one.
- **Reviewer agents and usage limits.** Six parallel reviewers were killed
  by a session limit, then three by an "out of credits" error on the
  second attempt. Launch them two at a time, and write the report file
  early and incrementally so a kill loses less.

### Commands

```sh
export PATH=$HOME/.cargo/bin:$PATH
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all --check
cargo run -q -p tmark-ir --example schema && cargo run -q -p tmark-ir --example registries && git diff --exit-code
cargo build -p tmark-syntax --example dump && python3 scripts/fixture-ir.py   # then review the diff
cargo run -q -p tmark-cli -- check spec/tmark.md
cargo run -q -p tmark-cli -- lint --fix FILE
cargo run -q -p tmark --example fixes -- FILE                                  # what --fix would do
cargo run --release -q -p tmark-syntax --example bench -- spec/tmark.md
(cd editors/vscode && npm install && npm test && npm run build:grammar && npm run bundle:server && npm run package)
# corpus fixed point: for f in $(find /home/ycr/texsmith/docs -name '*.md'); do tmark fmt "$f" > /tmp/a.md; tmark fmt /tmp/a.md | cmp -s - /tmp/a.md || echo "$f"; done
code --install-extension editors/vscode/vscode-tmark-0.1.0.vsix
```
