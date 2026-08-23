Verbatim copies of `src/code_abi.h` and `src/runtime.c` from the main
`code` repo. **Do not hand-edit these** — copy the originals over them
again if they change. `tests/native_crate_vendor_sync.rs` (in the
workspace root package) fails the build if the two drift apart.

Vendored rather than referenced by relative path so `cargo publish`
(which builds in an isolated copy containing only this crate's own
`include`d files) can compile this crate standalone, with no dependency
on the rest of the `code` repo being present.
