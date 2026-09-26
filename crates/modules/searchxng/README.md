# `searchxng` — bounded SearXNG search

The module queries one configured SearXNG endpoint. It does not crawl pages,
invent URLs, follow redirects, or accept a caller-supplied endpoint.

```code
link "searchxng.so" as search
emit Config { endpoint = "http://127.0.0.1:8082", max_results = 10 } to search get ready
emit Search { query = "public weather API OpenAPI", limit = 10 } to search get found
```

Handlers:

```
Config { endpoint, max_results? } → ConfigResult { ok }
Search { query, limit?, language?, categories? }
  → SearchResult { ok, status, results }
```

Each result contains only `title`, `url`, `content`, `engine`, and
`published_at`. Transport and malformed-response failures are `Exception`
particles. Results are capped at ten by the module.
