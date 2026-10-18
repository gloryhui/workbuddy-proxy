# 功能与技术设计（冻结契约 v1.0）

## 1. 现状与已核实事实

本机 WorkBuddy 的 `~/.workbuddy/workbuddy.db` 有 `session_usage`，包含 `used / size / updated_at / credit_json`，没有独立逐请求摘要表。安装代码的 `persistUsage` 对 used、size 执行覆盖写；size 来自 contextWindow，不能累加为消耗。`usage-log.json` 是技能/MCP 活跃记录。业务实现不得将两者作为 Token 总账。

`~/.workbuddy/projects/<project>/<session>.jsonl` 在 `providerData.rawUsage`、`providerData.usage` 保存用量。实际观察到 `message(role=assistant)` 与 `function_call(role 缺失)` 都携带用量。只读取 assistant message 会漏掉工具调用轮次。`reasoning`、`function_call_result`、用户消息、session-meta 不作为用量事实。

观察到 `requestModelId=custom-local:...` 同时用于直连与代理模型；响应 `model` 不能证明路由。会话数据库中的 model 可能是最后选中的模型，不能覆盖历史消息的模型。`conversationRequestId`、traceId 可跨多步复用，不是去重键。本地 117 条含 rawUsage 记录均有 providerData.messageId，49 条非自定义用量记录有 49 个 messageId，但只有 12 个 conversationRequestId；这是样本证据，不是永远完整的承诺。

当前 Proxy：`src/stats/mod.rs` 拥有独立 writer/reader 线程，队列有界；`store.rs` 独占写连接、迁移与清理；`query.rs` 复用过滤器；`dto.rs` 被 GPUI 直接 include；UI 使用 scheduler 和 reply gate。不得另开生产 SQLite 写连接或在 Tokio 转发路径进行文件扫描。

## 2. 目标、口径和不做的事项

目标：三类模型在一个统计页面可筛选地展示，代理请求准确保留一次；外部日志仅作补充，显示可追溯来源和不完整性。统计单位是“已保存的模型用量记录”，不是用户消息数、会话数或账单数。JSONL 无法证明失败重试和未落盘请求的完整覆盖。

第一版包含：历史回填、自动增量、暂停/继续、手动检查、完整重扫、去重、历史归属确认、Token/缓存/推理/点数、来源筛选、导入健康、清除/保留策略、崩溃恢复。

第一版不包含：云账号余额或账单、HTTP 请求拦截、读取登录令牌、写 WorkBuddy 文件、监听数据库触发器、修改上游请求体/headers、依据聊天正文匹配请求、推断未提供 Token、为导入历史重算货币费用、账号云同步、自动安装/发布。

## 3. 数据流和线程

```text
WorkBuddy JSONL（只读）
  → workbuddy-import OS worker：目录发现 → 有界读行 → 白名单解析 → 规范化
  → import queue（2 批，每批 ≤128 行且规范化载荷 ≤512 KiB，最多一个待确认批次）
  → 现有 stats-writer：事务内再分类 → 身份去重 → usage_requests / ledger / checkpoint
  → 现有 stats-reader → 管理 API → GPUI scheduler → 统计页

Proxy 转发 → 现有 RequestGuard/Collector → 原事件队列 → 同一个 stats-writer
```

worker 不持写连接；只通过请求-应答批次取得持久化游标。每批携带 expected file generation/offset/import revision/mapping revision；writer 在当前事务核对，过期批次拒绝，worker 从已提交游标重读。只在收到事务成功应答后释放批次、读取下一批；提交成功但应答丢失时重放由账本去重。

writer 优先处理控制命令和实时事件，每次最多处理一批导入；有实时事件/恢复 backlog 时导入让出。导入失败只更新导入状态，不把原代理采集状态改成 disabled；SQLite 整体故障仍走原有恢复机制。不能让导入事件占满原 EVENT_QUEUE_CAPACITY。

## 4. 数据源归属与去重

