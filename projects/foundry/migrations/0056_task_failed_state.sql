-- Failure is a task state, not a workflow stage. Allow tasks to park in
-- `failed` while keeping their current stage for retry.
alter table tasks
    drop constraint if exists tasks_state_check;

alter table tasks
    add constraint tasks_state_check
    check (state in ('pending', 'in_progress', 'failed', 'completed'));
