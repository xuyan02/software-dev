-- Foundry 0013: task link list.

create table if not exists task_links (
    id          uuid primary key,
    task_id     uuid not null references tasks (id) on delete cascade,
    kind        text not null,
    target_id   uuid,
    url         text,
    title       text not null,
    relation    text not null,
    created_by  jsonb not null,
    created_at  timestamptz not null default now(),
    metadata    jsonb not null default '{}'::jsonb,
    check (
        (kind = 'url' and url is not null and target_id is null)
        or (kind <> 'url' and target_id is not null and url is null)
    )
);

create index if not exists task_links_task_idx
    on task_links (task_id, created_at, id);

create unique index if not exists task_links_target_uidx
    on task_links (task_id, kind, relation, target_id)
    where target_id is not null;

create unique index if not exists task_links_url_uidx
    on task_links (task_id, kind, relation, url)
    where url is not null;
