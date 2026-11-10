-- Foundry 0023: durable activation data for agent runs.
--
-- Notifications themselves are ephemeral; the durable record is the trigger
-- bundle on the run plus the watermarks used to rebuild missed notifications.

alter table agent_runs
    add column if not exists trigger jsonb not null default '{}'::jsonb;

alter table agent_runs
    add column if not exists pending_changes jsonb not null default '[]'::jsonb;

create index if not exists agent_runs_open_task_idx
    on agent_runs (task_id, agent_definition_id)
    where status in ('queued', 'running', 'waiting_for_issue_comment');
