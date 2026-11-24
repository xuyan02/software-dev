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
- technical roots and system/module/layer/directory/file/namespace objects;
- class, struct, enum, interface, method, function, mixin and part objects;
- initial technical object model and `ObjectList`;
- `DocumentText` position field authoring;
- commit snapshots mapping qname to object hash;
- worktree/index snapshots and staged commits;
- independent comments attached by target qname;
- concept directories and concept creation under `concept/domain`,
  `concept/requirement`, and `concept/technical`;
- requirement user, story, system, module, function, and interaction objects;
- typed document fields: text, list, topic, Mermaid, and HTML;
- requirement `flow` and interaction `ui` fields;
- worktree search and graph checks;
- source Git fetch, checkout, worktree, and status;
- concept relations expressed as a text list;
- local, HTTP, and SSH remotes: `remote add/show`, `clone`, `push`, `fetch`,
  `pull`;
- HTTP server library (`arche_server::serve`) and `arche serve`;
- SSH stdio server library (`arche_server::serve_stdio`) and `arche serve --stdio`;
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

## Crates

- `arche-core` (`arche_core`): object model, codec, store, snapshots, local
  repository and wire helpers;
- `arche-client` (`arche_client`): push, fetch, pull and clone over local,
  HTTP and SSH transports;
- `arche-server` (`arche_server`): HTTP and stdio server library used by
  Foundry and by `arche serve`;
- `arche`: command-line interface.

## Build

```bash
cargo build -p arche
cargo test -p arche -p arche-core -p arche-client -p arche-server
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
