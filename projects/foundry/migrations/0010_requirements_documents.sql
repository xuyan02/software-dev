-- Foundry 0010: structured requirements documents.
--
-- One document per project. Drafts are the shared working copy; published
-- versions are immutable. The operation log is append-only and is used for
-- idempotency and audit.

create table if not exists requirements_documents (
    id              uuid primary key,
    project_id      uuid not null unique references projects (id),
    current_version integer not null default 0 check (current_version >= 0),
    status          text not null default 'draft',
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now()
);

create table if not exists requirements_document_versions (
    id            uuid primary key,
    document_id   uuid not null references requirements_documents (id),
    version       integer not null check (version >= 1),
    body          jsonb not null,
    content_hash  text not null,
    created_by    uuid not null references accounts (id),
    approved_by   uuid references accounts (id),
    created_at    timestamptz not null default now(),
    published_at  timestamptz,
    unique (document_id, version)
);

create index if not exists requirements_versions_document_idx
    on requirements_document_versions (document_id, version desc);

create table if not exists requirements_document_drafts (
    id                    uuid primary key,
    document_id           uuid not null unique references requirements_documents (id),
    revision              bigint not null default 1 check (revision >= 1),
    base_version          integer not null default 0,
    body                  jsonb not null,
    updated_by_account_id uuid not null references accounts (id),
    updated_at            timestamptz not null default now()
);

create table if not exists requirements_operations (
    id            bigserial primary key,
    draft_id      uuid not null references requirements_document_drafts (id),
    op_id         uuid not null,
    base_revision bigint not null,
    type          text not null,
    target        jsonb not null default '{}'::jsonb,
    payload       jsonb not null,
    actor_id      uuid not null references accounts (id),
    result        text not null,
    error         text,
    applied_at    timestamptz not null default now(),
    unique (draft_id, op_id)
);

create index if not exists requirements_operations_draft_idx
    on requirements_operations (draft_id, id);
