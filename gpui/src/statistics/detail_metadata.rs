//! Normalized metadata shown above the existing usage detail rows.
use crate::stats_dto::{RecordOrigin, RequestRowDto, TimeBasis};

pub(super) fn fields(item: &RequestRowDto, zh: bool) -> Vec<(&'static str, String)> {
    metadata(
        item.origin,
        item.time_basis,
        item.total_tokens_derived,
        item.workbuddy_credit_nanos.as_deref(),
        item.stream,
        zh,
    )
}

fn metadata(
    origin: RecordOrigin,
    basis: TimeBasis,
    derived: bool,
    credits: Option<&str>,
    stream: Option<bool>,
    zh: bool,
) -> Vec<(&'static str, String)> {
    let text = |cn, en| if zh { cn } else { en };
    let capture = if origin.is_imported() {
        "WorkBuddy JSONL"
    } else {
        text("代理采集", "Proxy capture")
    };
    let basis = match basis {
        TimeBasis::RequestStart => text("请求开始时间", "Request start"),
        TimeBasis::MessageTimestamp => text("消息记录时间", "Message timestamp"),
    };
    vec![
        (
            text("模型来源", "Model source"),
            crate::usage_table::origin_text(origin, zh).into(),
        ),
        (text("采集方式", "Capture method"), capture.into()),
        (text("时间依据", "Time basis"), basis.into()),
        (
            text("总量口径", "Total basis"),
            if derived {
                text("由输入 + 输出推导总量", "Derived from input + output").into()
            } else {
                text("记录报告值", "Reported value").into()
            },
        ),
        (
            text("WorkBuddy 点数", "WorkBuddy credits"),
            credits
                .and_then(crate::usage_table::format_credit_nanos)
                .unwrap_or_else(|| "—".into()),
        ),
        (
            text("流式响应", "Streaming"),
            match stream {
                Some(true) => text("是", "Yes"),
                Some(false) => text("否", "No"),
                None => "—",
            }
            .into(),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::metadata;
    use crate::stats_dto::{RecordOrigin, TimeBasis};

    #[test]
    fn imported_metadata_preserves_credits_unknown_stream_and_message_time() {
        let rows = metadata(
            RecordOrigin::WorkbuddyBuiltin,
            TimeBasis::MessageTimestamp,
            true,
            Some("1140000000"),
            None,
            true,
        );
        assert!(rows.contains(&("采集方式", "WorkBuddy JSONL".into())));
        assert!(rows.contains(&("时间依据", "消息记录时间".into())));
        assert!(rows.contains(&("总量口径", "由输入 + 输出推导总量".into())));
        assert!(rows.contains(&("WorkBuddy 点数", "1.14".into())));
        assert!(rows.contains(&("流式响应", "—".into())));
        let rows = metadata(
            RecordOrigin::WorkbuddyCustom,
            TimeBasis::MessageTimestamp,
            false,
            Some("0"),
            None,
            false,
        );
        assert!(rows.contains(&("WorkBuddy credits", "0".into())));
        assert!(
            metadata(
                RecordOrigin::Proxy,
                TimeBasis::RequestStart,
                false,
                None,
                Some(false),
                false
            )
            .contains(&("WorkBuddy credits", "—".into()))
        );
    }
}
