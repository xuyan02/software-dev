-- Foundry 0007: immutable workflow definitions, pinned Task versions and the
-- project root reference.
--
-- Published versions are never updated or deleted. Tasks pin
-- (workflow, workflow_version); the startup bootstrap seeds the nine platform
-- v1 definitions before the deferred foreign key is validated.

create table if not exists workflow_definitions (
    id                text primary key,
    owner_account_id  uuid references accounts (id),
    title             text not null,
    created_at        timestamptz not null default now(),
    updated_at        timestamptz not null default now()
);

create index if not exists workflow_definitions_owner_idx
    on workflow_definitions (owner_account_id, updated_at desc);

create table if not exists workflow_versions (
    workflow_id              text not null references workflow_definitions (id),
    version                  integer not null check (version >= 1),
    body                     jsonb not null,
    published_by_account_id  uuid references accounts (id),
    published_at             timestamptz not null default now(),
    primary key (workflow_id, version)
);

create table if not exists workflow_drafts (
    workflow_id              text primary key references workflow_definitions (id),
    revision                 bigint not null default 1 check (revision >= 1),
    body                     jsonb not null,
    last_client_save_id      uuid,
    updated_by_account_id    uuid not null references accounts (id),
    updated_at               timestamptz not null default now()
);

create table if not exists workflow_layouts (
    workflow_id              text primary key references workflow_definitions (id),
    layout                   jsonb not null default '{}'::jsonb,
    updated_by_account_id    uuid not null references accounts (id),
    updated_at               timestamptz not null default now()
);

alter table tasks
    add column if not exists workflow_version integer not null default 1;

do $$
begin
    if not exists (
        select 1 from pg_constraint where conname = 'tasks_workflow_version_fk'
    ) then
        alter table tasks
            add constraint tasks_workflow_version_fk
            foreign key (workflow, workflow_version)
            references workflow_versions (workflow_id, version)
            not valid;
    end if;
end $$;

create index if not exists tasks_workflow_version_idx
    on tasks (workflow, workflow_version);

alter table projects
    add column if not exists root_task_id uuid;

do $$
begin
    if not exists (
        select 1 from pg_constraint where conname = 'projects_root_task_fk'
    ) then
        alter table projects
            add constraint projects_root_task_fk
            foreign key (root_task_id) references tasks (id)
            deferrable initially deferred;
    end if;
end $$;

create unique index if not exists projects_root_task_idx
    on projects (root_task_id)
    where root_task_id is not null;
