-- Project briefs no longer carry stakeholders or assumptions. Remove the
-- legacy fields from stored versions and drafts, drop annotations that
-- targeted them, and update the annotation target check to the current
-- section list.

delete from project_brief_annotations
where target_type in ('stakeholders', 'assumptions');

alter table project_brief_annotations
    drop constraint if exists project_brief_annotations_target_type_check;
alter table project_brief_annotations
    add constraint project_brief_annotations_target_type_check check (
        target_type in (
            'document', 'problem', 'goals', 'scope', 'constraints',
            'risks', 'terms', 'budget', 'open_questions'
        )
    );

update project_briefs
set body = body - 'stakeholders' - 'assumptions';

update project_brief_drafts
set body = body - 'stakeholders' - 'assumptions';
