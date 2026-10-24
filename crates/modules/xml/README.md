# `xml` — XML text into a tree

For reading what arrives as XML: an RSS or Atom feed, a sitemap, a SOAP or
OpenAPI document. One handler, `Parse`, turns the text into elements a
program walks like any other value.

```code
link "xml.so" as xml

emit Parse { value = feed_text } to xml get feed
channel = feed.value.children[0]     | feed.value.tag = "rss"
```

## Handlers

```
Parse { value }   → ParseResult { value }   | the root element
```

**Stateless** — there is nothing to configure and no setup particle.

An element is `{ tag, attrs, children }`:

| Field | Kind | Meaning |
|---|---|---|
| `tag` | String | the name as written, prefix kept (`dc:creator`, `atom:link`) |
| `attrs` | Object | each attribute's name to its value, entities decoded |
| `children` | Array | texts and elements, in document order |

- Text that is only whitespace between elements is dropped: it is the
  document's layout, not its content.
- Entities (`&amp;`, `&#233;`) are decoded; a CDATA section is text, joined
  to any text beside it.
- Comments, processing instructions, the `<?xml ?>` declaration and the
  DOCTYPE are not part of the tree.
- Text that is not well-formed answers an `Exception` saying where: a tag
  closed out of order, one never closed, a second root, text outside the
  root, an unknown entity.

Native only: there is no browser half.
