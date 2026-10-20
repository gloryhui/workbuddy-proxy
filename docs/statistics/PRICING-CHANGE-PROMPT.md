# 交给DeepSeek：按用户新口径解耦价格与历史统计

以下任务是对现有开发工作的最新纠正，优先级高于旧需求、旧审查Prompt和你当前正在实现的价格生效区间/历史重算方案。

---

用户明确要求：

> 价格表应该可以随便改；统计明细必须有本次单价、总价等数据，与价格表解耦；拿不到模型价格就默认0元。

请在现有worktree 统计开发 worktree（通过 `git worktree list --porcelain` 核实 `codex/usage-statistics` 对应目录） 继续实现，不重置、不覆盖已有修复、不新建另一套分支。

先阅读主工作区的最新文件（开发worktree旧副本可能尚未同步）：

- `docs/statistics/PRICING-DECISION.md`
- `docs/statistics/schema.sql`
- `docs/statistics/API.md`
- `docs/statistics/validate_design.py`

只复制本次设计修订所需文档进入开发worktree；保留你已经完成的F01–F09和正在开发的F10/F11。价格冲突以用户新口径及PRICING-DECISION为准，其他审查任务继续完成。

## 一、数据结构

1. 当前价格用stats_model_prices，一条provider_id+upstream_model组合唯一。用户可直接新增、修改、删除，四项单价、currency、input_mode、revision、updated_at。去掉生效区间、引用后不可改/不可删、区间不重叠限制。
2. usage_costs逐调用保存四项单价快照、币种、计价模式、price_source、price_snapshot_at_ms、可选来源ID/revision、分项金额、本次总金额、算法版本及计算时间。
3. 来源价格ID不对当前价格建外键。价格删除不影响历史请求、单价、金额、详情和汇总。费用对request_id的外键/级联删除保留。
4. 无模型价格或单价项缺省时单价为0；无模型价格时price_source=default_zero，终态各项/总金额0，默认币种来自stats_settings.default_currency（初始CNY，可选USD）。
5. Token没拿到仍保留NULL，不为了0元费用编造Token。手动非零价格但缺用量/拆分时仍保留金额未知及原因。

## 二、接纳时冻结，结束时结算

1. writer维护已提交的当前价格快照缓存，变更成功后发布，热路径不得同步读SQLite。
2. 每次调用接纳时复制PriceSnapshot进Guard/Start事件，在Start事务写入该调用的费用快照，pending金额暂空。
3. Finish只用本次已保存的单价/模式/币种与Token结算，绝不重新查当前价格。请求进行中调价也保持原快照。
4. 重复Finish不覆盖快照/终态费用。错误、取消、连接测试、正常退出/重启均沿用相同价格快照规则。
5. 无价格快照全0时费用可直接确认为0，即使用量未知；不要把缺价格当unpriced/null。
6. dashboard/series/breakdown直接汇总已保存金额，detail直接返回单价和金额；查询不能JOIN可变价格表取当前单价。

## 三、API与界面

1. stats_price_save直接修改当前配置，stats_price_delete删除当前配置；乐观revision只防并发覆盖，不限制已被历史使用的配置。
2. save/list/detail所有*_rate_nanos统一整数十进制字符串，$2/M为2000000000；UI普通金额定点转换一次。保留F03单位修复，不给数据库整体乘除1e9。
3. UI去掉生效时间、关闭旧规则、已引用锁定提示；有直接编辑/删除及四项价格输入。
4. 历史详情显示本次单价、用量、分项金额、总金额和快照来源，来源配置已删也正常显示。
5. 缺价显示0元，可附“未配置价格，按0计算”；总览包含这些0金额，新增default_zero_request_count。
6. 删除历史重算按钮，stats_reprice_start返回stats_operation_removed。调价后不自动或手动覆盖历史。旧维护日志仍能只读查看。
7. schema/API设计升v2，核查核心与GUI控制协议兼容，必要时同步升CONTROL_PROTOCOL，不能让旧GUI误读。

## 四、正式迁移

新设计schema.sql仅供新库验证。对已有v1增加正式0002迁移，不只改0001、不删库重建、不操作真实用户库。

迁移前backup，事务内先把旧rule_id对应的四项单价复制到历史费用，保留原Token与已保存金额；随后解除价格外键/区间等旧限制，提取当前配置。原unpriced按新规则变default_zero和0金额。旧记录已有金额但无法找回单价时保留金额、标legacy_unknown，不以今天价格或伪造0单价覆盖。参考PRICING-DECISION的详细迁移规则。

## 五、必须新增的回归

- P1价调用A完成→当前价改P2→A单价/金额不变，新B用P2。
- A开始后调价→A结束仍用开始时P1。
- 删除当前价→A历史仍完整，新C四项单价/费用全0。
- 缺价D为0→以后补价→D仍0，新E用新价。
- 没Token仍NULL；缺价金额0与有非零价但缺Token金额未知区分。
- 编辑/删除价格不需要扫描/更新历史；查询不依赖当前价格。
- v1→v2迁移、重复Finish、取消、重启、并发调价、分币种汇总、请求清理级联、单价单位往返通过。

在IMPLEMENTATION-STATUS新增“用户价格口径修订”，说明旧区间/引用锁定/历史重算要求撤销。按批次修复并验证，保留F01–F11非价格任务，不能只改文档或静态界面就宣布完成。

本轮不授权push、release、安装或改真实用户配置/数据库；提交遵守实际授权和AGENTS。现在执行，无需反复确认价格表是否允许修改，用户已明确授权该产品行为。

---
