# Label names

Spec §Anchor and §Round-trip: an id is an identifier, and the LaTeX writer
prints it as a label *name* — what `\label`, `\ref`, `\hyperref[…]` and the
`id=` key of `tscode` take. A name is not prose: `a_b` stays `a_b`, never
`a\_b`, which names an anchor no `\label` declares (design 07
§Implementation notes, challenge C67). Only the characters that break the
reading of a brace argument or of a `\csname` are mapped, and an id holds
none of them: `#`, `%`, `{`, `}`, `\`, `^`, `$`, `~` and blanks are outside
the identifier grammar, so every id of a TMark document reaches LaTeX as
written. A key that is not csname-safe, `k&r`, cannot be written here
either — `&` ends a key — and comes from a glossary declared in the front
matter, whose `\newacronym` TeXSmith writes. Typst takes the id raw either
way.

## input

````md
## One {#sec:a_b}

## Two {#sec:a-b}

## Three {#sec:a.b}

## Four {#sec:a:b}

See @sec:a_b, @sec:a-b, @sec:a.b, @sec:a:b and [the first](#sec:a_b).

$$
E = mc^2
$$ {#eq:m_c}

```py
print("hi")
```

Listing: Hello. {#lst:h_i}
````

## canonical

````md
## One {#sec:a_b}

## Two {#sec:a-b}

## Three {#sec:a.b}

## Four {#sec:a:b}

See @sec:a_b, @sec:a-b, @sec:a.b, @sec:a:b and [the first](#sec:a_b).

$$
E = mc^2
$$ {#eq:m_c}

```py
print("hi")
```

Listing: Hello. {#lst:h_i}
````

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
          "text": "One"
        }
      ],
      "attrs": {
        "id": "sec:a_b"
      }
    },
    {
      "type": "Header",
      "level": 2,
      "content": [
        {
          "type": "Str",
          "text": "Two"
        }
      ],
      "attrs": {
        "id": "sec:a-b"
      }
    },
    {
      "type": "Header",
      "level": 2,
      "content": [
        {
          "type": "Str",
          "text": "Three"
        }
      ],
      "attrs": {
        "id": "sec:a.b"
      }
    },
    {
      "type": "Header",
      "level": 2,
      "content": [
        {
          "type": "Str",
          "text": "Four"
        }
      ],
      "attrs": {
        "id": "sec:a:b"
      }
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "See "
        },
        {
          "type": "Ref",
          "items": [
            {
              "key": "sec:a_b"
            }
          ]
        },
        {
          "type": "Str",
          "text": ", "
        },
        {
          "type": "Ref",
          "items": [
            {
              "key": "sec:a-b"
            }
          ]
        },
        {
          "type": "Str",
          "text": ", "
        },
        {
          "type": "Ref",
          "items": [
            {
              "key": "sec:a.b"
            }
          ]
        },
        {
          "type": "Str",
          "text": ", "
        },
        {
          "type": "Ref",
          "items": [
            {
              "key": "sec:a:b"
            }
          ]
        },
        {
          "type": "Str",
          "text": " and "
        },
        {
          "type": "Link",
          "content": [
            {
              "type": "Str",
              "text": "the first"
            }
          ],
          "target": {
            "type": "Anchor",
            "value": "sec:a_b"
          }
        },
        {
          "type": "Str",
          "text": "."
        }
      ]
    },
    {
      "type": "MathBlock",
      "text": "E = mc^2",
      "attrs": {
        "id": "eq:m_c"
      }
    },
    {
      "type": "CodeBlock",
      "text": "print(\"hi\")",
      "lang": "py"
    },
    {
      "type": "Caption",
      "kind": "listing",
      "content": [
        {
          "type": "Str",
          "text": "Hello."
        }
      ],
      "attrs": {
        "id": "lst:h_i"
      }
    }
  ]
}
```
