-- Foundry 0011: a project must name the git repository it works on.

alter table projects
    add column if not exists git_repository text not null default '';

-- Existing projects predate the requirement; they keep an empty value until
-- updated. New projects validate it in the domain and API.
