-- Foundry 0049: Task creation input.
--
-- The input is immutable task payload supplied at creation time. Workflow
-- variables remain internal; input is available to the assigned agent and to
-- operators as the task's declared work item.

alter table tasks
    add column if not exists input jsonb not null default '{}'::jsonb;
