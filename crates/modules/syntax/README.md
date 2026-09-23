# `syntax` — code source as coloured spans

What an editor colours `code` with, by the language's own lexer — so a
keyword is exactly what the compiler calls one.

```code
link "syntax.so" as syntax

emit Highlight { source = text, from = 10, until = 50 } to syntax get h
| h.lines[0] is line 10: [{ col = 2, len = 4, kind = "keyword" }, …]
```

## Handlers

```
Highlight { source, from?, until? }  → Highlighted { lines }
```

- `lines` has one entry per source line from `from` (default 0) up to, not
  including, `until` (default: the end) — the lines an editor shows. (`until`,
  not `to`: `to` is a keyword.)
- Each entry is a list of `{ col, len, kind }`, in characters, left to right.
  `kind` is one of `comment string number keyword class property variable
  operator` — the language server's classification. What no span covers is
  plain.
- **Source that does not lex still colours.** The whole source is lexed
  first; if that fails (an unterminated string while typing), each line is
  lexed on its own, and a line that still fails is scanned by hand, asking
  the lexer whether each word is a keyword. Colours go from exact to close,
  never out.
