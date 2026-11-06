# OpenAPI

`openapi.yaml` will be generated from the Axum router and checked in CI, so the
spec cannot drift from the implementation. Until then the authoritative
contract is `docs/api/README.md` plus the handler tests in
`crates/api-http/src/routes.rs`.

Planned workflow:

1. Annotate DTOs with `utoipa`.
2. `cargo xtask openapi` writes `openapi/openapi.yaml`.
3. CI fails if the generated file differs from the committed one.
