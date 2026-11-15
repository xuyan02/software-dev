# design-space (`ds`)

A git-like, versioned design space for conceptual design elements. The command is `ds`
(formerly `design`).

The store is built from immutable, content-addressed nodes:

- concept objects (one per design element);
- design text;
- index nodes (views such as namespaces, files, packages);
- versions.

See `../../docs/design.md` for the full design.

## Build

```bash
cargo build -p design-space
cargo test -p design-space
```

The binary is `ds`:

```bash
cargo run -p design-space -- --help
```

## Workspace

Run `ds init --source <url>` in the directory that should be the design
workspace. `init` creates a new local workspace and clones the source as a
bare repository; that directory stays yours: the tool only creates `.ds/`
inside it (config, objects, refs and the bare source clone). `init` never
talks to a design-space remote; connect one afterwards with
`ds remote add <url>`. Commands work from the workspace root or any
subdirectory; `--dir <root>` selects another workspace.

To copy an existing design space, use `ds clone`:

```bash
ds clone ds://host/<project-id> [<directory>]
ds clone ds://host/<project-id> --source <git-url> [<directory>]
```

It creates `<directory>` (default: the URL's last segment), initializes the
workspace, and fast-forwards the chosen design branch (`--branch`; default:
remote `main`, else `master`, else the only branch). The source git URL comes
from the project's design-space metadata (set at creation with
`project create --source` or by Foundry); `--source` overrides it and is
needed for projects created without that metadata.

The source ref is managed explicitly:

```bash
ds source status
ds source branch
ds source fetch
ds source checkout <branch|tag|commit>
ds source log [<ref>] [--limit N]
```

The source clone has no local branches: `origin/<name>`, `refs/remotes/origin/<name>`
and `<name>` all mean the same remote branch. Source commands only give an
agent read-only code at a commit; they do not produce design-space elements.

## Conceptual elements

A design-space element is a *conceptual* object — a capability, domain concept,
component responsibility or constraint — not a syntax node. The tool does not
parse code. An agent (or a human) reads the code and creates elements
explicitly; each command advances the current branch with a new version:

```bash
ds create Auth --design "认证域" --path src/auth.rs --lines 1-80
ds create Auth::Login --design "登录流程" --path src/auth.rs --lines 20-60
ds create Auth::Session --design-file session.md
ds describe Auth::Login --design "更新后的登录流程"
```

`--path`/`--lines` are optional source anchors for evidence and navigation,
not part of identity. A missing `--design` creates the element undesigned
(`status --missing` lists it). `--parent` sets an explicit parent; otherwise a
parent is inferred from `::` name segments that already exist.

## Projects

The server is a standalone binary, `ds-server`. It owns projects and their
design-space repositories and serves all of them, over HTTP or SSH stdio.
Creating a project only allocates a design space and returns its address; the
caller (for example Foundry) owns any git-repository requirement:

```bash
ds-server project create --root /srv/ds --name "Demo App" \
  --source git@example.com:demo.git \
  --host ds.example.com --user ds
# Project Demo App
#   Design space: ds://ds@ds.example.com/01a1db53c0ffee42

ds-server project list --root /srv/ds
ds-server project show "Demo App" --root /srv/ds
ds-server project set-source "Demo App" --source git@example.com:demo.git --root /srv/ds
```

Names are display metadata, not identities: creating two projects with the
same name is allowed and each gets its own repository, stored under
`<root>/<project-id>`. Use the id from the address (or `project list`) to pick
one unambiguously. Repositories created by older versions keep their
`<root>/<slug>` directory and remain readable.

Or over HTTP:

```bash
ds-server serve --root /srv/ds --listen 0.0.0.0:7000 --host ds.example.com --user ds
curl -X POST http://server:7000/v1/projects \
  -H 'content-type: application/json' \
  -d '{"name":"Demo App"}'
```

## Remote

