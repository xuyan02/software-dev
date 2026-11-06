-- Foundry 0006: Task aggregate, workflow inputs, comments and the durable
-- event-bus consumer cursor.
--
-- Tasks have no version column: the `task-workflow` consumer serializes all
-- mutations per task through its cursor.

create table if not exists tasks (
    id            uuid primary key,
    project_id    uuid not null references projects (id) on delete cascade,
    parent_id     uuid references tasks (id),
    workflow      text not null,
    title         text not null,
    state         text not null check (state in ('pending', 'in_progress', 'completed')),
    stage         text,
    result        text,
    created_at    timestamptz not null default now(),
    updated_at    timestamptz not null default now(),
    completed_at  timestamptz,
    check (state <> 'completed' or completed_at is not null)
);

create unique index if not exists tasks_one_root_idx
    on tasks (project_id)
    where parent_id is null;

create index if not exists tasks_project_updated_idx
    on tasks (project_id, updated_at desc);
create index if not exists tasks_pending_idx
    on tasks (state)
    where state = 'pending';
create index if not exists tasks_parent_idx
    on tasks (parent_id);

create table if not exists task_dependencies (
    task_id            uuid not null references tasks (id) on delete cascade,
    depends_on_task_id uuid not null references tasks (id),
    primary key (task_id, depends_on_task_id),
    check (task_id <> depends_on_task_id)
);

create table if not exists task_actors (
    task_id     uuid not null references tasks (id) on delete cascade,
    role_key    text not null,
    account_id  uuid not null references accounts (id) on delete cascade,
    active      boolean not null default false,
    work        text,
    primary key (task_id, role_key, account_id)
);

create index if not exists task_actors_active_idx
    on task_actors (task_id, active);
create index if not exists task_actors_account_idx
    on task_actors (account_id, task_id);

create table if not exists task_comments (
    id                 uuid primary key,
    task_id            uuid not null references tasks (id) on delete cascade,
    author_account_id  uuid not null references accounts (id),
    body               text not null,
    created_at         timestamptz not null default now()
);

create index if not exists task_comments_task_idx
    on task_comments (task_id, created_at, id);

create table if not exists task_inputs (
    id               uuid primary key,
    task_id          uuid not null references tasks (id) on delete cascade,
    signal           text not null,
    actor_id         uuid not null references accounts (id),
    idempotency_key  text not null,
    created_at       timestamptz not null default now(),
    unique (task_id, idempotency_key)
);

create table if not exists event_consumer_cursors (
    consumer_id    text not null,
    partition_key  text not null,
    last_seq       bigint not null default 0,
    updated_at     timestamptz not null default now(),
    primary key (consumer_id, partition_key)
);

create index if not exists events_kind_seq_idx
    on events (kind, seq);
