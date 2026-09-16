# The braces-only fence info string

Spec §Lexical grammar, family 4, and Appendix "PyMdownX compatibility
profile": `pymdownx.superfences` accepts an info string that is one
`attr_list` group with no bare word before it — `{ .c .annotate }` is the
spelling MkDocs Material documents for code annotations. The first class
is the language, the rest of the list the fence's options; the parser
never yields a language of `{`. The canonical spelling is the bare word
plus a trailing list, so the form is deprecated and `fmt` rewrites it.

## input

````md
``` { .c .annotate }
int x;
```
````

````md
```{.c .annotate}
int x;
```
````

## canonical

````md
```c {.annotate}
int x;
```
````

## ir

```json
{
  "blocks": [
    {
      "type": "CodeBlock",
      "text": "int x;",
      "lang": "c",
      "options": {
        "classes": [
          "annotate"
        ]
      }
    }
  ]
}
```

## diagnostics

```text
deprecated @ 1:1-3:4
```
