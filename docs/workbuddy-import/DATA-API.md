# 数据、接口与算法契约 v1.0

## 1. 解析输入和记录身份

只接受 UTF-8 JSON 对象，未知字段通过 serde IgnoredAny 跳过，不把整行 Value clone 到多个层。允许带 BOM 的文件仅首行去 BOM。最外层白名单：type、role、sessionId、id、timestamp、providerData、_fromSubAgent；providerData 白名单：messageId、requestModelId、requestModelName、model、responseModelId、rawUsage、usage、credit、source、stageId、conversationRequestId。其他字段不得进入持久化/日志。

| 条件 | 处理 |
|---|---|
| type=message 且 role=assistant | 进入候选处理 |
| type=function_call，role 缺失或 assistant | 进入候选处理 |
| type=model-usage，role 缺失或 assistant | v2 响应用量回执；缺 sessionId 时取所属文件 stem，与同 messageId 的 assistant/function_call 共用原有去重身份 |
| type=credit-usage | v2 独立收费阶段，见下文；不读取工具结果正文 |
| _fromSubAgent=true 的转发模型消息 | 不作为主会话的独立模型用量；使用其 team_member 收费回执 |
| 其他 type/role | ignored，推进完整行游标 |
| 两个 usage 对象都缺失 | ignored_no_usage，不能创建“0 Token”记录 |
| 有 usage 对象但字段不全 | 允许 partial/missing，用量保持 null |
| 缺 sessionId / 与文件 stem 不符 | malformed，诊断计数 |
| 缺 providerData.messageId | pending_identity 诊断，不创建 usage/ledger；完整重扫可再检查 |
| 缺有效 requestModelId | pending_classification 候选，不从 sessions 表补模型 |
| timestamp 非整数毫秒、负值或超过当前时间 24h | invalid_timestamp 诊断；不以导入时间替代 |
| 非有限/负数/小数 Token、超上限 | invalid_numbers；可识别身份的记录标 invalid，不计 Token |
| 字符串中的 ID 含控制字符/超长 | malformed，不截断或改写 ID |

namespace_id 在首次启用导入时生成一次并存 stats_settings；会话/消息 ID 不落日志，账本保存它们各自的 SHA256。entry_key 使用逐字段 `u32 big-endian byte_length + UTF-8 bytes`，前置域 `wbp-wb-message-v1`，以 SHA256 输出 64 hex；不做 lower-case、trim ID 或拼接冒号去碰撞。requestModelId 仅验证，不重命名。

providerData.messageId 表示模型响应的已保存标识，**不是已证明的 HTTP request ID**。同一响应拆为多个工具项且共享 messageId 时只能计一次；多步调用共享 conversationRequestId 但有不同 messageId 时分别计数。不读取 reasoning/function_call_result 中的 usage 或 rawResponse.credit，后者是独立收费回执的镜像。

v2 的 `credit-usage` 只读取 `timestamp`、可选 `sessionId` 和 `providerData` 中的 `credit/source/stageId/conversationRequestId`。缺 sessionId 时使用文件 stem；显式不匹配仍拒绝。收费身份使用独立 hash 域及 namespace + conversationRequestId + stageId，不使用随机历史 id、模型 messageId、金额或相近时间推断。跨批次、文件副本、重扫及重启都通过同一账本去重；同阶段金额不一致仍为 conflict。缺阶段/请求标识不猜测收费身份。

独立收费以 `workbuddy-credit:<source>` 单列内置收费项（WorkBuddy 支持 generation_tool、team_member），参与 WorkBuddy 积分总计，不伪造 Token、模型推理次数、HTTP 状态或货币费用。生成工具包含图片生成/编辑、视频生成和音频转录，金额以 WorkBuddy 已保存的收费回执为准，不再次累加工具结果。完整原文、图片、工具参数仍不保存。

## 2. Token 规范化算法（固定优先级）

rawUsage 中有效字段优先；只有对应字段真正 absent/null 才读 normalized usage。存在但非法的值不可被 fallback 掩盖。若同一事实在两组对象均存在且矛盾，usage_status=invalid、reason=conflicting_usage，原始数值不直接保存到可统计列；保留合法的身份和错误码即可。

