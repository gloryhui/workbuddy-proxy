-- Migration 0002: decouple the price configuration from the per-call cost rows.
--
-- The v1 schema stored interval-based rules (stats_price_rules) and referenced
-- them from usage_costs, which made "edit/delete a price" impossible once a
-- request had used it. v2 keeps an editable current price per provider+model and
-- gives every request its own snapshot of the unit prices it was admitted with.
--
-- The whole script runs inside one transaction (see store::migrate) after a
-- backup, with foreign keys disabled so the table rebuild can copy data.
-- Nothing here multiplies or divides the stored nano rates: v1 already stored
-- nano currency units per million tokens.

-- 1. Current prices: one editable row per provider + upstream model.
CREATE TABLE stats_model_prices (
    price_id TEXT PRIMARY KEY CHECK (length(price_id) BETWEEN 1 AND 64),
    provider_id TEXT NOT NULL CHECK (length(provider_id) BETWEEN 1 AND 160),
    upstream_model TEXT NOT NULL CHECK (length(upstream_model) BETWEEN 1 AND 160),
    currency TEXT NOT NULL CHECK (currency IN ('USD','CNY')),
    input_mode TEXT NOT NULL CHECK (input_mode IN ('flat','read_discount','disjoint')),
    input_rate_nanos INTEGER NOT NULL DEFAULT 0 CHECK (input_rate_nanos BETWEEN 0 AND 1000000000000000),
    output_rate_nanos INTEGER NOT NULL DEFAULT 0 CHECK (output_rate_nanos BETWEEN 0 AND 1000000000000000),
    cache_read_rate_nanos INTEGER NOT NULL DEFAULT 0 CHECK (cache_read_rate_nanos BETWEEN 0 AND 1000000000000000),
    cache_write_rate_nanos INTEGER NOT NULL DEFAULT 0 CHECK (cache_write_rate_nanos BETWEEN 0 AND 1000000000000000),
    label TEXT NOT NULL CHECK (length(label) <= 160),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= created_at_ms),
    UNIQUE(provider_id, upstream_model)
);

-- Carry over the rule that is in effect at migration time. The newest
-- effective_from_ms wins; two rules that both match with the same start would
-- violate the UNIQUE key and abort the migration loudly rather than pick one.
INSERT INTO stats_model_prices(
        price_id, provider_id, upstream_model, currency, input_mode,
        input_rate_nanos, output_rate_nanos, cache_read_rate_nanos, cache_write_rate_nanos,
        label, revision, created_at_ms, updated_at_ms)
SELECT r.rule_id, r.provider_id, r.upstream_model, r.currency, r.input_mode,
       r.input_rate_nanos, r.output_rate_nanos,
       COALESCE(r.cache_read_rate_nanos, 0), COALESCE(r.cache_write_rate_nanos, 0),
       r.label, 1, r.created_at_ms,
       MAX(r.created_at_ms, CAST(strftime('%s','now') AS INTEGER) * 1000)
FROM stats_price_rules r
WHERE r.effective_from_ms <= CAST(strftime('%s','now') AS INTEGER) * 1000
  AND (r.effective_to_ms IS NULL OR r.effective_to_ms > CAST(strftime('%s','now') AS INTEGER) * 1000)
  AND r.effective_from_ms = (
      SELECT MAX(r2.effective_from_ms) FROM stats_price_rules r2
      WHERE r2.provider_id = r.provider_id AND r2.upstream_model = r.upstream_model
        AND r2.effective_from_ms <= CAST(strftime('%s','now') AS INTEGER) * 1000
        AND (r2.effective_to_ms IS NULL OR r2.effective_to_ms > CAST(strftime('%s','now') AS INTEGER) * 1000));

-- 2. The currency used when a request had no usable price.
ALTER TABLE stats_settings ADD COLUMN default_currency TEXT NOT NULL DEFAULT 'CNY'
    CHECK (default_currency IN ('USD','CNY'));

-- 3. Rebuild usage_costs with the per-call snapshot columns. The view over it
-- is dropped first: SQLite would otherwise keep a stale binding to the table
-- that is being replaced.
DROP VIEW IF EXISTS stats_request_facts;

