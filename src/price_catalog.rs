//! Public model prices. Network and parsing run away from the forwarding path.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rates {
    pub input: i64,
    pub output: i64,
    pub read: Option<i64>,
    pub write: Option<i64>,
}

impl Rates {
    pub fn mode(&self) -> &'static str {
        if self.write.is_some() {
            "disjoint"
        } else if self.read.is_some() {
            "read_discount"
        } else {
            "flat"
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub provider: String,
    pub model: String,
    pub canonical: Option<String>,
    pub api: String,
    pub rates: Rates,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prices(pub Vec<Entry>);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub provider_id: String,
    pub upstream: String,
    pub model: String,
}

/// Convert the JSON decimal, including exponent notation, without float money.
fn rate(value: &Value) -> Option<i64> {
    let text = value.as_number()?.to_string();
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text.as_str(), 0), |(m, e)| {
            (m, e.parse::<i32>().unwrap_or(i32::MIN))
        });
    if mantissa.starts_with('-') {
        return None;
    }
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let coefficient = format!("{whole}{fraction}").parse::<i128>().ok()?;
    let scale = exponent
        .checked_add(9)?
        .checked_sub(fraction.len() as i32)?;
    let nanos = if scale >= 0 {
        coefficient.checked_mul(10i128.checked_pow(scale as u32)?)?
    } else {
        let divisor = 10i128.checked_pow(scale.checked_neg()? as u32)?;
        coefficient.checked_add(divisor / 2)? / divisor
    };
    let nanos = i64::try_from(nanos).ok()?;
    (nanos <= crate::stats::pricing::MAX_RATE_NANOS).then_some(nanos)
}

impl Prices {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let data: Value = serde_json::from_slice(bytes).map_err(|_| "Invalid price catalogue")?;
        let providers = data.as_object().ok_or("Invalid price catalogue")?;
        let mut entries = Vec::new();
        for (provider, record) in providers {
            let Some(models) = record.get("models").and_then(Value::as_object) else {
                continue;
            };
            for (model, record_model) in models {
                if model.is_empty() || model.len() > 160 {
                    continue;
                }
                let Some(cost) = record_model.get("cost") else {
                    continue;
                };
                let (Some(input), Some(output)) = (rate(&cost["input"]), rate(&cost["output"]))
                else {
                    continue;
                };
                let optional = |field: &str| -> Result<Option<i64>, ()> {
                    match cost.get(field) {
                        None | Some(Value::Null) => Ok(None),
                        Some(value) => rate(value).map(Some).ok_or(()),
                    }
                };
                let (Ok(read), Ok(write)) = (optional("cache_read"), optional("cache_write"))
                else {
                    continue;
                };
                entries.push(Entry {
                    provider: provider.clone(),
                    model: model.clone(),
                    canonical: record_model["canonical_model_id"]
                        .as_str()
                        .map(str::to_owned),
                    api: record["api"].as_str().unwrap_or("").to_owned(),
                    rates: Rates {
                        input,
                        output,
                        read,
                        write,
                    },
                });
            }
        }
        if entries.is_empty() {
            return Err("Price catalogue contains no valid prices".into());
        }
        Ok(Self(entries))
    }

    pub fn find(&self, upstream: &str, model: &str) -> Option<&Entry> {
        let matching: Vec<_> = self
            .0
            .iter()
            .filter(|entry| {
                entry.model == model
                    || entry.canonical.as_deref() == Some(model)
                    || format!("{}/{}", entry.provider, entry.model) == model
            })
            .collect();
        let endpoint = crate::catalog::endpoint(upstream);
        let direct: Vec<_> = matching
            .iter()
            .copied()
            .filter(|entry| endpoint.is_some() && crate::catalog::endpoint(&entry.api) == endpoint)
            .collect();
        if !direct.is_empty() {
            return agreed(&direct);
        }
        // A unique canonical identity lets a custom gateway use the lab's price.
        let canonical: std::collections::BTreeSet<_> = matching
            .iter()
            .filter_map(|entry| entry.canonical.as_deref())
            .collect();
        if canonical.len() == 1 {
            let canonical = *canonical.first()?;
            if let Some((lab, id)) = canonical.split_once('/') {
                let origin: Vec<_> = self
                    .0
                    .iter()
                    .filter(|entry| {
                        entry.provider == lab
                            && (entry.model == id || entry.canonical.as_deref() == Some(canonical))
                    })
                    .collect();
                if !origin.is_empty() {
                    return agreed(&origin);
                }
            }
        }
        // Older catalogues omit canonical IDs; a fully qualified ID is explicit.
        if let Some((lab, id)) = model.split_once('/') {
            let origin: Vec<_> = self
                .0
                .iter()
                .filter(|entry| entry.provider == lab && entry.model == id)
                .collect();
            if !origin.is_empty() {
                return agreed(&origin);
            }
        }
        agreed(&matching)
    }
}

