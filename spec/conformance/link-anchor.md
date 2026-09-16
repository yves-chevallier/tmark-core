# Link to an anchor

Spec §Ref, "textual reference": `[text](#id)` is the canonical spelling of a
reference that keeps the number out of the body. It is a link in plain
CommonMark, so every renderer reads it — which is why it, and not the
reference-style `[text][id]`, is what a document should hold.

`#id` addresses the rendered page: the print writers name the label
(`\hyperref[id]{text}`, `#link(<id>)[text]`, both of which see every page of
a book in one document), and the web lowering keeps a same-page anchor byte
for byte and splices the location of a label that lives on another page of
the book (`[text](other-page.md#id)`), which is what lets a site resolve a
cross-page reference without `mkdocs-autorefs`. The sibling case needs a
`book` map and lives in the writers' tests.

## input

```md
## The claim {#sec:claim}

As [the claim](#sec:claim) shows, the watchdog fires twice.
```

## canonical

```md
## The claim {#sec:claim}

As [the claim](#sec:claim) shows, the watchdog fires twice.
```

## resolution

```text
```

## ir

```json
{
  "blocks": [
    {
      "type": "Header",
      "level": 2,
      "content": [
        {
          "type": "Str",
          "text": "The claim"
        }
      ],
      "attrs": {
        "id": "sec:claim"
      }
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "As "
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
            "type": "Anchor",
            "value": "sec:claim"
          }
        },
        {
          "type": "Str",
          "text": " shows, the watchdog fires twice."
        }
      ]
    }
  ]
}
```

## latex

```latex
\subsection{The claim}\label{sec:claim}

As \hyperref[sec:claim]{the claim} shows, the watchdog fires twice.
```

## typst

```typst
== The claim <sec:claim>

As #link(<sec:claim>)[the claim] shows, the watchdog fires twice.
```

## html

```html
<h2 id="sec:claim">The claim</h2>
<p>As <a href="#sec:claim" class="reference">the claim</a> shows, the watchdog fires twice.</p>
```
