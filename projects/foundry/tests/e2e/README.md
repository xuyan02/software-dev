# End-to-end tests

Black-box tests that start the control plane and drive it over HTTP/MCP.

Currently the same flows are covered as in-process tests in
`crates/api-http/src/routes.rs`. This directory is for tests that need a real
process, real sockets, or Postgres (M1). Suggested first cases:

- issue intake → comment → agent resolves → human closes
- issue report → human rejects or marks duplicate, with the reason recorded
- agent closing an issue returns 403 and does not mutate state
- create requirement → agent review → human approve → design → development → verification → done
- stale `expected_version` returns 409 and does not mutate state
- restart with Postgres preserves state and history (M1)