来源是两维：记录来源 `proxy / workbuddy`；产品筛选 `all / builtin / supplier`。存储 record_origin 三值为 `proxy / workbuddy_builtin / workbuddy_custom`。代理连接测试仍由 source=connection_test 管理；不要用 origin 覆盖原 source。

归属顺序固定：

0. 匹配到 prepared/conflict 归属变更的受影响时间段：暂存 pending_classification；旧 direct 区间不得抢先计入。
1. 精确命中有效历史代理映射：ledger=excluded_proxy，统计表不插入。即使代理统计关闭、记录丢失、日期超限也不改成直连回填。
2. 明确 `requestModelId` 非 `custom-local:`，且符合当前已验证 WorkBuddy v1 消息格式：内置模型；auto 保留 requested_model=auto，response_model 从当次 providerData.model 读取。格式变更/缺失 ID 时 pending，不猜测。
3. `custom-local:<id>` 精确匹配已确认 direct 的历史区间：用户直连。
4. 其他：pending_classification；只存白名单候选，不参与报表。在设置区可按模型 ID + 半开时间区间预览并确认 direct 或 proxy。

同一别名历史上可能先直连后代理。当前 models.json 只能证明观察时刻之后的状态，不能倒推更早时间。自定义配置快照的 observation_start 不得回填到 1970。相同 alias 不同区间分开；冲突区间停止自动分类。用户规则不能覆盖已证实的代理区间。撤销错误的人工规则时撤回其导入明细、ledger 回 pending，不能改代理记录。

启动读取当前 models.json 和 Proxy installed 元数据时仅提取 id、managed、route 等非凭据字段；复用 `src/workbuddy.rs` 已有 managed_signature/installation 逻辑，通过小型共享纯函数导出；不得复制分叉一套判断。不要仅凭 localhost、供应商名称或斜杠判定代理。

新版本同步/取消同步：先在 writer 持久化 mapping intent（prepared，不可用于正常计数），再执行既有原子文件写，最后提交 verified observation。成功后才向 UI 报整个操作成功。开启导入时 prepare 失败，必须在改 WorkBuddy 文件前返回明确错误；关闭导入时保留原同步行为。文件写成功而 finalize 失败要准确报告“模型已同步，统计归属待恢复”，不能声称模型同步失败或已回滚。重启检查 prepared intent 对照实际非敏感配置后完成/转 conflict；不得凭空认定成功。

历史备份只可作为待确认线索，不得批量读取并保存密钥，不作为无条件的时间归属证明。第一版不自动扫描备份文件。

消息去重键：SHA256(length-prefix 编码的 namespace_id、sessionId、providerData.messageId)，产出 64 位小写 hex 作为 request_id / ledger.entry_key。不得只用模型+时间、traceId、conversationRequestId、文件 offset 或 JSONL 顶层 id。缺 messageId 时 pending_identity，不降级生成随机键。sessionId 必须与文件名 stem 一致，不一致记 malformed。复制/移动文件后的相同消息仍只有一条。

normalized_hash 仅覆盖路由模型、Token、credit 等语义字段；不包含正文、文件位置、序列化键顺序、消息类型或时间（同一响应可多次表现）。同键同 hash 为 duplicate；同键不同 hash 为 conflict，撤回已有 **workbuddy** 明细，账本保留冲突标志及摘要，默认不计入。不覆盖 proxy 明细，不采用最后一条优先。首条 timestamp 一旦接受即冻结。

## 5. 增量、重扫与一致性

默认根路径由现有 config::home() + `.workbuddy` 得到；仅发现 `projects/<单层项目目录>/*.jsonl`，不读 tool-results、traces、storage、SQLite、备份。第一版不开放任意目录 UI 输入。测试通过构造函数注入隔离 root；集成进程同时隔离 HOME/USERPROFILE 与 --config-dir。

目录发现使用分页/惰性迭代，不 collect 全目录，不 follow symlink；普通文件 canonical path 必须仍在 root 内。路径包含中文/空格应正常；不要假设 macOS 分隔符。用 namespace_id 隔离本地导入身份，不把账号 ID 当凭据或访问跨用户目录。

