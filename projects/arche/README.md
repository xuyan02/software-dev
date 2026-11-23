# Arche

Arche is the versioned design repository for Foundry: architecture, requirements,
concepts, history, and engineering artifacts in one content-addressed store.

The v2 design is in [`docs/design.md`](docs/design.md).

## Status

Initial implementation slice:

- canonical object records (`DSO2`) and commit records (`CMT2`);
- SHA-256 object/symbol/commit addressing;
- atomic object store with `.arche/`;
- technical, requirements, and concepts roots;
- `TechnicalSystem` and `TechnicalModule` creation;
- initial technical object model and `ObjectList`;
- `DocumentText` position field authoring;
- commit snapshots mapping qname to object hash;
- worktree/index snapshots and staged commits;
- independent comments attached by target qname;
- concept directories and concept creation under `concepts/domain`,
  `concepts/requirements`, and `concepts/technical`;
- requirement user, story, system, module, function, and interaction objects;
- typed document fields: text, list, topic, Mermaid, and HTML;
- requirement `flow` and interaction `ui` fields;
- worktree search and graph checks;
- source Git fetch, checkout, worktree, and status;
- concept links (`refines`, `realizes`, `corresponds`) with direction checks;
- local, HTTP, and SSH remotes: `remote add/show`, `clone`, `push`, `fetch`,
  `pull`;
- HTTP server (`arche-server`) with optional bearer token;
- SSH stdio server mode (`arche-server --stdio`);
- branches and `checkout`;
- JSON and Markdown `show --format`;
- requirement `ui` HTML and `flow` Mermaid validation;
- `source sync` and `add --file` for tracked source files;
- CLI: `init [--source]`, `clone`, `remote add/show`, `push`, `fetch`, `pull`,
  `branch list/create/delete`, `checkout`, `create technical-system`,
  `create technical-module`, `doc text/list/topic/mermaid/html`,
  `comment add/list/resolve`, `concept mkdir/create/list/link`,
  `requirement create/list`, `source fetch/checkout/worktree/status/sync`,
  `search`, `check`, `add [--file]`, `unstage`, `status`, `diff`, `commit`,
  `reset`, `show --format text|json|md`, `ls`, `log`.

Not yet implemented:

- HTTP/SSH remote push fast-forward and ref CAS;
- SSH forced-command/allowlist deployment;
- full source directory/namespace/class object sync;
- Foundry UI integration.

The detailed implementation status and remaining work are recorded in
[`docs/design.md` §23](docs/design.md#23-当前实现与剩余工作).

## Build

```bash
cargo build -p arche
cargo test -p arche
```

## Quick start

```bash
cargo run -p arche -- init /tmp/design
cd /tmp/design
cargo run --manifest-path /path/to/software-dev/Cargo.toml -p arche -- create technical-system app
cargo run --manifest-path /path/to/software-dev/Cargo.toml -p arche -- doc text set technical/app position '应用入口。'
cargo run --manifest-path /path/to/software-dev/Cargo.toml -p arche -- add -A
cargo run --manifest-path /path/to/software-dev/Cargo.toml -p arche -- commit -m 'add app'
cargo run --manifest-path /path/to/software-dev/Cargo.toml -p arche -- show technical/app
cargo run --manifest-path /path/to/software-dev/Cargo.toml -p arche -- ls
```
