-- Foundry 0043: each project records the ds:// address of its design space.

alter table projects
    add column if not exists design_space_url text not null default '';

-- Existing projects predate provisioning; the address is filled for new
-- projects when they are created.