| 目标字段 | 优先读取路径 |
|---|---|
| input_tokens | rawUsage.prompt_tokens → rawUsage.input_tokens → usage.inputTokens |
| output_tokens | rawUsage.completion_tokens → rawUsage.output_tokens → usage.outputTokens |
| total_tokens | rawUsage.total_tokens → usage.totalTokens；均缺失且输入/输出都合法时 checked_add 推导 |
| cache_read_tokens | rawUsage.prompt_tokens_details.cached_tokens → rawUsage.prompt_cache_hit_tokens → rawUsage.cache_read_input_tokens → usage.inputTokensDetails[0].cached_tokens |
| cache_write_tokens | rawUsage.prompt_cache_write_tokens → rawUsage.cache_creation_input_tokens → usage.inputTokensDetails[0].cache_creation_input_tokens |
| reasoning_tokens | rawUsage.completion_tokens_details.reasoning_tokens → rawUsage.completion_thinking_tokens → usage.outputTokensDetails[0].reasoning_tokens |
| workbuddy_credit_nanos | rawUsage.credit → usage.credit；独立收费回执使用 providerData.credit，均以原始十进制字面量精确解析 |

缓存专用兼容规则：观察到 WorkBuddy 同时写顶层 cache_read_input_tokens=0 与 prompt_cache_hit_tokens>0；它们不都代表同一可用字段。因此按上表优先级选第一个存在的合法值，低优先级缓存别名冲突仅 diagnostics += cache_alias_disagreement，不把高优先级值清零。normalized 中主输入/输出/总量仍做矛盾检查。`cached_tokens` 顶层歧义字段不使用。不得把缓存加到输入上。

若 future rawUsage.input_tokens 与缓存是互斥输入（Anthropic 风格）而无已验证包裹语义：parser 标 unsupported_usage_schema，不套 Chat 规则相加；第一版只支持已观察到的 WorkBuddy Chat 风格包装及规范化 usage schema，后续增加适配须修改 fixture/契约版本。

边界：输入/输出 ≤10^12，总数 ≤2×10^12，cache_read/cache_write ≤input，reasoning ≤output。总数存在时必须=input+output，缺一项不能用减法反推；输入缓存构成只标 read_subset 或 unknown，不宣称 disjoint。缺 cache_read 是 null，不是 0；显式合法 0 保留 0。

完整三项合法为 complete，部分主 Token 合法为 partial，usage 空对象为 missing；主值或从属约束非法为 invalid。invalid 记录 Token 统计列全为 null（诊断不能含原始正文）；partial 详情显示已知量，汇总仍按既有“完整记录才计 Token”口径。增加 total_tokens_derived bool 仅标导入器推导值，proxy 默认 false。

credit 接受非负普通十进制、最多 9 位小数，乘 10^9 后 checked i64，单条 ≤10^18 nanos；科学记数法/负值/超精度/溢出则 credit=null + credit_invalid 计数，不能影响合法 Token。累计 i128，API 十进制字符串；币种仍只有 USD/CNY，credit 不加入 Currency 枚举。

## 3. SQLite v7

当前实现升级到 schema v9 / parser v2：迁移仅为既有文件安排有界完整重扫，不重置增量游标，不删除事实、namespace 或清除墓碑。正常重扫补录过去忽略的收费阶段；模型行去重身份和原有语义 hash 保持兼容。对 parser v1 已导入且积分原本为 NULL 的记录，仅当其余语义字段完全一致时补齐新识别的 usage.credit；已知积分、Token 冲突、proxy 明细和删除墓碑不会被覆盖。升级前由现有 SQLite backup 流程保留一致性备份。

`schema-v7.sql` 是**待实施迁移的可执行设计稿**，只可在临时数据库验证；业务编码阶段按 S02 将其落实为 migrations/0007.sql 并通过 Rust 实际迁移。不可直接执行到用户数据库。本文件语义契约优先；设计稿与实现不一致时必须修正并重跑验证，不可跳过。

### 3.1 usage_requests：继续作为唯一统计事实表

