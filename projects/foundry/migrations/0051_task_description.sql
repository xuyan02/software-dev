-- Foundry 0051: narrative task assignment.
--
-- A title names the task; description states what the assigned actor is
-- expected to do. It is set at creation and shown on the task page.

alter table tasks
    add column if not exists description text not null default '';
