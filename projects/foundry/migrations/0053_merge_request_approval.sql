-- A merge request approval is bound to the reviewed design content, not to a
-- branch ref or comment state. The digest excludes review comments so the
-- mandatory pre-merge comment cleanup does not invalidate the approval.
alter table design_space_merge_requests
    add column if not exists approved_content_digest text,
    add column if not exists approved_source_commit text,
    add column if not exists approved_by uuid references accounts(id),
    add column if not exists approved_at timestamptz;
