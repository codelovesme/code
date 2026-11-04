# Optional wasm Trace cost

Run from the Code checkout:

```sh
cargo build --workspace
git show e7436b7:web/runtime.mjs > /var/tmp/code-baseline-host.mjs
node benchmarks/wasm-trace/run.mjs target/debug/code /var/tmp/code-baseline-host.mjs
```

The script builds release wasm for identical logic with/without `Trace`.
Each call enters `Tick`, emits `Next` and returns its answer. The traced
handler discards the trace. Two warmups, 11 alternating samples per variant,
10,000 calls per sample; every answer checked. The baseline JS host and the
current quiet host run the **same new quiet wasm**. This isolates bridge
cost, rather than comparing old/new compiled runtimes. Node v22.23.4,
source compiler reporting 2.23.0 (unreleased implementation).

2026-11-04 medians, raw samples in `results-2026-11-04.json`:

| Host | 10,000 calls |
|---|---:|
| Previous quiet JS host (e7436b7) | 32.298 ms |
| Current quiet JS host | 33.218 ms |
| Current host, Trace enabled | 154.549 ms |

The quiet wasm contains **no flow calls/imports**, and its JS event path
collects no steps, copies no trace roots and reads no tracing clocks. This
is not a claim of literally identical total bridge cost: tracking event-buffer
relocation for nested/large answers added a small branch, measured here at
about 0.09 microseconds per call (+2.8%). The sample ranges overlap.
Trace enabled adds about 12 microseconds per tiny interaction here; it is
intentionally opt-in. These are synthetic callback costs, not frame rates,
server throughput or application-load benchmarks.
