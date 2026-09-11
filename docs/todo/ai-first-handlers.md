# Make handlers inspectable by agents

The `code` language is intended to be written and operated by AI agents. The
handler/particle model is the one abstraction; this task does not add ordinary
functions or a second call syntax.

The first two vertical slices are now implemented:

- `code handlers [path]` emits the versioned handler catalog.
- `code check [path]` emits structured JSON diagnostics for statically known
  `emit ... to this` calls and fails on an unknown local handler without
  changing permissive runtime dispatch.

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
        {"wire_name": "who", "binding_name": "who"}
      ]
    }
  ]
}
```

The output must be deterministic and preserve source order. Native modules are
not guessed: they may be listed as linked modules later once their manifests
expose machine-readable handler contracts.

## Follow-up tasks

1. **Completed.** Add the handler catalog API and `code handlers` command.
2. Add structured diagnostics for source locations and particle-boundary errors.
3. Add development-time contract checking without changing permissive runtime
   dispatch where open message vocabularies are intentional.
4. Add handler/module execution tracing and replayable particle tests.
5. Add machine-readable module capability metadata (effects, configuration,
   timeouts, and handler contracts).

The first task should land before the others because the catalog becomes the
shared discovery surface for validators, traces, and coding agents.

## Acceptance criteria

- The command accepts a file or directory and defaults to `.` like `run`.
- It reports parse or module-resolution errors with a non-zero exit code.
- It emits valid, stable JSON for valid programs.
- It lists wire field names separately from renamed body bindings.
- It discovers handlers nested in resolved source modules.
- Existing language behavior and the full workspace test suite remain green.
