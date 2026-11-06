-- Workflow variables and the operation terminology.

alter table tasks
    add column if not exists variables jsonb not null default '{}'::jsonb;

alter table task_inputs rename to task_operations;
alter table task_operations rename column signal to operation;
