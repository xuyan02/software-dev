-- Foundry 0003: accounts, login sessions and project membership.
--
-- Identity is an account, never a value supplied in a request body. Humans and
-- agents are both accounts; the kind decides which authentication mechanism is
-- expected. Only a hash of a session token is stored, so a database leak cannot
-- be replayed as a live session.

create table if not exists accounts (
    id             uuid primary key,
    handle         text not null unique,
    display_name   text not null,
    kind           text not null check (kind in ('human', 'agent')),
    role           text not null default 'member' check (role in ('admin', 'member')),
    status         text not null default 'active' check (status in ('active', 'suspended')),
    email          text,
    password_hash  text,
    version        integer not null default 1,
    created_at     timestamptz not null default now(),
    updated_at     timestamptz not null default now(),
    check (kind = 'agent' or password_hash is not null)
);

create index if not exists accounts_role_status_idx
    on accounts (role, status);

create table if not exists sessions (
    id          uuid primary key,
    account_id  uuid not null references accounts (id) on delete cascade,
    token_hash  text not null unique,
    created_at  timestamptz not null default now(),
    expires_at  timestamptz not null,
    revoked_at  timestamptz
);

create index if not exists sessions_account_idx
    on sessions (account_id, expires_at);
create index if not exists sessions_live_idx
    on sessions (token_hash, expires_at)
    where revoked_at is null;

-- Projects are isolation boundaries: an issue or requirement belongs to exactly
-- one project, and membership decides who can see it.
alter table projects
    add column if not exists description text,
    add column if not exists status text not null default 'active'
        check (status in ('active', 'archived')),
    add column if not exists version integer not null default 1,
    add column if not exists updated_at timestamptz not null default now();

create table if not exists project_members (
    project_id  uuid not null references projects (id) on delete cascade,
    account_id  uuid not null references accounts (id) on delete cascade,
    role        text not null check (role in ('viewer', 'member', 'maintainer', 'owner')),
    added_at    timestamptz not null default now(),
    primary key (project_id, account_id)
);

create index if not exists project_members_account_idx
    on project_members (account_id);

-- Every operational row is project-scoped. Requirements were created in 0001
-- before project isolation existed; the column is already present there, so
-- this migration only documents the invariant and adds the lookup index.
create index if not exists work_items_project_updated_idx
    on work_items (project_id, updated_at desc);
create index if not exists issues_project_updated_idx
    on issues (project_id, updated_at desc);

-- Domain events may belong to an account or a project rather than an issue or
-- requirement. Keep project_id nullable for those control-plane events; the
-- query path still uses aggregate_id for per-object history.
alter table events
    alter column project_id drop not null;
