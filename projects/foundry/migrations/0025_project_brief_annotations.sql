-- Foundry 0025: annotations on the project brief.
--
-- Reviewers anchor feedback to a brief section or object. Annotations survive
-- draft revisions until the clarifier resolves them.

create table if not exists project_brief_annotations (
    id            uuid primary key,
    project_id    uuid not null references projects (id) on delete cascade,
    target_type   text not null check (
        target_type in (
            'document', 'problem', 'goals', 'success_metrics', 'scope',
            'constraints', 'risks', 'stakeholders', 'budget', 'assumptions',
            'open_questions'
        )
    ),
    target_id     text,
    field         text,
    body          text not null,
    author_id     uuid not null references accounts (id),
    status        text not null default 'open' check (status in ('open', 'resolved')),
    resolution    text,
    created_at    timestamptz not null default now(),
    resolved_at   timestamptz,
    resolved_by   uuid references accounts (id)
);

create index if not exists project_brief_annotations_project_idx
    on project_brief_annotations (project_id, status, created_at);
