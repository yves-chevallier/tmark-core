# A table cell that opens with `[`

Design 07 §Decisions: a `[` that opens a table cell is brace-protected
(`{[}`) by the LaTeX writer. The row break `\\`, booktabs' `\midrule` and
`\addlinespace` all take an optional argument and scan for it past the
line end, so a cell that starts with `[` — an unresolved reference-style
link (§Ref), or a literal `[note]` — would otherwise be read as that
argument (`Missing number, treated as zero`). Typst escapes `[` in markup
and has no such hazard.

## input

```md
| Directive | Role |
| --------- | ---- |
| [`#include`][preprocessor-include] | Pastes a file |
| [note] | A literal bracket |
```

## canonical

```md
| Directive                           | Role              |
| ----------------------------------- | ----------------- |
| [`#include`\][preprocessor-include] | Pastes a file     |
| [note]                              | A literal bracket |
```

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
            "name": "Directive"
          },
          {
            "type": "Leaf",
            "name": "Role"
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
                    "text": "["
                  },
                  {
                    "type": "Code",
                    "text": "#include"
                  },
                  {
                    "type": "Str",
                    "text": "][preprocessor-include]"
                  }
                ]
              },
              {
                "content": [
                  {
                    "type": "Str",
                    "text": "Pastes a file"
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
                    "type": "Str",
                    "text": "[note]"
                  }
                ]
              },
              {
                "content": [
                  {
                    "type": "Str",
                    "text": "A literal bracket"
                  }
                ]
              }
            ]
          }
        ]
      }
    }
  ]
}
```