checkpoint 保存 relative_path、generation、offset、文件 length/mtime、prefix hash、offset 前锚点 hash。常规未变文件只 stat，变化时验证长度和锚点；截断、替换或锚点变化：generation+1、offset=0，账本不清空。相同大小/mtime/锚点但中间重写无法靠增量检测：每日一次低优先级完整重扫（每轮仍受预算约束）补偿；UI 明示完整核对时间，不宣称实时发现任意修改。

普通可解析记录只提交完整换行符结束的行；CRLF 接受；EOF 未换行片段不推进游标。读到 4 MiB 以上单行后进入 discard-until-newline，不再累计到内存；超限确认后允许将排空位置与 discarding_oversized=true 同事务持久化（这是普通尾行规则的唯一例外）；到换行计一次 oversized 并清掉排空标志。超过预算可保存 discard 状态并继续，不死循环、不按片段伪造多条错误。JSON syntax 错误的完整行记固定原因、推进并继续。不可打印坏行正文。

读文件期间被替换/截断，提交前再次检查 generation 和锚点；校验失败整批丢弃，不插入旧文件内容。与外部文件无法做到跨进程原子快照，遇到无法证明一致性时重读，不承诺文件被并发恶意重写时绝无延迟。对已提交、后续被源文件删除的记录不自动删历史，只有用户清除或明确冲突才撤回。

## 6. 资源与调度固定值

| 项目 | 第一版固定上限/默认值 |
|---|---|
| 自动增量周期 | 固定 5 秒，由核心后台独立调度；关闭导入/暂停不读 JSONL |
| 目录发现 | 每 5 秒调度，包含新项目目录和新 JSONL；每次 slice ≤100 目录项 |
| 自动完整核对 | 每 24 小时一次，低优先级、有断点 |
| 文件读取 buffer | 64 KiB |
| 单行上限 | 4 MiB；超限排空到换行 |
| 单 worker slice | ≤8 MiB 原始读取或 50 ms，任一达到就让出；单次系统调用无法硬实时中断 |
| 单提交批 | ≤128 条，≤512 KiB 规范化数据 |
| import channel | 容量 2，最多 1 批等待提交确认 |
| writer 导入目标时间 | 正常无锁批次 ≤25 ms；无锁长批动态减小下一批，禁止增大原全局 DB 超时 |
| 导入退避 | 1 / 2 / 4 / 8 / 16 / 30 秒，封顶 30 秒；显式重试可立即唤醒 |
| 停止 | 复用 4 秒 shutdown 总预算；不等待所有历史导完 |
| 文本字段 | ID/模型按既有 ≤160 字符；超长不可静默截断成可碰撞 ID |
| 待确认候选 | 每条规范化 JSON ≤16 KiB；不含原始消息 |
| 内存 | 数据规模增大时 RSS 不线性增长；具体验收见 ACCEPTANCE |

调度使用单调时钟和固定常量 `WORKBUDDY_IMPORT_INTERVAL = Duration::from_secs(5)`。首次开启、恢复、核心重启且有效开启时立即调度一次。周期不读取 UI refresh_interval，不提供用户可调同步周期。统计页关闭、切页、窗口隐藏、页面自动刷新关闭均不停止导入；核心进程运行且 effective_enabled 才执行，系统休眠期间不承诺同步。

单 worker、一个在途扫描；到期 tick 和手动增量检查合并为至多一个待办，不重叠执行或无限排队。任务未完成时继续分片推进，不能每批再睡 5 秒；唤醒后不补发积压 tick。历史回填/完整核对每个 slice 后服务到期增量与目录发现，优先处理已追平文件的新追加，再轮转推进回填。巨型文件自身未追平前不得跳到 EOF 伪造已完成；其尾部延迟单列回填积压。故障仍按既定退避显示状态，不能声称故障期间 5 秒必达。

进度以已处理字节/文件和已导入/排除/待确认数量表达；遍历未结束时不得给虚假百分比或完成 ETA。

## 7. 清除、保留与暂停

