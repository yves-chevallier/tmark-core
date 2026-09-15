# 04 — Canonical printer and formatter

Crate: `tmark-fmt`. Input: `&Document`, a `Profile`. Output: `String`.
Spec: §Round-trip and source spans, the "Canonical" column of §Node
catalogue, Appendix "PyMdownX compatibility profile", Appendix "Deprecation
schedule".

## Contract

1. **Fixed point.** `parse(print(doc)) == doc` modulo spans and ids, for
   every document the parser produces. This is a property test, not a hope.
2. **Idempotent.** `print(parse(print(doc))) == print(doc)`.
3. **Deterministic.** No dependence on the environment, the clock, a hash
   seed. Attribute order, fence lengths, marker characters and caption
   position are normalised; everything else is preserved.
4. **Prose is not re-wrapped.** `SoftBreak` prints a newline. Line width is
   the author's.
5. **Front matter is copied byte for byte.** The printer emits
   `FrontMatter.raw` and never re-serialises YAML.
6. **Comments, raw passthroughs and escapes survive.** Text that looked like
   syntax but was not recognised is `Str` and prints as typed, re-escaped
   only where the canonical context requires it.

## What normalises

| Aspect | Canonical |
| ------ | --------- |
| Emphasis | `*x*`, `**x**` |
| Small caps, del, mark, sub, sup, keys, underline | roles: `{sc}[x]`, `{del}[x]`, `{mark}[x]`, `{sub}[x]`, `{sup}[x]`, `{keys}[ctrl+s]`, `{underline}[x]` |
| Headings | ATX `#` … `######`, attributes at end of line, `#id` first |
| Lists | `-` bullets, `1.` numbers with the source start, two-space nesting |
| Fences | backticks, length = longest inner backtick run + 1, minimum 3 |
| Containers | `:::`, nested fences longer by one per level |
| Callouts | `::: type {title="…"}`; `!!!` only under the `mkdocs` profile |
| Captions | after the block, `Kind: text {#id}` |
| References | `@key` bare; `@[…]` as soon as an item has a space or there are several |
| Index and counters | `{index}[…]`, `{counter}(prefix:key)` — the roles are canonical, `#[…]`/`#(…)` are sugar. (Spec challenge C6 asks whether the sigils should be canonical instead; until settled, the roles win because the printer must pick one.) |
| Raw | `{raw latex}(…)`, `latex raw` fences |
| Attributes | `{#id .class key=value}`; values quoted only when they contain whitespace or `}` |
| Math | `$…$`, `$$ … $$` on their own lines |
| Includes | `{include}(path)` alone on its line |
| Deprecated spellings | rewritten to their replacement (Appendix Deprecation schedule) |

## Profiles

```rust
pub enum Profile { Canonical, Strict, Mkdocs }
```

- `Canonical`: the table above.
- `Strict`: `Canonical` plus X1 and X2 rewritten to class-C forms
  (`{sc}[x]` stays a role; `---` becomes `{raw latex}(\clearpage)`? No: the
  strict profile keeps `---` as a divider and only *disables* the small-caps
  reading of `__x__`, so nothing to rewrite; the difference is in what the
  parser accepts). Rejects Appendix-PyMdownX sugar at parse time, which is a
  parser option, not a printer one. The printer difference between
  `Canonical` and `Strict` is therefore empty today; the variant exists so
  `tmark fmt --profile strict` can be a parse-with-strict-then-print.
- `Mkdocs`: emits the spellings a MkDocs Material site and TeXSmith 0.6
  render (`mkdocs.rs`, one function per construct; the table below). Every
  other construct prints as `Canonical`: the site shows it literally, which
  is the spec's degradation contract. This is the D0 bridge of the TeXSmith
  migration (`tmark fmt --profile mkdocs` on a canonical file builds with
  TeXSmith 0.6) and the spelling table `lower_web` reuses
  (`web-profile.md`).

Profiles are a *table* (construct → spelling function), not subclasses.

