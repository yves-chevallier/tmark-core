# Admonition, attribute list on the sugar spelling

Spec §Admonition: `!!! type` takes the attribute list of its canonical
`:::` spelling, at the end of the marker line. The bare PyMdownX words
before it stay classes (`!!! note inline {lines=5}`).

## input

```md
!!! note { lines=5 }
    Corps.
```

```md
!!! note {lines=5}
    Corps.
```

```md
::: note {lines=5}
Corps.
:::
```

## canonical

```md
::: note {lines=5}
Corps.
:::
```

## ir

```json
{
  "blocks": [
    {
      "type": "Admonition",
      "kind": "note",
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
      ],
      "attrs": {
        "kv": [
          ["lines", "5"]
        ]
      }
    }
  ]
}
```
