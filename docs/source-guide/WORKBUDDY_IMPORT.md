# WorkBuddy 历史导入：从 JSONL 到可去重的统计事实

本篇解释当前源码的生产路径：谁读取本地历史，怎样保留未知值，为什么重扫和丢失 ACK 不应让统计翻倍，以及文件读到哪里才算真正保存。先读 [统计持久化](STATS_PERSISTENCE.md) 和 [查询与界面](STATS_QUERY_UI.md)，再跟下面的入口。

## 1. 线程与事务：读文件的人不直接写统计

```text
统计设置 / 扫描命令
    ↓ Shared 开关、唤醒与扫描状态
workbuddy-import 线程
    ↓ 只读 SQLite 获取设置和检查点；发现并读取源 JSONL
parser → normalize → Candidate 摘要
    ↓ Work::Batch(FileBatch) + 独立 reply channel
有界导入 channel
    ↓
stats-writer 的 Runtime::poll
    ↓ 报表/导入准入协调
writer::apply_work → SQLite transaction
    ↓ 复核版本、归属、重放、事实、ledger、checkpoint
commit
    ↓ ACK（成功或错误）
源文件 worker 再安排后续读取
```

入口为 [`runtime.rs`](../../src/stats/workbuddy_import/runtime.rs)、[`runtime/worker.rs`](../../src/stats/workbuddy_import/runtime/worker.rs)、[`runtime/worker/file.rs`](../../src/stats/workbuddy_import/runtime/worker/file.rs) 和 [`runtime/writer.rs`](../../src/stats/workbuddy_import/runtime/writer.rs)。

Java 类比是一个后台文件消费任务，把领域摘要交给单线程写入服务；ACK 表示一次事务处理的结果。它不是 Kafka，也没有外部消息队列或 Spring 自动重试。源文件线程用自己的只读 SQLite 连接取快照，持久化修改仍由 stats-writer 完成。

[`mod.rs`](../../src/stats/workbuddy_import/mod.rs) 还保留 `ImportChannel / AckWindow / Scheduler` 等契约模型。它们参与启动检查或测试，但不能因此认定生产批次会实例化这些对象。跟实际 I/O 时从 `Runtime::start → worker::run_worker → Worker` 开始。

## 2. 找到哪些文件，读多少，怎样停下来

[`reader::ReaderRoot`](../../src/stats/workbuddy_import/reader.rs) 的默认根是用户目录下 `.workbuddy`，用量来源位于 `projects/<项目>/<会话>.jsonl`。生产代码检查允许的相对路径、各组件符号链接、文件类型和规范化边界；不能因为扩展名是 jsonl 就读任意位置。

发现与读取按片推进，不一次把所有历史文件装入内存。当前核心边界包括：

| 约束 | 作用 |
| --- | --- |
| 单行 4 MiB | 超大行进入丢弃状态，继续有界排空，不保留整行 |
| 单次源读取片约 8 MiB / 50ms | 在循环边界让出机会；不是硬实时 I/O 超时 |
| 单批最多 128 个候选 | 限制一次提交的对象数量 |
| 批次规范化候选载荷最多 512 KiB | writer 再检查批次大小 |
| 单个候选摘要最多 16 KiB | 避免把无界原始行写入 ledger |
| 导入 channel 容量 2 | 不与代理 begin/finish 的事件队列共用 |

不要把“源码循环检查 50ms”理解成任何磁盘调用都会在 50ms 内返回。数据库错误、源路径错误、文件中途改变等各有错误码；worker 可能退避重启，writer 不能把失败伪装成一次成功导入。

暂停或全局统计禁用后不应继续确认新批次。生产 `Runtime::drop` 标记 stopped、唤醒并 join worker；不是丢下线程继续读源，也不是自动删除用户历史。源码中的独立 shutdown 契约常量不能直接当成实际 join 的严格耗时保证。

## 3. 文件末尾半行与超大行为什么不同

