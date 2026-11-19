-- Foundry 0061: rename the existing project dispatcher display name.
--
-- Migration 0059 was already applied before the display rename was added, so
-- the rename lives in its own migration instead of modifying 0059.

update agent_definitions
set name = replace(name, 'PM Agent', 'Task Dispatcher')
where role = 'dispatcher' and name like 'PM Agent %';
