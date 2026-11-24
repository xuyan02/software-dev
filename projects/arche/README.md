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
- `source sync`, `source ls`, `source cat`, and `add --file` for tracked source
  files;
- code authoring with `--file`, `--module`, and `--source` on create;
- structure membership: `structure assign/list/unassigned/check`;
- document coverage checks (`doc check`) and graph checks (`check`);
- PUSH2 bundles with incoming quarantine, fast-forward checks and ref CAS;
- SSH forced-command and AuthorizedKeysCommand deployment scripts;
- `commit --author` for audit trails;
- CLI: `init [--source]`, `clone`, `remote add/show`, `push`, `fetch`, `pull`,
  `branch list/create/delete`, `checkout`, `create technical-*`,
  `structure assign/list/unassigned/check`, `doc text/list/topic/code/mermaid/html`,
  `doc check`, `comment add/list/resolve/clear`, `concept mkdir/create/list`,
  `requirement create/list`, `source fetch/checkout/worktree/status/ls/cat/sync`,
  `search`, `check`, `add [--file]`, `unstage`, `status`, `diff`,
  `commit [--author]`, `reset`, `show --format text|json|md`, `ls`, `log`.

Foundry integration:

- new projects are provisioned as Arche repositories;
- the Foundry console reads Arche repositories;
- `arche-client`/`arche-server` provide the push/fetch transport over local,
  HTTP, and SSH remotes.

Not yet implemented:

- source AST parsing; namespaces, types and members are authored by agents;
- richer Arche-specific console pages (object editor and requirement views);
- tags and additional ref operations.

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
