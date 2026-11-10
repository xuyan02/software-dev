-- Foundry 0026: draft-based project brief editing and versions.
--
-- A draft is an independent, server-persisted working copy. Editing is a
-- sequence of structured operations. Submitting a draft creates a new
-- immutable version, moves the current pointer and deletes the draft.

alter table project_briefs drop constraint if exists project_briefs_pkey;
alter table project_briefs add column if not exists version integer not null default 1;
alter table project_briefs add column if not exists created_at timestamptz not null default now();
alter table project_briefs drop column if exists revision;
alter table project_briefs add primary key (project_id, version);

create table if not exists project_brief_current (
    project_id  uuid primary key references projects (id) on delete cascade,
    version     integer not null,
    updated_at  timestamptz not null default now(),
    foreign key (project_id, version)
        references project_briefs (project_id, version) on delete cascade
);

insert into project_brief_current (project_id, version, updated_at)
select project_id, version, updated_at from project_briefs
on conflict (project_id) do nothing;

create table if not exists project_brief_drafts (
    id          uuid primary key,
    project_id  uuid not null references projects (id) on delete cascade,
    body        jsonb not null,
    revision    bigint not null default 1 check (revision >= 1),
    created_by  uuid not null references accounts (id),
    updated_by  uuid not null references accounts (id),
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);

create index if not exists project_brief_drafts_owner_idx
    on project_brief_drafts (project_id, created_by, updated_at desc);

create table if not exists project_brief_draft_operations (
    id             bigserial primary key,
    draft_id       uuid not null references project_brief_drafts (id) on delete cascade,
    op_id          uuid not null,
    base_revision  bigint not null,
    operation      jsonb not null,
    actor_id       uuid not null references accounts (id),
    applied_at     timestamptz not null default now(),
    unique (draft_id, op_id)
);

create index if not exists project_brief_operations_draft_idx
    on project_brief_draft_operations (draft_id, id);
