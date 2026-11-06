# Current public-release measurement manifests

v2.23.2.json fixes the reviewed public release commit, tarball URL/digest and
actual launcher/driver SHA-256s verified in docs/releases/2.23.2-verification.json.
Both samplers accept --release-manifest; without it they retain the historical
v2.23.0 identity. Historical CSV/JSON files and workload programs are unchanged.

For website alignment 236, the manual `Measure current public website release`
workflow runs the existing comparison suite and all four history cases serially
on one fresh Ubuntu 24.04 VM, pinned to the same allowed logical CPU. Node 22,
Python and C/Rust compiler identities are recorded. Every existing baseline and
program stays: 35 comparison groups / 485 samples, plus 20 history groups /
300 samples, **785 total**. Fifteen runtime launches and five compile samples,
seeded shuffled order, no exclusions. Source/compile/prebuilt boundaries and
process-RSS limitations remain those of the existing protocols.

Initialization, algorithm, validation and cleanup remain inside complete-program
runtime measurements. No source changes, new compiled algorithms, favorable-only
entries or cross-host rankings are introduced. The current website will adopt
these results only after independent CSV/JSON/statistic/artifact verification;
previous reports remain accessible. Local --smoke-only runs check actual public
programs and cache preparation without collecting/publishing timing samples.
