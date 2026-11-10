-- Foundry 0011: agent runs and step trajectories.

create table if not exists agent_runs (
    id            uuid primary key,
    project_id    uuid not null references projects (id),
    task_id       uuid,
    agent_id      text not null,
    status        text not null default 'running',
    usage         jsonb not null default '{}'::jsonb,
    stop_reason   text,
    started_at    timestamptz not null default now(),
    finished_at   timestamptz
);

create index if not exists agent_runs_project_idx
    on agent_runs (project_id, started_at desc);

create table if not exists agent_steps (
    id            bigserial primary key,
    run_id        uuid not null references agent_runs (id) on delete cascade,
    step_index    integer not null,
    kind          text not null,
    name          text,
    input         jsonb,
    output        text,
    usage         jsonb not null default '{}'::jsonb,
    elapsed_ms    bigint not null default 0,
    error         text,
    occurred_at   timestamptz not null default now()
);

create index if not exists agent_steps_run_idx
    on agent_steps (run_id, step_index);
