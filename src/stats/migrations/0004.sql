-- Protocol metadata is additive. Frozen unit prices and historical costs are
-- deliberately untouched; old rows were all collected by the Chat adapter.
ALTER TABLE usage_requests ADD COLUMN upstream_protocol TEXT NOT NULL
    DEFAULT 'openai_chat'
    CHECK(upstream_protocol IN ('openai_chat','openai_responses',
        'anthropic_messages','gemini_generate_content'));