新增 `record_origin`（proxy/workbuddy_builtin/workbuddy_custom，旧数据默认 proxy）、`time_basis`（request_start/message_timestamp，旧数据默认 request_start）、`total_tokens_derived`（bool）、`workbuddy_credit_nanos`（nullable 非负整数）。保留原 source，扩展 workbuddy_import；usage_source 扩展 workbuddy_jsonl；outcome 扩展 recorded（仅表示日志存在，不表示成功）。

导入行约束：source=workbuddy_import、record_origin 为 workbuddy_*、outcome=recorded、run_id=NULL、stream=NULL、upstream_protocol=NULL、upstream_started=NULL、HTTP/耗时/首内容/TTFB/error_kind=NULL、time_basis=message_timestamp。started_at_ms 与 ended_at_ms 均用源事件 timestamp；duration_ms 必须 NULL；不能拿两时间相减伪造 0ms。usage_source=workbuddy_jsonl（包括 missing/invalid）；新导入的 parser_version 为导入契约版本 2，未改变的历史行保留原版本。

代理行：保留 run_id FK、stream、protocol 原 NOT NULL 的逻辑约束和原 source/outcome/usage_source 集合，time_basis=request_start，workbuddy_credit_nanos=NULL。不因允许导入 null 而放松 proxy 行完整性。

主表 provider/route 的人工归属值固定：builtin provider_id=`wb:builtin`，provider_name=`WorkBuddy`；直连 provider_id=`wb:custom:`+SHA256(requestModelId) 前 48 hex，显示 `WorkBuddy 自定义 / WorkBuddy custom`；route_id=`wb:route:`+同 hash，route_name=requestModelName（验证后可显示）或 requestModelId。不会创建供应商配置，也不把导入伪供应商加入可编辑供应商侧栏/价格抓取 targets。

upstream_model 使用 providerData.responseModelId → providerData.model → requestModelId 的首个有效值；response_model 仅保存明确响应值，否则 NULL。UI 无响应值时标“请求模型”，不能显示为已确认响应模型。request_id=entry_key；不额外生成随机 ID。原 proxy 行的 id、自增排序、费用快照逐字保留。

### 3.2 workbuddy_import_files：文件检查点

| 字段 | 契约 |
|---|---|
| file_key / namespace_id / relative_path | file_key=SHA256(namespace+relative_path)；namespace+relative_path 唯一；不暴露完整路径到 API |
| generation / revision | 从 1 起，替换/截断提升 generation；每次提交提升 revision，乐观并发检查 |
| committed_offset / observed_size / modified_ms | 非负 i64；offset 指向下一条未处理数据，不能越过未完成行 |
| prefix_hash / anchor_hash | SHA256，小文件按实际长度；首次读取为空；同文件增大导致 prefix window 增大时按已保存 prefix_len 比较 |
| prefix_len / anchor_start / anchor_len | 明确散列覆盖范围，防止小文件追加误判替换 |
| discarding_oversized | 大行排空状态，重启继续，计一次 oversized |
| scan_offset / scan_generation | 完整核对独立游标，不破坏增量 committed_offset |
| last_seen_at_ms / last_completed_at_ms / last_error_code | 仅固定错误码；权限错误不标完成 |
| diagnostic_counts_json | 固定错误码→非负计数，最多 32 项；无任意字符串正文 |

文件暂时消失保留游标，不清 ledger；大量旧检查点可在 30 天未出现后删除（不删 ledger）。再次发现全读仍去重。重新发现无需按文件路径当消息身份。

### 3.3 workbuddy_import_ledger：持久身份账本

entry_key PK（与成功导入 request_id 相同）；namespace_id、session_hash、message_hash、requested_model(nullable)、event_at_ms、normalized_hash、state、request_id nullable FK ON DELETE SET NULL、candidate_json nullable、mapping_id nullable、mapping_revision、created/updated。

state 固定：`imported / excluded_proxy / excluded_retention / pending_classification / conflict / deleted_manual / deleted_retention`。duplicate 是计数，不新增状态。candidate_json 仅 pending_classification/conflict 允许（≤16 KiB，白名单类型）；其余置 NULL。已排除代理无需保存 tokens；账本不是第二张统计表。

唯一键先查再插，不允许 INSERT OR REPLACE；同事务处理账本、明细、checkpoint。人工清除/retention 先更状态再删主表；FK 只能 SET NULL，不能 CASCADE 抹掉账本。deleted_* 在任意重扫均不可恢复。冲突记录不进入主表；已有 workbuddy 行发生冲突则撤回并提升查询修订。

