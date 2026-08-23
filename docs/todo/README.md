# Open tasks

Things deliberately left undone, with enough context to pick them up cold.
One file per task, named for the problem rather than a number.

Nothing here is a regression: each was either found while building something
else and judged out of scope, or accepted as a known characteristic at the
time. Order below is roughly by how likely it is to bite someone.

| Task | Why it matters |
|---|---|
| [native-module-linking.md](native-module-linking.md) | Phase 1 (`.so` handlers) + Phase 2 (exported vars) shipped 2026-08-21, Phase 3 (`.a` static modules) shipped 2026-08-22; `.wasm`, `code build --lib` still open |
| [no-language-documentation.md](no-language-documentation.md) | The new language has no README at all |
| [temp-slots-pin-intermediates.md](temp-slots-pin-intermediates.md) | Memory held longer than necessary |

Done and removed (git log has the detail):

- *deep nesting blows the stack* — every traversal of a value in both
  runtimes is now iterative, covered by `tests/stress_deep_nesting.code`.
- *stress fixtures become playground examples* — `stress_*` joins `fail_*`
  as a prefix `site/build.py` holds back, and the two generated fixtures
  were renamed into it.
