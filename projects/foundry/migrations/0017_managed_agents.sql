-- Foundry 0017: managed agents.
--
-- One definition per agent. The control plane supervises one agentd process
-- per definition; bindings attach a definition to a project workflow role.
-- The model API key is never stored: definitions name the environment variable
-- the process reads.

create table if not exists agent_definitions (
    id                 uuid primary key,
    name               text not null unique,
    account_id         uuid not null references accounts (id),
    model_base_url     text not null,
    model_name         text not null,
    api_key_env        text not null default 'OPENAI_API_KEY',
    system_prompt      text not null,
    tools              jsonb not null default '[]'::jsonb,
    enabled            boolean not null default true,
    created_at         timestamptz not null default now(),
    updated_at         timestamptz not null default now()
);

create table if not exists agent_bindings (
    id                  uuid primary key,
    project_id          uuid not null references projects (id) on delete cascade,
    workflow_id         text not null,
    role                text not null,
    agent_definition_id uuid not null references agent_definitions (id) on delete cascade,
    created_at          timestamptz not null default now(),
    unique (project_id, workflow_id, role)
);

create table if not exists agent_processes (
    agent_definition_id uuid primary key references agent_definitions (id) on delete cascade,
    status              text not null default 'stopped',
    pid                 integer,
    heartbeat_at        timestamptz,
    current_run_id      uuid,
    last_error          text,
    started_at          timestamptz,
    stopped_at          timestamptz
);

alter table agent_runs
    add column if not exists agent_definition_id uuid references agent_definitions (id);

create index if not exists agent_runs_definition_idx
    on agent_runs (agent_definition_id, status);

-- Runs created before managed agents have no definition and can never be
-- claimed by an agentd process. This is a pre-release cleanup.
delete from agent_runs where agent_definition_id is null;
