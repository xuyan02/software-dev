-- The design-baseline and design-change workflows no longer have an agent
-- merge stage: human approval performs the merge mechanically. Move tasks that
-- were parked in the removed stage back to human review so the acceptor can
-- approve and trigger the merge.
update tasks
set stage = 'human_review',
    updated_at = now()
where workflow in ('design_baseline', 'design_change')
  and stage = 'merge';

update task_actors
set active = true
where role_key = 'acceptor'
  and task_id in (
      select id from tasks
      where workflow in ('design_baseline', 'design_change')
        and stage = 'human_review'
  );

update task_actors
set active = false
where role_key <> 'acceptor'
  and task_id in (
      select id from tasks
      where workflow in ('design_baseline', 'design_change')
        and stage = 'human_review'
  );
