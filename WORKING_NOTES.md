Compiled tracing lane (2026-09-11): code repository, trace CLI/API, LLVM emit and
handler instrumentation, runtime.c and its vendor copy, tracing tests and docs.
Starting HEAD: 799a066. Initial pull attempted; blocked by read-only .git/FETCH_HEAD.
No inbound or host-asked tracing work is included.

Implemented `code trace [path] --compiled [-o file]`: native executable LLVM
emit/handler instrumentation, runtime snapshots, existing schema-1 renderer and
replay. No syntax changes or ordinary-build instrumentation. Both runtime.c
copies are byte-identical. Tests were added and observed failing on the missing
CLI flag before implementation; final tests exercise real native binaries.

Verification (final source):
- `timeout 120 cargo test --workspace --test compiled_trace --test cli --test native_crate_vendor_sync`:
  3 tracing, 23 CLI, 1 vendor-sync tests passed. The tracing tests subsequently
  passed again with CODE_CHECK_LEAKS=1 and scalar/array native answers added.
- `timeout --kill-after=10s 180s cargo build --workspace`: exit 0.
- `timeout --kill-after=10s 300s cargo test --workspace`: exit 101; first failure
  cloud_drive_module cannot bind loopback (Operation not permitted).
- `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER='timeout --kill-after=5s 45s' timeout --kill-after=10s 300s cargo test --workspace --no-fail-fast`:
  exit 101; 173 passed, 18 failed, 5 ignored, plus run_language_tests timed out
  rebuilding native fixtures. Failed targets: cloud_drive_module, hosted_app,
  http_client_module, http_server_module, localai_module, mailer_module,
  module_template, net_module, oauth_module, run_language_tests. Socket binds
  are denied; hosted listener assertions fail; module_template cannot resolve
  index.crates.io. No tracing test failed.
- `timeout --kill-after=5s 120s cargo test --workspace --test run_language_tests`:
  completed in 16.62s, exit 101. Only net_server_lifecycle.code failed, at
  `assert l.ok`, in both interpreted and compiled modes (listener unavailable).
- `timeout --kill-after=5s 180s cargo clippy --workspace --all-targets`: exit 0,
  no warnings, including a final rerun after test edits.
- `cargo check --no-default-features --lib`: exit 0.
- `target/debug/code format --check tests/`: exit 0 (expected invalid fixtures
  skipped); `cargo fmt --all -- --check`, `git diff --check`, and runtime `cmp`:
  exit 0.

Full logs: /tmp/compiled-trace-{build,workspace-tests,all-tests,language,clippy,
format,final-targeted,no-llvm}.log.

Limits are documented in src/trace.rs, CLI help and
docs/todo/ai-first-handlers.md: native executable tracing validated on x86_64
Linux; no wasm/shared/static tracing, inbound or host-asked boundaries. Replay
still interprets. Native internals are opaque, snapshots consume memory, and
fatal runs do not publish a trace. Existing malformed-module-address Exception
wording differs between backends; traces retain actual answers without changing
untraced behavior to hide that existing difference.

Diff reviewed. Git staging of the shared runtime pair was attempted separately
as required, but `.git/index.lock` is also read-only. No commit or push was
possible; HEAD remains 799a066. No releases or tags. All changes remain in the
working tree for review and commit in a session with writable Git metadata.