沿用既有 clear preview token 的 before_ms + as_of_id 边界。清除时同一事务先把待删导入记录对应 ledger 标记 deleted_manual，再删明细；统计清除不能删除 ledger、映射和 checkpoint。晚到、未在预览内的记录不属于这次清除，继续导入；UI 明示“清除当前已入库记录，不清除 WorkBuddy 源文件”。重扫不能让被清除的同消息复活。

自动 retention 删除同样标记 deleted_retention；导入前应用当前保留期限，旧记录记 excluded_retention。把期限从 30 天改回 180 天不自动复活曾删除记录；第一版无恢复已删除历史功能。账本只保留最小身份与原因；已 imported/excluded/deleted 状态不得长存 candidate_json。保留账本占用与清空全部数据不是同一操作，不添加隐藏重置入口。

暂停只停止新导入，已有报表继续读；关闭全局统计时导入有效状态也暂停，不能绕过用户全局开关。import_enabled 单独保存不被全局开关覆盖。暂停应答返回后禁止新的导入事务提交；正在提交的事务允许在应答前完成。恢复读取原 checkpoint。

## 8. 代码结构和强制规范

新增模块固定为 `src/stats/workbuddy_import/{mod,types,parser,normalize,classify,reader,store}.rs`。mod 管生命周期/调度；types 无 I/O；parser/normalize/classify 是可单测纯函数；reader 只读源；store 通过现有 writer 借用事务，不创建连接。UI 新增 `gpui/src/workbuddy_import.rs` 和 `gpui/src/source_filter.rs`，原 statistics.rs 只接线，避免继续堆巨型函数。

Rust edition 2024 / 项目 MSRV 1.88；GPUI 继续使用锁定的 gpui-kit 0.7.3。新增代码 snake_case、类型 PascalCase、枚举表达状态；共享 DTO 仅 serde/std（原 hash helper 依赖 sha2 已存在，不能引入 GPUI/SQLite）。ID/游标整数 checked 转换；SQL 参数绑定；金额/credit 整数纳单位，不能 f64 运算；Option 表未知。

禁止：新增框架、ORM、SQLite 连接池、无界 channel、tokio::spawn 包同步扫描、每条消息 spawn 线程、在 UI render 中读文件/DB、将 Value 到处透传、unwrap/expect 处理外部输入、吞错返回 0、关 Clippy、降低旧断言、复制产品逻辑到测试专用实现、批量 rustfmt 无关文件、顺手重构代理协议。

新生产函数目标 ≤80 行（不含注释），模块目标 ≤600 行（测试可单独文件）；超过必须在 PROGRESS 给出具体原因和评审结论，不能用 suppress 属性绕过。新 API/状态转换写 Rustdoc 说明前置条件、不变量、失败行为。日志只固定错误码、计数、散列标识，不输出路径/完整 model 配置/聊天正文。测试合成数据，固定时间，禁止依赖公网或真实 HOME。

## 9. 兼容和版本

只新增迁移 0007，禁止改写 0001～0006。usage_requests 的 CHECK/nullable 改变需重建表；逐列复制且保留 id、request_id、原费用与时间。视图、索引、外键核对后事务提交。升级前复用 SQLite Backup API，不直接 cp 活跃 WAL 主文件。现有 migrate 尾部无条件 apply_step(6) 必须改为 version<6 才执行，并在 version<7 时执行0007，防止v6升级时重建已存在的catalog表。migration 后不能在旧 v6 二进制上继续使用新库；回滚说明需明确关闭进程后恢复整套备份再使用旧程序。

管理协议 10→11，统计 API 2→3，schema 6→7。在本基线固定采用这些值；若重放到其他分支发生冲突，登记设计变更而不是擅自重用已占用版本。应用版本保持 0.5.6，发布时另行决定。

不声称旧 helper 能解析新 source/outcome 枚举或 nullable stream。复用已有 frontend 协议校验/核心替换链路，更新兼容测试验证旧 core 不被新 UI 直接误用；人工连接不兼容 helper 应明确拒绝，不返回伪成功数据。
