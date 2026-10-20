-- v11: standalone imported charges have no Token usage by design.
-- Rebuild to allow that state while retaining the stricter proxy invariant.
-- The caller disables foreign keys before the transaction and checks them after
-- copying, preserving cost rows, import ledger identities and deletion tombstones.
DROP TRIGGER workbuddy_no_import_cost_insert;
DROP TRIGGER workbuddy_no_import_cost_update;
DROP VIEW stats_request_facts;
CREATE TABLE usage_requests_v11 (
    id INTEGER PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE CHECK (length(request_id) BETWEEN 1 AND 64),
    run_id TEXT REFERENCES stats_runs(run_id),
    source TEXT NOT NULL CHECK (source IN ('relay','connection_test','workbuddy_import')),
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
    stream INTEGER CHECK (stream IN (0,1)),
    -- NULL while a started record cannot yet prove whether the network call began.
    upstream_started INTEGER CHECK (upstream_started IN (0,1)),
    http_status INTEGER CHECK (http_status BETWEEN 100 AND 599),
    outcome TEXT NOT NULL DEFAULT 'in_flight'
        CHECK (outcome IN ('in_flight','completed','failed','cancelled','interrupted','rejected','recorded')),
    error_kind TEXT CHECK (error_kind IN (
        'missing_key','connect','upstream_http','upstream_stream','sse_error',
        'client_drop','unclean_shutdown','shutdown','other')),
    usage_status TEXT NOT NULL DEFAULT 'missing'
        CHECK (usage_status IN ('complete','partial','missing','invalid','not_applicable')),
    usage_reason TEXT CHECK (usage_reason IN (
        'no_usage','client_drop','stream_error','parse_error','event_too_large',
        'body_too_large','unsupported_encoding','invalid_numbers','inconsistent',
        'conflicting_usage','unclean_shutdown','not_sent')),
    usage_source TEXT NOT NULL DEFAULT 'none' CHECK (usage_source IN ('none','upstream','workbuddy_jsonl')),
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
    -- Kept from v6: omitting the column still yields a concrete protocol, while
    -- imported rows pass NULL explicitly and the proxy branch below still
    -- requires a non-NULL value.
    upstream_protocol TEXT DEFAULT 'openai_chat' CHECK(upstream_protocol IN ('openai_chat','openai_responses','anthropic_messages','gemini_generate_content')),
    record_origin TEXT NOT NULL DEFAULT 'proxy' CHECK(record_origin IN ('proxy','workbuddy_builtin','workbuddy_custom')),
    time_basis TEXT NOT NULL DEFAULT 'request_start' CHECK(time_basis IN ('request_start','message_timestamp')),
    total_tokens_derived INTEGER NOT NULL DEFAULT 0 CHECK(total_tokens_derived IN (0,1)),
    workbuddy_credit_nanos INTEGER CHECK(workbuddy_credit_nanos BETWEEN 0 AND 1000000000000000000),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    CHECK ((outcome = 'in_flight' AND ended_at_ms IS NULL AND duration_ms IS NULL)
        OR (outcome <> 'in_flight' AND ended_at_ms IS NOT NULL)),
    CHECK (input_tokens IS NULL OR output_tokens IS NULL OR total_tokens IS NULL
        OR total_tokens = input_tokens + output_tokens),
    CHECK (usage_status <> 'complete' OR (
        input_tokens IS NOT NULL AND output_tokens IS NOT NULL AND total_tokens IS NOT NULL
        AND usage_source IN ('upstream','workbuddy_jsonl'))),
    CHECK (usage_source <> 'none' OR (
        input_tokens IS NULL AND output_tokens IS NULL AND total_tokens IS NULL
        AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL
        AND reasoning_tokens IS NULL AND input_uncached_tokens IS NULL)),
    CHECK (usage_status <> 'not_applicable' OR
        (record_origin='proxy' AND upstream_started IS 0) OR
        (record_origin IN ('workbuddy_builtin','workbuddy_custom')
         AND requested_model GLOB 'workbuddy-credit:?*'
         AND input_tokens IS NULL AND output_tokens IS NULL AND total_tokens IS NULL
         AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL
         AND reasoning_tokens IS NULL AND input_uncached_tokens IS NULL)),
    CHECK (input_breakdown <> 'disjoint' OR (
        input_tokens IS NOT NULL AND cache_read_tokens IS NOT NULL
        AND cache_write_tokens IS NOT NULL AND input_uncached_tokens IS NOT NULL
        AND input_tokens = input_uncached_tokens + cache_read_tokens + cache_write_tokens)),
    CHECK (input_uncached_tokens IS NULL OR input_breakdown = 'disjoint')
,
    CHECK (
      (record_origin='proxy' AND source IN ('relay','connection_test')
       AND run_id IS NOT NULL AND stream IS NOT NULL AND upstream_protocol IS NOT NULL
       AND outcome<>'recorded' AND usage_source IN ('none','upstream')
       AND time_basis='request_start' AND workbuddy_credit_nanos IS NULL)
      OR
      (record_origin IN ('workbuddy_builtin','workbuddy_custom') AND source='workbuddy_import'
       AND run_id IS NULL AND stream IS NULL AND upstream_protocol IS NULL
       AND outcome='recorded' AND usage_source='workbuddy_jsonl' AND time_basis='message_timestamp'
       AND upstream_started IS NULL AND http_status IS NULL AND error_kind IS NULL
       AND duration_ms IS NULL AND ttfb_ms IS NULL AND first_content_ms IS NULL
       AND ended_at_ms=started_at_ms)
    )
);
INSERT INTO usage_requests_v11(id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,response_model,started_at_ms,ended_at_ms,duration_ms,ttfb_ms,first_content_ms,stream,upstream_started,http_status,outcome,error_kind,usage_status,usage_reason,usage_source,parser_version,input_tokens,output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,input_uncached_tokens,input_breakdown,upstream_protocol,record_origin,time_basis,total_tokens_derived,workbuddy_credit_nanos,created_at_ms,updated_at_ms)
SELECT id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,response_model,started_at_ms,ended_at_ms,duration_ms,ttfb_ms,first_content_ms,stream,upstream_started,http_status,outcome,error_kind,usage_status,usage_reason,usage_source,parser_version,input_tokens,output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,input_uncached_tokens,input_breakdown,upstream_protocol,record_origin,time_basis,total_tokens_derived,workbuddy_credit_nanos,created_at_ms,updated_at_ms FROM usage_requests;
DROP TABLE usage_requests;
ALTER TABLE usage_requests_v11 RENAME TO usage_requests;
CREATE INDEX usage_requests_time ON usage_requests(started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_provider_time ON usage_requests(provider_id, started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_model_time ON usage_requests(upstream_model, started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_route_time ON usage_requests(route_id, started_at_ms DESC, id DESC);
CREATE INDEX usage_requests_pending ON usage_requests(run_id) WHERE outcome = 'in_flight';
CREATE VIEW stats_request_facts AS
SELECT r.*,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN 1 ELSE 0 END AS counted_usage,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.input_tokens END AS counted_input_tokens,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.output_tokens END AS counted_output_tokens,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.total_tokens END AS counted_total_tokens,
    c.status AS cost_status, c.currency, c.total_cost_nanos
FROM usage_requests r LEFT JOIN usage_costs c ON c.request_id = r.request_id;

CREATE INDEX usage_requests_origin_time ON usage_requests(record_origin, started_at_ms DESC, id DESC);

CREATE TRIGGER workbuddy_no_import_cost_insert BEFORE INSERT ON usage_costs
WHEN EXISTS(SELECT 1 FROM usage_requests r WHERE r.request_id=NEW.request_id AND r.record_origin<>'proxy')
BEGIN SELECT RAISE(ABORT,'imported cost is unknown'); END;
CREATE TRIGGER workbuddy_no_import_cost_update BEFORE UPDATE ON usage_costs
WHEN EXISTS(SELECT 1 FROM usage_requests r WHERE r.request_id=NEW.request_id AND r.record_origin<>'proxy')
BEGIN SELECT RAISE(ABORT,'imported cost is unknown'); END;

CREATE INDEX idx_usage_origin_started ON usage_requests(record_origin, started_at_ms);

-- Existing receipts are repaired without needing the original log files.
-- Amounts, identity hashes, names, timestamps and scan checkpoints stay intact.
UPDATE usage_requests SET usage_status='not_applicable',usage_reason=NULL
WHERE record_origin IN ('workbuddy_builtin','workbuddy_custom')
 AND requested_model GLOB 'workbuddy-credit:?*' AND usage_status='missing'
 AND input_tokens IS NULL AND output_tokens IS NULL AND total_tokens IS NULL
 AND cache_read_tokens IS NULL AND cache_write_tokens IS NULL
 AND reasoning_tokens IS NULL AND input_uncached_tokens IS NULL;