| Construct | `Mkdocs` spelling | Falls back to canonical when |
| --------- | ----------------- | ---------------------------- |
| `{mark}[x]`, `{del}[x]`, `{sub}[x]`, `{sup}[x]` | `==x==`, `~~x~~`, `~x~`, `^x^` | the content is empty, starts or ends with whitespace, holds a node of the same delimiter family, or follows that delimiter |
| `{sc}[x]` | `__x__` | as above, or an alphanumeric or `_` touches the run on either side (emphasis flanking) |
| `{keys}[ctrl+s]` | `++ctrl+s++` | a key holds anything but letters, digits, `-`, spaces, `"` (the run is read as Markdown before the split) |
| `{code lang=py}[x]` | `` `#!py x` `` | the text is empty, starts with whitespace or spans lines |
| `{index registry=r main=true}[a][b]` | `{index:r}[a][b]{b}` | the registry is not a bare name; a plain `{index}[…]` is canonical already |
| `{counter}(p:k)` | `#{p:k}` | the prefix is neither predeclared nor in `declare.counters` (the parser reads `#{…}` only for a declared prefix, C16) |
| `{aside side=left}[x]` | `{margin}[x]{l}` (`r`, `o`, `i`) | the aside holds blocks (`::: aside` container) |
| `{raw latex}(x)` (`typst`, `html`) | `{latex}[x]` | the text holds a bracket or a line break, ends with `\`, or the format is not a backend name |
| `@key` citation | `[^key]`; `@[k1; k2]` → `^[k1,k2]` | an item has a prefix, suffix or `-`; a key is a footnote label of the document; the key is a label (see below) |
| `@gls:term` | `[](gls:term)` | |
| `@fig:x`, `@fw:x`, `@alias:key`, `@id` of an anchor | `@…` unchanged (TeXSmith 0.6 reads them) | |
| `::: type {title="T" .cls collapsed=true}` | `!!! type cls "T"`, `??? type` (collapsed), `???+ type` (open), body indented four spaces | an `#id`, another attribute, a `"` or a line break in the title, an empty body |
| `Table: … {#id}` after a table | the same line *before* the table (then the table, then its `yaml table-config`) | the block printed before the caption is a float: the parser attaches a caption to the previous host first |
| `Figure:`, `Listing:` lines | `/// caption` block with `attrs: {id: …}` and the text | the attributes hold more than an id, or a text line is `///` |
| `latex raw` fence | `/// latex … ///` | another format, or a `///` line inside |
| `{include}(f)` | `--8<-- "f"` | a `base`, or a `"` in the path |
| `[]{#id}` anchor | `[](){#id}` (the only spelling `attr_list` turns into an element, hence the only one `mkdocs-autorefs` can register) | the span carries content: `[x]{#id}` is an attributed phrase |
| `$…$`, `$$…$$`, `---`, `::: figure`, `::: aside`, `::: name`, `yaml table`, `{underline}`, `[x]{…}`, `[text][id]`, `{{ var }}`, `@[see key, p. 3]`, `@doi:…` | canonical | |

Citation versus label is the spec's lookup rule (§Registries) applied to
what the document itself declares, since the printer has no `Resolved`:
`@a:b` is a label when `a` is a predeclared prefix, a `declare.counters`
key or a `sources.crossrefs` alias, a glossary reference when `a` is
`gls`, a bibliography key otherwise; a bare `@key` is a label when an
attribute list of the document defines `#key`, a citation otherwise. A
node printed on its own (`print_node_with`) knows the predeclared
prefixes only.

## Local edits

```rust
pub enum Replacement { Block(Block), Inline(Inline), Text(String) }
pub struct NodeEdit { pub id: NodeId, pub replacement: Replacement }
pub fn edit(text: &str, doc: &Document, edit: NodeEdit) -> String
pub fn edit_many(text: &str, doc: &Document, edits: Vec<NodeEdit>) -> Result<String, EditError>
```

Prints `replacement` in the context of its parent (block vs inline, current
indentation for nested blocks) and splices it into `doc[id].span`. Every
other byte is untouched. This is how the LSP renames a label, rewrites a
citation, converts sugar to canonical on a code action. A whole-document
`format` is the same operation applied to the root.

