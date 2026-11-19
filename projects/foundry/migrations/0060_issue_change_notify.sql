-- Foundry 0060: notify the triage check agent when Issues change.
--
-- Issue comments and status changes are stored on the issue row, so one
-- insert/update trigger covers new issues, comments and reopen events.

create or replace function foundry_notify_issue_change() returns trigger as $$
begin
    perform pg_notify(
        'foundry_issue_changes',
        json_build_object('issue_id', new.id, 'operation', tg_op)::text
    );
    return new;
end;
$$ language plpgsql;

drop trigger if exists foundry_issue_change on issues;

create trigger foundry_issue_change
after insert or update on issues
for each row
execute function foundry_notify_issue_change();
