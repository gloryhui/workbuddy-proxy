-- Schema v7: WorkBuddy local usage import.
-- Applied by store::apply_step inside one IMMEDIATE transaction. The caller
-- disables foreign keys before BEGIN (usage_costs cascades into usage_requests),
-- runs PRAGMA foreign_key_check afterwards, then sets user_version=7 and
-- restores foreign keys. Do not add transaction or PRAGMA commands here;
-- store::apply_step owns them.
-- Body copied verbatim from docs/workbuddy-import/schema-v7.sql, validated on a
-- disposable v6 database in S02.01; see evidence/S02.01.md.
--
-- Deviation from that design script, recorded in evidence/S02.01.md and
-- evidence/S02.02.md:
--   * upstream_protocol keeps v6's DEFAULT 'openai_chat'. Dropping the default
--     made every insert that omits the column violate the proxy branch of the
--     record_origin CHECK and broke four existing regression tests; imported
--     rows still pass NULL explicitly, and the proxy branch still requires a
--     non-NULL value. DATA-API section 3 requires the design script to be
--     corrected when it disagrees with the implementation.
DROP VIEW stats_request_facts;
CREATE TABLE usage_requests_v7 (
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
    CHECK (usage_status <> 'not_applicable' OR upstream_started IS 0),
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
INSERT INTO usage_requests_v7(id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,response_model,started_at_ms,ended_at_ms,duration_ms,ttfb_ms,first_content_ms,stream,upstream_started,http_status,outcome,error_kind,usage_status,usage_reason,usage_source,parser_version,input_tokens,output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,input_uncached_tokens,input_breakdown,created_at_ms,updated_at_ms,upstream_protocol)
SELECT id,request_id,run_id,source,provider_id,route_id,provider_name,route_name,requested_model,upstream_model,response_model,started_at_ms,ended_at_ms,duration_ms,ttfb_ms,first_content_ms,stream,upstream_started,http_status,outcome,error_kind,usage_status,usage_reason,usage_source,parser_version,input_tokens,output_tokens,total_tokens,cache_read_tokens,cache_write_tokens,reasoning_tokens,input_uncached_tokens,input_breakdown,created_at_ms,updated_at_ms,upstream_protocol FROM usage_requests;
DROP TABLE usage_requests;
ALTER TABLE usage_requests_v7 RENAME TO usage_requests;
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
ALTER TABLE stats_settings ADD COLUMN workbuddy_import_enabled INTEGER NOT NULL DEFAULT 0 CHECK(workbuddy_import_enabled IN (0,1));
ALTER TABLE stats_settings ADD COLUMN workbuddy_namespace_id TEXT NOT NULL DEFAULT '' CHECK(length(workbuddy_namespace_id) IN (0,48,64));
ALTER TABLE stats_settings ADD COLUMN workbuddy_mapping_revision INTEGER NOT NULL DEFAULT 1 CHECK(workbuddy_mapping_revision>0);
ALTER TABLE stats_settings ADD COLUMN workbuddy_last_full_scan_ms INTEGER CHECK(workbuddy_last_full_scan_ms>=0);

CREATE TABLE workbuddy_import_files (
 file_key TEXT PRIMARY KEY CHECK(length(file_key)=64),
 namespace_id TEXT NOT NULL CHECK(length(namespace_id) IN (48,64)),
 relative_path TEXT NOT NULL CHECK(length(relative_path) BETWEEN 1 AND 4096),
 generation INTEGER NOT NULL DEFAULT 1 CHECK(generation>0),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
 committed_offset INTEGER NOT NULL DEFAULT 0 CHECK(committed_offset>=0),
 observed_size INTEGER NOT NULL DEFAULT 0 CHECK(observed_size>=0),
 modified_ms INTEGER NOT NULL DEFAULT 0 CHECK(modified_ms>=0),
 prefix_hash TEXT CHECK(length(prefix_hash)=64),
 prefix_len INTEGER NOT NULL DEFAULT 0 CHECK(prefix_len BETWEEN 0 AND 4096),
 anchor_hash TEXT CHECK(length(anchor_hash)=64),
 anchor_start INTEGER NOT NULL DEFAULT 0 CHECK(anchor_start>=0),
 anchor_len INTEGER NOT NULL DEFAULT 0 CHECK(anchor_len BETWEEN 0 AND 4096),
 discarding_oversized INTEGER NOT NULL DEFAULT 0 CHECK(discarding_oversized IN (0,1)),
 scan_offset INTEGER NOT NULL DEFAULT 0 CHECK(scan_offset>=0),
 scan_generation INTEGER NOT NULL DEFAULT 0 CHECK(scan_generation>=0),
 last_seen_at_ms INTEGER NOT NULL CHECK(last_seen_at_ms>=0),
 last_completed_at_ms INTEGER CHECK(last_completed_at_ms>=0),
 last_error_code TEXT CHECK(length(last_error_code)<=64),
 diagnostic_counts_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(diagnostic_counts_json) AND length(diagnostic_counts_json)<=4096),
 UNIQUE(namespace_id, relative_path)
);
CREATE TABLE workbuddy_model_history (
 mapping_id TEXT PRIMARY KEY CHECK(length(mapping_id) BETWEEN 1 AND 64),
 namespace_id TEXT NOT NULL CHECK(length(namespace_id) IN (48,64)),
 requested_model TEXT NOT NULL CHECK(length(requested_model) BETWEEN 1 AND 160),
 classification TEXT NOT NULL CHECK(classification IN ('proxy','direct')),
 valid_from_ms INTEGER NOT NULL CHECK(valid_from_ms>=0),
 valid_to_ms INTEGER CHECK(valid_to_ms>valid_from_ms),
 evidence TEXT NOT NULL CHECK(evidence IN ('observed_config','managed_sync','manual')),
 state TEXT NOT NULL CHECK(state IN ('prepared','verified','conflict','revoked')),
 reconcile_state TEXT NOT NULL DEFAULT 'completed' CHECK(reconcile_state IN ('pending','running','completed')),
 reconcile_watermark INTEGER NOT NULL DEFAULT 0 CHECK(reconcile_watermark>=0),
 reconciled_rows INTEGER NOT NULL DEFAULT 0 CHECK(reconciled_rows>=0),
 reconcile_error_code TEXT CHECK(length(reconcile_error_code)<=64),
 route_id TEXT CHECK(length(route_id)<=160),
 provider_id TEXT CHECK(length(provider_id)<=160),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at_ms INTEGER NOT NULL CHECK(created_at_ms>=0),
 updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=created_at_ms)
);
CREATE INDEX workbuddy_model_history_lookup ON workbuddy_model_history(namespace_id, requested_model, valid_from_ms, valid_to_ms);
CREATE TRIGGER workbuddy_mapping_no_overlap_insert BEFORE INSERT ON workbuddy_model_history
WHEN NEW.state='verified' AND EXISTS (
 SELECT 1 FROM workbuddy_model_history h WHERE h.namespace_id=NEW.namespace_id
 AND h.requested_model=NEW.requested_model AND h.state='verified'
 AND h.valid_from_ms<COALESCE(NEW.valid_to_ms,9223372036854775807)
 AND NEW.valid_from_ms<COALESCE(h.valid_to_ms,9223372036854775807))
