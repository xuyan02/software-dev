alter table projects
    add column if not exists test_capsule text not null default 'cli';
