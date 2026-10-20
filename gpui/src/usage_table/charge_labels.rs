//! Friendly names for standalone charges. Routing/filter keys stay unchanged.
use crate::stats_dto::{RecordOrigin, RequestRowDto, Source};

fn charge_source(origin: RecordOrigin, source: Source, model: &str) -> Option<&str> {
    (origin.is_imported() && source == Source::JsonlImport)
        .then(|| model.strip_prefix("workbuddy-credit:"))
        .flatten()
}

pub fn is_tool_charge(item: &RequestRowDto) -> bool {
    charge_source(item.origin, item.source, &item.requested_model) == Some("generation_tool")
}

pub fn is_charge(item: &RequestRowDto) -> bool {
    charge_source(item.origin, item.source, &item.requested_model).is_some()
}

fn charge_name(source: &str, tool: Option<&str>, zh: bool) -> String {
    let text = |cn, en| if zh { cn } else { en };
    match source {
        "generation_tool" => match tool {
            Some(name) => {
                let kind = match name {
                    "ImageGen" => Some(text("图片生成", "Image generation")),
                    "ImageEdit" => Some(text("图片编辑", "Image editing")),
                    "VideoGen" => Some(text("视频生成", "Video generation")),
                    "AudioTranscribe" => Some(text("音频转录", "Audio transcription")),
                    _ => None,
                };
                kind.map_or_else(|| name.to_owned(), |kind| format!("{kind} ({name})"))
            }
            None => text("生成工具", "Generation tool").into(),
        },
        "team_member" => text("团队子代理", "Team subagent").into(),
        _ => text("其他收费项", "Other charge").into(),
    }
}

pub fn model_text(item: &RequestRowDto, zh: bool) -> String {
    match charge_source(item.origin, item.source, &item.requested_model) {
        Some(source) => charge_name(
            source,
            (item.route_name != item.requested_model && !item.route_name.is_empty())
                .then_some(item.route_name.as_str()),
            zh,
        ),
        None => item.upstream_model.clone(),
    }
}

pub fn route_text(item: &RequestRowDto, zh: bool) -> String {
    if is_charge(item) {
        model_text(item, zh)
    } else {
        item.route_name.clone()
    }
}

pub fn requested_text(item: &RequestRowDto, zh: bool) -> String {
    if is_charge(item) {
        model_text(item, zh)
    } else {
        item.requested_model.clone()
    }
}

/// Summary/filter rows have a category key, rather than a per-call tool name.
pub fn model_label(value: &str, zh: bool) -> String {
    value
        .strip_prefix("workbuddy-credit:")
        .map_or_else(|| value.to_owned(), |source| charge_name(source, None, zh))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_rows_use_the_saved_tool_snapshot_without_changing_normal_models() {
        let mut row: RequestRowDto = serde_json::from_value(serde_json::json!({
            "id":1,"request_id":"charge","source":"workbuddy_import",
            "provider_id":"wb:builtin","provider_name":"WorkBuddy","route_id":"wb:route:tool",
            "route_name":"ImageGen","requested_model":"workbuddy-credit:generation_tool",
            "upstream_model":"workbuddy-credit:generation_tool","started_at_ms":1,
            "outcome":"recorded","usage_status":"missing","usage_source":"workbuddy_jsonl",
            "input_breakdown":"unknown","record_origin":"workbuddy_builtin"
        }))
        .unwrap();
        assert!(is_tool_charge(&row));
        assert_eq!(model_text(&row, true), "图片生成 (ImageGen)");
        assert_eq!(requested_text(&row, true), "图片生成 (ImageGen)");
        assert_eq!(route_text(&row, true), "图片生成 (ImageGen)");
        row.route_name = row.requested_model.clone();
        assert_eq!(model_text(&row, true), "生成工具");
        row.requested_model = "deepseek-v4.1-flash".into();
        row.upstream_model = row.requested_model.clone();
        row.route_name = "DeepSeek".into();
        assert!(!is_tool_charge(&row));
        assert_eq!(model_text(&row, true), "deepseek-v4.1-flash");
        assert_eq!(route_text(&row, true), "DeepSeek");
    }

    #[test]
    fn concrete_tools_and_old_or_unknown_charges_have_readable_names() {
        assert_eq!(
            charge_name("generation_tool", Some("ImageGen"), true),
            "图片生成 (ImageGen)"
        );
        assert_eq!(
            charge_name("generation_tool", Some("VideoGen"), false),
            "Video generation (VideoGen)"
        );
        assert_eq!(
            charge_name("generation_tool", Some("FutureTool"), true),
            "FutureTool"
        );
        assert_eq!(
            model_label("workbuddy-credit:generation_tool", true),
            "生成工具"
        );
        assert_eq!(
            model_label("workbuddy-credit:team_member", true),
            "团队子代理"
        );
        assert_eq!(
            model_label("workbuddy-credit:future", false),
            "Other charge"
        );
        assert_eq!(
            model_label("deepseek-v4.1-flash", true),
            "deepseek-v4.1-flash"
        );
    }

    #[test]
    fn only_actual_imported_generation_charges_get_the_tool_badge() {
        let key = "workbuddy-credit:generation_tool";
        assert_eq!(
            charge_source(RecordOrigin::WorkbuddyBuiltin, Source::JsonlImport, key),
            Some("generation_tool")
        );
        assert_eq!(
            charge_source(RecordOrigin::WorkbuddyCustom, Source::JsonlImport, key),
            Some("generation_tool")
        );
        assert_eq!(charge_source(RecordOrigin::Proxy, Source::Relay, key), None);
        assert_eq!(
            charge_source(
                RecordOrigin::WorkbuddyBuiltin,
                Source::JsonlImport,
                "deepseek-v4.1-flash"
            ),
            None
        );
        assert_ne!(
            charge_source(
                RecordOrigin::WorkbuddyBuiltin,
                Source::JsonlImport,
                "workbuddy-credit:team_member"
            ),
            Some("generation_tool")
        );
    }
}
