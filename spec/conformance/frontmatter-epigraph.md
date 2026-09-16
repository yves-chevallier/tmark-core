# Front matter, epigraph

Spec §BlockQuote and the metadata table: `epigraph: {quote, source}` is a
typed front-matter key and nothing more. The core reads it into
`Keys::epigraph` — the schema declares it, the printer gives it back — and
renders it nowhere: where an epigraph belongs on a page is the consumer's
decision, not the core's (challenge C69). What the core does render is the
block quote a consumer splices, `> {.epigraph}` with its `source`, wherever
that consumer puts it; the front-matter key below and the quote under the
heading are the same epigraph written twice, and only the quote reaches a
backend.

## input

```md
---
epigraph:
  quote: Simplicity is prerequisite for reliability.
  source: Edsger W. Dijkstra
---

# Reliability

> Simplicity is prerequisite for reliability.
> {.epigraph source="Edsger W. Dijkstra"}

Everything else follows.
```

## canonical

```md
---
epigraph:
  quote: Simplicity is prerequisite for reliability.
  source: Edsger W. Dijkstra
---

# Reliability

> Simplicity is prerequisite for reliability.
> {.epigraph source="Edsger W. Dijkstra"}

Everything else follows.
```

## ir

```json
{
  "front_matter": {
    "raw": "---\nepigraph:\n  quote: Simplicity is prerequisite for reliability.\n  source: Edsger W. Dijkstra\n---",
    "keys": {
      "epigraph": {
        "quote": "Simplicity is prerequisite for reliability.",
        "source": "Edsger W. Dijkstra"
      }
    }
  },
  "blocks": [
    {
      "type": "Header",
      "level": 1,
      "content": [
        {
          "type": "Str",
          "text": "Reliability"
        }
      ]
    },
    {
      "type": "BlockQuote",
      "content": [
        {
          "type": "Para",
          "content": [
            {
              "type": "Str",
              "text": "Simplicity is prerequisite for reliability."
            }
          ]
        }
      ],
      "attrs": {
        "classes": [
          "epigraph"
        ],
        "kv": [
          [
            "source",
            "Edsger W. Dijkstra"
          ]
        ]
      }
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "Everything else follows."
        }
      ]
    }
  ]
}
```

## latex

```latex
\section{Reliability}

\tsepigraph[source={Edsger W. Dijkstra}]{Simplicity is prerequisite for reliability.}

Everything else follows.
```

## typst

```typst
= Reliability

#ts-epigraph(source: [Edsger W. Dijkstra])[Simplicity is prerequisite for reliability.]

Everything else follows.
```

## html

```html
<h1>Reliability</h1>
<blockquote class="epigraph">
<p>Simplicity is prerequisite for reliability.</p>
</blockquote>
<p>Everything else follows.</p>
```
