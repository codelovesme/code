# Numeric accumulation study (263)

Checked Number+Number shortcut in `code_add_into`; all other additions retain
existing behavior. No ABI, syntax, interpreter, release or site change.

The [fixed protocol](PROTOCOL.md) requires >=10% elapsed improvement in the
large threshold aggregation, with no material regression among all 30 cases.
**Outcome: retained.** [The complete 900-sample study](results/2026-11-07/README.md)
passes the unchanged gates: large threshold elapsed -29.33%, no material
regression across 30 cases. All unchanged cases and small regressions remain.
Local profiles contain
instruction/call evidence only; no C/Rust ranking is implied.

Fresh baseline source is `3c6cbae` (2.25.0). Local complete threshold32768 runs
made 257,808 `code_add_into` calls; before, each continued to `code_add` and
`code_release`. After, none did. Total instructions: 36,457,707 -> 26,403,195.
Construction still uses `code_add_take` 32,768 times; its source/output remains
unchanged. These are whole-program instruction counts, not CPU-time shares or
exact phase subtraction. Weighted/nearest profiles remain as controls.

`run.py --smoke-only` checks all 60 variants without publishing local timings.
`verify.py RESULTS --artifacts BUNDLE` audits the 900 samples and artifact
hashes; `gate.py RESULTS` applies the preregistered acceptance decision.


The change was measured as source 2.25.0 and is integrated with the separate
owner 2.26.0 main update; that update leaves runtime/codegen/ABI unchanged.
No tag, public-package claim or comparison-page edit follows from this study.
`archived-workflow.yml` is inactive; reproduce from candidate `8206a24` rather
than timing a later main against a pinned old baseline by accident.

The snapshots control also fell 34.76%; it sums every saved matrix (over
34 million element additions), so this does not isolate snapshot creation or
memory management. Weighted/nearest and the scalar sum-squares control stay
essentially unchanged. This optimization targets boxed numeric accumulation.
