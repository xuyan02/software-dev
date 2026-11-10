-- Project briefs no longer carry a scope section. The non-goal list becomes a
-- top-level `non_goals` field; the old `scope` object (including `in_scope`)
-- is removed. Annotations that targeted `scope` move to `non_goals`.

alter table project_brief_annotations
    drop constraint if exists project_brief_annotations_target_type_check;

update project_brief_annotations
set target_type = 'non_goals', field = null
where target_type = 'scope';

alter table project_brief_annotations
    add constraint project_brief_annotations_target_type_check check (
        target_type in (
            'document', 'problem', 'goals', 'non_goals', 'constraints',
            'risks', 'terms', 'budget'
        )
    );

update project_briefs
set body = (body || jsonb_build_object(
        'non_goals',
        coalesce(body -> 'scope' -> 'out_of_scope', '[]'::jsonb)
    )) - 'scope';

update project_brief_drafts
set body = (body || jsonb_build_object(
        'non_goals',
        coalesce(body -> 'scope' -> 'out_of_scope', '[]'::jsonb)
    )) - 'scope';
