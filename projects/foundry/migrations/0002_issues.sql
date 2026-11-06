-- Foundry 0002: issues and discussion.
--
-- Issues are the problem space. They are intentionally lighter than
-- requirements: intake is cheap, classification happens at triage, and only an
-- explicit disposition turns an issue into planned work.
--
-- `body` is a JSONB snapshot of the Issue aggregate (including history and
-- links). Scalar columns mirror the fields needed for filtering and indexes.
-- Comments are also projected into `issue_comments` so discussion can be
-- queried without deserializing every aggregate.

create table if not exists issues (
    id              uuid primary key,
    project_id      uuid not null references projects (id),
    kind            text not null check (
        kind in ('feedback', 'bug', 'question', 'idea', 'task_request', 'incident')
    ),
    status          text not null check (
        status in ('open', 'triaged', 'in_progress', 'resolved', 'closed', 'rejected', 'duplicate')
    ),
    title           text not null,
    source_kind     text not null check (
        source_kind in ('internal', 'customer', 'support', 'monitoring', 'analytics', 'incident', 'other')
    ),
    reporter_kind   text not null check (reporter_kind in ('human', 'agent', 'system')),
    reporter_id     text not null,
    assignee_kind   text check (assignee_kind in ('human', 'agent', 'system')),
    assignee_id     text,
    severity        text check (severity in ('s1', 's2', 's3', 's4')),
    area            text,
    duplicate_of    uuid references issues (id),
    version         integer not null default 1,
    body            jsonb not null,
    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now()
);

create index if not exists issues_project_status_idx
    on issues (project_id, status, kind);
create index if not exists issues_assignee_idx
    on issues (project_id, assignee_kind, assignee_id);
create index if not exists issues_source_idx
    on issues (project_id, source_kind);
create index if not exists issues_title_search_idx
    on issues using gin (to_tsvector('simple', title || ' ' || (body ->> 'description')));

create table if not exists issue_comments (
    id           uuid primary key,
    issue_id     uuid not null references issues (id) on delete cascade,
    author_kind  text not null check (author_kind in ('human', 'agent', 'system')),
    author_id    text not null,
    body         text not null,
    internal     boolean not null default false,
    created_at   timestamptz not null default now()
);

create index if not exists issue_comments_issue_idx
    on issue_comments (issue_id, created_at);

-- An issue can motivate a requirement, a task, a bug fix, or a decision. This
-- table is generic so the same link can point at any aggregate.
create index if not exists links_issue_idx
    on links (from_id, relation)
    where to_kind = 'issue';
