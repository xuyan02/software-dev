-- Rename the clarification task to "撰写项目简报" and its acceptance stage
-- from `awaiting_acceptance` to `acceptance`.
--
-- New tasks get the new title from the project workflow definition; this
-- updates tasks created before the rename.

update tasks
set title = '撰写项目简报'
where workflow = 'goal_clarification'
  and title = 'Clarify project goals';

update tasks
set stage = 'acceptance'
where workflow = 'goal_clarification'
  and stage = 'awaiting_acceptance';
