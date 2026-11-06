# Bounded UTF-8 character counting — preregistered source experiment

Task 239 follows the retained one-walk slice change in 238. Its matched local
document-1m profile places 40.56% of self instructions in `code_length_of`.
That is an instruction share, not an elapsed-time share.

Frozen baseline: `3f9f23c2ba0d442837db86af52a81a5a6ab94978`, version 2.24.0.
The candidate changes only character counting in `runtime.c` and its identical
vendored copy. Existing array/object and error paths, compiler, interpreter,
ABI 2 and value layout remain. No new cache/header state, external SIMD/ISA
requirement, host import or language feature is introduced.

## Candidate

`strlen` supplies the existing NUL-terminated byte bound. Whole eight-byte
groups are loaded with `memcpy`, avoiding unaligned or aliasing casts. A
portable unsigned word calculation counts bytes whose top bits are `10`,
then a scalar loop handles the final zero to seven bytes. Subtracting those
continuation bytes from the byte length gives exactly the old count, including
its treatment of arbitrary nonzero byte patterns. Loads stay entirely within
the byte bound; an embedded first NUL still terminates counting. The per-byte
arithmetic is independent of byte order. No character-count cache is written.

## Fixed protocol and decision gate

Fresh hosted VM; baseline and candidate built with identical LLVM 17/Rust
toolchains in separate Cargo output directories. Both compilers must identify
as Code 2.24.0 and have different recorded hashes. Prepare and check every
executable before timing. One allowed logical CPU, seeded shuffled serial
sampling, 15 launches per entry: **17 unchanged workloads, 34 entries, 510
samples**, all retained. No concurrent compiler/test/profiler during sampling;
physical isolation/fixed frequency are not guaranteed.

The suite preserves all task-238 inputs and complete checks: both matrix,
sieve and merge-sort sizes; binary trees, dictionary, point records, spectral
norm, string building, numeric sum, retained snapshots and all four retained
configuration/document histories. Startup, construction, computation, all
verification and cleanup are included. Compilation and Python/wrapper startup
are excluded. OS caches remain warm. RSS is wait4 largest process-resident
peak, not live heap, allocation count or summed process-tree memory.

Retain only with **at least 10% lower complete document-1m median time** and
no material broader regression: **>=10% slower AND absolute increase greater
than max(0.2 ms, 3×before MAD, 3×after MAD)**. Otherwise revert the candidate
and retain evidence. Keep all smaller costs, samples, sizes and baselines.
These source results must not replace public-release website evidence or be
combined with another host's timings/C/Rust ranks. No website update, release
or live application restart is included.

Local untimed preparation:

```sh
PYTHONDONTWRITEBYTECODE=1 python3 benchmarks/character-count/run.py \
  --before /path/to/frozen-source-before --after /path/to/frozen-source-after \
  --output-dir /tmp/new-count-smoke --smoke-only
```

Publication timing requires the hosted workflow and a clean tracked checkout.
Independently verify all samples/statistics, source/native/wrapper hashes,
compiler identities and matching unchanged generated inputs.

## Correctness proof

The language regression constructs 97 independently retained strings, tracking
scalar counts through mixed-width Unicode appends. A unique sentinel exposes
incorrect length values, with combining scalars, emoji, every byte-tail size,
literal/owned/shared/sliced/grown/empty values and an unchanged array path.
Run in interpreter/native dev+release/compiled-WASM dev+release and source-built
WASM interpreter. Wrong-kind length diagnostics retain message parity. Its
existing stress_ prefix keeps the public gallery unchanged.

`guard.c` includes the actual selected runtime. It tests every byte value
1..255 at all lengths 0..256, mixed byte patterns and first embedded NUL,
with borrowed and output-aliased owned inputs. A NUL at the final readable
byte is followed by a protected page; every word-tail/alignment is exercised.
Run normally and with address/undefined-behavior/leak sanitizers:

```sh
cc -O1 -g -fsanitize=address,undefined -fno-omit-frame-pointer \
  -ffunction-sections -fdata-sections -Wl,--gc-sections \
  -Isrc benchmarks/character-count/guard.c -o /tmp/count-guard -lm -ldl -lpthread
ASAN_OPTIONS=detect_leaks=1 CODE_CHECK_LEAKS=1 /tmp/count-guard
```

A retained implementation also needs full workspace and required PR CI,
runtime/vendor synchronization, clippy/formatting, source-WASM/gallery checks,
applicable application regressions and Aquarium WASM size. More than 5% growth
needs explanation. Record preparation problems as well as successful checks.
