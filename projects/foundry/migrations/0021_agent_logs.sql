-- Foundry 0021: human-readable agent logs.
--
-- Separate from agent_steps (the model/tool trajectory): logs also cover
-- process lifecycle and dispatcher sweeps, and are what humans read in the
-- /agents/{id}/logs page.

create table if not exists agent_logs (
    id                  bigserial primary key,
    agent_definition_id uuid not null references agent_definitions (id) on delete cascade,
    run_id              uuid,
    level               text not null,
    event               text not null,
    message             text not null,
    payload             jsonb not null default '{}'::jsonb,
    created_at          timestamptz not null default now()
);

create index if not exists agent_logs_agent_created_idx
    on agent_logs (agent_definition_id, created_at desc);
