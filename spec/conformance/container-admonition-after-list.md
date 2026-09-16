# Admonition after a list

Spec §Admonition: the body of a `!!!` callout is the indented block that
follows its marker line, wherever the callout stands. A marker line that
closes a list (Python-Markdown reads it as one block, the list ends) keeps
its body all the same.

## input

```md
- [x] Vrai

!!! note
    Corps.

Fin.
```

```md
- [x] Vrai

::: note
Corps.
:::

Fin.
```

## canonical

```md
- [x] Vrai

::: note
Corps.
:::

Fin.
```

## ir

```json
{
  "blocks": [
    {
      "type": "BulletList",
      "items": [
        {
          "content": [
            {
              "type": "Para",
              "content": [
                {
                  "type": "Str",
                  "text": "Vrai"
                }
              ]
            }
          ],
          "task": "done"
        }
      ]
    },
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
      ]
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "Fin."
        }
      ]
    }
  ]
}
```
