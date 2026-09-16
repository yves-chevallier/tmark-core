# Math (display), hugging fences

Spec §Math (display): `$$ … $$` is the canonical display. A corpus written
for Pandoc or `python-markdown-math` hugs its fences — the math starts on
the opening `$$` and the closing `$$` ends the last line of the math — and
reads the same way: a line that *ends* with `$$` closes the display.

## input

```md
Avant.

$$\left\{ \begin{matrix}
x = 0 \\
y = y_{0}
\end{matrix} \right.$$

Après.
```

```md
Avant.

$$
\left\{ \begin{matrix}
x = 0 \\
y = y_{0}
\end{matrix} \right.
$$

Après.
```

## canonical

```md
Avant.

$$
\left\{ \begin{matrix}
x = 0 \\
y = y_{0}
\end{matrix} \right.
$$

Après.
```

## ir

```json
{
  "blocks": [
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "Avant."
        }
      ]
    },
    {
      "type": "MathBlock",
      "text": "\\left\\{ \\begin{matrix}\nx = 0 \\\\\ny = y_{0}\n\\end{matrix} \\right."
    },
    {
      "type": "Para",
      "content": [
        {
          "type": "Str",
          "text": "Après."
        }
      ]
    }
  ]
}
```
