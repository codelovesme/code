# api_registry

Cached search over the APIs.guru OpenAPI directory.

- `Config { endpoint, max_results?, cache_seconds? }`
- `Warm { wait? }` starts a background refresh; `wait = true` is for tests.
- `Search { query, limit? }`
- `Stop {}` joins an active refresh before the host unloads the worker.

`SearchResult.results` contains `provider`, `title`, `description`, `url`, and
`score`. Only HTTPS JSON specification URLs from the registry are returned.
Search never waits for the network: while the cache is warming it returns
`ready = false` and an empty result list.
