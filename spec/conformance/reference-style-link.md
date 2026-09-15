# Reference-style link

Spec §Ref, "reference-style form". `[text][id]` with no link definition is
what a MkDocs site writes to point at an anchor it does not know the page
of. TMark reads it as a textual reference to the label `id`: the same node
as `[text](#id)`, spelled so that `mkdocs-autorefs` still resolves it on
the web. A key that is no label is not a reference — the node renders the
brackets CommonMark reads there, and says nothing.

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
```

## latex

```latex
\phantomsection\label{claim}

See \hyperref[claim]{the claim}, not [that][no-such-label].
```