```bash
ds remote add <url>           # set origin on a workspace created by init
ds remote show                # print the configured origin
ds remote set-url <url>       # replace origin
ds push [--branch <name>]     # upload new nodes as a pack, update the remote ref
ds fetch [--branch <name>]    # download into refs/remotes/origin/<branch>
ds pull [--branch <name>]     # fetch, then fast-forward the local branch
# server side
ds-server serve --root /srv/ds --listen 0.0.0.0:7000
ds-server stdio --root /srv/ds --project 01a1db53c0ffee42  # for sshd / ssh
ds-server stdio --repo /srv/ds/demo-app     # legacy: explicit repository path
```

Storage maintenance:

```bash
ds pack     # pack loose nodes into packs/ and remove the loose copies
ds pack --output <file>   # export every node; deletes nothing
ds repack   # one pack for every node; removes old packs and loose copies
ds gc       # reachable from refs; repack; drops unreachable nodes and stale caches
```

The store reads a node from loose objects first, then from the multi-pack index
built by scanning each pack's trailer and index. Node payloads are read on
demand, so packs can replace loose files safely.

The remote URL is set by `ds remote add <url>` on an initialized workspace or
by `ds clone <url>` on a new one; `ds remote show` prints it and
`ds remote set-url <url>` replaces it. It may be `http(s)://...` or
`ssh://[user@]host[:port]/<project-id>`. The preferred form is
`ds://[user@]host[:port]/<project-id>`; it is equivalent to SSH. The address
carries only the project's stable 16-hex id, never a filesystem path: the
server resolves it under its root (`DESIGN_SPACE_ROOT`, default
`~/.foundry/design-spaces`) and runs `ds-server stdio --project <id>`. SSH
uses the system `ssh` and the same framed protocol served by
`ds-server stdio`. A `ds://localhost/...` (or `ds://127.0.0.1/...`) address is
served directly from the local design-space root — `DESIGN_SPACE_ROOT`, default
`~/.foundry/design-spaces` — when that project exists there, so a local
Foundry setup works without an sshd; use `ssh://localhost/...` to force SSH.
Multi-segment paths are still accepted for an explicit repository
(`--repo /abs/path`). Set `DS_TOKEN` on both sides to
require/attach `Authorization: Bearer` for HTTP.

## Implemented

- node encoding/decoding and SHA-256 addressing;
- loose object store;
- single-file pack with an embedded index and checksum;
- pack-aware store with an in-memory multi-pack index, `repack` and `gc`;
- `init` clones the fixed source repository for read-only agent access;
  source refs are switched and fetched explicitly with `ds source`;
- local branches: `branch [<name>]`, `checkout [-b]`; a branch without a
  version is marked in `.ds/branches/<name>`;
- `create` adds one conceptual element (name, optional design, optional source
  anchor, optional parent) and advances the branch with a new version;
- `describe` writes or replaces one design and advances the branch;
- no code parsing: tree-sitter adapters and automatic element extraction have
  been removed. Elements are authored by agents/humans reading the source;
- source anchors are optional (`--path`, `--lines`); they provide evidence and
  a source view but do not define identity;
- parent/child links come from `--parent` or existing `::` name segments; the
  design-space server stores the authored model, not syntax;
- reads: `status`, `show` (including `--source` when an anchor exists),
  `list`, `log`;
- remote: HTTP (`axum`) and SSH stdio transports; tree diff push, on-demand
  pack fetch, fast-forward pull, atomic server refs;
- CLI: the above plus `init`, `pack`, `verify`, `cat`, `ls`, `serve`, `push`,
  `fetch`, `pull`, `repack`, `gc`. The server binary `ds-server` owns
  `serve`, `stdio`, `project create|list|show`, `gc` and `repack`.

Elements and file objects are created explicitly by authors; the tool no
longer walks git blobs to invent them. A design space can start empty and is
filled by `ds create` as an agent reads the code.
More language adapters, the packages view, and zstd/MIDX are next.
