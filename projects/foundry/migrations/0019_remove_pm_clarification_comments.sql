-- Foundry 0019: remove the PM dispatcher's premature clarification comments.
--
-- `issue_comments.author_id` stores the actor handle, not the account UUID, so
-- the cleanup in 0018 compared the wrong values for agent comments.

delete from issue_comments
where author_id = 'pm-agent';