JSONL 是“一个完整换行结束的记录”。普通行写到一半时，EOF 不意味着内容完整。`LineReader` 不将普通未换行的尾巴当作一行，检查点留在之前可提交位置；等 WorkBuddy 追加换行后再读。

超大行已经确定不可能作为候选，继续保存其内容没有意义。它可以在尚未看到换行时推进“正在丢弃”的 offset，并把 discarding 状态随检查点保存，下一片继续排空。两条路径不同，不能笼统说“所有没有换行的数据绝不能推进任何 offset”。

源文件被截断、替换或修改时，旧 offset 不能盲目复用。读前加载文件元数据、已提交检查点与前缀/锚点指纹；读取后再检查元数据并生成本批指纹。版本或来源不一致时批次失败，不能靠偏移较大就宣布“已经读完”。

全量重扫有独立 `scan_offset / scan_generation`，增量有 `committed_offset`。全量扫描的游标用于复核旧内容，不应把主增量位点倒退。发现阶段完成也只说明“文件总数已知”，不等于所有文件已导入。

## 4. 解析：只提取白名单摘要

[`parser::evaluate`](../../src/stats/workbuddy_import/parser.rs) 接收一行字节、文件 stem、namespace 和当前时间，输出 `ParseDisposition`。它先判断行类型与结构，再处理身份与数值，不能把反序列化成功直接等同于可写事实。

普通模型记录需要 session、message、时间等身份；普通 session 必须与文件 stem 一致。当前还有两条实际分支：

- `model-usage` 回执缺 sessionId 时允许使用文件 stem；这不放宽其他普通消息的身份规则。
- `credit-usage` 转入 [`credits::evaluate`](../../src/stats/workbuddy_import/credits.rs)，独立校验收费阶段身份。

没有 usage、缺身份、JSON 损坏、时间非法或行类型无关都有各自 disposition/诊断。没有 messageId 的记录不会凭顶层随机 id 造一条用量明细，也不会拿导入时间替换缺失来源时间。

源码处理的是白名单字段。Candidate 和持久化摘要没有聊天正文、图片、密钥、原始工具参数或整条原始 JSONL。Java 的宽松 Map 反序列化很容易把多余字段顺手传下去；这里应沿 Candidate 的明确字段判断数据边界。

## 5. 用量归一：None 不是零，原始与归一包装不能互相遮掩

[`normalize_usage`](../../src/stats/workbuddy_import/normalize.rs) 分别检查 rawUsage 和 usage 中的父字段。两处都给值时必须一致；高优先级字段非法不能通过换用低优先级合法值“洗成正常”。

input/output 均可信且 total 缺失时可相加推导 total，并记录 derived 标记。显式 total 与两项相加冲突时 invalid；缺少一项父量时不能用 total 相减补出它。非法整组 token 清空，未知仍为 None，确认的 0 保持 Some(0)。

cache/reasoning 和 credit 有独立后处理，不要把它们的解析失败无条件等同于父量失效。credit 用十进制文本解析成 nano 积分，最多 9 位小数，不经过 f64、不支持指数或符号；解析失败留未知并记诊断，不猜成 0。

独立收费回执没有 token 使用量，状态为 not_applicable，避免混入 token 完整率的适用分母。credit 是 WorkBuddy 的收费量，既不是 USD/CNY，也不自动表示余额或供应商账单。

## 6. 去重身份与内容指纹：同一消息，不同语义

模型消息的 `entry_key` 由带域和长度前缀的 `(namespace, sessionId, messageId)` 做 hash。文件 offset、顶层 id、trace id、conversationRequestId 不参与这条模型消息身份。文件检查点有自己的 file_key，不能拿它代替消息键。

`normalized_hash` 则描述规范化后的模型与用量语义，不依赖原始 JSON 键顺序、正文、文件路径或诊断变化。当前重放还会沿用 ledger 已保存的 event_at_ms，避免同一消息重复出现时悄悄改接受时间。

