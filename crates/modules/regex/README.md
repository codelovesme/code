# `regex` — text by pattern

Match, find, replace and split text by a regular expression. For the rules a
program would otherwise write by hand or ask a model for: a product title
reduced to a key, a date pulled out of a subject line, a list split on any of
its separators.

```code
link "regex.so" as regex

emit Replace { value = title, pattern = "\\s*\\(.*?\\)\\s*|\\s+", replacement = " " } to regex get key
emit Find { value = subject, pattern = "(?P<year>\\d{4})-(\\d{2})" } to regex get dates
first_year = dates.value[0].named.year
```

## Handlers

```
Match   { value, pattern }               → MatchResult   { value }   | true if it matches anywhere
Find    { value, pattern }               → FindResult    { value }   | every match, in order
Replace { value, pattern, replacement }  → ReplaceResult { value }   | every match replaced
Split   { value, pattern }               → SplitResult   { value }   | the texts between matches
```

**Stateless** — there is nothing to configure and no setup particle.

Each match `Find` answers is:

| Field | Kind | Meaning |
|---|---|---|
| `text` | String | what matched |
| `start`, `end` | Number | where, in characters (Unicode scalars, as `strings` counts them); `end` is one past the last |
| `groups` | Array | each numbered group's text, null for a group the match went around |
| `named` | Object | each named group `(?P<name>…)` to its text, or null |

- **Linear time.** The engine is Rust's `regex`: matching takes time in
  proportion to the text, whatever the pattern, so a pattern from a config
  file or a user cannot hang the program. The price is no backreferences
  (`\1`) and no look-around (`(?=…)`).
- **Flags** go inside the pattern: `(?i)` ignores case, `(?m)` lets `^` and
  `$` match at each line, `(?s)` lets `.` match a newline.
- **Replacement** text brings a group in with `$1` or `${name}`; `$$` is a
  literal `$`. In a code string each `$` is written `\$`, and each `\` of the
  pattern `\\`.
- A pattern that does not compile, or a missing or non-text field, answers an
  `Exception` saying which.

Native only: there is no browser half.
