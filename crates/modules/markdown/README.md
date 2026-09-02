# `markdown` — CommonMark + GFM to HTML

`pulldown-cmark` under a particle interface, plus the two things a docs or
notes page always needs on top of the HTML: a table of contents, and a way
to cut a long document into sections.

```code
link "markdown.so" as md

emit RenderMarkdown { src = "# Title\n\nA [link](https://x).\n\n## Details\n" } to md get r
assert r.title = "Title"
assert r.toc = [
    { _class = "HeadingEntry", level = 1, text = "Title",   slug = "title" },
    { _class = "HeadingEntry", level = 2, text = "Details", slug = "details" }
]
| r.html has `<h1 id="title">` and `<h2 id="details">`
```

## Handlers

```
RenderMarkdown { src }            → RenderedMarkdown { html, toc, links, title, slug }
SplitByHeading { src, level }     → Chapters         { chapters }
```

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `RenderMarkdown.src` | String | — | the document. `""` renders to `""`; only a missing or non-string value is an `Exception` |
| `SplitByHeading.src` | String | — | as above |
| `SplitByHeading.level` | Number | `2` | which ATX heading level splits — a whole number `1..=6`, else an `Exception` |

`RenderedMarkdown`:

| Field | Meaning |
|---|---|
| `html` | the rendered HTML. Every heading carries `id="<slug>"` |
| `toc` | `[{ level, text, slug }]`, one per heading, in document order |
| `links` | `[{ text, href }]`, one per link |
| `title` / `slug` | the first heading's text and slug (`""` when there are no headings) |

`Chapters.chapters` is `[{ slug, title, body }]` — one per heading of
`level`. Everything before the first such heading is dropped, matching the
euglena organelle this was ported from.

## GFM

Tables, strikethrough, task lists and footnotes are on. Code fences keep
their `class="language-<lang>"` for a client-side highlighter (highlight.js,
Shiki) — there is no server-side highlighting here, and adding one would
pull a syntax-grammar corpus into every program that links this.

## Slugs

GitHub-style: lowercased, every run of non-alphanumerics becomes one `-`, no
leading or trailing `-`. A repeated heading is disambiguated the way GitHub
does it — the second `## Notes` is `notes-1`, the third `notes-2` — and the
`id` in the HTML uses the same disambiguated slug as the `toc` entry, so an
anchor link always resolves.

## The decisions, and why

**Ported from `euglena-language`'s `markdown` organelle**, with three
changes:

- **Heading `id`s are real event rewriting.** The organelle rendered the
  HTML and then searched it for `<hN>` to splice an `id` in — fragile the
  moment a heading contained anything the search didn't expect. This sets
  the `id` on the parser's own heading tag before rendering.
- **Repeated headings are disambiguated.** The organelle gave every `## Notes`
  the slug `notes`, so only the first anchor worked.
- **An empty `src` renders to empty**, where the organelle returned an
  `Error`. Rendering nothing is a valid thing to ask for. A missing or
  non-string `src` is still an `Exception`.

**No sanitization.** The HTML is trusted output for a trusted document. A
program rendering user-submitted Markdown into a page other users see must
sanitize the result itself — that policy is the caller's, and a
half-sanitizer here would be worse than none.

## Build

```sh
cargo build --release        # -> target/release/libmarkdown.so
```