### 3.4 workbuddy_model_history：历史归属

mapping_id PK；namespace_id；requested_model（精确完整 ID）；classification=`proxy/direct`；valid_from_ms（含）/valid_to_ms（不含，NULL=无上界）；evidence=`observed_config/managed_sync/manual`；state=`prepared/verified/conflict/revoked`；route_id/provider_id nullable（只内部 ID，无 URL/Key）；revision；created/updated；reconcile_state（pending/running/completed）、reconcile_watermark、reconciled_rows、reconcile_error_code（固定码）。

同模型两个 verified 区间禁止重叠，使用 SQL trigger 加 Rust 校验；相邻区间合法。人工确认不跨越已 verified proxy 区间；不能通过 UI把已证实代理改成直连。新 observed 时间点关闭旧 open 区间并新建，旧记录不 rewrite。读取时间范围内匹配 0/1 条，>1 条为 conflict，绝不挑最新。

manual 规则只限有 pending 候选的精确模型 ID；预览绑定 ledger 最大 rowid/时间边界、mapping_revision、受影响条数和 token；确认 token 有效 60 秒，一次使用；选择“排除代理记录”或“用户直连”明确写入。撤销仅 manual，先预览已导入受影响数量，再将其 workbuddy 行回 pending 并从总量撤回；已删除 tombstone 不复活。没有付费金额或聊天内容输入。

### 3.5 stats_settings 增量字段

`workbuddy_import_enabled INTEGER DEFAULT 0`，`workbuddy_namespace_id TEXT DEFAULT ''`，`workbuddy_mapping_revision INTEGER DEFAULT 1`，`workbuddy_last_full_scan_ms INTEGER NULL`。沿用 settings.revision 做 settings CAS；不能新增独立 JSON 作为持久化权威。运行态状态、队列长度、当前轮计数可在内存缓存，但 restart 后从文件/账本计算 durable counters（分页/SQL聚合），不得把所有累计量重新设为 0 当历史事实。

### 3.6 费用与查询视图

导入行不创建 usage_costs；RequestRowDto.cost=NULL，详情 price_snapshot=NULL；货币汇总只包含原 cost rows，导入数纳入 unpriced_request_count。注意当前 query::has_classified_costs 优化路径和 scan_costs 的 NULL JOIN 行：必须显式补算“workbuddy 行且无 cost row”的 unpriced 计数，所有分组/趋势/详情一致。

stats_request_facts 对 usage_source=workbuddy_jsonl 且 complete/recorded 的行允许 counted_usage；原 proxy 逻辑不变。所有 SQL 入口统一应用过滤器，包括 filters 下拉、dashboard、series、requests、detail授权/筛选范围、breakdown。不能只改页面表格不改总卡片。

## 4. 筛选和报表契约

新增 `SourceScope {all,builtin,supplier}`，`CaptureMethod {all,proxy,workbuddy}`；默认均 all。组合 all+all 全部；builtin 仅 workbuddy_builtin；supplier 包含 proxy/workbuddy_custom。builtin+proxy 为 invalid_argument，UI 切到 builtin 自动把 capture_method 重置 all。include_tests 只控制 connection_test，不再用 `source='relay' OR include_tests` 的旧条件（它会误排除全部导入记录）。

StatsFilter 新字段都进入 filter.key；增加导入数据修订 import_revision 到 pagination/summary consistency，并纳入 cursor 校验。追加记录继续沿用 as_of_id 水位；会改旧行的冲突撤回/归属撤销/删除必须提升 existing maintenance_revision 使游标失效。普通 append 不让每页都失效；旧游标遇到 API3 格式不匹配返回 stats_cursor_expired。

Summary 新增 `proxy_request_count`、`workbuddy_record_count`、`status_known_count`、`workbuddy_credit_nanos: Option<String>`、`credit_known_count`、`pending_import_count`。request_count 为可见事实行数，UI 标签改为“用量记录”；finished_count 只统计已终结 proxy 状态（recorded 不进入），success_rate=proxy completed / proxy finished（既有口径），纯导入集合为 NULL；详情状态为“已记录”，不显示 HTTP 200。平均耗时、TTFB、tok/s 仅使用有实际数据的 proxy 行。

