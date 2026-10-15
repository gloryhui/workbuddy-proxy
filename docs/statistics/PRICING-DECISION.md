# 价格维护与调用费用快照：用户确认的新口径

日期：2026-10-15；设计修订v2。**本文件按用户最新明确要求，替代原设计中的价格生效区间、已引用价格不可修改、缺价为未定价，以及按当前价格覆盖历史重算等规则。** 本轮更新设计与开发任务，不直接改动正在开发的Rust/GPUI业务代码。

## 1. 最终业务规则

1. 价格表是“当前价格配置”，用户可以新增、直接修改、删除；被历史调用使用过也不限制修改/删除。
2. 每个供应商配置+上游模型只有一条当前价格。无需维护生效区间，不要求先关闭旧规则。
3. 接纳一次调用时复制当时的单价、币种、计价模式，作为这次调用的价格快照。请求进行中调价也不影响它。
4. 调用终结时，将快照、分项金额和总金额写进该调用自己的费用明细。统计和详情只读这些数据，不再关联当前价格取单价或算金额。
5. 拿不到模型价格时，各项单价默认0，本次总金额为0。以后补充价格不改这些历史零金额。
6. 单价为0是明确业务默认。Token用量没有收到仍保留NULL，不能为了0元费用填造Token=0。
7. 后续改价格只影响修改成功后新接纳的调用；不自动或手动用当前价格覆盖历史明细。本轮删除/停用原“按当前价格重算历史”入口。
8. 金额是按本机配置价格计算的估算值；用户当前要求的0元默认不表示已经获取供应商真实账单。

## 2. 数据保存方式

`stats_model_prices`保存可修改的当前价格：

| 字段 | 含义 |
| --- | --- |
| price_id | 配置记录ID |
| provider_id / upstream_model | 精确匹配键，组合唯一 |
| currency | USD / CNY |
| input_mode | flat / read_discount / disjoint |
| input/output/cache_read/cache_write_rate_nanos | 每百万Token的四项单价，缺省项为0 |
| revision / updated_at_ms | 配置版本和修改时间，用于并发保存与快照来源追踪 |

`usage_costs`是每次调用自己的费用明细，与`usage_requests`通过request_id一对一关联。必须独立保存：

- 四项`*_rate_nanos`单价快照，币种、计价模式。
- `price_snapshot_at_ms`：选取这份单价的时间。
- `price_source=manual/default_zero/legacy_unknown`。
- `source_price_id/source_price_revision`：可选来源线索，**没有指向当前价格表的外键，也不作为查询历史金额的依据**。
- `input/output/cache_read/cache_write_cost_nanos`：各项金额。
- `total_cost_nanos`：本次总金额。
- `status/calculated_at_ms/pricing_version`：结算状态、计算时间和算法版本。

用量仍在usage_requests中保存，费用详情接口组合两张请求事实表返回完整记录。**“独立明细”不要求把全部字段塞进同一物理表，但不能依赖可变价格表。**

## 3. 单位和计算

单价：每百万Token的纳货币整数；金额：纳货币整数。接口均用整数十进制字符串传输金额和单价；数据库INTEGER，乘法用i128。

例如$2/M存`2000000000`；1000输入Token的输入金额为`2000000`纳美元，即$0.002。UI把用户输入的`2`转换一次为`2000000000`，API不要再乘一次1e9。

flat/read_discount/disjoint沿用原计算公式，每项独立half-up到纳单位后相加。新增规则：

- 没有价格记录：四项单价全0、input_mode=flat、price_source=default_zero。总金额和各项金额明确为0，即使Token未知。
- 有价格记录但某个单价输入留空：该项保存0；不能以NULL混淆默认0。
- 明确未发上游：金额0、status=not_applicable；Token保持原有不适用语义。
- 有手动价格，已知用量能够计算：status=priced，金额落库。
- 有手动非零价格但缺必需Token/缓存拆分：金额仍未知、status=usage_incomplete/unsupported_breakdown，不能把“缺用量”误当“缺价格”。若所需各项单价均0可直接落0。
- 默认币种由stats_settings.default_currency提供（初始CNY，可设置USD）。缺价调用复制当时默认币种；以后改默认币种不改变历史。

## 4. Rust接入

writer加载/维护当前价格缓存，提供按provider/model查找的不可变快照。只缓存已配置路由需要的价格，复用现有有限路由规模；不能让热路径同步查SQLite或复制全价格表。

