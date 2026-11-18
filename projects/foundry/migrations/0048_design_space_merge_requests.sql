-- Foundry 0048: design-space merge requests.
--
-- A merge request stores branch and merge metadata only. Approval is derived
-- from the linked Task; this table intentionally has no approval status,
-- reviewer or comment columns.

create table if not exists design_space_merge_requests (
    id             uuid primary key,
    project_id     uuid not null references projects (id) on delete cascade,
    title          text not null,
    description    text not null default '',
    source_branch  text not null,
    target_branch  text not null,
    source_commit  text not null,
    target_commit  text not null,
    task_id        uuid not null unique references tasks (id) on delete cascade,
    merge_commit   text,
    created_by     uuid not null references accounts (id),
    created_at     timestamptz not null,
    updated_at     timestamptz not null,
    check (source_branch <> target_branch)
);

create index if not exists design_space_merge_requests_project_idx
    on design_space_merge_requests (project_id, created_at desc);
