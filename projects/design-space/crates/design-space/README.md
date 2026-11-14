# design-space (`ds`)

A git-like, versioned design space for code elements. The command is `ds`
(formerly `design`).

The store is built from immutable, content-addressed nodes:

- element objects (one per language element);
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
workspace. `init` clones the source as a bare repository; that directory stays
yours: the tool only creates `.ds/` inside it (config, objects, refs and
the bare source clone). Commands work from the workspace root or any
subdirectory; `--dir <root>` selects another workspace.

The source ref is managed explicitly:

```bash
ds source status
ds source branch
ds source fetch
ds source checkout <branch|tag|commit>
ds source log [<ref>] [--limit N]
```

The source clone has no local branches: `origin/<name>`, `refs/remotes/origin/<name>`
and `<name>` all mean the same remote branch. `scan` takes no ref, never
fetches, and scans the current `source-head`.

## Remote

```bash
ds push [--branch <name>]     # upload new nodes as a pack, update the remote ref
ds fetch [--branch <name>]    # download into refs/remotes/origin/<branch>
ds pull [--branch <name>]     # fetch, then fast-forward the local branch
ds serve --listen 127.0.0.1:7000 --repo <dir>
ds serve --stdio --repo <dir> # for sshd / ssh
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

The remote URL comes from `ds init --remote <url>` and may be `http(s)://...`
or `ssh://[user@]host[:port]/abs/path`. SSH uses the system `ssh` and the same
framed protocol served by `ds serve --stdio`. Set `DS_TOKEN` on both sides to
require/attach `Authorization: Bearer` for HTTP.

## Implemented

- node encoding/decoding and SHA-256 addressing;
- loose object store;
- single-file pack with an embedded index and checksum;
- pack-aware store with an in-memory multi-pack index, `repack` and `gc`;
- `init` clones the fixed source repository; `scan` reads blobs straight
  from the git object store (no checkout, no code execution, no implicit
  fetch); source refs are switched and fetched explicitly with `ds source`;
- local branches: `branch [<name>]`, `checkout [-b]`; a branch without a
  version is marked in `.ds/branches/<name>`;
- `scan` builds objects, views, root and version and advances the branch;
- `describe` writes one design and advances the branch with a new version;
- `scan` only moves forward: the new source commit must descend from the
  branch's recorded commit;
- `scan` compares blob ids and only parses files that changed; unchanged files
  reuse their elements and designs (a local tree cache holds parent/child
  links; if it is missing they are re-parsed);
- `scan` reuses designs from the base version when input hashes are unchanged;
- tree-sitter adapters for C++, Rust, Python, Go and Java (macros and
  preprocessor blocks kept as written, never expanded);
- reads: `show` (including `--source` for the exact source span),
  `list`, `log`;
- remote: HTTP (`axum`) and SSH stdio transports; tree diff push, on-demand
  pack fetch, fast-forward pull, atomic server refs;
- CLI: the above plus `init`, `pack`, `verify`, `cat`, `ls`, `serve`, `push`,
  `fetch`, `pull`, `repack`, `gc`.

Every git blob becomes a `file` object without a design; language files also
produce element objects. Files without an adapter, files whose adapter finds
nothing, and non-UTF-8 or oversized blobs are not parsed, but their file
objects still exist (`unparsed=` in the result, per-file reasons with `-v`).
`status --files`, `list --files`, and `show <path> [--source]` expose them.
More language adapters, the packages view, and zstd/MIDX are next.
