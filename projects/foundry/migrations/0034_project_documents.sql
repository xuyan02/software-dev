-- Foundry 0034: separate project document.
--
-- One project document per project. Drafts are the shared working copy;
-- approved versions are immutable. Every revision must be approved.

create table if not exists project_documents (
    id              uuid primary key,
    project_id      uuid not null unique references projects (id),
    current_version integer not null default 0 check (current_version >= 0),
    status          text not null default 'draft',
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now()
);

create table if not exists project_document_versions (
    id               uuid primary key,
    document_id      uuid not null references project_documents (id),
    version          integer not null check (version >= 1),
    body             jsonb not null,
    content_hash     text not null,
    source_revision  bigint,
    created_by       uuid not null references accounts (id),
    approved_by      uuid references accounts (id),
    approval_comment text,
    created_at       timestamptz not null default now(),
    published_at     timestamptz,
    unique (document_id, version)
);

create unique index if not exists project_document_versions_source_revision_idx
    on project_document_versions (document_id, source_revision)
    where source_revision is not null;

create index if not exists project_document_versions_document_idx
    on project_document_versions (document_id, version desc);

create table if not exists project_document_drafts (
    id                    uuid primary key,
    document_id           uuid not null unique references project_documents (id),
    revision              bigint not null default 1 check (revision >= 1),
    base_version          integer not null default 0,
    body                  jsonb not null,
    updated_by_account_id uuid not null references accounts (id),
    updated_at            timestamptz not null default now()
);

create table if not exists project_document_operations (
    id            bigserial primary key,
    draft_id      uuid not null references project_document_drafts (id),
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

create index if not exists project_document_operations_draft_idx
    on project_document_operations (draft_id, id);

create table if not exists project_document_approvals (
    id            uuid primary key,
    project_id    uuid not null references projects (id) on delete cascade,
    revision      bigint not null check (revision >= 1),
    decision      text not null check (decision in ('approved', 'rejected')),
    actor_id      uuid not null references accounts (id),
    comment       text,
    content_hash  text not null,
    created_at    timestamptz not null default now()
);

create index if not exists project_document_approvals_project_idx
    on project_document_approvals (project_id, created_at);

create unique index if not exists project_document_approvals_revision_decision_idx
    on project_document_approvals (project_id, revision, decision);