配置变更在SQLite事务提交成功后发布新缓存。Start接纳时从已发布缓存复制小型`PriceSnapshot`进RequestGuard和Start事件：

```text
PriceSnapshot {
  source_price_id?, source_price_revision?, price_source,
  currency, input_mode,
  input_rate_nanos, output_rate_nanos,
  cache_read_rate_nanos, cache_write_rate_nanos,
  price_snapshot_at_ms
}
```

writer在Start事务创建费用快照行，status=pending，金额暂空；Finish只用该行快照和已验证Token结算。两者同批时先Start后Finish。中途退出/重启仍保留接纳时价格，不读取当前价格重新挑选。

Finish更新条件必须保证终态只结算一次；重复事件不能覆盖已结算快照/金额。查询dashboard、series、breakdown只聚合usage_costs中的已存金额；详情直接返回明细中的单价快照。

价格保存/删除失败不发布缓存，不反馈成功；并发调价以事务提交和缓存发布顺序为边界。进行中请求、普通转发、连接测试均用相同快照机制。

## 5. 价格界面和接口

- `stats_prices_list`返回当前配置，不展示生效区间或“已引用不可编辑”限制。
- `stats_price_save`直接新增/更新当前记录，携带price_id（新建null）、provider_id、upstream_model、currency、input_mode、四项单价、label、expected_revision。已有键的修改必须显式指向记录并校验revision，避免重复/误覆盖。
- `stats_price_delete`删除当前配置，携带price_id和expected_revision；历史不受影响，删除后新请求自动0元。
- 详情cost返回四项单价快照、分项金额、总金额、币种、模式、price_source、snapshot时间及可选来源ID；来源已删除也能完整展示。
- GUI缺价显示0元，并可附小标签“未配置价格，按0计算”；不再显示“未定价”替代金额。
- 总览/趋势包含这些零金额；增加`default_zero_request_count`，不要把缺价请求从金额统计覆盖中排除。币种分开汇总。
- 当前价格页面修改成功后只刷新价格列表，新请求采用新价；历史页数据不被改写。
- API统计协议设计升v2，数据库schema升v2，核心与GUI一起适配；实施时核查控制协议兼容并按需升CONTROL_PROTOCOL，不让旧GUI误读新DTO。
- 停用`stats_reprice_start`及GUI历史重算按钮，旧调用返回固定`stats_operation_removed`；旧维护记录可保留用于审计。

## 6. 已有开发数据库迁移

正式实现必须新增v1→v2迁移，不能仅改0001.sql或删除重建库。迁移前SQLite backup，事务内迁移并校验，失败rollback。

1. 先把已有费用引用的旧规则单价复制进新usage_costs快照列，保持原Token和已保存金额。
2. 没有历史价格且原状态unpriced的记录，按本次新口径迁为default_zero、四项单价和金额0；币种使用迁移时默认币种，并明确迁移来源。
3. 老记录已有非零金额但无法可靠找回单价：保留原金额，price_source=legacy_unknown，单价NULL，不用0假冒当时单价。不重新用今天价格计算。
4. 为旧in_flight尽可能保存原规则快照，无法恢复的使用明确的迁移标记；新接纳请求无价格永远default_zero。
5. 从旧区间价格中提取迁移时刻有效的当前价格，组合键唯一；未来/重叠/异常规则需要确定性选择或明确报错，不悄悄选择任意一行。
6. 去除费用对旧价格表外键、区间和不可编辑限制后，允许当前价格直接修改/删除。费用仍保持request_id外键和删除级联。
7. 当前schema.sql是v2新库设计参考，不是对实际开发库执行的升级脚本；设计验证脚本也只在临时库运行。

## 7. 必须通过的验收

1. 价格P1下调用A完成；直接把当前价格改P2；A的单价、分项和总价不变，后续调用B使用P2。
2. A开始后、结束前改P2，A仍使用接纳时P1。
3. 删除价格记录后，A仍能查全所有单价/金额；新调用C单价和总金额全0。
4. 缺价调用D默认0；以后补价格，D仍0，新调用E使用新价。
5. 显式0单价、空白单价项、缺价默认0、缺Token保持NULL分别验证。
6. 修改/删除当前价格无需扫描或更新历史明细；统计查询不JOIN当前价格表。
7. v1→v2迁移保留原Token/总金额，快照无伪造，FK/integrity检查通过；并发写入、重复Finish、重启和清理请求测试通过。
8. 原F01–F11中非价格缺陷继续修复；价格相关F03的纳单位要求继续有效，旧F07/F11的区间/重算要求按本文件替代。
