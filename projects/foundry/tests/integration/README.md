# Integration tests

Tests that exercise adapters against real infrastructure:

- Postgres repository: optimistic concurrency, event persistence, migrations.
- Git document store: front matter round-trip, branch/commit, metadata sync.
- Policy engine: OPA decision fixtures.

Nothing here yet; M1 starts with the Postgres repository.