CREATE TABLE usage_costs_v2 (
    request_id TEXT PRIMARY KEY REFERENCES usage_requests(request_id) ON DELETE CASCADE,
    source_price_id TEXT CHECK (length(source_price_id) BETWEEN 1 AND 64),
    source_price_revision INTEGER CHECK (source_price_revision > 0),
    price_source TEXT NOT NULL CHECK (price_source IN ('manual','default_zero','legacy_unknown')),
    price_snapshot_at_ms INTEGER CHECK (price_snapshot_at_ms >= 0),
    currency TEXT NOT NULL CHECK (currency IN ('USD','CNY')),
    input_mode TEXT CHECK (input_mode IN ('flat','read_discount','disjoint')),
    input_rate_nanos INTEGER CHECK (input_rate_nanos BETWEEN 0 AND 1000000000000000),
    output_rate_nanos INTEGER CHECK (output_rate_nanos BETWEEN 0 AND 1000000000000000),
    cache_read_rate_nanos INTEGER CHECK (cache_read_rate_nanos BETWEEN 0 AND 1000000000000000),
    cache_write_rate_nanos INTEGER CHECK (cache_write_rate_nanos BETWEEN 0 AND 1000000000000000),
    status TEXT NOT NULL CHECK (status IN ('pending','priced','default_zero','usage_incomplete','unsupported_breakdown','not_applicable','overflow','legacy_unknown')),
    input_cost_nanos INTEGER CHECK (input_cost_nanos >= 0),
    output_cost_nanos INTEGER CHECK (output_cost_nanos >= 0),
    cache_read_cost_nanos INTEGER CHECK (cache_read_cost_nanos >= 0),
    cache_write_cost_nanos INTEGER CHECK (cache_write_cost_nanos >= 0),
    total_cost_nanos INTEGER CHECK (total_cost_nanos >= 0),
    pricing_version INTEGER NOT NULL DEFAULT 2 CHECK (pricing_version > 0),
    calculated_at_ms INTEGER NOT NULL CHECK (calculated_at_ms >= 0),
    CHECK (price_source = 'legacy_unknown' OR (
        price_snapshot_at_ms IS NOT NULL AND input_mode IS NOT NULL
        AND input_rate_nanos IS NOT NULL AND output_rate_nanos IS NOT NULL
        AND cache_read_rate_nanos IS NOT NULL AND cache_write_rate_nanos IS NOT NULL)),
    CHECK (price_source <> 'default_zero' OR (
        input_rate_nanos IS 0 AND output_rate_nanos IS 0
        AND cache_read_rate_nanos IS 0 AND cache_write_rate_nanos IS 0
        AND source_price_id IS NULL AND source_price_revision IS NULL AND input_mode = 'flat')),
    CHECK (status <> 'default_zero' OR price_source = 'default_zero'),
    CHECK (price_source <> 'default_zero' OR status IN ('pending','default_zero','not_applicable')),
    CHECK ((status IN ('priced','default_zero','not_applicable','legacy_unknown')
        AND input_cost_nanos IS NOT NULL AND output_cost_nanos IS NOT NULL
        AND cache_read_cost_nanos IS NOT NULL AND cache_write_cost_nanos IS NOT NULL
        AND total_cost_nanos IS NOT NULL
        AND total_cost_nanos = input_cost_nanos + output_cost_nanos + cache_read_cost_nanos + cache_write_cost_nanos)
        OR (status IN ('pending','usage_incomplete','unsupported_breakdown','overflow')
            AND total_cost_nanos IS NULL AND input_cost_nanos IS NULL AND output_cost_nanos IS NULL
            AND cache_read_cost_nanos IS NULL AND cache_write_cost_nanos IS NULL)),
    CHECK (status NOT IN ('default_zero','not_applicable') OR total_cost_nanos IS 0)
);

-- Mapping, per PRICING-DECISION section 6:
--   * priced            -> manual, unit prices copied from the rule it used,
--                          original tokens and amounts untouched;
--   * unpriced          -> the new "no price configured" default: zero prices
--                          and a confirmed zero amount;
--   * not_applicable    -> still not applicable, zero amount, default snapshot;
--   * incomplete rows   -> keep their status and unknown amount; the rule's
--                          prices are copied when one is known, otherwise the
--                          row is marked legacy_unknown rather than inventing
--                          zero unit prices;
--   * amounts that exist but whose prices cannot be recovered keep their amount
--                          and are marked legacy_unknown.
INSERT INTO usage_costs_v2(
        request_id, source_price_id, source_price_revision, price_source, price_snapshot_at_ms,
        currency, input_mode, input_rate_nanos, output_rate_nanos,
        cache_read_rate_nanos, cache_write_rate_nanos,
        status, input_cost_nanos, output_cost_nanos, cache_read_cost_nanos,
        cache_write_cost_nanos, total_cost_nanos, pricing_version, calculated_at_ms)
