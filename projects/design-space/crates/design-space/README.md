# design-space (`ds`)

A git-like, versioned design space for conceptual design objects.

Design objects are immutable and content-addressed. A View binds the files and
namespaces trees to a source git commit. Symbols and design documents are
parallel content-addressed objects. Local worktree/index state and commits are
managed like Git.

The format is specified in [`../../docs/storage-format.md`](../../docs/storage-format.md)
and the CLI in [`../../docs/cli.md`](../../docs/cli.md).

## Build

```bash
cargo build -p design-space
cargo test -p design-space
```

This builds `ds` and `ds-server`.

## Quick start

```bash
ds init --source https://example.com/repo.git
ds source fetch

ds create directory src --parent files
ds create file auth.rs --parent files/src --source src/auth.rs
ds create namespace app --parent namespaces
ds create class Auth --namespace namespaces/app --file files/src/auth.rs --source src/auth.rs
ds create method login --parent namespaces/app/Auth --signature 'login()' --source src/auth.rs

ds add -A
ds status
ds commit -m "describe auth"
ds show namespaces/app/Auth
ds log
```

## Implemented

- design object kinds: view, directory, file, namespace, class, interface,
  method, function, part, mixin;
- canonical DSO1/CMT1 encoding and SHA-256 addressing;
- zero-hash None sentinel and local structural validation;
- content-addressed object/doc/symbol/commit store with atomic publication;
- workspace config, refs, HEAD, index and worktree state;
- catalog loading and full View graph rebuilding, including multi-parent
  children and cycle/orphan detection;
- CLI: `init`, `clone`, `create`, `edit`, `dep`, `source`, `add`, `reset`,
  `diff`, `rm`, `checkout`, `ls`, `status`, `commit`, `show`, `log`, `gc`,
  `repack`;
- automatic qualified-name migration when renaming or reparenting;
- semantics checks for parent kinds, relation targets, relation cycles,
  override closures, and structural dependencies;
- bare source clone and source fetch/checkout/ls/cat/log;
- `show --source` reads the anchored span from the view's source commit;
- reachability-based GC and PAK1 pack read/write (`repack`);
- remotes: local directories, HTTP (`ds-server`), and SSH; `remote
  add/show/set-url`, `push`, `fetch`, `pull`;
- HTTP bearer-token authentication;
- server-side upload verification and canonical path checks.

## Remote

```bash
# On the remote host: one server manages a collection of repositories.
# Foundry creates repositories with POST /repos.
mkdir -p /srv/design-spaces
ds-server --root /srv/design-spaces --listen 127.0.0.1:7700 \
  --token-file /srv/design.token

# Create a repository (name + optional branch, default main).
# source_url is recorded in the repository's repo.toml for `ds clone`.
curl -X POST http://127.0.0.1:7700/repos \
  -H 'Authorization: Bearer <token>' \
  -H 'Content-Type: application/json' \
  -d '{"name":"design","source_url":"https://example.com/repo.git"}'

# The client reads the HTTP token from a preconfigured path, never from a
# command argument:
install -m 600 /srv/design.token ~/.config/ds/token

# In the workspace:
ds remote add http://127.0.0.1:7700/design
ds push
ds fetch
ds pull

# Clone (source URL comes from the remote's repo.toml):
ds clone http://127.0.0.1:7700/design --branch main

# SSH: ds-server must be on the remote PATH; the path is one repository.
ds remote add ssh://user@host/srv/design-spaces/design
ds push
```

Remote URLs also accept local paths and `file://` URLs. HTTP authentication is a
single bearer token; omit `--token-file` on `ds-server` to serve without
authentication. The client reads `/run/foundry/ds_token`, then
`$XDG_CONFIG_HOME/ds/token`, then `~/.config/ds/token`; commands do not accept a
token argument. There is no built-in TLS, so authenticate only over a trusted
network, a TLS reverse proxy, or SSH. SSH remotes use SSH authentication and do
not read the token file.
