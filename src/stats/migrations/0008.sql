-- v8: classify logs as builtin/custom automatically; native proxy facts win
-- only when the exact WorkBuddy session/message identity is recorded.
CREATE TABLE workbuddy_proxy_identity (
 request_id TEXT PRIMARY KEY REFERENCES usage_requests(request_id) ON DELETE CASCADE,
 session_hash TEXT NOT NULL CHECK(length(session_hash)=64),
 message_hash TEXT NOT NULL CHECK(length(message_hash)=64),
 pending_dedup INTEGER NOT NULL DEFAULT 1 CHECK(pending_dedup IN (0,1))
);
CREATE INDEX workbuddy_proxy_identity_pending ON workbuddy_proxy_identity(request_id) WHERE pending_dedup=1;
CREATE INDEX workbuddy_proxy_identity_message ON workbuddy_proxy_identity(session_hash,message_hash);
CREATE INDEX workbuddy_ledger_message ON workbuddy_import_ledger(session_hash,message_hash);
-- Former model-level proxy exclusions are not proof of a duplicate request.
UPDATE workbuddy_import_ledger SET state='pending_classification',mapping_id=NULL
 WHERE state='excluded_proxy';
UPDATE workbuddy_model_history SET reconcile_state='completed',reconcile_error_code=NULL;
UPDATE stats_settings SET workbuddy_mapping_revision=workbuddy_mapping_revision+1,
 workbuddy_last_full_scan_ms=CASE WHEN EXISTS(SELECT 1 FROM workbuddy_import_ledger
   WHERE state='pending_classification' AND candidate_json IS NULL)
   THEN 0 ELSE workbuddy_last_full_scan_ms END WHERE singleton=1;