可类比 Java 领域模型中的“业务主键”和“事实内容指纹”：

| 关系 | 一般处理 |
| --- | --- |
| 新 entry_key | 按当前分类和保留期决定是否生成事实与 ledger |
| 同 key、同语义 | duplicate，不插入第二条事实 |
| 同 key、语义改变 | 撤回相应导入事实并记 conflict，不静默覆盖；已有代理排除决定仍保留 |
| tombstone | 已清理的身份不因重扫复活 |

有明确兼容例外，例如旧 parser 没读 credit 时仅补未知 credit，或补收费工具的显示名称；这些例外不能扩展成“任何新值都可覆盖旧值”。具体看 `store::resolve_replay` 的条件。

独立收费用另一套域，以 namespace + conversationRequestId + stageId 生成 entry_key，message hash 也基于 request/stage。它不复用模型 messageId，不把工具结果镜像作为第二个收费来源。两类回执出现在同一文件里也不应重复计费。

## 7. 当前生产分类与历史映射机制要分开读

当前 [`store::attribute_candidate`](../../src/stats/workbuddy_import/store.rs) 的直接规则是：

1. 若 session_hash/message_hash 已对应一个实际代理事实，返回 Proxy，避免导入重复。
2. 缺少 requested_model 时返回 Pending。
3. 模型以 `custom-local:` 开头时返回 Direct。
4. 其他已知模型返回 Builtin。

它当前不查询历史映射区间，也不选择具体供应商，返回的 mapping_id 为 None。Direct 在这里表示导入自定义来源，不能仅凭这个标签证明它当时使用了今天的哪个 provider。

另一方面，`observation / history / reconcile` 仍维护配置观测、同步意图与历史不确定区间。独立 `classify::attribute_with_intervals` 也有区间算法。阅读这些模块有价值，但不能把它们的模型自动套成当前候选分类规则。

[`manual.rs`](../../src/stats/workbuddy_import/manual.rs) 的手工 preview/apply/revoke 入口已退役，返回错误；规则分页是空兼容响应。旧文档或测试名中出现“确认”不表示现在界面允许用户确认归属。

## 8. 代理优先，为什么晚到的代理还能撤回导入

只在导入时查询代理是否存在还不够：导入可能先到，代理 Start 稍后才保存。

`record_proxy_identity` 在原生 Start 的事务中记录 session/message hash，随后撤回匹配的导入事实并把 ledger 改成 excluded_proxy。大批历史有界处理，剩余工作由 `pending_dedup` 与 `resume_proxy_dedup` 继续。不会为了去重删除或覆盖原生代理事实。

因此先后顺序不能决定最终重复计数。这里证明的是两条来源拥有相同身份时的代理优先；身份缺失或不一致时，不能单靠时间和模型相近就强行配对。

## 9. 写事务和 ACK：三个持久化对象一起前进

跟 `runtime::writer::apply_work → apply_batch → store::commit_batch`：

1. 开启写事务，重新读取全局与导入设置，确认仍允许导入。
2. 检查 namespace、settings_revision、mapping_revision 与预期 checkpoint。
3. 需要时建立或重置文件 checkpoint，构造提交 guard。
4. 将 Candidate 归一为 CommitItem，计算语义 hash，再检查数量与摘要大小。
5. 处理 ledger 重放与归属，为允许导入的项生成 usage_requests 事实。
6. 在同一事务更新 ledger、检查点及扫描细节。
7. 外层提交成功后，Runtime 更新相应计数并向 Envelope 回复结果。

`store::commit_batch` 接收调用方事务，本身不 commit。它返回统计报告不意味着外层已经提交；最后的 checkpoint CAS 失败也会使这一事务不能按成功路径完成。这像显式 Repository 操作共享同一个 `@Transactional` 边界，但 Rust 中谁持有并提交事务写在调用关系里。

源 worker 的 `submit` 用 try_send 提交，满了返回 busy，不占代理事件队列；提交后它可以等待自己的 ACK，每 100ms 检查状态并唤醒 writer。不能把“非阻塞发送”误读成“worker 从来不等待任何确认”。

