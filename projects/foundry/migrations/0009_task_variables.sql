-- Task-level variables persist across stages; stage variables are per stage.

alter table tasks rename column variables to stage_variables;

alter table tasks
    add column if not exists task_variables jsonb not null default '{}'::jsonb;
