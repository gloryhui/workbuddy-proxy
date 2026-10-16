-- Request upstream usage by default. Existing rows are explicit user choices;
-- preserve their mode, timestamp and optimistic-concurrency revision.
CREATE TABLE stats_provider_settings_v5 (
    provider_id TEXT PRIMARY KEY CHECK (length(provider_id) BETWEEN 1 AND 160),
    usage_mode TEXT NOT NULL DEFAULT 'request'
        CHECK (usage_mode IN ('observe','request')),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);
INSERT INTO stats_provider_settings_v5(provider_id, usage_mode, updated_at_ms, revision)
    SELECT provider_id, usage_mode, updated_at_ms, revision FROM stats_provider_settings;
DROP TABLE stats_provider_settings;
ALTER TABLE stats_provider_settings_v5 RENAME TO stats_provider_settings;