BEGIN SELECT RAISE(ABORT,'overlapping model mapping'); END;
CREATE TRIGGER workbuddy_mapping_no_overlap_update BEFORE UPDATE ON workbuddy_model_history
WHEN NEW.state='verified' AND EXISTS (
 SELECT 1 FROM workbuddy_model_history h WHERE h.mapping_id<>OLD.mapping_id
 AND h.namespace_id=NEW.namespace_id AND h.requested_model=NEW.requested_model AND h.state='verified'
 AND h.valid_from_ms<COALESCE(NEW.valid_to_ms,9223372036854775807)
 AND NEW.valid_from_ms<COALESCE(h.valid_to_ms,9223372036854775807))
BEGIN SELECT RAISE(ABORT,'overlapping model mapping'); END;
CREATE TABLE workbuddy_import_ledger (
 entry_key TEXT PRIMARY KEY CHECK(length(entry_key)=64),
 namespace_id TEXT NOT NULL CHECK(length(namespace_id) IN (48,64)),
 session_hash TEXT NOT NULL CHECK(length(session_hash)=64),
 message_hash TEXT NOT NULL CHECK(length(message_hash)=64),
 requested_model TEXT CHECK(length(requested_model) BETWEEN 1 AND 160),
 event_at_ms INTEGER NOT NULL CHECK(event_at_ms>=0),
 normalized_hash TEXT NOT NULL CHECK(length(normalized_hash)=64),
 state TEXT NOT NULL CHECK(state IN ('imported','excluded_proxy','excluded_retention','pending_classification','conflict','deleted_manual','deleted_retention')),
 request_id TEXT REFERENCES usage_requests(request_id) ON DELETE SET NULL,
 candidate_json TEXT CHECK(candidate_json IS NULL OR (json_valid(candidate_json) AND length(CAST(candidate_json AS BLOB))<=16384)),
 mapping_id TEXT REFERENCES workbuddy_model_history(mapping_id),
 mapping_revision INTEGER NOT NULL CHECK(mapping_revision>0),
 created_at_ms INTEGER NOT NULL CHECK(created_at_ms>=0),
 updated_at_ms INTEGER NOT NULL CHECK(updated_at_ms>=created_at_ms),
 CHECK(state IN ('pending_classification','conflict') OR candidate_json IS NULL),
 CHECK((state='imported' AND request_id IS NOT NULL AND request_id=entry_key) OR (state<>'imported' AND request_id IS NULL)),
 UNIQUE(namespace_id, session_hash, message_hash)
);
CREATE INDEX workbuddy_ledger_pending ON workbuddy_import_ledger(state, requested_model, event_at_ms);
CREATE INDEX workbuddy_ledger_mapping ON workbuddy_import_ledger(mapping_id, state);

-- Imported history never receives a fabricated current-price cost row.
CREATE TRIGGER workbuddy_no_import_cost_insert BEFORE INSERT ON usage_costs
WHEN EXISTS(SELECT 1 FROM usage_requests r WHERE r.request_id=NEW.request_id AND r.record_origin<>'proxy')
BEGIN SELECT RAISE(ABORT,'imported cost is unknown'); END;
CREATE TRIGGER workbuddy_no_import_cost_update BEFORE UPDATE ON usage_costs
WHEN EXISTS(SELECT 1 FROM usage_requests r WHERE r.request_id=NEW.request_id AND r.record_origin<>'proxy')
BEGIN SELECT RAISE(ABORT,'imported cost is unknown'); END;

CREATE INDEX workbuddy_ledger_model_time ON workbuddy_import_ledger(namespace_id, requested_model, event_at_ms);

-- Query-plan indexes for the import's own hot paths. `origin/time` serves the
-- paged fact queries, and the ledger indexes serve pending confirmation and the
-- state/time sweep. Without them a page would scan the whole ledger.
CREATE INDEX idx_usage_origin_started ON usage_requests(record_origin, started_at_ms);
CREATE INDEX idx_wb_ledger_state_event ON workbuddy_import_ledger(state, event_at_ms);
CREATE INDEX idx_wb_ledger_model_state ON workbuddy_import_ledger(requested_model, state);
