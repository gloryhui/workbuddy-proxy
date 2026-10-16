-- Keep the complete public price catalogue locally, together with the last
-- successful local calendar day. A failed download never advances this marker.
CREATE TABLE stats_price_catalog (
    singleton INTEGER PRIMARY KEY CHECK (singleton=1),
    catalogue_json TEXT NOT NULL,
    last_success_day TEXT NOT NULL,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms>=0),
    updated_count INTEGER NOT NULL CHECK (updated_count>=0),
    unmatched_count INTEGER NOT NULL CHECK (unmatched_count>=0),
    catalog_count INTEGER NOT NULL CHECK (catalog_count>0),
    targets_key TEXT NOT NULL
);
