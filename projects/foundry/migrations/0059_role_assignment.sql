-- Foundry 0059: roles carry one assignment and exactly one actor.
--
-- Agent roles name the workflow whose agent implementation they need; human
-- roles name the human identity they require. The old actor ranges and the
-- `any` kind are gone.

alter table project_members
    add column if not exists revoked_identities text[] not null default '{}';

-- Keep one actor per task role. Prefer an active actor, then the lowest
-- account id, so the result is deterministic.
delete from task_actors a
using task_actors b
where a.task_id = b.task_id
  and a.role_key = b.role_key
  and a.account_id <> b.account_id
  and (a.active < b.active or (a.active = b.active and a.account_id > b.account_id));

create unique index if not exists task_actors_one_per_role
    on task_actors (task_id, role_key);

-- Rewrite published workflow bodies: drop actor ranges, resolve `any`, and
-- add the assignment field.
with expanded as (
    select wv.workflow_id,
           wv.version,
           role.value as role,
           role.ordinality,
           case
               when role.value ->> 'kind' = 'any'
                    and role.value ->> 'name' = 'acceptor' then 'human'
               when role.value ->> 'kind' = 'any' then 'agent'
               else role.value ->> 'kind'
           end as kind
    from workflow_versions wv
    cross join lateral jsonb_array_elements(wv.body -> 'roles')
        with ordinality as role(value, ordinality)
),
assigned as (
    select workflow_id,
           version,
           role,
           ordinality,
           kind,
           case
               when kind = 'human' then
                   case
                       when role ->> 'name' = 'acceptor'
                            and workflow_id in ('design_baseline', 'design_change')
                           then 'design_space_reviewer'
                       when role ->> 'name' = 'acceptor' and workflow_id = 'code_change'
                           then 'code_reviewer'
                       when role ->> 'name' = 'acceptor' then 'acceptor'
                       when role ->> 'name' = 'reviewer' and workflow_id = 'design_space_merge'
                           then 'design_space_reviewer'
                       else role ->> 'name'
                   end
               else workflow_id
           end as assignment
    from expanded
),
roles as (
    select workflow_id,
           version,
           jsonb_agg(
               (role - 'min_actors' - 'max_actors')
               || jsonb_build_object('kind', kind, 'assignment', assignment)
               order by ordinality
           ) as roles
    from assigned
    group by workflow_id, version
)
update workflow_versions wv
set body = jsonb_set(wv.body, '{roles}', roles.roles)
from roles
where wv.workflow_id = roles.workflow_id
  and wv.version = roles.version;

-- Drafts are loaded with the same schema, so normalize them too.
with expanded as (
    select wd.workflow_id,
           role.value as role,
           role.ordinality,
           case
               when role.value ->> 'kind' = 'any'
                    and role.value ->> 'name' = 'acceptor' then 'human'
               when role.value ->> 'kind' = 'any' then 'agent'
               else role.value ->> 'kind'
           end as kind
    from workflow_drafts wd
    cross join lateral jsonb_array_elements(wd.body -> 'roles')
        with ordinality as role(value, ordinality)
),
assigned as (
    select workflow_id,
           role,
           ordinality,
           kind,
           case
               when kind = 'human' then
                   case
                       when role ->> 'name' = 'acceptor'
                            and workflow_id in ('design_baseline', 'design_change')
                           then 'design_space_reviewer'
                       when role ->> 'name' = 'acceptor' and workflow_id = 'code_change'
                           then 'code_reviewer'
                       when role ->> 'name' = 'acceptor' then 'acceptor'
                       when role ->> 'name' = 'reviewer' and workflow_id = 'design_space_merge'
                           then 'design_space_reviewer'
                       else role ->> 'name'
                   end
               else workflow_id
           end as assignment
    from expanded
),
roles as (
    select workflow_id,
           jsonb_agg(
               (role - 'min_actors' - 'max_actors')
               || jsonb_build_object('kind', kind, 'assignment', assignment)
               order by ordinality
           ) as roles
    from assigned
    group by workflow_id
)
update workflow_drafts wd
set body = jsonb_set(wd.body, '{roles}', roles.roles)
from roles
where wd.workflow_id = roles.workflow_id;
