-- Foundry 0020: notify agents when task state changes.
--
-- The PM dispatcher listens on `foundry_task_changes`, re-queries the task pool
-- on startup and after every task change. There is no periodic sweep.

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
    'dispatch_requested'
))
execute function foundry_notify_task_change();
