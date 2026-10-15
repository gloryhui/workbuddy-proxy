-- WorkBuddy Proxy statistics schema v1; DESIGN, not installed by current core.
-- Execute on a fresh temporary DB to review. Runtime migration owns the transaction.
-- Connection setup (journal_mode, synchronous, busy_timeout) is documented separately.

CREATE TABLE stats_settings (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1)),
    retention_days INTEGER NOT NULL DEFAULT 180
        CHECK (retention_days = 0 OR retention_days BETWEEN 7 AND 3650),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);
INSERT INTO stats_settings(singleton, updated_at_ms) VALUES (1,0);

CREATE TABLE stats_provider_settings (
    provider_id TEXT PRIMARY KEY CHECK (length(provider_id) BETWEEN 1 AND 160),
    usage_mode TEXT NOT NULL DEFAULT 'observe'
        CHECK (usage_mode IN ('observe','request')),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
);

CREATE TABLE stats_runs (
    run_id TEXT PRIMARY KEY CHECK (length(run_id) BETWEEN 1 AND 64),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    stopped_at_ms INTEGER CHECK (stopped_at_ms >= started_at_ms),
    stop_reason TEXT CHECK (stop_reason IN ('normal','unclean')),
    admitted_requests INTEGER NOT NULL DEFAULT 0 CHECK (admitted_requests >= 0),
    dropped_requests INTEGER NOT NULL DEFAULT 0 CHECK (dropped_requests >= 0),
    write_failures INTEGER NOT NULL DEFAULT 0 CHECK (write_failures >= 0),
    last_persisted_at_ms INTEGER CHECK (last_persisted_at_ms >= started_at_ms),
    CHECK ((stopped_at_ms IS NULL AND stop_reason IS NULL)
        OR (stopped_at_ms IS NOT NULL AND stop_reason IS NOT NULL))
);

