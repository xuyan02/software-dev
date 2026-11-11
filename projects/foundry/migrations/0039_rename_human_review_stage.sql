-- Rename the platform human review stage from awaiting_acceptance to
-- human_review. Workflow definitions were renamed in code; existing task rows
-- keep the old stage string and must be migrated with them.
update tasks
set stage = 'human_review'
where stage = 'awaiting_acceptance';
