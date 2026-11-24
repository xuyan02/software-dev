-- Foundry 0067: requirements and technical design live in design-space Arche
-- objects. The database-backed anchor document implementation is removed.

drop table if exists anchor_document_review_links cascade;
drop table if exists anchor_document_annotations cascade;
drop table if exists anchor_document_approvals cascade;
drop table if exists anchor_document_operations cascade;
drop table if exists anchor_document_drafts cascade;
drop table if exists anchor_document_versions cascade;
drop table if exists anchor_documents cascade;

drop table if exists requirements_draft_annotations cascade;
drop table if exists requirements_operations cascade;
drop table if exists requirements_document_approvals cascade;
drop table if exists requirements_document_drafts cascade;
drop table if exists requirements_document_versions cascade;
drop table if exists requirements_documents cascade;

drop table if exists project_document_approvals cascade;
drop table if exists project_document_operations cascade;
drop table if exists project_document_drafts cascade;
drop table if exists project_document_versions cascade;
drop table if exists project_documents cascade;
