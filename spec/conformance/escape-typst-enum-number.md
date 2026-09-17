# Typst enum-number escaping

`design/07-writers.md` §Implementation notes: a line of Typst markup that
starts with digits, a `.` and whitespace is an *enum item*, not text —
`0.` alone is `enum.item(number: 0, body: [])`, so a task item whose whole
text is `0.` rendered as an empty numbered item and the text was lost. The
Typst writer escapes the dot (`0\.`) at the start of a text run, which is
the start of a line for Typst wherever the writer puts it: a paragraph, a
list item's body, a `#ts-task(…)[…]` content block, and the same block once
a template unwraps it. Digits-dot-*digits* (`3.5 kg`) is a number, not a
marker, and a `.` whose digits do not start the line (`at 10. o'clock`) is
a full stop; neither is escaped. `)` after the digits is not a Typst marker
at all (checked on typst 0.14.2: `1) bar` is text).

## input

```md
- [ ] 0.
- [x] 1.
- [ ] 3.5 kg

The exam is at 10. o'clock.

10\. Not a list: a sentence that opens on a number.
```

## canonical

```md
- [ ] 0\.
- [x] 1\.
- [ ] 3.5 kg

The exam is at 10. o'clock.

10\. Not a list: a sentence that opens on a number.
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
                  "text": "0."
                }
              ]
            }
          ],
          "task": "open"
        },
        {
          "content": [
            {
              "type": "Para",
              "content": [
                {
                  "type": "Str",
                  "text": "1."
                }
              ]
            }
          ],
          "task": "done"
        },
        {
          "content": [
            {
              "type": "Para",
              "content": [
                {
                  "type": "Str",
                  "text": "3.5 kg"
                }
              ]
            }
          ],
          "task": "open"
        }
      ]
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "The exam is at 10. o'clock."
        }
      ]
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "10. Not a list: a sentence that opens on a number."
        }
      ]
    }
  ]
}
```

## latex

```latex
\begin{tstasklist}
\item[\tstodo] 0.
\item[\tsdone] 1.
\item[\tstodo] 3.5 kg
\end{tstasklist}

The exam is at 10. o'clock.

10. Not a list: a sentence that opens on a number.
```

## typst

```typst
- #ts-task("open")[0\.]
- #ts-task("done")[1\.]
- #ts-task("open")[3.5 kg]

The exam is at 10. o'clock.

10\. Not a list: a sentence that opens on a number.
```

## html

```html
<ul>
<li><input type="checkbox" disabled /> 0.</li>
<li><input type="checkbox" disabled checked /> 1.</li>
<li><input type="checkbox" disabled /> 3.5 kg</li>
</ul>
<p>The exam is at 10. o'clock.</p>
<p>10. Not a list: a sentence that opens on a number.</p>
```
