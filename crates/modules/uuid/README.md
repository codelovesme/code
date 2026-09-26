# `uuid` — time-ordered unique IDs

Version-7 UUIDs, and reading one back. For a record that needs a name that
never changes and is never handed out twice — a device, a file, an upload —
without a counter kept anywhere.

```code
link "uuid.so" as uuid

emit V7 {} to uuid get id            | id.value = "0192f0c4-8b1e-7c3a-9d2f-5a6b7c8d9e0f"
emit Parse { value = id.value } to uuid get p
assert p.version = 7                 | p.unix_ms: when it was made
```

## Handlers

```
V7    {}          → UuidResult { value }
Parse { value }   → ParsedUuid { value, version, unix_ms }
```

**Stateless** — there is nothing to configure and no setup particle.

| Field | Kind | Meaning |
|---|---|---|
| `UuidResult.value` | String | lowercase, hyphenated, 36 characters |
| `Parse.value` | String | hyphenated, simple (32 hex), braced or `urn:uuid:`, either case |
| `ParsedUuid.value` | String | the same UUID in the canonical lowercase hyphenated spelling |
| `ParsedUuid.version` | Number | 1–8; 0 for the nil UUID |
| `ParsedUuid.unix_ms` | Number or null | for a version 7, the Unix time in milliseconds it was made; null otherwise |

`Parse` of anything that is not a UUID — or not a string — is an
`Exception`. That makes it the check for an ID that came from outside.

## The decisions, and why

**Version 7 only.** Its first 48 bits are the time in milliseconds, the
rest random: IDs sort in the order they were made, so a database index
stays in order and a directory of them lists oldest first, and no two
machines need to agree on anything. Version 4 (all random) sorts in no
useful order; versions 1 and 6 carry a MAC address. A program that needs
another version is a different question.

**Strictly increasing within one process.** Two IDs made in the same
millisecond still sort in the order they were asked for — the `uuid`
crate keeps a counter in the random bits for exactly that.

**The time is readable, and that is a choice.** Anyone holding a v7 ID
can tell when it was made. For a device or a record that is fine and
useful; for a secret token it is not — use `crypto.RandomCode` there.

## Build

```sh
cargo build --release        # -> target/release/libuuid.so
```
