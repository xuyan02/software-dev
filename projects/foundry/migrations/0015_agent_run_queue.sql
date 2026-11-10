-- Foundry 0015: agent run queue, leases and checkpoints.

alter table agent_runs
    add column if not exists input jsonb,
    add column if not exists checkpoint jsonb,
    add column if not exists resume_comments text,
    add column if not exists waiting_issue_id uuid,
    add column if not exists last_seen_comment_id uuid,
    add column if not exists lease_owner text,
    add column if not exists lease_expires_at timestamptz,
    add column if not exists attempts integer not null default 0,
    add column if not exists max_attempts integer not null default 3,
    add column if not exists last_error text,
    add column if not exists updated_at timestamptz not null default now();

create index if not exists agent_runs_queue_idx
    on agent_runs (status, started_at);

create index if not exists agent_runs_waiting_issue_idx
    on agent_runs (waiting_issue_id)
    where status = 'waiting_for_issue_comment';
