# Read-only collection iteration investigation (262)

**Outcome: neither candidate was retained.** Main keeps its existing compiler,
value layout, runtime and ABI. Snapshot regression tests and complete evidence
remain; no release, website edit or performance claim follows from this study.

Both trials used source baseline `f18461d` (2.25.0), independently built matched
compilers, one fresh VM/CPU per trial, 15 shuffled serial launches for all
25 cases (750 samples each). Construction, queries, verification, cleanup and
startup are included; compilation is excluded. Hosts differ between trials:
compare before/after only within each trial, never their absolute durations.

- [First candidate](results/2026-11-06-first-candidate/README.md), source
  `5db4eda`: ASCII +46.05%; large weighted -3.86%, nearest -9.51%. Rejected.
- [Revised candidate](results/2026-11-06-revised-candidate/README.md), source
  `991d238`: ASCII code restored byte-for-byte; large weighted -1.33%, nearest
  -5.97%. No material regression, but neither target passes the fixed 10% gain.
- All **1,500 samples / 100 summaries** remain with source/compiler/native
  identities, ranges and MAD. Both bundles passed independent audits. The
  [fixed protocol](PROTOCOL.md) and [unapplied patches](experiments/) preserve
  the experiment without enabling an ineffective optimization.

Local cumulative instruction profiles show substantial reductions in record
copying. They are not CPU-time shares or exact phase timings; the elapsed
studies did not justify the ownership/alias complexity. A separate browser
formatter callback probe exposed a snapshot violation in the first prototype;
the revised guard and stock baseline pass. Regression tests retain this case.

The next supported hypothesis concerns threshold accumulation: the complete
small-case profile attributes 22.63% of dynamic instructions to `code_add` and
20.51% to `code_add_into` (not elapsed percentages). Investigate a checked
numeric-add path as a fresh experiment, not a changed acceptance gate here.

`archived-workflow.yml` is inactive. Reproduce from a frozen candidate revision
with its matched baseline and original workflow; `run.py --smoke-only` can
validate supplied source compilers locally. Never time unpatched main as the
candidate and label it as this experiment. Existing historical reports/site
numbers stay unchanged.
