-- Project briefs are written before requirements analysis; they do not carry
-- success metrics or any quantitative standard. Remove the legacy field from
-- stored versions and drafts, drop annotations that targeted it, and update
-- the annotation target check to the current section list (which also adds
-- `terms`).

delete from project_brief_annotations
where target_type = 'success_metrics';

alter table project_brief_annotations
    drop constraint if exists project_brief_annotations_target_type_check;
alter table project_brief_annotations
    add constraint project_brief_annotations_target_type_check check (
        target_type in (
            'document', 'problem', 'goals', 'scope', 'constraints',
            'risks', 'stakeholders', 'terms', 'budget', 'assumptions',
            'open_questions'
        )
    );

update project_briefs
set body = body - 'success_metrics';

update project_brief_drafts
set body = body - 'success_metrics';
