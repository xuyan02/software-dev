-- Foundry 0035: move the project layer out of the requirements document.
--
-- The project document now owns positioning, background, goals, non-goals,
-- constraints, risks and glossary. Remove those keys from existing
-- requirements document versions and drafts.

update requirements_document_versions
set body = body - 'background' - 'goals' - 'non_goals' - 'constraints' - 'risks' - 'glossary'
where body ?| array['background', 'goals', 'non_goals', 'constraints', 'risks', 'glossary'];

update requirements_document_drafts
set body = body - 'background' - 'goals' - 'non_goals' - 'constraints' - 'risks' - 'glossary'
where body ?| array['background', 'goals', 'non_goals', 'constraints', 'risks', 'glossary'];
