-- Foundry 0030: move the flat problem string into the background structure.
--
-- Old document bodies stored a single `problem` string. The new shape is
-- `background: { context, problems }`. Preserve the old text as context;
-- reviewers can add structured problems on the next draft revision.

update requirements_document_versions
set body = (body - 'problem') || jsonb_build_object(
    'background',
    jsonb_build_object(
        'context', coalesce(body->>'problem', ''),
        'problems', '[]'::jsonb
    )
)
where body ? 'problem' and not body ? 'background';

update requirements_document_drafts
set body = (body - 'problem') || jsonb_build_object(
    'background',
    jsonb_build_object(
        'context', coalesce(body->>'problem', ''),
        'problems', '[]'::jsonb
    )
)
where body ? 'problem' and not body ? 'background';
