-- Project briefs no longer carry a budget section. Cost and budget figures are
-- tracked outside the brief. Remove the legacy field from stored versions and
-- drafts, drop annotations that targeted it, and update the annotation target
-- check.

delete from project_brief_annotations
where target_type = 'budget';

alter table project_brief_annotations
    drop constraint if exists project_brief_annotations_target_type_check;
alter table project_brief_annotations
    add constraint project_brief_annotations_target_type_check check (
        target_type in (
            'document', 'problem', 'goals', 'non_goals', 'constraints',
            'risks', 'terms'
        )
    );

update project_briefs
set body = body - 'budget';

update project_brief_drafts
set body = body - 'budget';
