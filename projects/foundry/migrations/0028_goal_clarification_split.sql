-- Goal clarification is split into two stages: `clarify` (Issue interview)
-- and `brief` (project brief writing). Tasks pinned to v1 may still store the
-- old single `work` stage; move them to the new clarification stage.
--
-- The old and new `clarify` stage both declare `completed: bool`, so the
-- persisted stage variables remain valid.

update tasks
set stage = 'clarify'
where workflow = 'goal_clarification'
  and stage = 'work';
