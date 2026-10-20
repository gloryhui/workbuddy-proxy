-- Parser v2 admits standalone credit stages and model-usage receipts. Resume
-- an ordinary bounded full scan after upgrade, even for files already at EOF.
-- Keep incremental offsets, facts, identities, retention and deletion tombstones.
UPDATE workbuddy_import_files SET scan_offset=0,scan_generation=0
WHERE namespace_id=(SELECT workbuddy_namespace_id FROM stats_settings WHERE singleton=1);
UPDATE stats_settings SET workbuddy_last_full_scan_ms=0
WHERE singleton=1 AND EXISTS (
 SELECT 1 FROM workbuddy_import_files WHERE namespace_id=workbuddy_namespace_id
);
