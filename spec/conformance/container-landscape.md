# Layout container: landscape

Spec §Div: `landscape` is a name of the closed container registry, a
`Div{name=landscape}` the paged backends render through the `tsdiv`
contract on pages of their own turned to landscape; no `container-unknown`.
Its content is ordinary Markdown: here a captioned table, the case the
container exists for, its caption line printed after the table as anywhere
else.

## input

```md
::: landscape
Table: A wide table. {#tbl:wide}

| Key | Value |
| --- | ----- |
| a   | 1     |
:::
```

## canonical

```md
::: landscape
| Key | Value |
| --- | ----- |
| a   | 1     |

Table: A wide table. {#tbl:wide}
:::
```

## ir

```json
{
  "blocks": [
    {
      "type": "Div",
      "name": "landscape",
      "content": [
        {
          "type": "Table",
          "model": {
            "settings": {
              "width": "auto"
            },
            "columns": [
              {
                "type": "Leaf",
                "name": "Key"
              },
              {
                "type": "Leaf",
                "name": "Value"
              }
            ],
            "rows": [
              {
                "type": "Data",
                "cells": [
                  {
                    "content": [
                      {
                        "type": "Str",
                        "text": "a"
                      }
                    ]
                  },
                  {
                    "content": [
                      {
                        "type": "Str",
                        "text": "1"
                      }
                    ]
                  }
                ]
              }
            ]
          }
        },
        {
          "type": "Caption",
          "kind": "table",
          "content": [
            {
              "type": "Str",
              "text": "A wide table."
            }
          ],
          "attrs": {
            "id": "tbl:wide"
          }
        }
      ]
    }
  ]
}
```
