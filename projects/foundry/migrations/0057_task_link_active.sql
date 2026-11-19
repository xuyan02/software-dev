-- An Issue has at most one current handling task. Other task links remain as
-- history. Only issue links are constrained; other link kinds are unaffected.
alter table task_links
    add column if not exists active boolean not null default true;

-- Keep only the latest issue link active for existing data.
with ranked as (
    select id,
           row_number() over (
               partition by target_id
               order by created_at desc, id desc
           ) as position
    from task_links
    where kind = 'issue'
)
update task_links
set active = false
where id in (select id from ranked where position > 1);

create unique index if not exists task_links_issue_active_uidx
    on task_links (target_id)
    where kind = 'issue' and active;
