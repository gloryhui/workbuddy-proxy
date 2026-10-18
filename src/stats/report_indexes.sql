-- Optional read accelerator, installed atomically by the sole writer at
-- startup for both new and existing v7 databases. No fact/schema version is
-- rewritten. Fields cover the actual dashboard aggregates; no message text.
CREATE INDEX IF NOT EXISTS usage_requests_dashboard_cover ON usage_requests(
    started_at_ms, source, record_origin, outcome, usage_status, upstream_started,
    input_tokens, output_tokens, total_tokens, cache_read_tokens, cache_write_tokens,
    input_uncached_tokens, input_breakdown, reasoning_tokens, duration_ms,
    workbuddy_credit_nanos
);
