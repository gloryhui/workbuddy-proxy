//! API-key provider presets, with provenance in docs/PROVIDER-PRESETS.md.
use crate::upstream_config::{Authentication, ProtocolSettings, UpstreamProtocol};
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub name_zh: String,
    pub url: String,
    #[serde(default)]
    pub protocol: UpstreamProtocol,
    #[serde(default)]
    pub authentication: Authentication,
}
impl Preset {
    pub fn title(&self, zh: bool) -> &str {
        if zh { &self.name_zh } else { &self.name }
    }
    pub fn apply_connection(&self, draft: &mut crate::state::Draft) {
        if self.authentication != Authentication::AzureApiKey || !draft.protocol_settings.is_azure()
        {
            draft.upstream = self.url.clone();
        }
        draft.protocol = self.protocol;
        draft.protocol_settings = ProtocolSettings {
            authentication: self.authentication,
            ..Default::default()
        };
    }
}
pub fn all() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/provider-presets.json"))
            .expect("Invalid bundled provider presets")
    })
}
pub fn matching_url(
    url: &str,
    protocol: UpstreamProtocol,
    settings: &ProtocolSettings,
) -> &'static str {
    if settings.is_azure() {
        return "azure";
    }
    all()
        .iter()
        .find(|p| {
            !p.url.is_empty()
                && p.protocol == protocol
                && p.url.trim_end_matches('/') == url.trim().trim_end_matches('/')
        })
        .map_or("custom", |p| p.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn applying_presets_keeps_credentials_and_models_and_requires_an_azure_resource() {
        let azure = all().iter().find(|p| p.id == "azure").unwrap();
        let anthropic = all().iter().find(|p| p.id == "anthropic").unwrap();
        let models = vec![serde_json::json!({"model":"existing","id":"keep"})];
        let mut draft = crate::state::Draft {
            key: "fixture-key".into(),
            upstream: "https://old-vendor.test/v1".into(),
            models: models.clone(),
            ..Default::default()
        };
        azure.apply_connection(&mut draft);
        assert!(draft.upstream.is_empty());
        assert!(draft.protocol_settings.is_azure());
        draft.upstream = "https://my-resource.openai.azure.com".into();
        azure.apply_connection(&mut draft);
        assert_eq!(draft.upstream, "https://my-resource.openai.azure.com");
        anthropic.apply_connection(&mut draft);
        assert_eq!(draft.upstream, "https://api.anthropic.com/v1");
        assert_eq!(draft.protocol, UpstreamProtocol::AnthropicMessages);
        assert!(!draft.protocol_settings.is_azure());
        assert_eq!(draft.key, "fixture-key");
        assert_eq!(draft.models, models);
    }
    #[test]
    fn new_presets_select_their_protocol_and_authentication() {
        let anthropic = all().iter().find(|p| p.id == "anthropic").unwrap();
        assert_eq!(anthropic.protocol, UpstreamProtocol::AnthropicMessages);
        assert_eq!(anthropic.authentication, Authentication::ProtocolDefault);
        assert_eq!(
            matching_url(
                &anthropic.url,
                anthropic.protocol,
                &ProtocolSettings::default()
            ),
            "anthropic"
        );
        assert_eq!(
            matching_url(
                &anthropic.url,
                UpstreamProtocol::OpenaiChat,
                &ProtocolSettings::default()
            ),
            "custom"
        );
        let azure = all().iter().find(|p| p.id == "azure").unwrap();
        let settings = ProtocolSettings {
            authentication: azure.authentication,
            ..Default::default()
        };
        assert_eq!(
            matching_url(
                "https://my-resource.openai.azure.com",
                UpstreamProtocol::OpenaiResponses,
                &settings
            ),
            "azure"
        );
        assert!(settings.is_azure());
        assert_eq!(
            all()
                .iter()
                .filter(|p| p.protocol == UpstreamProtocol::OpenaiChat
                    && p.authentication == Authentication::ProtocolDefault)
                .count(),
            76
        );
    }
    #[test]
    fn preset_paths_are_complete_compatible_bases_and_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        assert_eq!(all().len(), 78);
        for preset in all() {
            assert!(ids.insert(&preset.id));
            if preset.authentication == Authentication::AzureApiKey {
                assert_eq!(preset.id, "azure");
                assert!(preset.url.is_empty());
                continue;
            }
            let url = url::Url::parse(&preset.url).unwrap();
            assert!(!preset.name.is_empty() && !preset.name_zh.is_empty());
            assert!(
                url.scheme() == "https"
                    || matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
            );
            assert!(url.username().is_empty() && url.password().is_none());
            assert!(!preset.url.contains(['{', '}', '<', '>']));
        }
        assert_eq!(
            matching_url(
                " https://open.bigmodel.cn/api/coding/paas/v4/ ",
                UpstreamProtocol::OpenaiChat,
                &ProtocolSettings::default()
            ),
            "zhipu-coding"
        );
        assert_eq!(
            matching_url(
                "https://my-server.example/v1",
                UpstreamProtocol::OpenaiChat,
                &ProtocolSettings::default()
            ),
            "custom"
        );
    }
}
