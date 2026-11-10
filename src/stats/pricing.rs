//! 手工价格规则与整数费用估算。
//!
//! 金额全程不使用浮点数。费率单位是“每 1,000,000 token 的 nano 货币单位”，以 `i64`
//! 保存；每个费用分量独立做 half-up（四舍五入且正数恰好半单位进位），再用 `i128`
//! 相加并检查能否放回 `i64`。费率本身不随货币换算，`Currency` 决定同一 nano 数表示
//! USD 还是 CNY。
//!
//! Java 读者可把 `PriceRule` 看成不可变 DTO、把 `settle` 看成把 DTO 和 `UsageOutcome`
//! 映射成费用明细的服务函数；但这里没有 `BigDecimal` 对象，而是用固定的
//! “nano / 百万 token”标度和整数除法显式表达精度。
use crate::stats::collector::UsageOutcome;
use crate::stats::dto::{CostStatus, Currency, InputBreakdown, InputMode, UsageStatus};

/// 当前接受的最大费率：每 1,000,000 token 10^15 nano 货币单位。
pub const MAX_RATE_NANOS: i64 = 1_000_000_000_000_000;

/// 一次请求在准入时复制的价格快照。
///
/// `settle` 只使用传入快照，不按请求时间重新查询当前配置。正常调用方在请求准入时从
/// published price cache 复制 provider/model 对应的规则并随请求持久化，因此之后编辑或
/// 删除当前价格不会改变这条历史请求的费用。这里不做币种转换。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PriceRule {
    /// 仅用于追溯来源价格；计算时不会按该 id 联结可编辑的当前价格。
    pub rule_id: String,
    /// nano 金额所属币种。
    pub currency: Currency,
    /// 把 input tokens 分配到普通输入、缓存读取和缓存写入三个部分的方式。
    pub input_mode: InputMode,
    /// 每 1,000,000 个“普通输入”token 的 nano 费率；ReadDiscount/Disjoint 中用于
    /// uncached 部分，Flat 中用于全部 input。
    pub input_rate_nanos: i64,
    /// 每 1,000,000 个输出 token 的 nano 费率。
    pub output_rate_nanos: i64,
    /// 每 1,000,000 个缓存读取 token 的 nano 费率；`None` 表示快照没有可用费率，
    /// `Some(0)` 才表示配置确认缓存读取免费。
    pub cache_read_rate_nanos: Option<i64>,
    /// 每 1,000,000 个缓存写入 token 的 nano 费率；同样区分未知 `None` 与确认零。
    pub cache_write_rate_nanos: Option<i64>,
}

/// 一次请求的已确认费用明细，所有字段都是非负 nano 货币金额。
///
/// `input` 字段的业务含义随模式变化：Flat 中表示全部 input 的费用，
/// ReadDiscount/Disjoint 中表示未缓存 input 的费用。缓存读取和缓存写入费用各有独立
/// 分量。`total` 是四个分量分别舍入后再相加的结果，不是先加 token、再做一次舍入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CostParts {
    /// 未缓存输入费用；Flat 模式下它就是全部 input 的费用。
    pub input: i64,
    /// 全部 output token 的费用。
    pub output: i64,
    /// 缓存读取费用；Flat 模式下固定为 0，因为已包含在 input 分量中。
    pub cache_read: i64,
    /// 缓存写入费用；Flat 模式下固定为 0，因为已包含在 input 分量中。
    pub cache_write: i64,
    /// 以上四个已舍入分量之和。
    pub total: i64,
}

impl CostParts {
    /// 构造已确认的零金额，不能用来表示“金额未知”。
    ///
    /// 当前正常路径用它表达两种确定事实：请求没有发往上游，或准入快照明确选择了
    /// `default_zero`。未知额度使用 `parts: None` 并在持久化层写成 SQL NULL。
    pub fn zero() -> Self {
        Self {
            input: 0,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            total: 0,
        }
    }
}