`edit_many` is the batch form: the spans are located first, sorted, and
spliced from the end of the text so that earlier offsets stay valid. The
spans must be disjoint; an unknown node (`NotFound`), a span outside the
text or in another file (`OutOfRange`) or two edits that touch the same
bytes (`Overlap`, which includes a node and one of its descendants) is an
error and nothing is applied. `Replacement::Text` splices literal text: a
lowering whose output has no IR node (the HTML wrappers of the MkDocs
companion, `web-profile.md`) builds its edits with it. Property tests:
splicing any set of disjoint nodes with their own source is the identity;
splicing any set of top-level blocks of a canonical text with their own
printed form is the identity (adjacent lists excepted: their alternating
marker is a property of the sequence, not of one list).

## Implementation notes

- One module per node family (`inline.rs`, `block.rs`, `attrs.rs`,
  `frontmatter.rs`), each a set of functions `fn para(w: &mut Out, node:
  &Para, ctx: &Ctx)`. No trait per node.
- `Out` tracks column and indentation for nested containers and list items;
  it is the only state.
- Escaping is contextual and minimal: `\@` and `\#` only when the following
  characters would otherwise form a reference or a definition; `\{` only
  before a role or attribute head; standard CommonMark escapes elsewhere.
  The fixed-point test catches every miss.
- The printer is also the reference for the spec: when a canonical spelling
  is ambiguous in prose, the fixture wins and the spec is amended
  (`12-spec-challenges.md`).

## Tests

- `proptest`: generate documents from the IR (a generator per node, bounded
  depth), assert the fixed-point and idempotence properties.
- Conformance fixtures: `canonical` block of every fixture must print back
  identically.
- Snapshot tests for the `Mkdocs` profile on the fixture corpus
  (`tests/mkdocs.rs`, `insta`), and the D0 gate: the `Mkdocs` text of
  every fixture re-parses to the canonical IR and prints again unchanged.
  Fixtures whose shipping spelling the parser does not read back yet
  (`[^key]` citations, decision X7; `[](gls:term)`, `{index}[…]{b}`,
  `/// latex`, `/// caption`, challenge C20) are listed in the test as
  pending and must fail until the parser side lands; the list shrinks as
  it does.

## Implementation notes (milestone 1)

Decisions the printer had to take beyond the table above; each is a
candidate line for the spec's "Canonical" column.

- **Attribute values** are quoted only when they hold whitespace, `}`, `"`,
  `=` or are empty: `{title=Folded collapsed=true}`,
  `{title="LaTeX toolchain"}`.
- **Fence options** are always quoted (`title="x" linenums="1"`), because
  the PyMdownX syntax they come from requires it and the `mkdocs` profile
  must not have to touch them.
- **Pipe tables** are printed aligned: cells padded to the column width,
  delimiter cells as wide as the column with at least three dashes, alignment
  colons kept. A table is a pipe table when its model is plain (leaf
  columns, no spans, groups, separators, footer, width or placement), a
  `yaml table` fence otherwise, in a flow YAML style: bare column names,
  `{name: …, columns: […]}` groups, `- [a, b]` rows, `{value: …, rows: n,
  cols: n}` spanning cells, `~` absorbed slots, `{separator: true, label:
  …}` separators; scalars are plain when safe, double-quoted otherwise.
  The row shapes are the Python ones (wave 1 notes below).
- A listing whose language defaults to another node word keeps the word:
  `mermaid code`.
- A `yaml table-config` fence prints right after its table, before the
  caption line (spec §Table, rung 3); the parser orders the IR that way
  whatever the source order.
- An aside with more than one block, or a block that is not a `Plain`,
  prints as a `::: aside {side=…}` container; an inline aside prints as the
  role.
- Generated images (`Image` with `generate=` and `code=` attributes) print
  as their `<lang> image` fence with the remaining attributes as options.
- Block quote attributes (`{.epigraph}`) print on their own line after the
  quote's last block, inside the `> ` prefix.
- Reference keys print bare when the reference has one item without prefix,
  suffix or `-` and was written bare; bracketed otherwise. A lone `@[key]`
  keeps its brackets (`Ref.bracketed`): for a citation they carry the
  parenthetical meaning under `citations.narrative` (spec §Cite, C51), and
  the deprecated `[^key]` lowers as bare, so its fix stays `@key`. Autolink
  literals and `mailto:` links whose text is the address print bare.
