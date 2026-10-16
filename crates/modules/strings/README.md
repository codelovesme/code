# strings — text utilities

Native handlers and their browser half have the same inputs/results. The
browser half, Length and the search/slice/padding additions are unreleased.

| Handler | Inputs | Result value |
|---|---|---|
| Shout | value (string) | ASCII uppercase, followed by `!` |
| Echo | value (any, default null) | Unchanged value |
| Split | value (string), separator (nonempty string) | Array of substrings; empty parts survive |
| Join | value (array of strings), separator (string, may be empty) | Joined text |
| Trim | value (string) | Remove leading/trailing ASCII whitespace |
| Upper / Lower | value (string) | ASCII case conversion |
| Length | value (string) | Unicode scalar count, like core Length |
| Contains / StartsWith / EndsWith | value, search (strings), ignore_case? (Bool, default false) | Bool |
| IndexOf | value, search (strings), ignore_case? (Bool, default false) | First zero-based scalar position, or -1 |
| Replace | value, search, replacement (strings) | All literal, non-overlapping matches replaced |
| Slice | value (string), start? (default 0), end? (default length) | Text in [start, end) |
| Pad | value (string), width, fill? (default space), side? (default "left") | Text padded to width, never truncated |

Each answers `<Name>Result { value }`. Unknown classes answer null; invalid
operands answer Exception with source "strings" and innerException null.
Missing required fields and wrong types are errors. Optional fields default
only when absent; explicit null is an error. Echo is the exception: it accepts
any value, including null.

Length, IndexOf, Slice and Pad count Unicode scalars, not UTF-8 bytes, UTF-16
units or visual graphemes: `a😀é` has length 3; `e` plus a combining accent
has length 2. Slice can separate a combining mark from its base character.

Search is literal, with no regular expressions. Contains/StartsWith/EndsWith
match an empty search; IndexOf returns 0. `ignore_case` folds ASCII A-Z only:
`ABC` matches `bc`, but `É` does not match `é`. Replace is case-sensitive,
refuses an empty search and treats replacement text literally (including `$`).
For example, replacing `aa` with `b` in `aaa` gives `ba`.

Slice accepts finite safe integers (absolute value <= 9007199254740991).
Negative positions count from the end, bounds clamp to the string, and
end <= start returns empty text. Slice with start=1, end=2 of `a😀é` is `😀`.

Pad requires an integer width in 0..1000000. Fill is any nonempty string;
side is "left" or "right". Fill repeats and is cut at scalar boundaries:
padding `x` to width 4 with `ab` produces `abax` on the left or `xaba` on
the right. Validation also applies when the text already meets the width.
The width cap bounds newly padded output to at most 4 MB of UTF-8; existing
longer input is returned unchanged. No module state or Config is needed.

Split supports multi-character separators. Adjacent/leading/trailing
separators retain empty parts, and splitting empty text returns [""]. Split
refuses an empty separator; Join accepts one, concatenating without a delimiter.
This extends the former single-character-only contract: previously rejected
multi-character separators now succeed, empty Join separators now succeed,
and empty Split reports "Split requires a non-empty 'separator' field".
Successful single-character behavior and other prior error messages remain.

Case conversion leaves non-ASCII characters untouched. Trim removes only
space, tab, newline, carriage return, form feed and vertical tab, leaving
non-breaking spaces intact. Full Unicode case folding is separate work.

Core Length still accepts arrays and strings for compatibility. strings Length
is the text-specific entry point. Link strings in a browser shell as well as
in any guest that calls it; its page.mjs is bundled into generated host.mjs.