SELECT c.request_id,
       CASE WHEN c.status IN ('priced') OR r.rule_id IS NOT NULL THEN c.rule_id END,
       NULL,
       CASE
           WHEN c.status = 'priced' THEN 'manual'
           WHEN c.status IN ('unpriced', 'not_applicable') THEN 'default_zero'
           WHEN r.rule_id IS NOT NULL THEN 'manual'
           ELSE 'legacy_unknown'
       END,
       CASE WHEN c.status IN ('unpriced', 'not_applicable') THEN c.calculated_at_ms
            ELSE COALESCE(r.created_at_ms, c.calculated_at_ms) END,
       COALESCE(c.currency, (SELECT s.default_currency FROM stats_settings s WHERE s.singleton = 1), 'CNY'),
       CASE
           WHEN c.status IN ('unpriced', 'not_applicable') THEN 'flat'
           ELSE COALESCE(r.input_mode, 'flat')
       END,
       CASE
           WHEN c.status IN ('unpriced', 'not_applicable') THEN 0
           WHEN r.rule_id IS NOT NULL THEN r.input_rate_nanos
           ELSE NULL
       END,
       CASE
           WHEN c.status IN ('unpriced', 'not_applicable') THEN 0
           WHEN r.rule_id IS NOT NULL THEN r.output_rate_nanos
           ELSE NULL
       END,
       CASE
           WHEN c.status IN ('unpriced', 'not_applicable') THEN 0
           WHEN r.rule_id IS NOT NULL THEN COALESCE(r.cache_read_rate_nanos, 0)
           ELSE NULL
       END,
       CASE
           WHEN c.status IN ('unpriced', 'not_applicable') THEN 0
           WHEN r.rule_id IS NOT NULL THEN COALESCE(r.cache_write_rate_nanos, 0)
           ELSE NULL
       END,
       CASE
           WHEN c.status = 'priced' THEN 'priced'
           WHEN c.status = 'unpriced' THEN 'default_zero'
           WHEN c.status = 'not_applicable' THEN 'not_applicable'
           ELSE c.status
       END,
       CASE WHEN c.status IN ('priced', 'unpriced', 'not_applicable')
            THEN COALESCE(c.input_cost_nanos, 0) END,
       CASE WHEN c.status IN ('priced', 'unpriced', 'not_applicable')
            THEN COALESCE(c.output_cost_nanos, 0) END,
       CASE WHEN c.status IN ('priced', 'unpriced', 'not_applicable')
            THEN COALESCE(c.cache_read_cost_nanos, 0) END,
       CASE WHEN c.status IN ('priced', 'unpriced', 'not_applicable')
            THEN COALESCE(c.cache_write_cost_nanos, 0) END,
       CASE WHEN c.status IN ('priced', 'unpriced', 'not_applicable')
            THEN COALESCE(c.total_cost_nanos, 0) END,
       2,
       c.calculated_at_ms
FROM usage_costs c
LEFT JOIN stats_price_rules r ON r.rule_id = c.rule_id;

DROP TABLE usage_costs;
ALTER TABLE usage_costs_v2 RENAME TO usage_costs;

-- 4. The editable current-price table replaces the interval rules entirely.
DROP TABLE stats_price_rules;

-- 5. Recreate the reporting view over the rebuilt table, unchanged in shape.
CREATE VIEW stats_request_facts AS
SELECT r.*,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN 1 ELSE 0 END AS counted_usage,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.input_tokens END AS counted_input_tokens,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.output_tokens END AS counted_output_tokens,
    CASE WHEN r.usage_status = 'complete' AND r.outcome <> 'in_flight' THEN r.total_tokens END AS counted_total_tokens,
    c.status AS cost_status, c.currency, c.total_cost_nanos
FROM usage_requests r LEFT JOIN usage_costs c ON c.request_id = r.request_id;
