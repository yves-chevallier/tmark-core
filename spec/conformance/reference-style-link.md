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

## input

```md
[]{#claim}

See [the claim][claim], not [that][no-such-label].
```

## canonical

```md
[]{#claim}

See [the claim][claim], not [that][no-such-label].
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
    }
  ]
}
```

## resolution

```text
deprecated @ 3:5-3:23
```

## latex

```latex
\phantomsection\label{claim}

See \hyperref[claim]{the claim}, not [that][no-such-label].
```
