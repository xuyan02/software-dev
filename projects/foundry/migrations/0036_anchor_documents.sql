-- Foundry 0036: schema-described anchor documents.
--
-- Drafts carry one shared revision. Published versions are immutable. The
-- snapshot is the anchor tree; every anchor has a stable key and a SHA-256
-- content ID inside the snapshot.

create table if not exists anchor_documents (
    id              uuid primary key,
    project_id      uuid not null references projects (id) on delete cascade,
    kind            text not null,
    schema_version  integer not null,
    current_version integer not null default 0 check (current_version >= 0),
    status          text not null default 'draft',
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now(),
    unique (project_id, kind)
);

create table if not exists anchor_document_drafts (
    id                    uuid primary key,
    document_id           uuid not null unique references anchor_documents (id) on delete cascade,
    revision              bigint not null default 1 check (revision >= 1),
    base_version          integer not null default 0,
    schema_version        integer not null,
    root_anchor_id        text not null,
    snapshot              jsonb not null,
    updated_by_account_id uuid not null references accounts (id),
    updated_at            timestamptz not null default now()
);

create table if not exists anchor_document_versions (
    id               uuid primary key,
    document_id      uuid not null references anchor_documents (id) on delete cascade,
    version          integer not null check (version >= 1),
    snapshot         jsonb not null,
    root_anchor_id   text not null,
    content_hash     text not null,
    source_revision  bigint not null,
    created_by       uuid not null references accounts (id),
    approved_by      uuid references accounts (id),
    approval_comment text,
    created_at       timestamptz not null default now(),
    published_at     timestamptz,
    unique (document_id, version)
);

create unique index if not exists anchor_document_versions_revision_idx
    on anchor_document_versions (document_id, source_revision);

create table if not exists anchor_document_operations (
    id            bigserial primary key,
    draft_id      uuid not null references anchor_document_drafts (id) on delete cascade,
    op_id         uuid not null,
    base_revision bigint not null,
    type          text not null,
    payload       jsonb not null,
    actor_id      uuid not null references accounts (id),
    applied_at    timestamptz not null default now(),
    unique (draft_id, op_id)
);

create index if not exists anchor_document_operations_draft_idx
    on anchor_document_operations (draft_id, id);

create table if not exists anchor_document_approvals (
    id           uuid primary key,
    document_id  uuid not null references anchor_documents (id) on delete cascade,
    project_id   uuid not null references projects (id) on delete cascade,
    kind         text not null,
    revision     bigint not null check (revision >= 1),
    decision     text not null check (decision in ('approved', 'rejected')),
    actor_id     uuid not null references accounts (id),
    comment      text,
    content_hash text not null,
    created_at   timestamptz not null default now()
);

create index if not exists anchor_document_approvals_project_idx
    on anchor_document_approvals (project_id, kind, created_at);

create unique index if not exists anchor_document_approvals_revision_idx
    on anchor_document_approvals (project_id, kind, revision, decision);
