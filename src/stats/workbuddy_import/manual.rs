//! Compatibility refusals for retired manual model attribution commands.
use rusqlite::Connection;
use serde_json::Value;
#[derive(Default)]
pub(crate) struct PreviewCache {
    _retired: (),
}
fn automatic() -> Result<String, String> {
    Err(crate::stats::dto::ERR_INVALID.into())
}
pub(super) fn preview(
    _: &mut Connection,
    _: &Value,
    _: i64,
    _: &mut PreviewCache,
) -> Result<String, String> {
    automatic()
}
pub(super) fn revoke_preview(
    _: &mut Connection,
    _: &Value,
    _: i64,
    _: &mut PreviewCache,
) -> Result<String, String> {
    automatic()
}
pub(super) fn apply(
    _: &mut Connection,
    _: &Value,
    _: i64,
    _: &mut PreviewCache,
    _: bool,
) -> Result<String, String> {
    automatic()
}
pub(super) fn contract_check() -> Result<usize, &'static str> {
    if automatic().is_ok() {
        return Err("automatic_attribution_mutation_allowed");
    }
    Ok(1)
}
pub(super) mod rules {
    pub(in crate::stats::workbuddy_import) fn page(
        _: &mut rusqlite::Connection,
        _: &serde_json::Value,
    ) -> Result<String, String> {
        Ok(serde_json::json!({"api_version":crate::stats::dto::API_VERSION,"rules":[],"next_cursor":null,"mapping_revision":1}).to_string())
    }
}
#[cfg(test)]
mod tests;
