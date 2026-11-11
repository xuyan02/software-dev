-- Foundry 0038: agent skill list.
--
-- Skills are code-owned prompt files. Agent definitions store only skill IDs;
-- the agent runtime appends every configured skill to the system prompt.

alter table agent_definitions
    add column if not exists skills jsonb not null default '[]'::jsonb;
