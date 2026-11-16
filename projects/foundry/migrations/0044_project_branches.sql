-- Foundry 0044: each project records its git and design-space branches.

alter table projects
    add column if not exists git_branch text not null default 'main',
    add column if not exists design_space_branch text not null default 'main';
