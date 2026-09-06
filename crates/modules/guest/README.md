# `guest` — one application running inside another

```code
link "guest.so" as guest
link "dom.so" as dom
link "storage.so" as store

| A launcher: the reader picks an application, and it opens in the panel.
Open { app } => {
    emit Load { app = app, url = "/apps/$app.wasm", into = "#panel" } to guest get r
    return r
}

| Asked once, the first time that application reaches for a module.
Offer { app, name } => {
    if name = "storage" {
        return Offered { }
    }
}

| Everything it sends to a module this shell took. One session, and every
| application signed in with it.
Module { app, name, particle } => {
    emit particle to store get answer
    return answer
}
```

## The handlers

```
Load   { app, url, into? } → LoadResult   { ok, reason }
Unload { app }             → UnloadResult { ok }
Tell   { app, particle }   → TellResult   { ok }
```

`into` is a CSS selector, `"body"` by default. `app` is a name of letters,
digits, `-` and `_` — it is the mark on the container, the prefix on the
guest's stored keys and the head of its path, so it is kept to what is
literal in all three.

**One instance per name.** A second `Load` of a name already running is
refused with a reason rather than started beside it: the two would share
every stored key and the address. The name comes free again on `Unload`.

## What a guest is

One `.wasm`, and nothing else. Its own code, its modules and the language
runtime are already inside it, so there is no manifest to read, no assets to
sequence and nothing to fetch beside it.

**It does not know it is a guest.** The same file runs on its own page — it
is not built twice, and nothing in its source changes. The only way it could
tell is by asking the shell's own modules who is signed in, which an
application that does not ask, cannot.

## Who answers a guest

The same two questions a host answers on a machine, in the same words:

- **`Offer { app, name }`** — asked once, the first time a guest reaches for
  a module. `Offered { }` puts the host between the guest and that module for
  good; `Denied { }` refuses it; answering neither leaves the guest with the
  page's own half.
- **`Module { app, name, particle }`** — every particle to a module the host
  took. Answer in its place, or forward to the host's own copy and hand back
  what it said.

`app` says which guest is asking, so one may be offered what another is
denied. A denied module is not a failed load: the guest links it and gets an
`Exception` on first use, the way it would from a network that is not there.
A host is never ended by its own policy.

**A host that writes no handler still hosts.** Every module the guest asks
for is then its own, in a world of its own — which is what writing no `Offer`
means on a machine too. Hosting is not a thing you have to opt into twice.

What differs from a machine is what *its own* can mean. A held `.so` opens
its own modules — its own file, its own settings — and its host never sees
them. A page has no dlopen: every half a guest can reach is the page's, out
of the host's own build. So a guest's own module is the same half, given a
world of its own:

- its **`dom`** gets a document that stops at its container — `body` means
  the container, a selector cannot match outside it, and its stylesheet is
  moved under it, so two guests cannot restyle each other or the shell;
- its **`storage`** keys are prefixed with its name, so two applications that
  each keep a `token` keep two of them;
- its **`router`** reads the path after its name, so the page keeps one
  address bar and every application on it still starts at its own root.

None of that is a boundary. A guest shares this page's memory like everything
else on it, and the page could read all of it. It is containment of an honest
application, which is what a shell of one's own applications needs.

**What a guest draws stays the guest's.** The nodes are made by the same
`dom` half, so a click on them fires at the guest's handlers and not at the
shell's. The shell decides; this module does.

## Two things to know before writing a shell

**A guest reaches only the modules its host linked.** The halves that answer
on a page come out of the *host's* build, so an application that links
`storage` inside a shell that never did asks a page with nobody to answer,
and is handed null. A shell links what it means to offer.

**`Load` answers when the guest is on its way, not when it is running.**
Fetching cannot be waited for in a page without freezing the reader. The
arrival comes back as a particle at the host's own handlers — `Loaded { app }`,
or an `Exception` from `guest` saying what went wrong. `Tell` is the same
kind of answer: the particle is handed over, and the guest runs once the call
it was told in has returned. What its handler answers is let go of — it was
told, not asked — with one exception, which is an `Exception`: a handler that
fails answers one, and rather than dropping it on the floor this module fires
it at the host with the guest's name on it.

## Letting one go

`Unload` stops the guest, clears its container, takes its stylesheet off the
page and drops every reference this module held — so its memory comes back
rather than merely stopping being routed to. A delay it set before it went
fires into nothing, and a particle told to it in the meantime is dropped
rather than delivered to whatever takes its name next.

## Where it works

**A browser.** On a machine every handler answers an `Exception` saying so:
running another application on a machine is what *linking* one does — see
`code_abi.h`'s hosting section and
[`link` while the program runs](../../../README.md#linking-while-the-program-runs)
— and this is not that. The module is still linkable there, so one
application can be built both ways and ask
[`Linked`](../../../README.md#linked) which it is.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```
