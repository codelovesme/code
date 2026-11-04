# Composing one module from several source files (ticket 101)

Status: proposal, awaiting the owner's scope decision. No new syntax or scope
semantics are implemented. The visibility diagnostic described below is an
independent first step.

## Existing behavior and the reproduced gap

`link "state.code"` creates a module with private top-level bindings.
`link "views.code"` creates another one. A handler in views cannot read the
state module's bindings. Handlers share a program-wide dispatch table, but
variables do not share a scope. `export` was deliberately removed; an alias
on a source link holds an empty object.

This makes a large program keep its state and views in one file even when
those are two useful editing units. It does not mean ordinary libraries
should gain implicit access to a caller's private state.

## Recommended scope model, pending approval

A source file may explicitly contribute to **the same module** as its caller.
Ordinary links retain existing isolation. A possible spelling using existing
keywords is `link "views.code" to this`; the spelling is a proposal too.

Such a file would share its owner's top-level binding slots, including writes.
Its file path would still own its source spans and relative links. It would
not add a level to `to base`, change module instances, or export values into
an unrelated caller. Initialization would run in explicit source-link order;
existing no-shadowing and declaration-before-use rules would remain.

Native archives/shared libraries and runtime-computed links would not be
eligible for same-module source composition. A same-module source link would
have no alias, since it introduces no separate module value. Cycles would use
the loader's existing cycle refusal; duplicate handlers would stay errors.

The alternatives left in the original ticket are automatic cross-file sharing
and explicit exports/read-only shared bindings. Automatic sharing would change
existing module isolation. Read-only copied objects would not give a view a
live binding when state is reassigned; introducing exported references would
be a larger value-model change. Explicit same-module source composition
preserves ordinary module boundaries without inventing reference values.

## Application shape if approved

An application's existing source entry can compose ordered state, text and
view fragments. In Euglena, a small `todo.gene.code` could link fragments in
a subdirectory; the CLI scans top-level source genes, so those fragments would
not also be auto-linked as independent modules. Installed palette/outbox genes
and the date helper could keep their existing private scopes. This may avoid
changing Euglena's manifest/codegen at all; it must be proved against the
actual application before the choice is treated as settled.

A top-level expression would still run once. File composition does **not**
make values reactive or recompute a palette automatically. Existing handlers
that recompute state-dependent values remain the correct mechanism.

## Implementation audit required after approval

Parser/loader and source-origin handling; both verifier and static checker;
inference across one composed scope without merging ordinary module scopes;
interpreter tree/bytecode binding slots and scope lifetime; native/wasm codegen
binding slots and module depth; formatting, LSP and syntax-module compatibility.
Tests must exercise writes observed across fragments, nested ordinary modules,
independent entries, relative paths, cycles, invalid native/runtime targets,
no-shadowing, initialization order and `to base` behavior.

Todo must retain its handlers and user behavior, keep its installed pins, pass
its current fixtures and browser cases and demonstrate changes observed across
state/view fragments. A source-only implementation does not imply that the
installed compiler or live shell supports it. A release tag is separate.

## Independent visibility diagnostic

Before sharing semantics change, `code run` and `code build` can identify
where an inaccessible top-level binding is declared. Search only declarations
in the loaded program, excluding other handlers' locals and unlinked files.
Keep rejecting the same program; the hint explains why a spelling that exists
elsewhere is still unavailable. `tests/linked_binding_diagnostics.rs` proves
sibling-module, caller/child and independent-entry cases in both modes.