usage_eligible_count = 原 proxy eligible + 可见 workbuddy recorded；coverage 只表示“已记录条目的用量完整度”，不代表 WorkBuddy 全部真实调用覆盖率。pending_import_count 是当前日期、来源可对应范围的待确认数；待确认无确定来源时只在全局导入状态显示，不强行落某个来源的覆盖率分母。

request row 新增 record_origin、time_basis、total_tokens_derived、workbuddy_credit_nanos；stream→Option<bool>、upstream_protocol→Option<String>；Source/Outcome/UsageSource 同步扩展，共享 DTO 测试不得遗漏 enum label 的 exhaustive match。

## 5. 管理 API（仍走 /_control/action）

全部使用现有 management_key 和本地来源校验，供应商 API key / local model key 不能调用管理命令。envelope 沿用 `{message: JSON字符串}`；统计响应 api_version=3，request_tag 回传，响应上限 1MiB，禁止另开端口/公开导入 API。

| command | 请求参数 | 响应/行为 |
|---|---|---|
| stats_workbuddy_status | request_tag | state, enabled/effective_enabled, root_found, settings_revision, mapping_revision, last_incremental_at_ms, last_full_scan_at_ms, files_seen/files_pending, imported/excluded_proxy/pending/conflict/deleted counts, bytes_processed, last_error_code |
| stats_workbuddy_settings_update | enabled:boolean, expected_revision:i64 | CAS 返回新 revision；开启建立 namespace；关闭/全局关后停止新提交；不清历史 |
| stats_workbuddy_scan | mode:incremental/full | 返回 scan_id/state；只排队一次，相同任务运行时返回同 scan_id；disabled 返回 import_disabled；不在 HTTP 内扫描 |
| stats_workbuddy_pending | cursor?, limit:20/50/100 | 按模型+原因分组；model_id/count/first_at/last_at/reason；无原始正文与绝对路径；cursor 带 mapping revision |
| stats_workbuddy_mapping_preview | model_id, from_ms, to_ms, classification:direct/proxy | affected_count, exclusion/conflict counts, token, expires_at_ms；只涉及待确认 ledger 水位 |
| stats_workbuddy_mapping_apply | token | 返回任务 ID、mapping_revision；分页事务应用，不能 UI 批量逐消息请求 |
| stats_workbuddy_mapping_revoke_preview | mapping_id, expected_revision | 仅 manual；受影响已导入条数、token |
| stats_workbuddy_mapping_revoke | token | 分批撤回导入，不动 proxy，删除 tombstone 不复活 |

新错误码固定：workbuddy_import_disabled / root_missing / permission_denied / invalid_path / unsupported_format / stale_batch / mapping_conflict / source_changed / source_read_failed / ledger_conflict / scan_busy。对外加统一 stats_ 前缀（例如 stats_workbuddy_import_disabled）；返回码不能夹带文件内容/SQL/密钥。CAS 继续 stats_revision_conflict，DB failure 继续 stats_unavailable；error text 双语由 UI 按 code 映射。

状态机：disabled → discovering → backfilling → idle；idle → scanning → idle；任意活动态可 paused（全局关）/disabled（导入关）；缺目录 root_missing，权限/I/O/DB问题 degraded；单个坏行增加 skipped，不把全局状态变成致命失败。自动扫描可继续其他可读文件，存在未解决错误时不可显示“全部同步完成”。pending_classification 是数据质量计数，不是阻塞全部扫描的状态。

## 6. 事务伪代码

```text
BEGIN IMMEDIATE（现有 writer）
  verify settings effective_enabled + checkpoint CAS + namespace
  check mapping revision; if stale → rollback + reread/reclassify
  for normalized item:
    verify source fingerprint validation result for this batch
    if identity absent → file diagnostic only
    if ledger deleted_* → skip
    if same key + same semantic hash → duplicate (retain state)
    if same key + different semantic hash → withdraw workbuddy fact, conflict
    otherwise classify with current durable mapping:
      proxy → excluded_proxy, no fact
      too old → excluded_retention, no fact
      unknown → pending_classification + allowlisted candidate
      builtin/direct → one usage_requests row + ledger imported; no usage_costs
  update checkpoint and revision
COMMIT
send ACK (commit lost ACK is safely replayable)
```

