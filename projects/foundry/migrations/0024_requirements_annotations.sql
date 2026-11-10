-- Foundry 0024: annotations on the requirements draft.
--
-- Annotations are anchored to a structured object (user/function/story/
-- operation) and an optional field, not to a linear text position. They
-- survive draft revisions so review feedback remains actionable until the
-- analyst resolves it.

create table if not exists requirements_draft_annotations (
    id            uuid primary key,
    project_id    uuid not null references projects (id) on delete cascade,
    target_type   text not null check (
        target_type in ('document', 'user', 'function', 'story', 'operation')
    ),
    target_id     text,
    field         text,
    body          text not null,
    author_id     uuid not null references accounts (id),
    status        text not null default 'open' check (status in ('open', 'resolved')),
    resolution    text,
    created_at    timestamptz not null default now(),
    resolved_at   timestamptz,
    resolved_by   uuid references accounts (id),
    check (target_type = 'document' or target_id is not null)
);

create index if not exists requirements_annotations_project_idx
    on requirements_draft_annotations (project_id, status, created_at);
