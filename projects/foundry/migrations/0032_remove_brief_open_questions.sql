-- Project briefs no longer carry an open-questions section. Unresolved items
-- stay in the Issue; remove the legacy field from stored versions and drafts,
-- drop annotations that targeted it, and update the annotation target check.

delete from project_brief_annotations
where target_type = 'open_questions';

alter table project_brief_annotations
    drop constraint if exists project_brief_annotations_target_type_check;
alter table project_brief_annotations
    add constraint project_brief_annotations_target_type_check check (
        target_type in (
            'document', 'problem', 'goals', 'scope', 'constraints',
            'risks', 'terms', 'budget'
        )
    );

update project_briefs
set body = body - 'open_questions';

update project_brief_drafts
set body = body - 'open_questions';