ACK 失败或丢失不能反过来证明没有提交。重启后应重新读取数据库检查点与 ledger，再依赖身份去重处理重放。不是从内存记住的“读到哪里”直接继续。

## 10. 观测、同步与恢复：不把今天配置当成过去证据

[`observation::prepare_sync`](../../src/stats/workbuddy_import/observation.rs) 在 models.json 变更前记录整份计划的 prepared 意图；写文件后 `finish_sync` 按实际观测确认。数据库与外部 JSON 文件没有联合事务，prepared 用于记录这个不确定间隙。

同一毫秒内的确认仍可能保留 1ms 不确定区间；时钟回退不能证明顺序，不能强行把区间倒过来。同步移除也不意味着能猜出 Direct 目的地。恢复未完成意图需要排除仍在进行的同步，再按完整、成功解析的观测推进。

不可读配置不等于空配置。暂停观测时要结束可以继续信任的区间，重新启用不能把关闭期间补成已证实。已经有历史事实也不代表当前配置能追溯证明更早的每条消息归属。

这些历史机制的维护事务与当前 Candidate 自动分类是两条职责；断点跟踪时应分别进入生产调用点，避免仅凭某个 interval helper 的测试就推断整条导入路径。

## 11. 与报表、界面和清理的关系

Runtime 在 writer 取到 Envelope 后可能先保留 pending，等 `report_scheduling` 允许再提交；不能提前 ACK 或持续排空更多批次。普通代理事件与控制工作继续按自身优先级处理，导入不会接管代理队列。

新增事实一般不破坏分页的 as_of_id 上界；撤回/冲突等改变既有事实的动作可能推进维护 revision，使旧游标失效。界面发扫描命令取得 queued/scan_id，不代表扫描完成，还需读取状态；发现完成、读取完成、事务提交与页面刷新是四个阶段。

导入 ledger 的 `deleted_manual / deleted_retention` 是 tombstone，重扫不会把清理过的历史自动复活。普通 excluded_retention 与 tombstone 不是同一个状态，排错时看实际 ledger 枚举与重放分支。

## 12. 现有测试阅读入口

本轮只做注释和导读验证，下列业务测试没有执行：

| 源码位置 | 测试名 | 学习重点 |
| --- | --- | --- |
| [`runtime/tests.rs`](../../src/stats/workbuddy_import/runtime/tests.rs) | `real_worker_waits_for_newline_keeps_unlimited_history_and_stops_on_global_disable` | 半行、读取与开关 |
| 同上 | `full_scan_rechecks_a_hidden_middle_edit_without_resetting_incremental_offset` | 全量和增量位点分离 |
| 同上 | `report_guard_defers_all_import_writes_then_discovery_resumes` | 报表准入协调 |
| [`store.rs`](../../src/stats/workbuddy_import/store.rs) | `a_lost_ack_and_a_repeated_batch_count_exactly_once` | ACK 丢失与重放 |
| 同上 | `a_failed_item_rolls_the_whole_batch_back` | 单批事务边界 |
| 同上 | `a_different_hash_withdraws_the_detail_and_leaves_a_conflict` | 内容变化不静默覆盖 |
| [`observation/tests.rs`](../../src/stats/workbuddy_import/observation/tests.rs) | `same_millisecond_confirmation_leaves_one_millisecond_uncertainty` | 同毫秒不确定区间 |
| 同上 | `clock_regression_cannot_confirm_an_intent` | 时钟回退 |
| 同上 | `configuration_projection_uses_id_and_never_exports_keys_or_urls` | 观测摘要的隐私边界 |

建议推演两个故障：事务提交后 ACK 消失；导入成功后代理 Start 才到。分别指出持久化检查点、ledger、代理身份表和下一次读取怎样收敛。能够解释这些过程，就能理解导入为什么不只是“读文件后 INSERT”。
