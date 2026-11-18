-- Foundry 0050: the design-space branch is a fixed platform contract.
--
-- New repositories are created with `source`; existing projects are moved to
-- the same value so code never reads a stale configurable branch.

update projects
set design_space_branch = 'source'
where design_space_branch <> 'source';
