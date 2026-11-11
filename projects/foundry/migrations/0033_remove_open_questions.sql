-- Foundry 0033: remove open questions from the requirements document.
--
-- Open questions are no longer a separate section. Drop the old JSONB key
-- from existing document versions and drafts.

update requirements_document_versions
set body = body - 'open_questions'
where body ? 'open_questions';

update requirements_document_drafts
set body = body - 'open_questions'
where body ? 'open_questions';
