# Open tasks

Things deliberately left undone, with enough context to pick them up cold.
One file per task, named for the problem rather than a number.

Nothing here is a regression: each was either found while building something
else and judged out of scope, or accepted as a known characteristic at the
time. Order below is roughly by how likely it is to bite someone.

| Task | Why it matters |
|---|---|
| [temp-slots-pin-intermediates.md](temp-slots-pin-intermediates.md) | Memory held longer than necessary |
| [stress-fixtures-become-playground-examples.md](stress-fixtures-become-playground-examples.md) | Two generated stress fixtures ship as browser demos |
| [no-language-documentation.md](no-language-documentation.md) | The new language has no README at all |

Done and removed (git log has the detail):

- *deep nesting blows the stack* — every traversal of a value in both
  runtimes is now iterative, covered by `tests/deep_nesting.code`.
