# Make handlers inspectable by agents

The `code` language is intended to be written and operated by AI agents. The
handler/particle model is the one abstraction; this task does not add ordinary
functions or a second call syntax.

The first two vertical slices are now implemented:

- `code handlers [path]` emits the versioned handler catalog.
- `code check [path]` emits structured JSON diagnostics for statically known
  `emit ... to this` calls and gradual `∈ Type` annotations, without changing
  permissive runtime dispatch.

The remaining validation, diagnostics, tracing, and capability work stays
separate so it can be assigned to agents without changing these command
contracts.

The first problem is discoverability. An agent should be able to ask a project
which handlers exist and what fields each handler binds without reading every
source file or inferring the answer from a runtime failure.

## Non-goals

- no `fn`, closures, function values, or alternate call syntax
- no change to the runtime meaning of particles, `emit`, or `null`
- no duplicated handwritten contract language in source files
- no AI-specific syntax in the language

## First vertical slice

Add one canonical command:

```text
code handlers [path]
```

It loads the same source tree as `code run` and prints JSON to stdout. The
initial schema describes every source handler, including handlers in linked
`.code` modules:

```json
{
  "schema_version": 1,
  "handlers": [
    {
      "name": "Greet",
      "fields": [
        {"wire_name": "who", "binding_name": "who", "type": "String"}
      ]
    }
  ]
}
```

The output must be deterministic and preserve source order. Native modules are
not guessed: they may be listed as linked modules later once their manifests
expose machine-readable handler contracts.

## Tracing and replay slice

The fourth slice is implemented:

```text
code trace [path] [-o trace.json]
code replay trace.json [path]
```

`code trace` runs the real interpreter and records `emit` boundaries in call
order, including the target, particle, answer, and handler depth. The output
contains no clock, address, or other run-specific data, so identical runs are
byte-identical. `code replay` loads that JSON, runs the real program, re-asks
replayable top-level `to this` particles, and reports matches, mismatches, and
context-dependent boundaries it skipped.

This slice is intentionally interpreter-only. Core and module boundaries are
recorded, but nested or external boundaries are not independently re-driven;
compiled-path tracing and host-asked particles remain separate follow-up work.

## Follow-up tasks

1. **Completed.** Add the handler catalog API and `code handlers` command.
2. **Completed.** Preserve `∈ Type` annotations and check statically known
   assignments and local handler boundaries.
3. **Completed.** Add structured diagnostics for source locations and
   particle-boundary errors.
4. **Completed.** Add handler/module execution tracing and replayable particle
   tests.
5. **Completed.** Add machine-readable module capability metadata (effects,
   configuration, timeouts, and handler contracts). `code handlers` now adds a
   deterministic `modules` array. Source module contracts come from handler
   declarations; native names and capability fields come only from explicit
   `module.json` metadata or the corresponding lock entry. Missing native data
   stays unknown, and capability metadata uses schema version 1.

The first task should land before the others because the catalog becomes the
shared discovery surface for validators, traces, and coding agents.

## Acceptance criteria

- The command accepts a file or directory and defaults to `.` like `run`.
- It reports parse or module-resolution errors with a non-zero exit code.
- It emits valid, stable JSON for valid programs.
- It lists wire field names separately from renamed body bindings.
- It discovers handlers nested in resolved source modules.
- Existing language behavior and the full workspace test suite remain green.