CREATE TABLE usage_requests (
    id INTEGER PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE CHECK (length(request_id) BETWEEN 1 AND 64),
    run_id TEXT NOT NULL REFERENCES stats_runs(run_id),
    source TEXT NOT NULL CHECK (source IN ('relay','connection_test')),
    provider_id TEXT NOT NULL CHECK (length(provider_id) BETWEEN 1 AND 160),
    route_id TEXT NOT NULL CHECK (length(route_id) BETWEEN 1 AND 160),
    provider_name TEXT NOT NULL CHECK (length(provider_name) <= 160),
    route_name TEXT NOT NULL CHECK (length(route_name) <= 160),
    requested_model TEXT NOT NULL CHECK (length(requested_model) BETWEEN 1 AND 160),
    upstream_model TEXT NOT NULL CHECK (length(upstream_model) BETWEEN 1 AND 160),
    response_model TEXT CHECK (length(response_model) BETWEEN 1 AND 160),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    ended_at_ms INTEGER CHECK (ended_at_ms >= started_at_ms),
    duration_ms INTEGER CHECK (duration_ms >= 0),
    ttfb_ms INTEGER CHECK (ttfb_ms >= 0),
    first_content_ms INTEGER CHECK (first_content_ms >= 0),
    stream INTEGER NOT NULL CHECK (stream IN (0,1)),
    -- NULL while a started record cannot yet prove whether the network call began.
    upstream_started INTEGER CHECK (upstream_started IN (0,1)),
    http_status INTEGER CHECK (http_status BETWEEN 100 AND 599),
    outcome TEXT NOT NULL DEFAULT 'in_flight'
        CHECK (outcome IN ('in_flight','completed','failed','cancelled','interrupted','rejected')),
    error_kind TEXT CHECK (error_kind IN (
        'missing_key','connect','upstream_http','upstream_stream','sse_error',
        'client_drop','unclean_shutdown','shutdown','other')),
    usage_status TEXT NOT NULL DEFAULT 'missing'
        CHECK (usage_status IN ('complete','partial','missing','invalid','not_applicable')),
    usage_reason TEXT CHECK (usage_reason IN (
        'no_usage','client_drop','stream_error','parse_error','event_too_large',
        'body_too_large','unsupported_encoding','invalid_numbers','inconsistent',
        'conflicting_usage','unclean_shutdown','not_sent')),
    usage_source TEXT NOT NULL DEFAULT 'none' CHECK (usage_source IN ('none','upstream')),
    parser_version INTEGER NOT NULL DEFAULT 1 CHECK (parser_version > 0),
    input_tokens INTEGER CHECK (input_tokens BETWEEN 0 AND 1000000000000),
    output_tokens INTEGER CHECK (output_tokens BETWEEN 0 AND 1000000000000),
    total_tokens INTEGER CHECK (total_tokens BETWEEN 0 AND 2000000000000),
    cache_read_tokens INTEGER CHECK (cache_read_tokens BETWEEN 0 AND input_tokens),
    cache_write_tokens INTEGER CHECK (cache_write_tokens BETWEEN 0 AND input_tokens),
    reasoning_tokens INTEGER CHECK (reasoning_tokens BETWEEN 0 AND output_tokens),
    -- Only populated if this adapter proves that cache read/write are disjoint input subsets.
    input_uncached_tokens INTEGER CHECK (input_uncached_tokens BETWEEN 0 AND input_tokens),
    input_breakdown TEXT NOT NULL DEFAULT 'unknown'
        CHECK (input_breakdown IN ('unknown','read_subset','disjoint')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    CHECK ((outcome = 'in_flight' AND ended_at_ms IS NULL AND duration_ms IS NULL)
        OR (outcome <> 'in_flight' AND ended_at_ms IS NOT NULL)),
    CHECK (input_tokens IS NULL OR output_tokens IS NULL OR total_tokens IS NULL
        OR total_tokens = input_tokens + output_tokens),
    CHECK (usage_status <> 'complete' OR (
        input_tokens IS NOT NULL AND output_tokens IS NOT NULL AND total_tokens IS NOT NULL
        AND usage_source = 'upstream')),
    CHECK (usage_source <> 'none' OR (
        input_tokens IS NULL AND output_tokens IS NULL AND total_tokens IS NULL
        AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL
        AND reasoning_tokens IS NULL AND input_uncached_tokens IS NULL)),
    CHECK (usage_status <> 'not_applicable' OR upstream_started IS 0),
    CHECK (input_breakdown <> 'disjoint' OR (
        input_tokens IS NOT NULL AND cache_read_tokens IS NOT NULL
        AND cache_write_tokens IS NOT NULL AND input_uncached_tokens IS NOT NULL
        AND input_tokens = input_uncached_tokens + cache_read_tokens + cache_write_tokens)),
    CHECK (input_uncached_tokens IS NULL OR input_breakdown = 'disjoint')
);
CREATE INDEX usage_requests_time ON usage_requests(started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_provider_time ON usage_requests(provider_id, started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_model_time ON usage_requests(upstream_model, started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_route_time ON usage_requests(route_id, started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_pending ON usage_requests(run_id) WHERE outcome = 'in_flight';

CREATE TABLE stats_collection_intervals (
    id INTEGER PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES stats_runs(run_id),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    ended_at_ms INTEGER CHECK (ended_at_ms >= started_at_ms),
    state TEXT NOT NULL CHECK (state IN ('collecting','paused','degraded','unknown')),
    reason TEXT CHECK (reason IN ('user_disabled','queue_full','database_error','unclean_exit')),
    CHECK (state <> 'collecting' OR reason IS NULL)
);
CREATE INDEX stats_collection_intervals_time ON stats_collection_intervals(started_at_ms, ended_at_ms);
CREATE UNIQUE INDEX stats_collection_intervals_open ON stats_collection_intervals(run_id)
    WHERE ended_at_ms IS NULL;

CREATE TABLE stats_price_rules (
    rule_id TEXT PRIMARY KEY CHECK (length(rule_id) BETWEEN 1 AND 64),
    -- Exact provider/model match only; no wildcard precedence in v1.
    provider_id TEXT NOT NULL CHECK (length(provider_id) BETWEEN 1 AND 160),
    upstream_model TEXT NOT NULL CHECK (length(upstream_model) BETWEEN 1 AND 160),
    currency TEXT NOT NULL CHECK (currency IN ('USD','CNY')),
    effective_from_ms INTEGER NOT NULL CHECK (effective_from_ms >= 0),
    effective_to_ms INTEGER CHECK (effective_to_ms > effective_from_ms),
    -- Integer nano currency units per 1,000,000 tokens; NOT floating point prices.
    input_rate_nanos INTEGER NOT NULL CHECK (input_rate_nanos BETWEEN 0 AND 1000000000000000),
    output_rate_nanos INTEGER NOT NULL CHECK (output_rate_nanos BETWEEN 0 AND 1000000000000000),
    cache_read_rate_nanos INTEGER CHECK (cache_read_rate_nanos BETWEEN 0 AND 1000000000000000),
    cache_write_rate_nanos INTEGER CHECK (cache_write_rate_nanos BETWEEN 0 AND 1000000000000000),
    input_mode TEXT NOT NULL CHECK (input_mode IN ('flat','read_discount','disjoint')),
    label TEXT NOT NULL CHECK (length(label) <= 160),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    CHECK (input_mode <> 'read_discount' OR cache_read_rate_nanos IS NOT NULL),
    CHECK (input_mode <> 'disjoint' OR (cache_read_rate_nanos IS NOT NULL AND cache_write_rate_nanos IS NOT NULL))
);
CREATE INDEX stats_price_rules_lookup ON stats_price_rules(provider_id, upstream_model, effective_from_ms DESC);
CREATE TRIGGER stats_price_rules_no_overlap_insert
BEFORE INSERT ON stats_price_rules
WHEN EXISTS (
    SELECT 1 FROM stats_price_rules p
    WHERE p.provider_id = NEW.provider_id AND p.upstream_model = NEW.upstream_model
      AND p.effective_from_ms < COALESCE(NEW.effective_to_ms,9223372036854775807)
      AND NEW.effective_from_ms < COALESCE(p.effective_to_ms,9223372036854775807)
)
BEGIN SELECT RAISE(ABORT,'overlapping price rule'); END;
CREATE TRIGGER stats_price_rules_no_overlap_update
BEFORE UPDATE ON stats_price_rules
WHEN EXISTS (
    SELECT 1 FROM stats_price_rules p
    WHERE p.rule_id <> OLD.rule_id
      AND p.provider_id = NEW.provider_id AND p.upstream_model = NEW.upstream_model
      AND p.effective_from_ms < COALESCE(NEW.effective_to_ms,9223372036854775807)
      AND NEW.effective_from_ms < COALESCE(p.effective_to_ms,9223372036854775807)
)
BEGIN SELECT RAISE(ABORT,'overlapping price rule'); END;

CREATE TABLE usage_costs (
    request_id TEXT PRIMARY KEY REFERENCES usage_requests(request_id) ON DELETE CASCADE,
    rule_id TEXT REFERENCES stats_price_rules(rule_id),
    status TEXT NOT NULL CHECK (status IN ('priced','unpriced','usage_incomplete','unsupported_breakdown','not_applicable','overflow')),
    currency TEXT CHECK (currency IN ('USD','CNY')),
    input_cost_nanos INTEGER CHECK (input_cost_nanos >= 0),
    output_cost_nanos INTEGER CHECK (output_cost_nanos >= 0),
    cache_read_cost_nanos INTEGER CHECK (cache_read_cost_nanos >= 0),
    cache_write_cost_nanos INTEGER CHECK (cache_write_cost_nanos >= 0),
    total_cost_nanos INTEGER CHECK (total_cost_nanos >= 0),
    pricing_version INTEGER NOT NULL DEFAULT 1 CHECK (pricing_version > 0),
    calculated_at_ms INTEGER NOT NULL CHECK (calculated_at_ms >= 0),
    CHECK ((status = 'priced' AND rule_id IS NOT NULL AND currency IS NOT NULL
        AND input_cost_nanos IS NOT NULL AND output_cost_nanos IS NOT NULL
        AND cache_read_cost_nanos IS NOT NULL AND cache_write_cost_nanos IS NOT NULL
        AND total_cost_nanos IS NOT NULL
        AND total_cost_nanos = input_cost_nanos + output_cost_nanos + cache_read_cost_nanos + cache_write_cost_nanos)
        OR (status <> 'priced' AND total_cost_nanos IS NULL
            AND input_cost_nanos IS NULL AND output_cost_nanos IS NULL
            AND cache_read_cost_nanos IS NULL AND cache_write_cost_nanos IS NULL))
);

CREATE TABLE stats_maintenance_events (
    id INTEGER PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('migration','retention','clear','reprice')),
    started_at_ms INTEGER NOT NULL CHECK (started_at_ms >= 0),
    completed_at_ms INTEGER CHECK (completed_at_ms >= started_at_ms),
    affected_rows INTEGER NOT NULL DEFAULT 0 CHECK (affected_rows >= 0),
    status TEXT NOT NULL CHECK (status IN ('running','completed','failed')),
    -- Fixed machine error code only; no SQL text, path, prompt or provider error body.
    error_code TEXT CHECK (length(error_code) <= 64)
);

-- This view intentionally exposes confirmed totals separately from known partial numbers.
CREATE VIEW stats_request_facts AS
SELECT r.*,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN 1 ELSE 0 END AS counted_usage,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.input_tokens END AS counted_input_tokens,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.output_tokens END AS counted_output_tokens,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.total_tokens END AS counted_total_tokens,
    c.status AS cost_status, c.currency, c.total_cost_nanos
FROM usage_requests r LEFT JOIN usage_costs c ON c.request_id = r.request_id;