/// `settle` 的完整输出：状态和可选的已确认费用明细。
///
/// `parts: Some(...)` 表示金额已确认，即使 `total == 0`；`parts: None` 表示费用未知或
/// 无法根据当前事实计算，持久化时不能改写成零。本模块不会自行返回
/// `CostStatus::DefaultZero`：未配置价格时，`store` 会在调用 `settle` 前识别
/// `default_zero` 快照并构造确认零。
///
/// 三个容易混淆的边界是：`NotApplicable` 表示可证明请求没有发往上游，因此是确认零；
/// `UsageIncomplete` 表示上游已启动但 usage 不够完整，金额保持未知；`DefaultZero` 是
/// store 对“没有配置价格”的显式零策略，即使 usage 缺失也不会伪造成 token 数。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settlement {
    /// 费用生命周期或计算失败类别。
    pub status: CostStatus,
    /// 已确认的分项金额；未知状态保持 `None`。
    pub parts: Option<CostParts>,
}

impl Settlement {
    /// 构造没有可用金额的非 Priced 结果。
    fn status(status: CostStatus) -> Self {
        Self {
            status,
            parts: None,
        }
    }
}

/// 计算并舍入一个费用分量：`round_half_up(tokens * rate / 1_000_000)`。
///
/// `rate` 的单位是每百万 token 的 nano 金额，因此用 1,000,000 作除数；50% 的除数
/// 是 500,000。先转成 `i128` 再检查乘法，避免两个 `i64` 直接相乘溢出；当前费率均
/// 非负，所以先加半个除数再整除等价于 half-up。结果仍需放回 `i64`，放不下时返回
/// `None` 给调用方转换成 `CostStatus::Overflow`。
///
/// Java 的 `BigDecimal.setScale(..., HALF_UP)` 能表达这个业务意图，但当前实现没有
/// 任意精度小数对象；它固定了标度，并用整数运算得到确定结果。
fn part(tokens: i64, rate: i64) -> Option<i64> {
    let product = (tokens as i128).checked_mul(rate as i128)?;
    let rounded = (product + 500_000) / 1_000_000;
    i64::try_from(rounded).ok()
}

