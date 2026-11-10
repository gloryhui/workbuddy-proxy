//! models.dev 公共价格目录的下载、定点解析与本地模型匹配。
//!
//! 正常链路是 `Bridge::sync_prices` 决定何时刷新，`fetch` 从网络读取目录；解析放到
//! `spawn_blocking` 中完成，避免把大 JSON 的相对 CPU 工作放在异步请求 future 上。随后
//! `stats::store::import_price_catalog` 在 stats-writer 线程调用 [`Prices::find`] 把配置
//! 目标映射成当前价格表。本模块只处理目录和费率，不计算单次请求费用，也不负责数据库事务
//! 或把价格缓存发布给 GUI。
//!
//! Java 类比上，它更像一个无框架的目录解析器/查找服务，而不是 Spring 单例 Bean；这里没有
//! 容器注入，生命周期和跨线程共享由调用者显式持有引用决定。
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 一条目录模型的价格费率，单位均为 nano 货币单位/1,000,000 token。
///
/// 货币由目录约定为 USD；汇率换算不在本模块发生。`input`/`output` 必须存在；
/// `read`/`write` 的 `None` 表示目录未提供该 cache 分项，`Some(0)` 才表示明确的零价。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rates {
    /// 普通输入费率；目录 JSON 中的 `2` 会变成 `2_000_000_000`，即 2 USD/M token。
    pub input: i64,
    /// 输出费率。
    pub output: i64,
    /// cache read 费率；`None` 表示目录未提供，不等同免费。
    pub read: Option<i64>,
    /// cache write 费率；`None` 表示目录未提供，不等同免费。
    pub write: Option<i64>,
}

impl Rates {
    /// 根据目录字段推导已支持的三类输入计价方式。
    ///
    /// 优先级由字段结构决定：只要 `write` 存在就使用 `disjoint`；否则有 `read` 时使用
    /// `read_discount`；两者都缺失才是 `flat`。同一个函数返回静态字符串，供数据库和请求
    /// 快照沿用。
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

/// 目录中的一个 provider/model 条目及其匹配辅助字段。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    /// models.dev 的 provider/lab 标识，用于生成可读标签和 canonical 来源匹配。
    pub provider: String,
    /// provider 下的模型标识。
    pub model: String,
    /// 可选 canonical 标识；较新目录用它把自定义网关映射到原始模型身份。
    pub canonical: Option<String>,
    /// provider 的 API endpoint；匹配自定义网关时会比较规范化 endpoint。
    pub api: String,
    /// 该目录条目的四项费率字段：input、output、cache read、cache write。
    pub rates: Rates,
}

/// 已解析的价格目录。外层 `Vec` 保存目录中出现且通过基本完整性检查的全部条目。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Prices(pub Vec<Entry>);

/// 一次价格导入需要匹配的代理侧目标。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    /// 配置中的 provider id，作为最终价格表键的一半。
    pub provider_id: String,
    /// 该 provider 的上游 URL，用于 endpoint 精确匹配。
    pub upstream: String,
    /// 客户端/代理配置的上游模型名，用于模型与 canonical 匹配。
    pub model: String,
}

/// 把 JSON 数字（包括小数和指数形式）转换为整数 nano 费率，全程不经过 `f64`。
///
/// 输入来自 models.dev 的 `cost.input` 等字段。函数先拆出十进制尾数和 10 的指数，再用
/// `i128` 定点计算：负数、非数字、超出 `i64` 或 `MAX_RATE_NANOS` 的值返回 `None`。
/// 小数按“第 10 位半入”到 9 位小数，例如 `1.2345678915` 变成 `1_234_567_892` nano。
/// 这样 1 nano 货币单位是精确最小单位，避免二进制浮点把金额舍入漂移。
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
    /// 解析 models.dev JSON 的字节内容，并丢弃不完整或越界的目录条目。
    ///
    /// 顶层必须是 provider 对象；没有 `models` 的 provider、空/超长模型名、缺少有效
    /// input/output 的模型会被跳过。可选 cache 字段为 `null`/缺失时保持 `None`，但一旦
    /// 出现负值、非数字或超上限，就跳过整个模型条目，避免把坏 cache 价猜成 0。
    /// 至少保留一个有效条目；空目录或顶层 JSON 无效返回错误字符串。
    ///
    /// 返回 [`Prices`] 由调用者拥有，可存入 `stats_price_catalog.catalogue_json` 的同进程
    /// 数据副本；它和导入后的 `stats_model_prices` 不是同一个内存对象。
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

    /// 为代理目标选择当前目录中的一个价格条目，并按固定优先级消除歧义。
    ///
    /// 第一轮候选要求目录 `model`、`canonical` 或 `provider/model` 与目标模型文本一致。
    /// 选择顺序是：
    ///
    /// 1. 目标 upstream 与条目 `api` 的规范化 endpoint 相同；
    /// 2. 候选只给出一个明确 canonical 身份时，回到该 canonical 所属原始 lab；
    /// 3. 对 `provider/model` 这种完全限定模型名，按 provider + model 回退；
    /// 4. 最后才考虑剩余同名候选。
    ///
    /// 每一步的候选必须费率完全一致才会返回；同 endpoint/名称存在多个不同费率时返回
    /// `None`，宁可不自动定价，也不猜一个 reseller。这里只返回借用条目，落库和缓存发布由
    /// `store::import_price_catalog` 完成。
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

/// 仅当一组候选的完整 [`Rates`] 完全相同时接受第一项。
///
/// 该方法同时用于 endpoint、canonical 和最终名称候选。只比较四项费率字段；冲突候选
/// 返回 `None`，不会按列表顺序静默选中某个供应商。
fn agreed<'a>(entries: &[&'a Entry]) -> Option<&'a Entry> {
    let first = *entries.first()?;
    entries
        .iter()
        .all(|entry| entry.rates == first.rates)
        .then_some(first)
}

/// 下载并解析 models.dev 目录。
///
/// 调用者是显式的每日/强制价格同步流程。HTTP 下载上限为 16 MiB、超时 30 秒；测试环境可
/// 用 loopback 测试 URL 覆盖。响应字节通过 `move` 闭包交给 blocking 线程解析，避免在
/// async future 上同步解析大 JSON。失败只返回 `String`，不会触碰 SQLite；成功返回的
/// [`Prices`] 仍需由 `import_price_catalog` 在 writer 线程提交。
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
