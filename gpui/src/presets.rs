//! Fixed OpenAI Chat endpoint presets, with provenance in docs/PROVIDER-PRESETS.md.
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub name_zh: String,
    pub url: String,
}
impl Preset {
    pub fn title(&self, zh: bool) -> &str {
        if zh { &self.name_zh } else { &self.name }
    }
}
pub fn all() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    PRESETS.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/provider-presets.json"))
            .expect("Invalid bundled provider presets")
    })
}
pub fn matching_url(url: &str) -> &'static str {
    all()
        .iter()
        .find(|p| p.url.trim_end_matches('/') == url.trim().trim_end_matches('/'))
        .map_or("custom", |p| p.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preset_paths_are_complete_compatible_bases_and_ids_are_unique() {
        let mut ids = std::collections::HashSet::new();
        assert_eq!(all().len(), 76);
        for preset in all() {
            assert!(ids.insert(&preset.id));
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
            matching_url(" https://open.bigmodel.cn/api/coding/paas/v4/ "),
            "zhipu-coding"
        );
        assert_eq!(matching_url("https://my-server.example/v1"), "custom");
    }
}
