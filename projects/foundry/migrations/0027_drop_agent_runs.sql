-- Foundry 0027: notifications are transient hints.
--
-- Managed agents no longer claim durable AgentRuns. Task and Issue changes
-- notify listeners; each agent decides from persisted task state what to do.
-- Run history and step trajectories are removed with the queue.

drop table if exists agent_steps;
drop table if exists agent_runs;

alter table agent_processes drop column if exists current_run_id;

create or replace function foundry_notify_task_change() returns trigger as $$
begin
    perform pg_notify(
        'foundry_task_changes',
        json_build_object('aggregate_id', new.aggregate_id, 'kind', new.kind)::text
    );
    return new;
end;
$$ language plpgsql;

drop trigger if exists foundry_task_change on events;

create trigger foundry_task_change
after insert on events
for each row
when (new.kind in (
    'task_created',
    'task_state_changed',
    'task_stage_changed',
    'child_created',
    'child_state_changed',
    'actor_bound',
    'operation_received',
    'dispatch_requested',
    'task_reevaluated',
    'issue_commented'
))
execute function foundry_notify_task_change();
