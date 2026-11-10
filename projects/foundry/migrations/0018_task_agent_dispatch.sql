-- Foundry 0018: task-level agent dispatch.
--
-- Agent definitions declare the workflow role they can fill. A task role is
-- assigned per task, not per project. The PM agent is a dispatcher; it is no
-- longer bound to the clarification role.

alter table agent_definitions
    add column if not exists role text not null default '';

create table if not exists task_agent_assignments (
    task_id             uuid not null references tasks (id) on delete cascade,
    role                text not null,
    agent_definition_id uuid not null references agent_definitions (id) on delete cascade,
    created_at          timestamptz not null default now(),
    primary key (task_id, role)
);

create index if not exists task_agent_assignments_agent_idx
    on task_agent_assignments (agent_definition_id);

-- Clean up the old PM-as-clarifier behavior. The PM's clarification runs are
-- superseded by the dispatcher model; its premature comments must not be part
-- of the clarification discussion.
update agent_runs
set status = 'failed', last_error = 'superseded by dispatcher model', updated_at = now()
where status in ('queued', 'running', 'waiting_for_issue_comment')
  and agent_definition_id in (
      select id from agent_definitions where name = 'PM Agent'
  );

delete from issue_comments
where author_id = (select id::text from accounts where handle = 'pm-agent');