pending 候选被规则确认不重新读取聊天文件：使用 candidate_json 经过同一 normalize schema 验证后分类入库；导入时重新检查 retention、deleted 状态。冲突候选不能被普通 mapping_apply 当作分类问题直接解除，第一版要求保留冲突并报告，修复规则版本后可专门评审处理。


## 7. 归属修订的持久恢复补充

规则应用/撤销只允许一个用户归属任务在途，第二个返回stats_scan_busy；不影响实时代理记录。mapping行记录reconcile_state、预览冻结的ledger.rowid水位及已处理数，worker按规则ID和受影响区间分批处理。交易失败不推进reconciled_rows；重启找到pending/running继续；每批以ledger.mapping_revision/state判断幂等，不用内存cursor作为正确性依据。

status增加`mapping_task: {task_id, mapping_id, operation, state, affected_count, processed_count, error_code} | null`；task_id为mapping_id与revision的稳定组合，operation=apply/revoke；有新任务前保留最近完成结果，重启从mapping行恢复。scan状态同样返回scan_id；一个full/incremental任务在途，不允许无限排队。

撤销manual direct时，从统一事实行重建白名单candidate后，事务内撤回事实并回pending。撤销manual proxy时，原ledger可能没有candidate：回pending且candidate=NULL、排队full scan补读；找不到源文件时明确pending不可恢复，不伪造usage。相同key/hash但处于pending且缺candidate的重读允许补齐candidate并重新分类；不能套通用duplicate no-op。已verified managed_sync的代理事实不会被人工规则撤销。

mapping revision变化后，对受影响模型/区间的pending及相关imported ledger重核；未受影响记录不全表反序列化。旧mapping_id引用保留，不删除历史行。worker不把reconcile失败当完成；UI显示任务进度，snapshot规则确认影响的是预览水位内已有记录，未来晚到记录仍遵循同一时间区间规则。

## WB-RT-001：调度补充

固定5秒后台调度不依赖UI请求或页面生命周期；full任务分片之间服务到期增量及目录发现，复用单worker和事务ACK。自动tick与手动incremental合并为至多一个待办，不启动第二个扫描。last_incremental_at_ms只在增量检查成功完成后更新，排队或失败时不得提前更新。不增加API字段。

## 接手修复接线补充（WB-FIX-001）

人工归属预览在现有唯一writer保留最多32项的随机一次性缓存，期限使用monotonic 60秒；`apply/revoke`仅按token使用冻结参数，失败也消耗token，用户需重新预览。确认返回`task_id/mapping_id/mapping_revision/state/affected_count`；task ID由规则ID和revision构成，按现有history reconcile字段持久恢复。迟到记录单独后台补分类，不改第一次用户任务水位。

`stats_workbuddy_pending`原分组查询参数不变；增加`view:"rules"`用于U11历史manual规则列表。请求`limit:20/50/100,cursor?`，返回`rules:[{mapping_id,model_id,from_ms,to_ms,classification,state,revision,reconcile_state}],next_cursor,mapping_revision`。该视图只读manual，不返回自动代理规则，不暴露路径/凭据。cursor绑定mapping revision，修订后返回`stats_cursor_expired`，客户端清页重查。

`stats_workbuddy_status`增加`mapping_task:{task_id,mapping_id,operation,state,affected_count,processed_count,error_code}|null`与`mapping_tasks_pending`；未完成任务不得报全部完成，事务失败保留失败码和未推进的processed_count，后台可重试并在成功提交后清错。

筛选候选`stats_filters`同时接受`source_scope:all/builtin/supplier`和`capture_method:all/proxy/workbuddy`，缺省均all，非法或builtin+proxy拒绝。provider/model候选按真实record_origin与采集方式查询；仅允许proxy的范围才补当前配置但无历史流量的provider。

status增加`skipped`：按文件持久diagnostics汇总语法、时间、身份、超限及结构异常，不把正常ignored/无usage消息当异常；U10第5计数使用该字段，文件数仍独立为files_seen。

