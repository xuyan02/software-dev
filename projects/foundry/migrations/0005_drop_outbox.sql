-- The outbox had no relay consumer and nothing read it. Events are the durable
-- record; delivery to external systems is not part of the current design.
drop table if exists outbox;
