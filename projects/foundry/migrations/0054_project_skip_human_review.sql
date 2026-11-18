-- Project-level test switch: auto-approve human review stages.
alter table projects
    add column if not exists skip_human_review boolean not null default false;
