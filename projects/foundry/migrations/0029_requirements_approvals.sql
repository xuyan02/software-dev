-- Foundry 0029: requirements approval and project brief merge.
--
-- Every draft revision must be approved before it becomes a version.
-- Approval and publication happen in one transaction; rejections are recorded
-- without creating a version. The project brief is merged into the
-- requirements document, so its tables are dropped.

alter table requirements_document_versions
    add column if not exists source_revision bigint,
    add column if not exists approval_comment text;

create unique index if not exists requirements_versions_source_revision_idx
    on requirements_document_versions (document_id, source_revision)
    where source_revision is not null;

create table if not exists requirements_document_approvals (
    id            uuid primary key,
    project_id    uuid not null references projects (id) on delete cascade,
    revision      bigint not null check (revision >= 1),
    decision      text not null check (decision in ('approved', 'rejected')),
    actor_id      uuid not null references accounts (id),
    comment       text,
    content_hash  text not null,
    created_at    timestamptz not null default now()
);

create index if not exists requirements_approvals_project_idx
    on requirements_document_approvals (project_id, created_at);

create unique index if not exists requirements_approvals_revision_decision_idx
    on requirements_document_approvals (project_id, revision, decision);

-- Project brief is now part of the requirements document.
drop table if exists project_brief_draft_operations;
drop table if exists project_brief_drafts;
drop table if exists project_brief_current;
drop table if exists project_brief_annotations;
drop table if exists project_briefs;
