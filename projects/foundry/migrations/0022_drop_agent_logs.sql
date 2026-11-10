-- Foundry 0022: retire structured agent logs.
--
-- Agent output is now the process's own tracing stream, captured to a plain
-- text file by the control plane. Keep 0021 for checksum compatibility with
-- databases where it was already applied; dropping the table here removes the
-- per-entry JSON log layer.

drop table if exists agent_logs;
