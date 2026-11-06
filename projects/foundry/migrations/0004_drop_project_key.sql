-- Foundry 0004: projects are identified by UUID, not by a user-chosen key.
--
-- `key` was introduced for a config-seeded default project that does not
-- exist anymore. Humans select projects by display name; machines use the
-- UUID. Names are mutable and deliberately non-unique.

alter table projects drop column if exists key;
