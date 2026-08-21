# Stress fixtures ship as playground examples

`site/build.py` embeds every `tests/*.code` fixture as a playground example,
skipping only `fail_*` (bar one, kept to demonstrate error output):

```python
for path in sorted(tests_dir.glob("*.code")):
    name = path.stem
    if name.startswith("fail_") and name != "fail_undefined_variable":
        continue
    examples[name] = path.read_text()
```

That rule was right when every fixture was a small hand-written illustration
of a feature. It stopped being right with `tests/loop_bounded_memory.code`,
which is machine-generated, 28 lines of `a = a + a`, and runs 16 384
iterations — it exists to prove the compiled backend doesn't exhaust the
stack, which is exactly the thing a wasm playground cannot demonstrate. It is
currently in `dist/index.html` as a selectable example.

## Fix direction

Give the generator an explicit notion of which fixtures are *illustrations*
and which are *stress tests*. Options, cheapest first:

1. Skip a name prefix, the way `fail_` is already skipped — e.g. rename to
   `stress_loop_bounded_memory.code`. Consistent with what is there, costs
   one line, but adds a second magic prefix.
2. Keep an explicit exclude list in `build.py`. Obvious, but drifts.
3. Curate the example list positively rather than by exclusion — the
   playground would then show a chosen sequence rather than whatever
   alphabetical order produces, which is probably better for a first-time
   visitor regardless of this problem.

(3) is the one worth doing if the playground is meant to teach the language;
(1) if the goal is just to stop shipping the stress test.