- The round-trip and fixed-point tests compare documents modulo ids, spans
  and the two fields that record a spelling (`Caption.position`,
  `Ref.bracketed`), exactly as the conformance runner does. Adjacent `Str`
  nodes are merged by the parser so that text runs never depend on how the
  tokenizer split them.
- Escaping is conservative and contextual (`escape.rs`): sigils, braces,
  brackets and emphasis markers are escaped only where they would form a
  construct at that position; the line-start rules defuse block starts
  (`#`, `>`, `-`, `1.`, `Table:`, `:   `, `!!!`, `:::`) in the first line
  of a text run that opens a block. Literal fallbacks in the parser decode
  their backslash escapes, so `format` is idempotent on them.
- Profiles: `Strict` prints like `Canonical` (the difference is at parse
  time); `Mkdocs` is the table above (wave 1 of the TeXSmith migration,
  ahead of milestone 5's writers).

## Implementation notes (milestone 3, after the printer review)

`reviews/03-printer-critic.md` measured over-escaping as rare on real
prose and found eleven ways to break the round-trip. Fixed:

- U1: the block-start rules apply to the first run of a block even after
  a marker (`- \# x`, `# \# x`), through `Context::block_start`.
- U2: `|` inside a code span of a pipe cell prints `\|`; column names are
  escaped like cell text.
- U3: link and image destinations with whitespace, `<`, `>` or unbalanced
  parentheses print as `<…>` (`inline::destination`).
- U4: quoted attribute and fence-option values encode `\` and `"`
  (`attrs::quoted`); the spec gained the `\"`/`\\` escapes (challenge
  C25, closed). Fence options are encoded once more because CommonMark
  processes backslash escapes in an info string before TMark reads it.
- U5: a list right after a list of the same kind takes the other marker
  (`*`, `)`), so the two do not merge on re-parse.
- U6: a table with a line break in a cell is not plain: it prints as a
  `yaml table` fence with `\n` in the scalar.
- U7: YAML scalars that YAML would type (floats, exponents, `+1`, `0x1F`,
  the null and boolean words in any case) are quoted; plain integers stay.
- U8: a `[` right after an `IndexEntry` is escaped (`Context::after_index`).
- U10: GFM autolink literals are defused (`http\://`, `www\.`,
  `me\@x.y`); a `www.` link whose text is its address prints bare.
- U11: link titles encode `\` before `"`.

Open: U9 (a `RawInline` argument with unbalanced parentheses has no
spelling; challenge C26). The TeXSmith documentation (93 files), the spec
and the editor sample all reach the fixed point and round-trip
structurally (`scratchpad` loop, see `13-handoff.md` §Commands).

## Implementation notes (wave 1, tables — decision X9)

The `yaml table` fence prints the leaf matrix back in the shapes
TeXSmith's `parse_table` reads (design 03 §Tables), so that every fence of
the corpus round-trips modulo YAML normalisation:

- `table:` first (`width` when not `auto`, `placement`, `long` when not
  `auto`), then `columns`, `rows`, `footer`. Keys use the YAML spellings
  `width-group` and `double-rule`; `align` prints its long form.
- A column is its bare name when it has no layout, else
  `{name, columns, align, width, width-group}` in that order; groups
  nest in flow style.
- A positional row walks the top-level columns: a leaf's cell prints as
  itself; a rich cell (span or alignment) prints as itself wherever it
  starts and the slots its column span absorbs are skipped; a plain cell
  with leaves left in its column opens a list of the remaining leaves;
  every slot a row span absorbs prints `~`. An empty cell is `~`.
- A named row prints `Label: {Column: value}` for the data columns that
  hold something (a list for a group); the explicit `{label, cells}` form
  is used only when the label is the word `separator`. A row whose named
  spelling does not exist (unnamed or duplicated column names, a rich
  label, a span crossing its column) prints positionally.
- A `Table` or `TableConfig` with a kept `source` prints that text
  verbatim under its node word; a payload that was not YAML is a
  `CodeBlock` whose `lang` is the info string (`yaml table`), like
  `grid table`.
- `tests/tables.rs`: a generated valid model prints and parses back
  equal with no diagnostic (proptest), and TeXSmith's table corpus
  (`tests/data/yaml-tables.md`, copies) round-trips and is a fixed point.

