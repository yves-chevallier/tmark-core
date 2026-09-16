# Reference-style link, deprecated

Spec §Ref, "reference-style form", and Appendix "Deprecation schedule".
`[text][id]` with no link definition is what a documentation corpus written
for MkDocs holds, because `mkdocs-autorefs` resolves it across the pages of
a site. TMark reads it as the textual reference `[text](#id)` makes — and
no more than that: it is a compatibility spelling (class E), deprecated in
favour of the canonical one, which is a link to every Markdown renderer
where this is brackets. Where it refers, `deprecated` is reported with
`[text](#id)` as its fix; the web lowering writes the canonical spelling
too, so the site receives Markdown any parser understands.

A key that is no label is not a reference — the node renders the brackets
CommonMark reads there, says nothing, and is not deprecated either: the
spelling is an ordinary sentence ending a bracketed aside with a bracketed
word.

The text may carry markup, as it may in the canonical spelling: the
tokenizer leaves the brackets in the text nodes around it and the lowering
cuts the spelling out of them. What it will not do is read a spelling that
is not verbatim in the source, or that runs over a line end, or that holds
a link or an image — CommonMark's reading stands there.

## input

```md
[]{#claim}

See [the claim][claim], not [that][no-such-label].

The directive [`#include`][claim] keeps its code span.
```

## canonical

```md
[]{#claim}

See [the claim][claim], not [that][no-such-label].

The directive [`#include`][claim] keeps its code span.
```

## ir

```json
{
  "blocks": [
    {
      "type": "Para",
      "content": [
        {
          "type": "Span",
          "attrs": {
            "id": "claim"
          }
        }
      ]
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "See "
        },
        {
          "type": "Link",
          "content": [
            {
              "type": "Str",
              "text": "the claim"
            }
          ],
          "target": {
            "type": "Reference",
            "value": "claim"
          }
        },
        {
          "type": "Str",
          "text": ", not "
        },
        {
          "type": "Link",
          "content": [
            {
              "type": "Str",
              "text": "that"
            }
          ],
          "target": {
            "type": "Reference",
            "value": "no-such-label"
          }
        },
        {
          "type": "Str",
          "text": "."
        }
      ]
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "The directive "
        },
        {
          "type": "Link",
          "content": [
            {
              "type": "Code",
              "text": "#include"
            }
          ],
          "target": {
            "type": "Reference",
            "value": "claim"
          }
        },
        {
          "type": "Str",
          "text": " keeps its code span."
        }
      ]
    }
  ]
}
```

## resolution

```text
deprecated @ 3:5-3:23
deprecated @ 5:15-5:34
```

## latex

```latex
\phantomsection\label{claim}

See \hyperref[claim]{the claim}, not [that][no-such-label].
```
