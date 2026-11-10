-- Foundry 0012: project briefs written by clarification.

create table if not exists project_briefs (
    project_id    uuid primary key references projects (id),
    revision      bigint not null default 1 check (revision >= 1),
    status        text not null default 'draft',
    body          jsonb not null,
    created_by    uuid not null references accounts (id),
    approved_by   uuid references accounts (id),
    updated_at    timestamptz not null default now()
);
