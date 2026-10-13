//! Small static catalog: no locale framework or resources loaded at runtime.
pub fn message(text: &str, zh: bool) -> String {
    let prefix = if text.starts_with("OK ") {
        "OK "
    } else if text.starts_with("ERROR ") {
        "ERROR "
    } else {
        ""
    };
    let text = text.strip_prefix(prefix).unwrap_or(text);
    if let Some((a, b)) = text.split_once(" / ") {
        let chinese = |s: &str| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        let selected = if chinese(a) == zh { a } else { b };
        return format!("{prefix}{selected}");
    }
    let translated = if zh {
        match text {
            "WorkBuddy Proxy startup failed" => "WorkBuddy Proxy 启动失败",
            "Update check failed; check network" => "更新检查失败，请检查网络",
            "Update check timed out" => "更新检查超时",
            "Update check already running" => "正在检查更新",
            "Invalid release response" => "更新服务返回无效响应",
            "Invalid release version" => "发布版本号无效",
            "Expected a stable release" => "未找到正式发布版本",
            "Ready" => "就绪",
            "Saved" => "已保存",
            "Language saved" => "语言已保存",
            "Internal error" => "内部错误",
            "Service is not ready" => "服务尚未就绪",
            "Enter upstream API key" => "请填写上游 API Key",
            "Model ID is required" => "请填写模型 ID",
            "Invalid upstream URL" => "上游地址格式不正确",
            "Invalid thinking effort" => "思考强度无效",
            "Invalid language" => "语言选项无效",
            "At most 256 models" => "最多支持 256 个模型",
            "At most 256 models; upstream returned too many" => {
                "上游返回超过 256 个模型，请手动选择所需模型"
            }
            "Connection failed; check network and proxy" => "连接失败，请检查网络和代理",
            "Connection timeout" => "连接超时",
            "Four requests are already in flight" => "已有四个请求正在处理，请稍后重试",
            "Invalid model list" => "上游模型列表格式无效",
            "Invalid model ID" => "模型 ID 无效",
            "Expected OpenAI models data array" => "上游未返回 OpenAI 格式的模型列表",
            "Model list exceeds 1 MiB" => "模型列表超过 1 MiB 大小限制",
            "No available models returned" => "上游未返回可用模型",
            "Add models first" => "请先添加模型",
            "Core startup timed out" => "后台核心启动超时",
            "Invalid core response" => "后台核心返回无效响应",
            "Invalid upstream stream" => "上游流式响应无效",
            "Invalid upstream response" => "上游响应无效",
            "Upstream returned an error" => "上游返回错误",
            "No valid streaming response" => "未收到有效流式响应",
            "Unknown provider" => "供应商不存在",
            "WorkBuddy configuration exceeds 2 MiB" => "WorkBuddy 配置超过 2 MiB",
            "Invalid WorkBuddy models.json; no file changed" => {
                "WorkBuddy 模型配置无效，文件未修改"
            }
            "Unsupported WorkBuddy configuration format" => "不支持此 WorkBuddy 配置格式",
            "Duplicate model IDs; refusing to overwrite" => "模型标识重复，未覆盖配置",
            "Model alias already exists in WorkBuddy; refusing to overwrite" => {
                "WorkBuddy 已存在相同标识，未覆盖配置"
            }
            "Model was removed; restore manually from backup" => "模型已被移除，请从备份恢复",
            _ => text,
        }
    } else {
        text
    };
    if zh {
        if let Some(code) = text.strip_prefix("Update check HTTP ") {
            return format!("{prefix}更新检查返回 HTTP {code}");
        }
        if let Some(code) = text.strip_prefix("Upstream HTTP ") {
            return format!("{prefix}上游返回 HTTP {code}");
        }
        if let Some(count) = text.strip_prefix("Fetched ") {
            return format!("{prefix}已获取 {count}").replace(" models", " 个模型");
        }
    }
    format!("{prefix}{translated}")
}
#[cfg(test)]
mod tests {
    #[test]
    fn no_bilingual_messages() {
        assert_eq!(
            super::message("OK 已保存供应商和模型 / Saved", false),
            "OK Saved"
        );
        assert_eq!(
            super::message("ERROR Network error / 网络错误", true),
            "ERROR 网络错误"
        );
        assert_eq!(
            super::message("Upstream HTTP 401", true),
            "上游返回 HTTP 401"
        );
    }
}
