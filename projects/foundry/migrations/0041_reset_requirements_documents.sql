-- Foundry 0041: reset old requirements documents after the interaction/UI schema change.
--
-- Interaction and UI design changed from flat text chapters to subsystem trees.
-- Requirements documents at schema version 1 have no anchors for the new
-- chapters, so they are discarded in this development stage; drafts, versions,
-- annotations and approvals go with them. Documents already at version 2 are
-- left untouched.

delete from anchor_document_review_links l
where l.kind = 'requirements'
  and exists (
      select 1 from anchor_documents d
      where d.project_id = l.project_id
        and d.kind = 'requirements'
        and d.schema_version < 2
  );

delete from anchor_document_annotations a
where a.kind = 'requirements'
  and exists (
      select 1 from anchor_documents d
      where d.project_id = a.project_id
        and d.kind = 'requirements'
        and d.schema_version < 2
  );

delete from anchor_documents
where kind = 'requirements' and schema_version < 2;