fn agreed<'a>(entries: &[&'a Entry]) -> Option<&'a Entry> {
    let first = *entries.first()?;
    entries
        .iter()
        .all(|entry| entry.rates == first.rates)
        .then_some(first)
}

pub async fn fetch(client: &reqwest::Client) -> Result<Prices, String> {
    let url = std::env::var("WORKBUDDY_PROXY_CATALOG_TEST_URL")
        .ok()
        .filter(|s| {
            reqwest::Url::parse(s).ok().is_some_and(|u| {
                u.scheme() == "http" && matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
            })
        })
        .unwrap_or_else(|| "https://models.dev/api.json".into());
    let bytes = crate::catalog::download(
        client.get(url).header(
            "User-Agent",
            concat!("WorkBuddy-Proxy/", env!("CARGO_PKG_VERSION")),
        ),
        16 * 1024 * 1024,
        std::time::Duration::from_secs(30),
    )
    .await?;
    tokio::task::spawn_blocking(move || Prices::parse(&bytes))
        .await
        .map_err(|_| "Price catalogue parser failed".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decimal_prices_preserve_zero_small_values_and_reject_invalid_rates() {
        for (number, expected) in [
            (json!(0), Some(0)),
            (json!(0.075), Some(75_000_000)),
            (json!(1e-7), Some(100)),
            (json!(1.2345678915), Some(1_234_567_892)),
            (json!(-1), None),
            (json!(1000001), None),
            (json!("2"), None),
        ] {
            assert_eq!(rate(&number), expected, "{number}");
        }
        assert!(
            Prices::parse(br#"{"x":{"models":{"m":{"cost":{"input":-1,"output":2}}}}}"#).is_err()
        );
    }

    #[test]
    fn provider_prices_win_and_ambiguous_gateways_do_not_pick_a_reseller() {
        let prices = Prices::parse(&serde_json::to_vec(&json!({
            "lab":{"api":"https://lab.test/v1","models":{"m":{"canonical_model_id":"lab/m","cost":{"input":2,"output":4,"cache_read":0.2}}}},
            "reseller":{"api":"https://reseller.test/v1","models":{"m":{"canonical_model_id":"lab/m","cost":{"input":1,"output":3}}}}
        })).unwrap()).unwrap();
        assert_eq!(
            prices
                .find("https://reseller.test/v1", "m")
                .unwrap()
                .rates
                .input,
            1_000_000_000
        );
        assert_eq!(
            prices
                .find("https://gateway.test/v1", "m")
                .unwrap()
                .rates
                .input,
            2_000_000_000
        );
        assert_eq!(
            prices
                .find("https://gateway.test/v1", "lab/m")
                .unwrap()
                .rates
                .mode(),
            "read_discount"
        );
        assert!(
            prices
                .find("https://gateway.test/v1", "m-thinking")
                .is_none()
        );
        let mut ambiguous = prices;
        for entry in &mut ambiguous.0 {
            entry.canonical = None;
        }
        assert!(ambiguous.find("https://gateway.test/v1", "m").is_none());
    }
}
