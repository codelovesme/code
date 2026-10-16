# strings — text utilities

Native handlers and their browser half have the same inputs/results. Browser
support and Length are unreleased additions; the existing handlers remain.

| Handler | Inputs | Result value |
|---|---|---|
| Shout | value (string) | ASCII uppercase, followed by `!` |
| Echo | value (any, default null) | Unchanged value |
| Split | value (string), separator | Array of substrings; empty parts survive |
| Join | value (array of strings), separator | Joined text |
| Trim | value (string) | Remove leading/trailing ASCII whitespace |
| Upper / Lower | value (string) | ASCII case conversion |
| Length | value (string) | Unicode scalar count, like core Length |

Each answers `<Name>Result { value }`. Unknown classes answer null; invalid
operands answer Exception with source "strings". Split/Join's separator must
be exactly one Unicode scalar, including a non-BMP character such as 😀.
Length counts scalars, not UTF-8 bytes, UTF-16 units or visual graphemes:
`a😀é` has length 3, and `e` plus a combining accent has length 2.

Case conversion leaves non-ASCII characters untouched. Trim removes only
space, tab, newline, carriage return, form feed and vertical tab. It does not
remove non-breaking spaces. Existing single-character separator semantics
remain; multi-character separators and Unicode case folding are later work.

Core Length still accepts arrays and strings for compatibility. strings Length
is the text-specific entry point. Link strings in a browser shell as well as
in any guest that calls it; its page.mjs is bundled into generated host.mjs.
