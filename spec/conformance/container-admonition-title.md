# Markup in an admonition title

Spec §Admonition and §Round-trip and source spans: the title is an
attribute, its value is parsed as inline Markdown, and the nodes it yields
carry spans of the value itself when the value is a verbatim slice of the
line. Before that they were anchored at the start of the block, so the
`mkdocs` profile spliced a counter of the title at column 0 of the marker
line (challenge C66). A title the marker line cannot carry — a counter
becomes a `<span …>`, and PyMdownX reads a title up to the next `"` —
makes the profile write the `<div class="admonition …">` wrapper instead.

## input

```md
---
press:
  declare:
    counters:
      ex: {name: Exercice, format: "{n}"}
---

::: note {title="#(ex:un) : *Feu*"}
Corps.
:::
```

## canonical

```md
---
press:
  declare:
    counters:
      ex: {name: Exercice, format: "{n}"}
---

::: note {title="{counter}(ex:un) : *Feu*"}
Corps.
:::
```

## ir

```json
{
  "front_matter": {
    "raw": "---\npress:\n  declare:\n    counters:\n      ex: {name: Exercice, format: \"{n}\"}\n---",
    "keys": {
      "press": {
        "declare": {
          "counters": {
            "ex": {
              "name": "Exercice",
              "format": "{n}"
            }
          }
        }
      }
    }
  },
  "blocks": [
    {
      "type": "Admonition",
      "kind": "note",
      "title": [
        {
          "type": "CounterItem",
          "prefix": "ex",
          "key": "un"
        },
        {
          "type": "Str",
          "text": " : "
        },
        {
          "type": "Emph",
          "content": [
            {
              "type": "Str",
              "text": "Feu"
            }
          ]
        }
      ],
      "content": [
        {
          "type": "Para",
          "content": [
            {
              "type": "Str",
              "text": "Corps."
            }
          ]
        }
      ]
    }
  ]
}
```
