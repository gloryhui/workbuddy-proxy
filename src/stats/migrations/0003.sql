-- Durable backlog for terminal events that could not be written.
--
-- The in-memory orphan list is bounded; without a durable home the overflow was
-- dropped and the requests stayed `in_flight` until the next restart. This table
-- lets the same run retry them, in bounded batches, and is idempotent.
CREATE TABLE IF NOT EXISTS stats_recovery_backlog (
    request_id TEXT PRIMARY KEY,
    created_at_ms INTEGER NOT NULL
);
