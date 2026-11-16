alter table agent_definitions
    add column if not exists project_id uuid references projects (id) on delete cascade;

create index if not exists agent_definitions_project_idx
    on agent_definitions (project_id);

-- Existing template agents were created for one task each. Backfill the
-- project when the agent account has worked in exactly one project; ambiguous
-- agents stay NULL and are recreated per project on demand.
update agent_definitions d
set project_id = bound.project_id
from (
    select ta.account_id, min(t.project_id::text)::uuid as project_id
    from task_actors ta
    join tasks t on t.id = ta.task_id
    group by ta.account_id
    having count(distinct t.project_id) = 1
) bound
where d.account_id = bound.account_id
  and d.project_id is null
  and d.role <> 'dispatcher';
