//! Contract boundaries used by the native form, including unsafe responses.
use super::{TaskView, parse_range, replies::preview, timestamp};
use serde_json::json;

#[test]
fn editable_local_times_preserve_milliseconds_and_exclusive_end() {
    let from = 1_792_205_810_123;
    let to = from + 1;
    assert_eq!(
        parse_range(&timestamp(from), &timestamp(to)),
        Ok((from, to))
    );
    for (start, end) in [
        (timestamp(from), timestamp(from)),
        (timestamp(to), timestamp(from)),
        ("invalid".into(), timestamp(to)),
        (timestamp(from), "2026-02-30 12:00:00.000".into()),
    ] {
        assert!(parse_range(&start, &end).is_err());
    }
}

#[test]
fn unsafe_preview_replies_never_enable_confirmation() {
    let valid = json!({"token":"ab".repeat(24),"affected_count":1,
        "expires_at_ms":1_792_205_870_000_i64,"mapping_revision":1,"watermark_rowid":2});
    assert!(preview(&Ok(valid.clone())).is_ok());
    for (key, bad) in [
        ("token", json!("short")),
        ("token", json!("z".repeat(48))),
        ("affected_count", json!(-1)),
        ("expires_at_ms", json!(0)),
        ("mapping_revision", json!(0)),
        ("watermark_rowid", json!(-1)),
    ] {
        let mut value = valid.clone();
        value[key] = bad;
        assert!(preview(&Ok(value)).is_err(), "{key}");
    }
    assert!(preview(&Err("stats_workbuddy_stale_batch".into())).is_err());
}

#[test]
fn invalid_task_progress_is_rejected_instead_of_showing_success() {
    let valid = json!({"task_id":"manual:fixture","state":"queued","affected_count":2});
    assert!(
        serde_json::from_value::<TaskView>(valid.clone())
            .unwrap()
            .valid()
    );
    for (key, bad) in [
        ("task_id", json!("")),
        ("task_id", json!("unsafe\nidentifier")),
        ("state", json!("success")),
        ("affected_count", json!(-1)),
        ("processed_count", json!(3)),
    ] {
        let mut value = valid.clone();
        value[key] = bad;
        assert!(
            !serde_json::from_value::<TaskView>(value).unwrap().valid(),
            "{key}"
        );
    }
}
