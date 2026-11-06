# Compiled workload candidates (261)

Question: which additional useful **prebuilt native programs** can compete with
C and Rust in elapsed execution time? This fixed pilot preserves all outcomes,
not a guarantee of a win and not a website update. No runtime changes or release.

Before sampling, freeze three tasks at 32,768 and 262,144 records/items each:

- `weighted`: 16 weighted-score queries over `{x,y,z}` records, reducing scores.
- `threshold`: 16 threshold queries over numeric values, summing matches.
- `nearest`: 16 linear nearest-point searches by squared Euclidean distance.

All variants generate the same deterministic 1,024-value cycle by adding 17
and wrapping at 1,024. They use the same operation order and 16 query parameters.
Every total is checked against an independent Python exact-integer oracle;
values/results fit exactly in f64. No module I/O, network, strings or interpreter
is involved. Nearest uses brute force in all languages, not a spatial index.
This simple input distribution does not represent every production dataset.

Published Code 2.24.1 launcher/driver hashes are required. Code `build --release`,
GCC `-O2` **and** `-O3 -march=native -ffp-contract=off`, Rust `-C opt-level=3
-C target-cpu=native`. No fast-math or volatile/optimization barrier handicaps.
C/Rust use typed records/f64 vectors and integer indexes, Code generic values
and ordinary numeric indexes; their language/representation costs are included.
C malloc and Rust Vec reservation are idiomatic, without deliberate deep copies.
Rust range iteration permits bounds-check optimization. Known input literals
may enable normal compiler optimizations equally; no claim of opaque inputs.

Time includes startup, allocation, initialization, all queries, exact result
checks and cleanup. Compilation and untimed preparation are excluded. This is
not an isolated arithmetic-kernel measurement. Linux wait4 process peak RSS is
retained for completeness, but the question and ranking concern runtime only.

Run local `run.py --code /actual/public/code --output-dir /new/path --smoke-only`.
After source/protocol are committed, manually run `compiled-workloads.yml` on
one fresh Ubuntu VM: 15 seeded shuffled rounds, all 24 variants serially pinned
to one allowed logical CPU, 360 retained samples. Compiler/test/profiler overlap
is rejected. OS caches remain warm; physical isolation/frequency aren't promised.
All source/generated/binary identities, commands, versions, samples, medians,
ranges and MAD are retained. Independent `verify.py` audits every group/statistic
and optional artifact bundle. C safety and Code native leak checks are untimed.

A competitive pilot win must lead **both** C modes and Rust by at least 10%
and more than twice the sum of Code/competitor MADs, not merely one baseline.
Treat small differences relative to variability/startup as inconclusive and
repeat promising cases before website publication. Never splice pilot times
into the existing page's different-host table: rerun the selected suite together
on one host if a later website update is warranted. Full losing outcomes remain
available regardless of what is eventually highlighted on the page.
