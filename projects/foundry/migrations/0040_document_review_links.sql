-- Link a document to the task that is currently in human review.
--
-- A document has at most one active link. Leaving human review releases the
-- link; the row is kept for history.
create table if not exists anchor_document_review_links (
    id uuid primary key,
    project_id uuid not null references projects(id) on delete cascade,
    kind text not null,
    draft_revision bigint not null,
    task_id uuid not null references tasks(id) on delete cascade,
    bound_at timestamptz not null,
    released_at timestamptz
);

create unique index if not exists anchor_document_review_links_active_document
    on anchor_document_review_links (project_id, kind)
    where released_at is null;

create index if not exists anchor_document_review_links_active_task
    on anchor_document_review_links (task_id)
    where released_at is null;
