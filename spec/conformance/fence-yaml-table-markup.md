# Inline markup in a `yaml table` cell

Spec §Table rung 5 and §Round-trip and source spans: a cell of a `yaml`
`table` payload is parsed as inline Markdown, and the nodes it yields carry
spans of the payload — the cell is located by its single occurrence there.
Before that they were anchored at the start of the fence, so the `mkdocs`
profile read a code span back as the first bytes of the fence line and a
table of backticks rendered as ``` ``` ``` (challenge C66).

## input

````md
```yaml table
columns: [Op, {name: Sens, columns: [Court, Long]}]
rows:
  - ["`+`", [plus, "Addition *simple*"]]
  - ["`-`", [moins, "Voir @sec:ops"]]
```

## Les opérateurs {#sec:ops}
````

## canonical

````md
```yaml table
columns:
  - Op
  - {name: Sens, columns: [Court, Long]}
rows:
  - ["`+`", [plus, Addition *simple*]]
  - ["`-`", [moins, Voir @sec:ops]]
```

## Les opérateurs {#sec:ops}
````

## ir

```json
{
  "blocks": [
    {
      "type": "Table",
      "model": {
        "settings": {
          "width": "auto"
        },
        "columns": [
          {
            "type": "Leaf",
            "name": "Op"
          },
          {
            "type": "Group",
            "name": "Sens",
            "columns": [
              {
                "type": "Leaf",
                "name": "Court"
              },
              {
                "type": "Leaf",
                "name": "Long"
              }
            ]
          }
        ],
        "rows": [
          {
            "type": "Data",
            "cells": [
              {
                "content": [
                  {
                    "type": "Code",
                    "text": "+"
                  }
                ]
              },
              {
                "content": [
                  {
                    "type": "Str",
                    "text": "plus"
                  }
                ]
              },
              {
                "content": [
                  {
                    "type": "Str",
                    "text": "Addition "
                  },
                  {
                    "type": "Emph",
                    "content": [
                      {
                        "type": "Str",
                        "text": "simple"
                      }
                    ]
                  }
                ]
              }
            ]
          },
          {
            "type": "Data",
            "cells": [
              {
                "content": [
                  {
                    "type": "Code",
                    "text": "-"
                  }
                ]
              },
              {
                "content": [
                  {
                    "type": "Str",
                    "text": "moins"
                  }
                ]
              },
              {
                "content": [
                  {
                    "type": "Str",
                    "text": "Voir "
                  },
                  {
                    "type": "Ref",
                    "items": [
                      {
                        "key": "sec:ops"
                      }
                    ]
                  }
                ]
              }
            ]
          }
        ]
      }
    },
    {
      "type": "Header",
      "level": 2,
      "content": [
        {
          "type": "Str",
          "text": "Les opérateurs"
        }
      ],
      "attrs": {
        "id": "sec:ops"
      }
    }
  ]
}
```
