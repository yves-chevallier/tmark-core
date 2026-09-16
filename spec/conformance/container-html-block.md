# `/// html | <selector>` container

Spec §Div and Appendix "PyMdownX compatibility profile":
`pymdownx.blocks.html` wraps a Markdown body in the element a CSS-like
selector names (`div[class='two-column-list']`, `div#id.cls`,
`div[style='…']`). It is the same `Div` node `<div class="…" markdown>`
lowers to, so the `mkdocs` profile prints it back as that HTML and a
MkDocs page keeps the layout. A bare `/// html` with no selector stays
the raw fence of the deprecation schedule.

## input

```md
/// html | div[class='two-column-list']

1. one
2. two

///
```

```md
<div class="two-column-list" markdown>

1. one
2. two

</div>
```

## canonical

```md
::: div {.two-column-list}
1. one
2. two
:::
```

## ir

```json
{
  "blocks": [
    {
      "type": "Div",
      "name": "div",
      "content": [
        {
          "type": "OrderedList",
          "items": [
            {
              "content": [
                {
                  "type": "Para",
                  "content": [
                    {
                      "type": "Str",
                      "text": "one"
                    }
                  ]
                }
              ]
            },
            {
              "content": [
                {
                  "type": "Para",
                  "content": [
                    {
                      "type": "Str",
                      "text": "two"
                    }
                  ]
                }
              ]
            }
          ],
          "start": 1,
          "style": "decimal"
        }
      ],
      "attrs": {
        "classes": [
          "two-column-list"
        ]
      }
    }
  ]
}
```

## diagnostics

```text
deprecated @ 1:1-6:4
```
