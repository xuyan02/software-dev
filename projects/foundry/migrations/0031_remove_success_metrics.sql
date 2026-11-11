-- Foundry 0031: remove success metrics from the requirements document.
--
-- Success metrics are no longer a separate section. Drop the old JSONB key
-- from existing document versions and drafts.

update requirements_document_versions
set body = body - 'success_metrics'
where body ? 'success_metrics';

update requirements_document_drafts
set body = body - 'success_metrics'
where body ? 'success_metrics';
