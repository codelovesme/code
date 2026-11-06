# First candidate — rejected

Source `5db4eda19f920a8fded716499d1da6f2eea3db1c`, fixed baseline
`f18461d9706406a484ded1955c1d6ce62c3f4c54`.
[Hosted run 42278353069](https://github.com/codelovesme/code/actions/runs/42278353069).
All 750 samples/50 summaries and artifact/source hashes independently verified.

The fixed gate rejected this candidate: ASCII character iteration rose
22.671 → 33.110 ms (+46.05%). Large weighted improved only 3.86% and nearest
9.51%, below the fixed >=10% target gate. Unicode improved 21.57%; every
outcome remains in JSON/CSV/gate.json. No favorable-only publication or merge.

The next candidate must keep the same 25 workloads and acceptance gates.
Restrict selection to inferred Array roots to exclude known string sources;
keep runtime checks/generic ownership despite that hint. Preserve a direct
fast-path SSA element pointer for alias tagging, and publish the shared cell
for later generic bailouts. These are hypotheses to measure, not accepted gains.
