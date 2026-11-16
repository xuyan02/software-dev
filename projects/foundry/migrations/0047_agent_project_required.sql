-- Every agent belongs to a project. Legacy dispatchers were platform-wide;
-- assign the remaining ones to the oldest project, where the control plane
-- adopts them as that project's PM.
update agent_definitions
set project_id = (
    select id from projects order by created_at, id limit 1
)
where project_id is null;

-- No project exists: a legacy auto-provisioned agent has no valid owner and is
-- recreated per project on demand.
delete from agent_definitions where project_id is null;

alter table agent_definitions
    alter column project_id set not null;

-- A project has exactly one PM dispatcher.
create unique index if not exists agent_definitions_one_dispatcher
    on agent_definitions (project_id) where role = 'dispatcher';
