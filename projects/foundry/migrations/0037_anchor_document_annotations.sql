-- Foundry 0037: annotations anchored to document anchors.
--
-- An annotation stores both the stable anchor key and the content hash it was
-- written against. When the anchor changes, the annotation becomes stale; when
-- the anchor disappears, it becomes orphaned. Stale/orphaned are computed at
-- read time and are not stored.

create table if not exists anchor_document_annotations (
    id            uuid primary key,
    project_id    uuid not null references projects (id) on delete cascade,
    kind          text not null,
    anchor_key    text not null,
    anchor_id     text not null,
    quote         text,
    body          text not null,
    author_id     uuid not null references accounts (id),
    author_handle text not null,
    author_name   text not null,
    status        text not null default 'open' check (status in ('open', 'resolved')),
    resolution    text,
    created_at    timestamptz not null default now(),
    resolved_at   timestamptz,
    resolved_by   uuid references accounts (id)
);

create index if not exists anchor_document_annotations_anchor_idx
    on anchor_document_annotations (project_id, kind, anchor_key, created_at);

create index if not exists anchor_document_annotations_status_idx
    on anchor_document_annotations (project_id, kind, status, created_at);