### 后台提交的单调时钟诊断（R09验收增补）

`stats_workbuddy_status`增加可选诊断，不增加命令，不改变页面刷新与5秒导入周期：

- `monotonic_clock_id`：当前统计运行期的随机时钟标识，重启后不可混用。
- `monotonic_elapsed_ns`：该运行期时钟的纳秒数，十进制字符串，避免长运行后的JSON数值精度丢失。
- `last_source_commit`：无源批次提交时为null；否则包含`file_key`（哈希）、`offset`（已提交主游标）、`full`（是否完整核对）及`commit_ack_elapsed_ns`（同一单调时钟的十进制字符串）。
- `source_commit_receipts`：最多32个不同文件的最近成功回执，字段同上；同一文件覆盖旧回执，达到上限时淘汰最早文件。用于竞争回填时区分目标文件的提交，避免其他文件的正常提交覆盖唯一回执后无法验证。固定内存上限，不持久化，不存路径或正文。

回执只在唯一writer的SQLite提交成功返回后、发送ACK前更新；失败事务、暂停、读取错误不得推进。它不是源文件创建时间或模型响应开始时间，不包含源路径、正文或密钥。性能脚本用请求发送/接收的客户端单调时钟区间校准服务端时钟，分别记录源flush、提交确认与SQL首次可见时间；跨时钟校准保留上下界，不把壁钟毫秒伪装成单调时间或无误差的精确跨进程时间。

### R09 查询索引与发现调度补充

`src/stats/report_indexes.sql`由现有唯一writer在启动迁移检查末尾幂等安装，包括已是schema7的库。它仅增加覆盖索引，不改变schema版本/事实/归属/费用；未来版本库先被拒绝，不给它补索引。索引创建失败按既有数据库初始化错误处理，不宣称统计已ready。dashboard在相同只读事务中按桶聚合proxy/import两类，复用给总览和趋势；保持查询deadline、行数上限、NULL、币种和精确积分，不使用结果缓存。

发现批次只含路径/哈希/size/mtime/预期revision，不含正文。每轮分批stat登记后，再处理源数据；已有文件标脏仅置last_completed_at_ms=NULL并增加revision，原size/mtime/指纹/offset留给实际源读取核验。状态files_pending包含该NULL完成标记，不能将同长度改写误报已追平。调度提示队列硬上限1024个路径，正常≤1000文件时可覆盖全部；每个提示最多500项/8MiB读取机会，事务ACK后才继续，未完成文件的持久检查点负责溢出/重启后的恢复。源候选批次128项/512KiB、通道容量2、后台固定5秒均不变。


### AC048 生产运行期实测诊断

`stats_workbuddy_status.runtime_metrics`为进程内有界诊断，不改变扫描/刷新周期，不落库：

- `queue_depth`、`queue_peak`：真正成功进入导入专用通道、尚未由writer开始处理的消息数与运行期峰值；失败的`try_send`不增加计数。不是通道容量或Proxy事件队列峰值。单生产者等待ACK，接收与计数使用同一短锁排序。
- `writer_batch_count`、`writer_batch_failures`：实际`Work::Batch`事务尝试与失败数；统计包含成功/失败，不把Observe/Reconcile控制操作伪装成源批次。
- `writer_batch_total_us`、`writer_batch_max_us`：由writer中单调`Instant`测量`apply_work`事务的总/最大微秒数，含失败事务，发送ACK前记录；不含等待源读取或队列的时间。总值饱和不回绕。
- `recent_batches`：最多32个最近源批次样本，每项`elapsed_us`、`candidates`、`committed`。所有批次都计入总数与最大值，不因淘汰样本丢掉峰值。内存固定上限，无路径/正文/密钥。
- `source_io_thread_checks`、`source_io_thread_violations`：在真实文件快照/发现/配置观察I/O边界检查调用OS线程名称是否为独立`workbuddy-import`，记录检查数与违例数。它们不是读取字节计数；正文I/O仍由独立`source_body_bytes_read`证明。

每次进程启动重新计量；不可将运行期计数当持久历史。严格规模脚本记录这些实际数值并检查非零观测、有界队列/样本和零错误线程调用；不能再用常量容量或单元测试总耗时替代实际峰值/批次耗时。
