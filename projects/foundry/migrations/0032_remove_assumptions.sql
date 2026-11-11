-- Foundry 0032: remove assumptions from the requirements document.
--
-- Assumptions are no longer a separate section. Drop the old JSONB key from
-- existing document versions and drafts.

update requirements_document_versions
set body = body - 'assumptions'
where body ? 'assumptions';

update requirements_document_drafts
set body = body - 'assumptions'
where body ? 'assumptions';
