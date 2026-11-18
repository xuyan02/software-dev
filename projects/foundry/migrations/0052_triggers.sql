create table if not exists triggers (
    id uuid primary key,
    project_id uuid not null references projects(id) on delete cascade,
    name text not null,
    description text not null default '',
    workflow_id text not null,
    input jsonb not null default '{}'::jsonb,
    schedule text not null default 'manual',
    enabled boolean not null default true,
    title_template text not null,
    description_template text not null default '',
    created_by jsonb not null,
    created_by_id uuid not null,
    next_run_at timestamptz,
    last_run_at timestamptz,
    version integer not null default 1,
    created_at timestamptz not null,
    updated_at timestamptz not null
);

create index if not exists triggers_due_idx
    on triggers (next_run_at)
    where enabled and schedule <> 'manual';

create table if not exists trigger_runs (
    id uuid primary key,
    trigger_id uuid not null references triggers(id) on delete cascade,
    task_id uuid not null references tasks(id) on delete cascade,
    reason text not null,
    triggered_by jsonb not null,
    created_at timestamptz not null
);

create index if not exists trigger_runs_trigger_idx
    on trigger_runs (trigger_id, created_at desc);
