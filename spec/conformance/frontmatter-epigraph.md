# Front matter, epigraph

Spec §BlockQuote and the metadata table: `epigraph: {quote, source}` is set
under the document's opening heading, between that heading and its content.
The writers render it as the block quote a `> {.epigraph}` line makes, so
the key needs no node of its own in the IR — the front matter keeps it and
each writer splices it where the spec says.

## input

```md
---
epigraph:
  quote: Simplicity is prerequisite for reliability.
  source: Edsger W. Dijkstra
---

# Reliability

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
<footer>Edsger W. Dijkstra</footer>
</blockquote>
<p>Everything else follows.</p>
```
