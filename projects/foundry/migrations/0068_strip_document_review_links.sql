-- Foundry 0068: anchor document review links were removed from the stage
-- effect language. Old workflow versions may still contain
-- `document_review_link` effects, which no longer deserialize. Strip them from
-- stored workflow bodies so existing databases can start.

with cleaned as (
    select
        workflow_id,
        version,
        jsonb_set(
            body,
            '{stages}',
            coalesce(
                (
                    select jsonb_agg(
                        case
                            when stage ? 'created' then jsonb_set(
                                stage,
                                '{created}',
                                coalesce(
                                    (
                                        select jsonb_agg(effect)
                                        from jsonb_array_elements(stage->'created') effect
                                        where not (effect ? 'document_review_link')
                                    ),
                                    '[]'::jsonb
                                )
                            )
                            else stage
                        end
                    )
                    from jsonb_array_elements(body->'stages') stage
                ),
                '[]'::jsonb
            )
        ) as body
    from workflow_versions
    where body::text like '%document_review_link%'
)
update workflow_versions version
set body = cleaned.body
from cleaned
where version.workflow_id = cleaned.workflow_id
  and version.version = cleaned.version;

with cleaned as (
    select
        workflow_id,
        jsonb_set(
            body,
            '{stages}',
            coalesce(
                (
                    select jsonb_agg(
                        case
                            when stage ? 'created' then jsonb_set(
                                stage,
                                '{created}',
                                coalesce(
                                    (
                                        select jsonb_agg(effect)
                                        from jsonb_array_elements(stage->'created') effect
                                        where not (effect ? 'document_review_link')
                                    ),
                                    '[]'::jsonb
                                )
                            )
                            else stage
                        end
                    )
                    from jsonb_array_elements(body->'stages') stage
                ),
                '[]'::jsonb
            )
        ) as body
    from workflow_drafts
    where body::text like '%document_review_link%'
)
update workflow_drafts draft
set body = cleaned.body
from cleaned
where draft.workflow_id = cleaned.workflow_id;