/// 根据准入时价格快照和最终 usage 估算一次请求的费用。
///
/// 调用方通常是 `store::finish_request`，恢复流程也会用 Start 时保存的快照调用它。
/// `rule` 是已经取好的快照借用，不会按 `started_at_ms` 查找历史价格；`None` 表示调用
/// 方明确没有规则，但只有通过“上游已派发”和“usage 为 Complete”两个前置门禁后才会
/// 返回 Unpriced/`parts: None`。更早的门禁仍可能返回 NotApplicable 或
/// UsageIncomplete；`None` 不能等同于免费。正常当前配置没有价格时，store 使用外层
/// `default_zero` 分支，而不是把 `None` 传进这里。
///
/// 判定顺序固定为：
///
/// 1. `upstream_started == false` -> NotApplicable，并给出确认零；
/// 2. usage 不是 Complete -> UsageIncomplete，金额未知；
/// 3. 没有规则 -> Unpriced，金额未知；
/// 4. 规则存在但 input/output 缺失 -> UsageIncomplete；
/// 5. 按 input mode 校验拆分并计价，成功为 Priced，拆分不足为
///    UnsupportedBreakdown，整数范围不足为 Overflow。
///
/// 输入都由调用方借用，函数无 I/O、无持久化，也不修改价格或 usage。失败状态通过返回
/// `Settlement` 表达；`parts` 为 `None` 的 SQL 写入由 store 层负责。
pub fn settle(
    rule: Option<&PriceRule>,
    usage: &UsageOutcome,
    upstream_started: bool,
) -> Settlement {
    if !upstream_started {
        // The upstream was never called, so this request is a confirmed zero
        // rather than an unknown amount. A NULL here would violate the v2
        // `not_applicable` constraint and fail the whole settle transaction.
        return Settlement {
            status: CostStatus::NotApplicable,
            parts: Some(CostParts::zero()),
        };
    }
    if usage.status != UsageStatus::Complete {
        return Settlement::status(CostStatus::UsageIncomplete);
    }
    let Some(rule) = rule else {
        return Settlement::status(CostStatus::Unpriced);
    };
    let numbers = &usage.numbers;
    let (Some(input), Some(output)) = (numbers.input, numbers.output) else {
        return Settlement::status(CostStatus::UsageIncomplete);
    };
    let mut cache_read = 0i64;
    let mut cache_write = 0i64;
    let input_cost = match rule.input_mode {
        InputMode::Flat => {
            // Flat：全部 input 都按普通输入费率计价；cache 明细已经包含在 input 中，
            // 所以 cache_read/cache_write 分量保持确认零，不能再重复收费。
            part(input, rule.input_rate_nanos)
        }
        InputMode::ReadDiscount => {
            let Some(read) = numbers.cache_read else {
                // 已知读取量是应用折扣的必要条件，未知拆分不能猜成全命中或零命中。
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            let Some(read_rate) = rule.cache_read_rate_nanos else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            let Some(uncached) = input.checked_sub(read).filter(|v| *v >= 0) else {
                // cache_read 必须是 input 的子集；read > input 说明数据不能用于折扣。
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            cache_read = match part(read, read_rate) {
                Some(value) => value,
                None => return Settlement::status(CostStatus::Overflow),
            };
            // 标准 input 费率只覆盖未缓存部分。
            part(uncached, rule.input_rate_nanos)
        }
        InputMode::Disjoint => {
            // Disjoint 要求上游/解析层已经证明 read、write、uncached 三部分互斥。
            // 仅看到 cached_tokens 子集时不能自动升级成 Disjoint。
            if usage.breakdown != InputBreakdown::Disjoint {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            }
            let (Some(read), Some(write), Some(uncached)) =
                (numbers.cache_read, numbers.cache_write, usage.uncached)
            else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            let (Some(read_rate), Some(write_rate)) =
                (rule.cache_read_rate_nanos, rule.cache_write_rate_nanos)
            else {
                return Settlement::status(CostStatus::UnsupportedBreakdown);
            };
            cache_read = match part(read, read_rate) {
                Some(value) => value,
                None => return Settlement::status(CostStatus::Overflow),
            };
            cache_write = match part(write, write_rate) {
                Some(value) => value,
                None => return Settlement::status(CostStatus::Overflow),
            };
            part(uncached, rule.input_rate_nanos)
        }
    };
    // 输出始终独立按 output 费率计价；input/cache 各分量也已在上面分别舍入。
    let Some(input_cost) = input_cost else {
        return Settlement::status(CostStatus::Overflow);
    };
    let Some(output_cost) = part(output, rule.output_rate_nanos) else {
        return Settlement::status(CostStatus::Overflow);
    };
    // 每个分量最大约为 i64::MAX，四个分量转成 i128 后相加不会溢出；真正需要检查的是
    // 总和能否再次收窄为 i64，否则不能截断或回绕。
    let total =
        (input_cost as i128) + (output_cost as i128) + (cache_read as i128) + (cache_write as i128);
    let Ok(total) = i64::try_from(total) else {
        return Settlement::status(CostStatus::Overflow);
    };
    Settlement {
        status: CostStatus::Priced,
        parts: Some(CostParts {
            input: input_cost,
            output: output_cost,
            cache_read,
            cache_write,
            total,
        }),
    }
}

/// 解析 API 和持久层使用的 nano 费率字符串。
///
/// 输入是“每 1,000,000 token 的 nano 货币单位”的普通非负整数串：
/// `"2000000000"` 表示每百万 token 2 个货币单位，`"1"` 表示 1 nano。可以接受首尾
/// 空白和前导零，但空串、正负号、小数点、科学记数法、内部空白或超过
/// `MAX_RATE_NANOS` 的值都会被拒绝。此函数不会替调用方舍入，也不会把非法文本猜成零。
///
/// 设置界面中用户输入的 `"2"`、`"0.2"` 是人类金额，应先经过
/// [`crate::stats::dto::parse_human_amount`] 一次性转换成 nano 整数，再进入这里；
/// 重复转换会让单位不一致。
pub fn parse_rate(text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("rate must not be empty".into());
    }
    if !text.chars().all(|c| c.is_ascii_digit()) {
        return Err("rate must be a whole number of nano units".into());
    }
    let nanos: i64 = text.parse().map_err(|_| "rate is too large".to_string())?;
    if nanos > MAX_RATE_NANOS {
        return Err("rate is out of range".into());
    }
    Ok(nanos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::collector::UsageNumbers;

    fn usage(input: i64, output: i64, cache_read: Option<i64>) -> UsageOutcome {
        UsageOutcome {
            status: UsageStatus::Complete,
            reason: None,
            numbers: UsageNumbers {
                input: Some(input),
                output: Some(output),
                total: Some(input + output),
                cache_read,
                cache_write: None,
                reasoning: None,
            },
            breakdown: if cache_read.is_some() {
                InputBreakdown::ReadSubset
            } else {
                InputBreakdown::Unknown
            },
            uncached: None,
        }
    }

    fn rule(mode: InputMode, input: &str, output: &str, read: Option<&str>) -> PriceRule {
        PriceRule {
            rule_id: "price-a".into(),

            currency: Currency::Usd,
            input_mode: mode,
            input_rate_nanos: crate::stats::dto::parse_human_amount(input).unwrap(),
            output_rate_nanos: crate::stats::dto::parse_human_amount(output).unwrap(),
            cache_read_rate_nanos: read
                .map(|rate| crate::stats::dto::parse_human_amount(rate).unwrap()),
            cache_write_rate_nanos: None,
        }
    }

    #[test]
    fn api_rate_strings_are_nano_units_per_million_tokens() {
        // F03 regression: `API.md` fixes `*_rate_nanos` as an integer count of
        // nano currency units per million tokens, so "2000000000" means $2/M
        // and "1" means one nano unit (not one dollar).
        assert_eq!(parse_rate("2000000000").unwrap(), 2_000_000_000);
        assert_eq!(parse_rate("1").unwrap(), 1);
        assert_eq!(parse_rate("0").unwrap(), 0);
        assert_eq!(parse_rate("1000000000").unwrap(), 1_000_000_000);
        assert_eq!(
            parse_rate(&MAX_RATE_NANOS.to_string()).unwrap(),
            MAX_RATE_NANOS
        );
        // Fractional nano units are rejected instead of being guessed.
        for bad in [
            "0.2",
            "2.5",
            "1.0000000001",
            "-1",
            "1e3",
            "",
            "abc",
            " 1 2 ",
        ] {
            assert!(parse_rate(bad).is_err(), "{bad} must be rejected");
        }
        assert!(parse_rate(&(MAX_RATE_NANOS + 1).to_string()).is_err());
    }

    #[test]
    fn the_settings_form_converts_a_human_amount_exactly_once() {
        assert_eq!(
            crate::stats::dto::parse_human_amount("2").unwrap(),
            2_000_000_000
        );
        assert_eq!(
            crate::stats::dto::parse_human_amount("0.2").unwrap(),
            200_000_000
        );
        assert_eq!(
            crate::stats::dto::parse_human_amount("1.234567891").unwrap(),
            1_234_567_891
        );
        assert!(crate::stats::dto::parse_human_amount("1.2345678912").is_err());
        assert!(crate::stats::dto::parse_human_amount("-1").is_err());
        assert!(crate::stats::dto::parse_human_amount("1e3").is_err());
        // Round trip through the display helper must be lossless.
        for nanos in [0i64, 1, 200_000_000, 2_000_000_000, 1_234_567_891] {
            let human = crate::stats::dto::format_amount(nanos);
            let reparsed = if human.starts_with('<') {
                0
            } else {
                crate::stats::dto::parse_human_amount(&human).unwrap()
            };
            assert!(
                (reparsed - nanos).abs() <= 100_000,
                "{nanos} -> {human} -> {reparsed}"
            );
        }
    }

    #[test]
    fn read_discount_matches_the_documented_example() {
        let discount = rule(InputMode::ReadDiscount, "2", "8", Some("0.2"));
        let settlement = settle(Some(&discount), &usage(1000, 100, Some(800)), true);
        assert_eq!(settlement.status, CostStatus::Priced);
        let parts = settlement.parts.unwrap();
        assert_eq!(parts.input, 400_000);
        assert_eq!(parts.cache_read, 160_000);
        assert_eq!(parts.output, 800_000);
        assert_eq!(parts.total, 1_360_000);
        assert_eq!(crate::stats::dto::format_amount(1_360_000), "0.00136");
    }

    #[test]
    fn flat_prices_every_input_token_once() {
        let flat = rule(InputMode::Flat, "2", "8", Some("0.2"));
        let parts = settle(Some(&flat), &usage(1000, 100, Some(800)), true)
            .parts
            .unwrap();
        assert_eq!(parts.input, 2_000_000);
        assert_eq!(parts.cache_read, 0);
        assert_eq!(parts.total, 2_800_000);
    }

    #[test]
    fn disjoint_requires_a_proven_split() {
        let disjoint = rule(InputMode::Disjoint, "2", "8", Some("0.2"));
        let subset = settle(Some(&disjoint), &usage(1000, 100, Some(800)), true);
        assert_eq!(subset.status, CostStatus::UnsupportedBreakdown);

        let mut split = usage(1000, 100, Some(800));
        split.breakdown = InputBreakdown::Disjoint;
        split.numbers.cache_write = Some(100);
        split.uncached = Some(100);
        let mut disjoint = disjoint;
        disjoint.cache_write_rate_nanos =
            Some(crate::stats::dto::parse_human_amount("2.5").unwrap());
        let parts = settle(Some(&disjoint), &split, true).parts.unwrap();
        assert_eq!(parts.input, 200_000);
        assert_eq!(parts.cache_read, 160_000);
        assert_eq!(parts.cache_write, 250_000);
        assert_eq!(parts.total, 1_410_000);
    }

    #[test]
    fn missing_inputs_never_become_free_or_zero() {
        // No rule at all.
        assert_eq!(
            settle(None, &usage(10, 1, None), true).status,
            CostStatus::Unpriced
        );
        // Usage was never confirmed.
        let mut partial = usage(10, 1, None);
        partial.status = UsageStatus::Partial;
        let flat = rule(InputMode::Flat, "2", "8", None);
        assert_eq!(
            settle(Some(&flat), &partial, true).status,
            CostStatus::UsageIncomplete
        );
        // Local rejection before the upstream call.
        assert_eq!(
            settle(Some(&flat), &usage(10, 1, None), false).status,
            CostStatus::NotApplicable
        );
        // read_discount with unknown cache read.
        let discount = rule(InputMode::ReadDiscount, "2", "8", Some("0.2"));
        assert_eq!(
            settle(Some(&discount), &usage(10, 1, None), true).status,
            CostStatus::UnsupportedBreakdown
        );
    }

    #[test]
    fn overflow_is_reported_instead_of_wrapping() {
        let mut flat = rule(InputMode::Flat, "2", "8", None);
        flat.input_rate_nanos = MAX_RATE_NANOS;
        flat.output_rate_nanos = MAX_RATE_NANOS;
        let settlement = settle(
            Some(&flat),
            &usage(1_000_000_000_000, 1_000_000_000_000, None),
            true,
        );
        assert_eq!(settlement.status, CostStatus::Overflow);
    }

    #[test]
    fn rounding_is_half_up_per_part() {
        // 1 token at 5 nanos per million -> 0.5 nanos -> 1 (half up).
        let thumb = PriceRule {
            input_mode: InputMode::Flat,
            input_rate_nanos: 5,
            output_rate_nanos: 0,
            ..rule(InputMode::Flat, "0", "0", None)
        };
        let parts = settle(Some(&thumb), &usage(100_000, 0, None), true)
            .parts
            .unwrap();
        assert_eq!(parts.input, 1);
        // 1 token at 4 nanos per million -> 0.4 nanos -> 0.
        let thumb = PriceRule {
            input_rate_nanos: 4,
            ..thumb
        };
        let parts = settle(Some(&thumb), &usage(100_000, 0, None), true)
            .parts
            .unwrap();
        assert_eq!(parts.input, 0);
    }
}
