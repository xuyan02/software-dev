-- software_development no longer has a setup stage: project setup is
-- deterministic in the control plane. Move root tasks that were started in
-- the removed stage onto the surviving running stage.
update tasks
set stage = 'running',
    updated_at = now()
where workflow = 'software_development'
  and stage = 'setup';

-- The setup agent had no work left once the workflow moved to deterministic
-- setup. Keep the definitions for history, but never start them again.
update agent_definitions
set enabled = false,
    updated_at = now()
where role = 'software_development:project_setup'
  and enabled;
