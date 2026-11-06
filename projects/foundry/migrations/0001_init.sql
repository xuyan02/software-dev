-- Foundry 0001: initial schema.
--
-- Content (long-form requirements, design docs, ADRs) lives in Git. This
-- schema stores the operational state: work item status, ownership, events,
-- decisions and links. `body` is a JSONB snapshot of the aggregate so a row can
-- be rebuilt without replaying every event; `events` remains the audit log.

create table if not exists projects (
    id          uuid primary key,
    key         text not null unique,
    name        text not null,
    created_at  timestamptz not null default now()
);

create table if not exists work_items (
    id          uuid primary key,
    project_id  uuid not null references projects (id),
    kind        text not null check (kind in ('requirement', 'bug', 'task')),
    status      text not null,
    title       text not null,
    owner_kind  text not null check (owner_kind in ('human', 'agent', 'system')),
    owner_id    text not null,
    priority    text not null,
    risk        text not null,
    version     integer not null default 1,
    body        jsonb not null,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

create index if not exists work_items_project_status_idx
    on work_items (project_id, kind, status);
create index if not exists work_items_owner_idx
    on work_items (project_id, owner_kind, owner_id);
create index if not exists work_items_title_search_idx
    on work_items using gin (to_tsvector('simple', title));

-- Immutable domain events. One row per fact, never updated.
create table if not exists events (
    seq          bigserial,
    id           uuid primary key,
    project_id   uuid not null references projects (id),
    aggregate_id uuid not null,
    kind         text not null,
    actor        jsonb not null,
    payload      jsonb not null,
    occurred_at  timestamptz not null default now()
);

create index if not exists events_aggregate_idx on events (aggregate_id, seq);
create index if not exists events_project_seq_idx on events (project_id, seq);

-- Transactional outbox: written in the same transaction as the state change,
-- delivered asynchronously by a relay so a slow consumer cannot block writes.
create table if not exists outbox (
    id           bigserial primary key,
    event_id     uuid not null references events (id),
    attempts     integer not null default 0,
    available_at timestamptz not null default now(),
    published_at timestamptz
);

create index if not exists outbox_pending_idx
    on outbox (available_at) where published_at is null;

-- Documents are stored in Git; this is the searchable metadata mirror.
create table if not exists documents (
    id           uuid primary key,
    project_id   uuid not null references projects (id),
    kind         text not null check (kind in ('design', 'adr', 'contract', 'rfc')),
    path         text not null,
    git_revision text not null,
    status       text not null,
    version      integer not null default 1,
    updated_at   timestamptz not null default now(),
    unique (project_id, path)
);

-- Human decisions are first-class objects, not comments.
create table if not exists decisions (
    id             uuid primary key,
    project_id     uuid not null references projects (id),
    question       text not null,
    status         text not null check (status in ('open', 'decided', 'reopened', 'superseded')),
    authority_kind text not null,
    authority_id   text not null,
    reversibility  text not null,
    options        jsonb not null default '[]'::jsonb,
    evidence       jsonb not null default '[]'::jsonb,
    decision       text,
    deadline       timestamptz,
    created_at     timestamptz not null default now(),
    decided_at     timestamptz
);

create index if not exists decisions_open_idx
    on decisions (project_id, status)
    where status in ('open', 'reopened');

create table if not exists links (
    from_id  uuid not null,
    relation text not null,
    to_kind  text not null,
    to_id    uuid not null,
    note     text,
    primary key (from_id, relation, to_kind, to_id)
);

create index if not exists links_target_idx on links (to_kind, to_id);

-- Idempotency for mutating API calls. A retried request returns the stored
-- response instead of performing the action twice.
create table if not exists idempotency_keys (
    key          text primary key,
    request_hash text not null,
    response     jsonb,
    created_at   timestamptz not null default now()
);
