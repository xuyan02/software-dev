# Schemas

Canonical JSON Schemas for MCP tool inputs and domain documents will live here.
The MCP tool schemas are currently defined inline in
`crates/api-mcp/src/lib.rs` so the server and the published schema cannot
disagree; they will be extracted here once a second consumer exists.

Planned files:

- `requirement.schema.json`
- `design-doc.schema.json`
- `adr.schema.json`
- `decision.schema.json`
