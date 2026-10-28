//! Bounded, local-only filtering of the visible provider names.
use crate::{config::Provider, state::Drafts};

pub fn display_name<'a>(provider: &'a Provider, drafts: &'a Drafts) -> &'a str {
    drafts
        .providers
        .get(&provider.id)
        .map(|draft| draft.name.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(&provider.name)
}

pub fn filtered<'a>(providers: &'a [Provider], drafts: &Drafts, query: &str) -> Vec<&'a Provider> {
    let query = query.trim().to_lowercase();
    providers
        .iter()
        .filter(|provider| {
            display_name(provider, drafts)
                .to_lowercase()
                .contains(&query)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Draft;

    #[test]
    fn supported_list_sizes_keep_order_and_reveal_every_row_after_clearing() {
        for count in [0, 1, 2, 20, 64] {
            let providers: Vec<_> = (0..count)
                .map(|index| Provider {
                    id: format!("id-{index}"),
                    name: format!("Provider {index:02}"),
                    ..Default::default()
                })
                .collect();
            let drafts = Drafts::default();
            let expected: Vec<_> = providers.iter().map(|provider| &provider.id).collect();
            assert_eq!(
                filtered(&providers, &drafts, " ")
                    .iter()
                    .map(|provider| &provider.id)
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(filtered(&providers, &drafts, "missing").is_empty());
            assert_eq!(filtered(&providers, &drafts, "").len(), count);
            if count == 64 {
                assert_eq!(
                    filtered(&providers, &drafts, "  PROVIDER 63 ")[0].id,
                    "id-63"
                );
            }
        }
    }

    #[test]
    fn chinese_and_case_insensitive_names_use_the_visible_draft_name() {
        let provider = Provider {
            name: "Saved name".into(),
            ..Default::default()
        };
        let mut drafts = Drafts::default();
        drafts.providers.insert(
            provider.id.clone(),
            Draft {
                name: "临时 OpenAI".into(),
                ..Default::default()
            },
        );
        let providers = [provider];
        assert_eq!(filtered(&providers, &drafts, " 临时 openai ").len(), 1);
        assert!(filtered(&providers, &drafts, "Saved name").is_empty());
        drafts
            .providers
            .get_mut(&providers[0].id)
            .unwrap()
            .name
            .clear();
        assert_eq!(filtered(&providers, &drafts, "saved name").len(), 1);
    }

    #[test]
    fn keys_urls_and_internal_ids_are_never_search_targets() {
        let providers = [Provider {
            id: "secret-id".into(),
            name: "Public name".into(),
            api_key: "secret-key".into(),
            upstream: "https://secret-url.example.test/v1".into(),
            ..Default::default()
        }];
        assert!(filtered(&providers, &Drafts::default(), "secret").is_empty());
    }
}
