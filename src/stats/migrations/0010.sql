-- Tool display names are stored in the synthetic charge route's name snapshot.
-- Reread old receipts to enrich that name; billing keys and amounts stay intact.
UPDATE workbuddy_import_files SET scan_offset=0,scan_generation=0
WHERE namespace_id=(SELECT workbuddy_namespace_id FROM stats_settings WHERE singleton=1);
UPDATE stats_settings SET workbuddy_last_full_scan_ms=0
WHERE singleton=1 AND EXISTS (
 SELECT 1 FROM workbuddy_import_files WHERE namespace_id=workbuddy_namespace_id
);
