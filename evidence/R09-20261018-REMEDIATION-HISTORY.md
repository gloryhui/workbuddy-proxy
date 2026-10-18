# 接手修复进度 / 2026-10-17

> Current core24986942 passed537 tests/fmt/Clippy/release but FAILED its first strict1M/single phase at the unchanged600s backfill gate. Post-timeout snapshot has876027 imported+408 proxy facts, exact partial token/credit totals and zero imported money costs; checkpoint940852998/1074000000. This is a diagnostic snapshot, not the exact deadline count or complete import. Evidence: [strict phase failure](../../evidence/R09-20261017-CACHE16-WAL16K-SINGLE-FAILURE.json). No followup stage or parameter change after this result. Goal paused per user one-next-task limit; wait for explicit instructions. Older rows below retain their original binary scopes.


最新：f402c103… 的规模矩阵最终为 9/12 阶段通过，百万行单文件首轮在原定 600 秒回填门槛超时；cc06ee81… 的语句缓存候选同场景仍超时，两次失败及快照保留，后续队列均未执行。受保护的真实 writer 在隔离合成库上的三次诊断中，约 55%/60%/70% 样本停在 fsync；该采样是诊断证据，不是性能验收。语句缓存改动已按哈希精确还原，当前仅调整 writer 自动 PASSIVE 检查点触发值为 4096 页，保留 WAL、NORMAL 同步、128 条事务及 ACK 边界；较大的 WAL 触发值不是硬上限。新候选 537 项核心测试、fmt、Clippy 已通过，release 和严格运行验收待完成。原生验证等待用户稍后解锁；本机 macOS x64 工具可用，实际验收仍待执行。详见[诊断证据](../../evidence/R09-20261017-WRITER-FSYNC-DIAGNOSTIC.json)和[最新状态](../../evidence/R09-20261017-CURRENT.json)。

以下为先前 f402 候选推进过程，阶段性“正在运行”不代表当前进程状态：

当前候选f402c103…（writer SQLite缓存8MiB）已通过537核心测试、fmt、Clippy、release、21组串行功能门禁和16项接口边界；严格规模矩阵正在串行续跑，最新逐轮计数以机器可读状态为准；全12阶段快照与RSS增长聚合尚未完成。百万首阶段回填552.942秒，20次增量最大4.992秒，查询P95最大323.934ms，真实300秒空闲CPU中位0.143%、未变化正文读取不增长。当前核心状态轮询13/13、保留期限53/53补验通过；其余阶段已串行启动，私有包、原生生命周期、完整GUI及80条证据审计仍待完成。真实签名配对已证明程序非签名字节一致，核心/UI实际进程隔离检查通过；原生操作因Mac锁屏暂缓，未计GUI通过。详见[机器可读状态](../../evidence/R09-20261017-CURRENT.json)及[签名与隔离证据](../../evidence/R09-20261017-CACHE8-GUI-IDENTITY.json)。

用户已授权 Codex 接手并开启 Goal，按独立复核 R01～R09 逐项修复。原始复核位于 `dist/workbuddy-import-review/REVIEW.md`；原 DeepSeek 证据保留，其 PASS 不自动视为当前验收通过。

旧ac03候选外置盘失败记录（保留，不套用于当前f402）：六个100k阶段通过后，1M/many回填触发原600秒超时，严格矩阵当前FAIL；快照含896431条唯一事实/导入账本。失败和清理证据见[规模失败报告](../../evidence/R09-20261017-SCALE-FAILURE.json)。同程序观测开销对照也超时，结果保留，不计严格验收，也未改门槛。GUI保护发现与CLI的实际策略差异：现统一禁止真实内容读取和全部写入、保留元数据观察；预热AppKit的未运行原型已撤回，避免提前创建GPUI自己的应用对象。当前签名配对及实际PID保护已验证；CUA原生操作等待解锁。

| 顺序 | 项目 | 状态 |
|---|---|---|
| 1 | R01 Token、缓存、推理、积分贯通事实表 | 新核心黄金/全字段51及完整导入157通过；完整交付随R09 |
| 2 | R02 来源归属与代理镜像排除 | 生产配置观察、同步日志及历史撤回链路已接通，本机回归通过；完整验收随R09 |
| 3 | R03 专用worker、有界增量、轮转、完整行和ACK | 当前537回归、release真实故障与调度通过；f402首个百万完整阶段通过，其余阶段正在串行续跑；旧ac03超时保留 |
| 4 | R04 全局/导入开关一致性 | 真实writer开关与最终release调度/X45独立性通过 |
| 5 | R05 当前保留期限与错误排除恢复边界 | 当前f402保留期限0/7/30/180天53/0，含真实一分钟清理；旧不可证明排除不自动恢复 |
| 6 | R07 扫描任务与真实状态 | 后台任务/真实状态/持久计数已接线；最终调度27/0、OS故障15/0、status成本13/0 |
| 7 | R08 归属预览、确认、撤销和恢复 | 生产预览/确认/撤销/CAS与持久分页reconcile回归通过；完整原生异常态待补 |
| 8 | R06 实际UI控件、事件、过滤与回复接线 | 最终GPUI137测试和原生12图、U03/U04、X45及竞争回填事件预算通过；完整隔离/平台验收继续 |
| 9 | R09 修复假阳性测试、重验并更正原完成状态 | 21个最终串行gate和私有包/原生生命周期通过；严格矩阵、GUI隔离及外部平台仍有缺口 |

执行约束：保护当前worktree和原证据；每项增加有效回归并验证后再继续；不修改旧迁移0001～0006、不新增依赖、不动应用版本；不自动提交、推送、合并、发布。不以外部平台缺失替代尚未完成的本机业务修复。

接手时已确认的反例：当前release下，100/20/120与1.14积分入库全为NULL；未知自定义错误记内置；未换行尾记录提前入库；不限期仍排除旧记录；全局关闭仍导入；110文件多轮仅100检查点。详见复核probe-results.json及expectations.json。核心494单测通过不能替代这些实际链路验收。

## 接手修复证据（增补，保留原报告）

- R01：CommitItem携带规范化usage及实际响应模型，参数绑定写入全部Token/缓存/推理/credit列；使用确定的白名单语义序列化hash，替代Debug hash。新增parse→store字段测试；原batch组6项通过。release隔离复测100/20/120及1140000000纳积分正确（dist/workbuddy-import-review/probe-after-r01-r02.json）。
- R02：提交时匹配持久历史；prepared/conflict优先pending；半开区间分辨direct/proxy，未知custom-local不计内置。新增边界、代理缺本地事实、未决intent回归通过。配置观察和同步hooks的实际持久接线仍需继续，不能据此宣称全部来源功能完成。
- R03～R05：新增runtime.rs负责专用OS worker；只读SQLite取检查点，唯一writer通过独立容量2通道逐批提交ACK。发现迭代器跨slice保留；每批128候选/512KiB规范化数据、读取8MiB/50ms；未变只stat，完整行边界和超限排空状态可恢复；当前retention及两个开关由writer复核。新runtime两项真实线程/真实SQLite回归通过（110文件、准确数值、空闲5秒正文读取不变、无换行尾、不限期400天、未知自定义pending、全局关停止、5秒内退出）；原reader组67项通过。
- 发现并修复共享settings revision缓存失配：import开关更新后同步writer.settings及inner.settings，避免后续全局更新用旧revision。普通追加不提升maintenance_revision，冲突撤回才使旧游标失效。
- 原始测试日志：dist/workbuddy-import-review/store-after-r01-r02.log、runtime-tests.log、reader-tests.log。定向结果不代表完整性能/GUI/平台验收通过。

## 当前续工锚点（2026-10-17）

- 核心完整回归在最初runtime接线后498/498通过；后续新增API及来源矩阵测试后应再次全跑，不能沿用498作为最终数量。
- R07：真正的queued/running/completed/failed任务已接worker；full用scan_offset/scan_generation独立检查点，每slice让出，自动24小时核对；同任务ID合并，缺根/权限/读取问题不得伪完成。新增真实full隐藏中间修改回归、扫描合并、全局暂停状态回归；runtime组4项通过。source_reader错误可上报。
- R01新增API红绿证据：api-row-red.log确认原请求明细接口遇到import NULL stream/protocol会失败。已修DTO Option、recorded/workbuddy_jsonl枚举、真实record_origin、time_basis、derived标记和积分；移除row关键枚举silent fallback。api-row-green.log目前通过：后台→SQLite→明细→详情→总览Token/精确积分/无伪成功率/导入未定价110/状态已记录。
- 积分总览改i128有界流式累加，API字段按DATA-API命名workbuddy_credit_nanos（未知null）、credit_known_count；proxy_request_count/workbuddy_record_count映射正确。UI精确十进制显示，1.14不能当1.14B计数，零保留，1纳单位可见。查询测试已覆盖两个i64::MAX积分累加不溢出。
- R02查询口径修复：原scope_predicate错误将custom归builtin，已改为builtin仅workbuddy_builtin、supplier=proxy+workbuddy_custom。新增CaptureMethod all/proxy/workbuddy、builtin+proxy拒绝、filter.key包含capture；生产SQL来源×采集方式7组合测试通过。原错误测试预期按DESIGN§4纠正，保持总量严格相等；query组目前21项通过。
- GPUI cargo check在DTO调整后通过；依赖block0.1.6存在预先已有future-incompat提示，不等同新增业务warning。新增积分格式单测尚未实际执行，不可声称通过。

### 接手时的待办记录（历史；当前缺口以上表与ACCEPTANCE-AUDIT为准）

1. R03/R07的runtime.rs当前含集中调度循环、读slice和writer适配，需按DESIGN风格目标拆分worker/writer/status和独立tests；新生产函数目标80行、模块600行，超标必须具体复核说明，不能靠allow。继续补>1000文件、超限排空/截断/替换/源错误恢复和完整性能证据。
2. R09查询尚有待补：breakdown仍用OriginCounts::default，分组来源/积分元数据未接通；混合invalid/partial导入时derived_cost_counts把导入质量当代理费用不全，需严格拆分；Summary缺status_known_count/pending_import_count。查询过滤器下拉也尚未接source/capture过滤。不能把当前总览和明细通过说成所有报表都通过。
3. R02当前models.json观察与实际同步prepare/finalize持久接线仍缺：classify.current_model_id忽略真实配置id；生产proxy.rs install仍传None hooks。需提取安全别名→custom-local身份及不含凭据的观察，当前快照不得倒推历史，prepared崩溃恢复需实际运行。
4. R08所有原人工归属API仍错误，必须修：preview把model_id当mapping_id；apply重新预览导致token永久延长、忽略分类/区间/水位；confirm_pending_rows固定插builtin且丢用量；revoke缺安全预览token和仅manual保护。需要随机一次性60秒token、CAS、持久任务分批/恢复，正确direct/proxy与墓碑。
5. R06真实UI依旧未完成：三图标、过滤参数、导入面板开关/按钮、pending列表/人工归属弹窗、任务状态回复均需事件接线；当前已修的DTO适配不等于这些控件完成。
6. R09既有脚本假阳性、黄金完整三来源链路、X45～47、故障/1GiB1M行性能/平台证据与原PROGRESS/报告状态仍需修正重验。
7. 无提交/推送/合并/发布授权；不要操作真实数据库或WorkBuddy源；目标平台缺失最后如实报告，不可据此掩盖本机业务缺口。

## 最新验证补充

- 完成runtime拆分：runtime.rs（协调/状态）、runtime/worker.rs（源读取/调度）、runtime/writer.rs（事务适配）、runtime/tests.rs（真实线程与API回归）。生产模块均低于600行；worker里的长函数还需继续拆分或具体复核，未假报风格全部通过。
- 明细/详情/总览/供应商分组的真实API回归已通过：Token13200、积分125400000000纳、110条导入费用未知、纯导入代理成功率null、finished_count=0、usage_eligible_count=110；分组来源与积分元数据现已接通。
- Summary补status_known_count和pending_import_count；费用质量分母只计代理，导入未知费用独立计数，已修正default_zero重复计入cost_known_count的问题；混合与性能完整矩阵仍待后续验收。
- 当前完整核心回归502 passed / 0 failed（core-current.log）；核心clippy --all-targets -D warnings通过。
- 已确认并修复GPUI测试SIGBUS的代码根因：usage_table.source_column_tests、usage_chart.currency_line_tests、statistics.metric_text_tests使用super通配导入，导入GPUI test宏遮蔽内建test，宏展开递归导致rustc栈崩。改为显式导入后，新增积分精确显示单测实际1 passed；这不是屏幕录制权限问题，也不能再将这次编译崩溃笼统算外部平台阻塞。正在跑全套GPUI测试。

## 本轮验证检查点

- 核心完整单测：502 passed / 0 failed（dist/workbuddy-import-review/core-current.log）。
- GPUI完整单测：129 passed / 0 failed（dist/workbuddy-import-review/gpui-current.log）。宏导入修复解除此前SIGBUS，不能再把这项归为环境阻塞。
- 核心与GPUI两侧Clippy --locked --all-targets -- -D warnings均退出0；GPUI依赖block0.1.6仍有既有future-incompat提示，未升级依赖。
- git diff --check退出0。未提交、未推送、未合并、未发布；未修改真实WorkBuddy/Proxy数据。
- 本轮只形成修复检查点，未宣称原80项全部重新通过。Goal保持active。
- 下一步从R02当前配置观察/真实同步prepare-finalize接线继续，然后修R08人工归属任务、R06实际UI；R09脚本与严格大规模/故障/平台验收仍须完成。人工归属原API及三图标/导入面板交互尚未完成，不能把129个单测通过当作实际GUI验收通过。

## R02 生产同步与配置观察接线检查点

- `models.json.id` 按真实 JSONL 身份转成 `custom-local:<id>`，不把上游 model 名当路由身份；复用 `workbuddy::managed_signature`，输出只含模型身份、路由和观察时间，不含 key、URL、正文。
- 新增 `observation.rs`、`runtime/observer.rs` 和内部 `stats/workbuddy_sync.rs`。后台固定5秒观测非敏感配置；成功完整快照才关闭消失条目的自动区间；损坏/不可读配置保留错误，暂停新的源读取，不能当空配置或“已完成”。配置正文有2MiB上限，本地key投影4MiB上限；批次≤128。
- 生产 install/restore 已从 None hooks 补齐：实际文件改动前唯一writer持久提交prepared；改动后复读真实文件并提交确认。prepare失败（导入开启）零文件写入；finalize失败返回“已同步，归属未确认”，不假报回滚。
- 活跃同步用可取消lease保护；同步请求取消、ACK丢失或进程重启后，观察实际配置恢复遗留intent。配置快照携带epoch，禁止同步前排队的旧观察覆盖新边界；无任意30秒超时后直接把活跃同步当崩溃恢复的漏洞。
- 时间区间是半开：prepare到确认保留conflict间隙；同毫秒保留1毫秒不确定区间；时钟回退不确认，不把现在的profile倒推为历史证据。
- 暂停观察时截断仍开放的verified区间，已结束历史不变；恢复后自定义旧记录缺证据即pending。导入关闭仍保留原同步可用性；已有namespace且writer可用时继续metadata journal，writer不可用时允许既有同步，无namespace不强制记录。
- R02配置观察/同步的9项SQLite边界与隐私测试通过；之后新增暂停区间测试1项。核心完整回归 `core-after-observer.log` 518/518通过（尚未包含最后新增的损坏profile测试）；定向生产同步7项最新结果以 `sync-runtime-tests.log` 为准。核心Clippy上一轮退出0，最后新增测试后需复验。
- **仍未宣称R02全部验收完成**：已入库历史受区间修订影响后的有界撤回/再分类需和R08共用持久reconcile实现；旧人工归属API仍未修，UI未完成。不能把新配置接线通过代替这些验收。

## R08 人工归属与自动历史修订实现检查点

- 新 `manual.rs` 替换生产路径的四个人工归属命令，删除原错误command实现。最多32个随机一次性preview，monotonic 60秒有效；确认只取token保存的参数，不重建预览、不延长过期、不接受请求覆盖分类/时间/水位。namespace与mapping revision CAS；只允许一个manual任务在途。
- `reconcile.rs` 复用现有history字段持久化任务、水位、已处理数和错误；每批128条，事务内写事实、ledger revision和进度，回滚不推进。重启根据ledger revision继续，不依赖内存cursor。proxy归属只排除、不补造本地代理事实；direct准确写workbuddy_custom及原始Token/积分；当前retention与tombstone重新检查。
- 撤销只允许verified manual规则；managed_sync拒绝。direct撤销从事实表规范化列重建候选，保留Token、缓存、推理、积分；proxy撤销缺候选时保持pending并排队full reread。仅pending+无candidate+相同semantic hash允许源重读补齐；墓碑仍永久不恢复。
- `history.rs` 为自动观察、同步intent和人工确认共用区间修订。明确确认仅切分指定范围内的conflict间隙，两侧未知历史保留；prepared或verified代理证据不能被用户规则覆盖。所有新/受影响区间排队有界reconcile，不只影响未来新读入的记录。
- 预览后新增的旧pending记录由独立catch_up处理，不改变用户任务冻结的watermark/processed_count；catch_up每批128，存在backlog即继续，不每128条额外等待5秒。
- 状态API新增持久mapping_task与mapping_tasks_pending。写失败保留固定错误码，不显示完成；恢复成功清除任务错误。原8命令数不变；`stats_workbuddy_pending`增加`view:rules`、同样20/50/100分页及revision绑定cursor，提供仅manual的历史规则，供R06实际UI接线。
- 专项真实SQLite测试覆盖随机/过期/单次/修订冲突、半开边界、300条分批和回滚重启、精确积分、代理撤销候选缺失、未知区间切分、managed保护、迟到300条的128/128/44批次、tombstone和当前retention；实际worker/API测试覆盖direct→撤销→proxy→full补读→direct，以及生产同步撤回已误导入的direct事实。
- 上一个完整核心检查点528/528通过；GPUI129/129通过；新增迟到/墓碑/规则历史测试后，最新完整核心结果以 `core-after-manual.log` 为准。核心Clippy最新 `clippy-manual.log` 退出0。
- **剩余边界**：旧store内preview_mapping/claim_mapping_task/confirm_pending_rows等错误脚手架仍被旧startup contract和旧单测调用，已不在生产command路径，需R09改成真实链路自检后删除，不能把旧自检算作新业务证据。R06图标/面板/弹窗实际事件仍待接线；worker中run_worker/read_file两个长函数待拆分；大规模/完整故障/平台/真实GUI严格验收仍未完成。Goal保持active，不提交、推送、合并或发布。

## R06 实际控件接线与真实窗口核验检查点

- 新 `gpui/src/statistics/source_controls.rs`：三SVG来源图标组作为筛选条第一个完整组件，RadioGroup/RadioButton语义、tooltip、焦点、左右/Home/End键；当前选项重复点击不重置。来源与采集方式变化清供应商/模型/状态、清旧option身份和页码，保留日期/合法用量条件；builtin锁采集下拉且effective请求all，清不兼容测试连接。页面关再开重置all/all。
- 所有报表filter包含source_scope/capture_method。供应商/模型候选API与SQL也接两字段；builtin/WorkBuddy capture不混入未产生数据的代理配置。真实SQL7组合×provider/model及配置供应商分页回归通过，修复原配置候选超过一页却不返回next_key的问题。
- 新 `import_controls.rs` 实际放到“采集与存储”展开区底部：持久开关CAS/失败回滚、检查更新/完整核对真实write intent、状态/两个时间/六计数、错误回复和检查按钮禁用。status回复有seq检查，API版本及计数验证。页面刷新仍默认关闭，不控制后台5秒同步。
- 修正原六计数把“文件数”当“跳过异常”的错误；核心status汇总真实diagnostics，忽略普通ignored/无usage消息；独立真实worker回归证明syntax/time/identity错误计3、正常ignored不混入。
- 真实窗口揭露并修复两个额外集成错误：旧overview把输入/输出/缓存重复渲染为14卡，改为唯一`overview.rs`九卡组合与5/3/2列断点；显式筛选的debounce原先被window active条件卡住，改为可见且无模态时提交，自动周期刷新仍遵守前台规则。
- 九卡货币走i128字符串formatter，移除parse_i64().unwrap_or(0)；点数保持精确小数。增加实际composition回归，验证九项顺序、未知/0/1.14及布局断点，不只是孤立formatter。
- 明细真实TableDelegate改11列：时间后加来源，末尾加点数；原列保序，recorded灰色、无伪HTTP/耗时/速度/费用。宽度测试按新增两列更新合法水平滚动预期，时间仍固定110，source默认100/最小80、credits默认90。原tab改“用量明细”。

### 当前真实GUI证据（只覆盖本机中文宽窗口，非完整G08）

- 隔离fixture：`dist/workbuddy-import-review/ui-live-1792205810021541000/`。核心和GPUI都用新建HOME/config；当前JSONL与mock上游全是synthetic，未读写真实WorkBuddy/Proxy数据。
- 三类事实实际经生产链：代理550 Token、内置120 Token/1.14点、直连240 Token/2.28点。真实窗口核验全部910/3.42点、builtin120/1.14点、supplier790/2.28点；按Right键从supplier循环到all，核验910。仅WorkBuddy采集360 Token、成功率—、3.42点。
- `all.png`、`builtin.png`、`supplier.png`及对应AX文本；截图只取测试应用窗口，未截桌面或其他应用。窗口里的原生指标容器带精确数值，避免截图hash不同却数据相同的假阳性。
- 原生开关关闭后SQLite workbuddy_import_enabled=0，已有3事实保留；重新开启=1，总Token仍910。`import-paused.png/import-enabled.png`保存面板状态。点击完整核对后scan_state=completed/mode=full，事实仍3条、910Token、3420000000纳点；`ui-check-result.json`保存真实API与SQLite断言。
- 这不是全部12图矩阵，不是所有平台/显示缩放/安装验收；尚未进行来源确认/撤销真实弹窗、窄窗口、英文、详情全矩阵。

### 最新可复现回归

- 核心 `core-latest.log`：534 passed / 0 failed；`clippy-latest.log`：--locked --all-targets -D warnings退出0。
- GPUI `gpui-latest-tests.log`：131 passed / 0 failed；`gpui-latest-clippy.log`：同等Clippy退出0。block0.1.6既有future-incompat提示保留，未改依赖。
- `git diff --check`退出0。核心/GPUI debug build均成功，隔离测试.app仅为本机UI测试容器，不是安装验证或正式Release。
- 下一步：先补R06 pending/rules列表和确认/撤销原生弹窗、任务完成刷新与过期/CAS错误，再做完整GUI矩阵；随后R09清理旧错误store自检脚手架、重写假阳性脚本、拆长worker函数、跑大规模/故障/平台验收。Goal不得标complete；无提交/推送/合并/发布授权。

## R06 来源确认与撤销的真实操作检查点

- 原生 pending/rules 弹窗已接生产命令，模型只读、毫秒本机时间、终点不含、默认不选分类、预览与最终确认分离。修改时间会立即废弃预览；关闭、切页、改变分类和预览到期均不保留可用旧 token；提交只传一次性 token。
- 新增弹窗响应校验测试：毫秒往返与非法日期/逆序区间、非法预览token/负计数/非法revision、异常任务进度。Clippy的items_after_test_module已通过移动函数解决，没有加allow。
- 真实GUI操作：新增待确认记录360Token/3.42点，原总量910Token/3.42点。先验证无默认选择及确认禁用，再预览影响1条；修改终点后确认按钮禁用，再重新预览、确认。SQLite变为4事实/1270Token/6840000000纳点；关闭弹窗后真实dashboard AX同步为1270/6.84。
- 真实GUI撤销：只选新增manual规则，预览影响后确认；SQLite恢复3事实/910Token/3420000000纳点，原ledger回pending_classification；规则显示已撤销。关闭弹窗后真实dashboard及待确认入口恢复910/3.42/1。源JSONL未修改。
- 证据目录：`dist/workbuddy-import-review/ui-mapping-1792207250954792000/`，含未选/预览/修改废弃/确认/撤销窗口截图、AX、两个只读DB快照和`mapping-check-result.json`，hash对应实际运行的隔离.app binary。仅证明本机中文宽窗口这条业务链，不是完整GUI矩阵。
- 核心完整回归535/535（core-mapping-tests.log），核心build及Clippy退出0；GPUI弹窗阶段134/134（gpui-mapping-tests.log）。补健康提示/详情元数据后GPUI135/135（gpui-notices-tests.log），build与Clippy退出0；该补充的新GUI尚在复核，不能沿用旧截图算通过。
- 新补U07：WorkBuddy健康文本、最多两条按错误/待确认/冲突/目录/未开启优先的提示，待确认直达弹窗；设置入口展开并滚至导入区。前台状态轮询不再依赖storage展开，不受页面刷新间隔控制。新补U09：详情顶部来源/采集方式/时间依据/推导标志/精确点数/未知stream。U10清除预览增加保留去重标记且不修改源文件说明。
- 仍待完成：上述新增提示/详情实际事件，pending/rules真实分页与超时/CAS异常，窄屏/英文/缩放矩阵；R09旧错误自检清理、长worker拆分、严格集成/性能/故障/平台验收。Goal保持active；无提交、推送、合并、发布。

## R06 补充真实事件及 R09 本机清理检查点

- 最新隔离窗口目录`dist/workbuddy-import-review/ui-mapping-1792208019150875000/`：57组待确认；20条第一页000～019/第二页020～039，50条第一页000～049/尾页7组，100条页57组唯一，均通过真实控件事件与AX集合断言，保存分页AX/截图。
- 发现并修复轻量status查询依赖window active导致可见页提示陈旧：现在仅要求统计页visible，2秒读已入库状态，不读JSONL；来源数据后台仍固定5秒、报表自动刷新仍保留原前台/间隔策略。真实验证storage收起、页面刷新关闭时，源追加使提示56→57，总量910/3.42不变。
- 真实内置详情核验：来源内置、WorkBuddy JSONL、消息时间、报告/推导口径、1.14点、未知stream/HTTP/耗时/速率、未定价，无价格快照；新增元数据用AX可读，详情限制高度并有独立滚动，实际滚至底部后能关闭。
- 60秒预览真实过期：原影响1条消失、确认禁用、固定中文提示重新预览，点击不新增事实。真实CAS异常：预览后经管理API新增零影响的另一manual规则，旧预览确认被拒绝，显示固定冲突提示，原记录仍pending；无SQL/私有路径回显。已保存expiry/CAS截图，当前数据库3事实/910Token/3420000000纳点、pending57。
- worker长函数已按职责拆为`runtime/worker.rs`（ACK/入口/安全路径）、`worker/scheduler.rs`（436行、暂停/观察/任务/发现/积压/完整核对/ACK推进）、`worker/file.rs`（279行、快照/变更检测/续读/有界slice/提交候选）。新增函数均≤80行，模块≤600行，无压行/allow；只在ACK成功后推进full cursor。两轮实际runtime各15/15通过，拆分后完整核心535/535、Clippy退出0（core-after-worker-split.log / worker-file-split-clippy.log）。
- 删除旧`preview_mapping/claim_mapping_task/revoke_mapping_rows/confirm_pending_rows`及旧15分钟确定性token类型。删除旧task_tests与3项依赖伪确认的测试；其中保留期/墓碑语义已由真实manual命令回归覆盖。旧错误API不再存在于生产或测试。启动自检新增`manual/contract.rs`走生产8命令入口：随机60秒token、单次消费、精确直连用量/积分、持久reconcile、撤销回pending；store自检仅保留存储原语检查。
- 清理后核心完整526/526，Clippy退出0（core-legacy-cleanup.log / core-legacy-cleanup-clippy.log）。数量从535减为526是淘汰10项旧脚手架、增加1项实际命令自检；不把测试数量减少掩饰成未运行。
- 新`scripts/workbuddy_import_golden.py`替换原仅1条builtin也能通过的mixed chain：管理API开关、生产install生成managed_sync证据、真实本地Mock代理请求550Token、builtin120/1.14点、direct240/2.28点经预览确认；镜像JSONL真实排除。全流程没有SQL写事实/设置/归属，SQL只读断言。
- 黄金定向17/17通过（golden-r09.log）：五种来源/采集组合的dashboard与列表、纯导入成功率null、详情未知transport与精确积分、full后仍3事实/910Token/3.42点、ledger唯一3个身份且proxy镜像1条排除。此定向runner报告计数17/0但result原为NOT_RUN（runner未设置汇总状态），不将该JSON的result视为PASS；实际退出0与日志断言为本次依据，完整主脚本重跑待续。
- 原集成/故障/bench/UI脚本仍有待审的宽松断言，不能将上述定向结果说成完整S10通过。性能100k/1M/≥1000文件/巨型文件/4并发P95/5分钟空闲、真实磁盘写失败、英文窄屏/缩放及其他平台安装证据仍未完成。Goal保持active，无提交、推送、合并、发布。

## R09 实际写失败、release GUI及性能反例续报

- `os-fault-r09-final.log/report.json`：6项真实检查通过。仅对子进程设置512KiB RLIMIT_FSIZE并忽略SIGXFSZ，实际WAL达到524288字节、事务失败且提交0条；代理真实上游调用仍返回550Token。解除限制重启后5000条/60000Token/500000000000纳点全部恢复。此项是实际OS文件写拒绝EFBIG，未称物理磁盘填满。
- 同一独立故障链：已有5000条，再写50000条；实际观察提交5128条时SIGKILL（新回填仅128条已提交），重启后精确55000条/660000Token/5500000000000纳点；ledger总数与唯一entry_key均55000，未拿已完成的空闲进程强杀冒充回填崩溃。
- 两侧release构建均成功。`ui-release-1792209558446363000/`完成3来源×2尺寸×2语言12个原生事件+数值断言；截图实际是CUA返回的JPEG，未放大、拼图或生成图像，原生尺寸2880×1800和2200×1400；矩阵manifest标明2x backing、1x未验。人工查看英文宽/窄截图后发现并修正旧英文副标题仍只称代理、pending单复数文案；新GPUI135/135及Clippy/release均通过，最终文案窗口复验待续。新隔离release窗口`ui-release-1792210598532584000/`仍为旧核心（提交时钟/趋势修复之前），不能将它当后续源码最终证据。
- 主集成脚本已有两次停机冷读失败、一次推进至升级阶段再冷读失败，日志均保留，未算全脚本通过。已实际复现本机Apple SQLite3.43.2对已停止且无WAL/SHM的WAL模式库，mode=ro查询报unable to open，immutable只读和正常rw可读。脚本现在只在明确stop后、确认无非空WAL/journal时才用immutable冷快照；活跃库仍普通只读，非空journal严格拒绝。修复后升级/回滚定向12项通过（upgrade-r09.log）；完整主脚本最后重跑仍待续。
- 新严格性能脚本`workbuddy_import_performance.py/resources.py`，源数据不以SQL种事实，100k为134300000字节/1000文件；400个真实Mock流式代理请求，分导入开关各200个/4并发；RSS记录全核心总峰值（可保守约束worker额外内存），不称按线程RSS。黄金/性能子进程现在先复制被测核心binary，防止后续编译更换可执行文件而证据hash指错版本。
- 100k首轮得到实质反例：20/20次500条增量均≤10秒（最高5435.713ms），回填约27秒、总RSS约17MiB、4并发P95变化约0.646ms；但真实dashboard查询P95最高1178.289ms/30天550.429ms，超过500ms（scale-100k-many-final.log/report.json，result=FAIL）。不能用旧脚本的简单COUNT/SUM代替它，未降低门槛。
- 性能定位还揭露趋势SQL仍用source=relay，导致导入记录在总览有值而趋势缺失。新增真实110文件回归先红（series-import-red.log，趋势0而应13200），改为与统一筛选同样排除connection_test后转绿，黄金链路新增5组合的series计数/Token必须与summary一致。
- 查询首轮优化：来源计数/精确积分/导入未定价合并一个SQL扫描，积分用1e9整数高低位SUM后i128重组，1M上界内两位均不溢出，不经REAL；代理费用扫描明确限制proxy，避免给大量不可能有费用行的导入逐条查价格。完整527/527及Clippy退出0（core-after-query-optimization.log / query-optimization-clippy.log）。优化后的release与性能重测仍在进行，尚未宣称达标。
- 新`runtime/metrics.rs`提交诊断及真实线程回归：事务回滚期间回执/offset不动，解除错误后才推进；纳秒使用字符串，时钟运行期标识固定，公开字段只含哈希和游标。为严格验收提供flush/提交确认/查询可见三个单调时钟观测，校准保留区间误差，详见DATA-API增补。
- 性能查询冻结语料日期窗口，确保百万语料查询恰好匹配1000000条；400个代理控制请求及后续增量时间在窗口外，避免把有意新增的控制记录超过原1M查询行预算，再错误要求关闭预算。不减少百万语料本身、不改变超时或500ms门槛。仍缺百万/单巨型/5分钟空闲/完整核对正文IO/新目录竞争回填/跨规模RSS比较，以及1x和其他平台安装验收。Goal保持active。

## R09 严格矩阵入口、百万查询与竞争回填整改检查点

- 最新完整隔离集成已实际退出0：`integration-r09-current/report.json`，106 passed / 0 failed。包含三来源真实黄金链路、五组合总览/趋势/列表一致、代理镜像排除、原故障矩阵、升级与回滚。它对应本次查询/发现调度优化之前的二进制；优化后仍须重跑，不能沿用为最终程序验收。
- 原100k查询反例保留：优化首轮`scale-100k-many-optimized`仍FAIL，当天dashboard冷测689.542ms；旧1M多文件轮`scale-1m-many-query-probe`实际1000000条/1074000000字节，回填435769.5ms、全核心RSS峰值20463616字节，4并发400次测量无丢事件，20/20次500条增量及单调提交回执达标，但dashboard命令失败，因此该轮FAIL，不能算全部性能通过。
- 旧百万快照用于查询诊断；曾在该合成快照上创建明确命名的`diagnostic_report_cover`，只改索引，未改事实。查询专用runner复制快照后删除该诊断索引，启动当前生产核心创建真实索引，不能把快照称为未经诊断改动的原始文件，也不能把查询runner当新二进制的完整回填证明。
- `query/dashboard_aggregate.rs`将同一读事务内的分桶结果同时用于总览和趋势；代理保留原完整表达式，导入按v7 CHECK的recorded/未知transport语义聚合；费用只扫描真实proxy，导入成本仍未知。积分使用整数高低位累加、i128合并，不经REAL。请求上限1000000与2秒超时未变，无结果缓存。
- `report_indexes.sql`是唯一writer启动时原子创建的可选覆盖索引；新库和既有v7库均幂等补齐，不改0001～0006，不升schema/API/应用版本，不写或重算事实。真实v7约束测试对照原完整SQL及原series：160个来源/采集/测试连接/状态/供应商/模型/路由/时间组合，空态/未知/缓存0/跨桶平均耗时/超i64积分保持一致；验证真实查询计划使用覆盖索引，禁止导入费用行、重复启动不改事实。
- 查询优化后核心531/531、Clippy及release退出0（core-dashboard-aggregate.log / dashboard-aggregate-clippy.log / dashboard-aggregate-release.log）。新查询专用runner`query-1m-aggregate`13/13通过：当天/30天dashboard与第一页明细各50次、共3轮，dashboard最大P95=406.4755ms、明细最大47.614583ms；每轮保留预热与50个原始耗时，只是百万快照查询证据，非整个S10通过。
- 新严格`workbuddy_import_bench.py`按2规模×2布局×3独立轮次核对实际程序哈希、原始查询样本、20次精确增量、内存/字节数/文件数；缺任何阶段返回非零。`strict-matrix`当前0/12，未沿用旧9 PASS。新UI runner只审计CUA窗口JPEG/AX/程序哈希，缺证据NOT_RUN并非零；旧12图在指定旧程序时可验证，拿来验当前程序则FAIL（historical-ui-audit / stale-ui-rejection）。不再按名称取首个窗口、截无关全屏或用截图哈希代替行为。
- 新`scheduling-red`给出实际反例：979个竞争回填文件存在时，已追平文件追加250条，10秒只有128条可查，故FAIL。不是只看总数递增的假X46。普通手动500条5秒内可查、2秒内排队反馈，以及完整核对真实正文计数和不重复先已通过。
- 调度修正：目录发现每批最多100个entry，只有stat与只读检查点查询；唯一writer持久登记/标脏，保持旧offset/size/mtime/fingerprint/full游标直到真实源批提交。内存只保留最多1024个路径的调度提示（无正文/候选），新变更优先，每个提示最多500项/8MiB机会；其余回到持久未完成检查点，重启/溢出不丢事实。完成标记NULL也计pending，防止同长度改写在未读取前假报完成。
- 提交诊断新增最多32个不同文件的最近成功回执，事务失败不推进；用于竞争回填时验证目标文件，而非误取其他回填文件的提交。新增回执有界/去重、真实回滚检查通过。元数据登记的两项真实SQLite回归通过：同长度改写保留旧指纹及offset并标pending；一个批次CAS失败会回滚前面已登记的文件，重试可成功。
- 新调度runtime定向初次16/17；唯一失败是旧测试把“登记3个文件”等同“3文件已处理”的竞态。现在等待实际fact=1且pending ledger=1，仍在既有10秒上限内；无换行尾offset=0、400天历史、不限期及全局关闭原断言保留，定向1/1重新通过（newline-registration-tests.log）。完整runtime及最终核心回归仍需补齐，不能写全部通过。
- 调度优化后的`scheduling-priority`正执行20次双目标严格测试，已通过的样本按原始日志保留；全部结论待最终report。后续仍须完整新程序矩阵、5分钟空闲、巨型文件/关闭、最终GUI/刷新场景、平台与安装。Goal保持active，无提交/推送/合并/发布。


## 新会话接续：报表页缓存与真实趋势图复核（2026-10-17）

- 已读取原会话 `01a143c7-ff35-7df1-8e50-00fee380d8a9` 最近中断记录，在同一 `codex/workbuddy-usage-import` worktree 接续并开启新 Goal。主目录的独立修改保持原状。没有提交、推送、合并、发布。
- 接续基线完整核心 `core-resume-20261017.log` 为533/533。定位严格百万新回填快照：真实覆盖索引108892160字节/26585页，默认报表页缓存会反复读取；原线程采样和QUERY_TRACE表明大量时间在pread。独立诊断链接与生产相同的bundled SQLite 3.53.2，128MiB页缓存预热后同一聚合约337～340ms；系统Python SQLite版本不同，其实验不能作为生产时延结论。
- `store::open_connection(read_only=true)` 仅为按需报表读取连接设置128MiB有界SQLite页缓存，不缓存报表结果；导入worker和唯一writer不变，读取线程沿用60秒空闲退出。新真实WAL回归覆盖事务快照稳定、下一次报告看到新增/删除、只读写入拒绝和writer未采用大缓存。完整核心 `core-reader-page-cache.log` 为534/534；Clippy退出0，release构建退出0。
- `query-1m-reader-cache`真实API第一轮1天P95=373.947ms，但后续4项超500ms，整体FAIL（10通过/4失败）；其中与诊断release编译并发的测量也保留，不能作为无额外负载的最终性能通过。记录报表读取峰值RSS=138100736字节；该成本单独测量，不能宣称整个核心仍只有约20MiB。脚本现在保留 `report_reader_memory`，导入期内存仍独立保留原始采样。
- 后续诊断副本 `query-trace-reader-cache`（不是最终验收二进制）在无并发编译时10次dashboard为约359～462ms，包含实际CPU/缺页/线程切换计数。仍须用正式release完整重验3轮×当天/30天×50次；不将诊断PASS或中位数作为S10通过。
- 新增持久脚本 `scripts/workbuddy_import_ui_fixture.py`，启动仅含合成JSONL/Mock上游的新HOME/config和复制binary；每次唯一bundle ID，避免原生应用解析复用既有窗口，清理时仅停止该fixture子进程。当前 `ui-release-1792221163870239000` 已实际核验Review供应商、全部910/3.42、内置120/1.14、供应商790/2.28；保存原始窗口JPEG和AX。CUA原生外框为1440×901/1100×701逻辑点；UI审计增加明确0或1点的原生边框声明，仍严格校验原始像素，不裁剪或缩放。
- 实际英文窄屏滚动暴露R06遗漏：`Statistics::trends`把仅有选定币种费用才存在的currency_series用于Token图，未定价导入连Token图也空白。现用 `ChartPoint::split_series` 独立保留Token序列和可选费用序列；新增未定价导入在USD/CNY下Token仍100/10、费用图为空、未知覆盖提示保留的回归。GPUI完整 `gpui-token-trend-tests.log` 为136/136，Clippy/release构建及新窗口复验仍待完成。当前已采窗口属于修复前证据，不能算最终程序12图矩阵。
- 下一步：完成新GPUI构建及非空Token趋势真实核验，再以最终程序重新采12图和X45刷新/切页/隐藏窗口；无并发编译时重跑百万查询，再启动完整2规模×2布局×3轮严格性能矩阵和5分钟空闲，随后更新AC/X逐项证据。Goal保持active，平台/安装缺口仍明确保留。


### 接续实测追加

- 冻结黄金 `canonical-frozen-r09/report.json`：36/36 PASS，源JSONL与expected.json均与原设计包字节相同；事实7/总Token750、导入6/完整4/partial1/invalid1、内置180、供应商570、完整导入410、积分2130000000纳、已知积分3条含显式0、缓存60、代理缓存100/CNY123000000纳费用均经真实链路和API核对。精确字段含无效null、部分input7和显式0，详情未知transport；只读SQL核对，未种事实。
- GPUI `gpui-token-trend-clippy.log` 与 `gpui-token-trend-release.log` 退出0；既有block0.1.6 future-incompat提示保留。新窗口 `ui-release-1792221681617877000` 实际看见输入800/输出110两个非空数据点和USD费用空态；截图 `token-trend-unpriced-fixed.jpg` 原始保存。
- 当前核心755f597b4a627aafad7b93f9ed862ba4f923a769a6514139d524b6759a164dfc、GPUI89478936f3be048a4f08cf4c4c126d8e1ab29afb132fd98d234428f655306655的12图矩阵 `ui-final-trend-audit/report.json` 全部验证通过；每项原生点击后核对910/120/790及对应3.42/1.14/2.28，原始2x图像2880×1802/2200×1402含1点原生边框，未裁剪缩放。仍非1x/Windows/安装证明。X45正在同窗口逐项运行。
- `query-1m-reader-cache-serial`仍整体FAIL，后续P95超500ms；运行期间只读发现其他checkout `gpui_desktop_ui_starter` 的cargo/rustc进程，未停止或修改它。减少报表RSS取样的子进程干扰到1秒一次（导入监测仍100ms），新 `query-1m-reader-cache-sparse-rss` 仍有超标，因此不归咎或豁免性能缺口。仅在新派生合成快照试验的表达式索引未证实改善，未采用到生产源码。


### X45与完整严格矩阵启动

- `ui-release-1792221681617877000/x45/report.json` 保存7个原生操作与数据配对：关闭/5/10/30/60秒、切页、原生最小化按钮后，每次新增均为1条/12Token/100000000纳积分，最慢4724.893875ms；每个样本包含源flush、commit ACK校准区间、查询可见的单调时钟。最终事实10/994Token/4120000000纳，账本10个唯一身份，其中9导入、1待确认。
- 页面刷新关闭时源已入库但AX仍910/3.42；5/10秒页面随后934/3.62、946/3.72，切走实际供应商表单时仍入库，返回统计页982/4.02。最小化按CUA原生按钮操作、AX焦点消失取证；原生截图库仍能返回缓存的窗口图，**该图不作为在屏/隐藏属性证明**，取证限制在report中明示。未把单个样本代替性能20次竞争回填矩阵。
- 当前最终release的严格12阶段矩阵已启动：`dist/workbuddy-import-review/strict-matrix-reader-cache.log`。因内部磁盘空间有限，报告与SQLite快照写入独立合成证据目录 `/Volumes/1TB/Downloads/git/workbuddy-proxy-import-evidence/20261017-reader-cache-matrix`（当前worktree忽略目录下的同名symlink可访问）；源数据和运行库仍是原system-temp隔离布局，未改变被测I/O位置或数据规模。该新目录在334GiB空闲的外部盘，未写主工作树业务文件。
- 当前12阶段尚无最终聚合结论，不能标S10.03通过。表达式索引及QoS诊断文件只在忽略目录，未采用到生产。核心534/GPUI136仍是本次完整单测结果；新增冻结黄金已纳入主集成脚本，但整个主脚本仍待最终程序重跑。Goal保持active。


### 全字段生产链补验已准备（待运行，不计通过）

- 对照R01要求发现，原store单测已覆盖reasoning和缓存读取，但当前完整reader→API脚本缺少非空缓存创建/推理/total推导/1纳积分的同记录实测。新增独立合成 `session-fields.jsonl`（100/20→推导120、cache read40优先于0、cache write10、reasoning2、积分字面量0.000000001→1纳），不改原冻结样例与预期。
- `workbuddy_import_canonical.py` 已接独立临时库的SQL只读/列表/详情/总览逐字段断言，并加强混合partial/invalid时代理费用分母、status_known/pending和缓存160的断言。脚本py_compile通过，新增断言尚未运行；原36/36证据仅对应加强前脚本，不能算新断言通过。待性能测量结束后运行加强版及完整主集成，避免并发测试干扰时延。
- 当前严格矩阵100k/1000文件三独立轮均51项通过，已进入100k单巨型第一轮。它们尚不代表百万规模或完整性能关卡通过。


## U03/U04严格规格复核与修复（性能运行期间，待编译/GUI复验）

- 本轮为实质进展：性能原session2137经wait确认仍在运行，没有重启；10万条多文件和单巨型两布局的6轮均51项通过，当前开始1M/1000文件/1.074GB第一轮。报告仍为分阶段PHASE_PASS_GATE_INCOMPLETE，完整矩阵未过。
- 按UI-SPEC逐条复核发现原实际视图仍违约：来源选中背景浅蓝而非主色/白图标，未选项统一灰色，缺分隔线/明确hover与键盘focus提示；filter布局虽然调用layout()，却只取is_single_row()调宽，未使用rows，窄屏仍挤一行。这些不能因旧纯布局函数测试通过而算完成。
- 已在 `source_controls.rs` 修正真实来源渲染：主色356FC0/白图标、三项各自未选颜色、悬停底色、36/72位置分隔线、2点焦点框与disabled透明度；group的focus/blur接入真实SourceFilter状态，焦点提示用deferred原生Tooltip，Space/Enter明确激活当前项并阻止向上冒泡（重复选中仍零新请求），左右/Home/End保持切换。
- `filters()`改为直接遍历 `FilterLayout.rows` 渲染生产控件；每槽使用既有FilterSlot.width，严格136/180/112/88/168/32/32，统一高度32；1012/1011/560/559分行规则来自同一预算函数，不再让CSS自由wrap替代要求。移出刷新/更多按钮到小方法，未改变其原事件逻辑。实际A扣除12点padding和1点边框两侧。
- 新生产方法均按职责拆分，source_controls模块当前333行；仅cargo fmt及diff --check通过。**没有在性能过程中并发编译或跑其他测试**，新GPUI类型检查/完整测试/Clippy/release/真实GUI尚待矩阵结束后执行，不能沿用之前136/12图算新代码通过。
- 之前当前程序894789...的136单测/12图/X45证据继续保留，但它属于U03/U04修复前程序。核心755f597...未改变，运行中的性能矩阵仍对应当前核心源码。固定750黄金与新增缓存创建/推理/total推导/1纳积分补验的加强脚本也等待串行运行，原36项不可替代新增断言。

- 后续验收还必须覆盖本机真实打包helper的启动/关闭重开及旧core协议拒绝，不能把手工启动独立headless核心的窗口fixture当全部打包生命周期验证。性能矩阵完成后与加强黄金、完整集成、调度/故障/退出串行运行；新UI还需键盘/tooltip/行布局原生核验及更新12图。


## 串行验收接续与百万回填首证据

- 前一Goal轮次为实质进展，本轮通过原session2137确认矩阵live，不创建重复矩阵。当前百万首轮实际回填503241.4ms、全核心导入峰值20299776字节、4并发流off/on P95为47.098/45.688ms，408条含预热的代理事实和1M唯一导入总量均已核对；20次500条增量逐项达标。查询/5分钟空闲及余下百万轮次继续，不能据此将AC075标PASS。
- 已启动本地串行协调器 `dist/workbuddy-import-review/post_matrix_validation.py`，其工具session6445，原矩阵PID86884的完整启动身份已只读确认并记入 `post-matrix-acceptance/report.json`。协调器现在仅等待真实进程终止和矩阵终态报告，未提前编译或跑测试。出现矩阵失败仍保留失败，不将其他检查PASS解释为矩阵通过。
- 后续命令固定使用独立复制且SHA256=755f597...的被测核心、已验证存在的Python3.12.14/tomllib；先执行加强黄金/全字段链，再新GPUI fmt/test/Clippy/release、单线程核心回归、完整导入/OS故障/调度/退出，以及适用的既有CI核心CLI回归。每项保留准确命令、退出码、日志哈希、真实profile元数据检查，失败即停止等待修复，不改断言、不覆盖旧证据。Python用额外隔离HOME/Profile/TMP环境，Cargo编译保持原依赖环境。
- 包构建、真实原生GUI、helper生命周期和平台/安装不由该队列冒报完成，仍列为后续独立验收。待做的百万状态API轮询开销也应实际测量：当前status在唯一writer中按ledger状态聚合，源码检查不是性能通过或失败证据，不在当前基准期间另发负载扰动测量。


## 本机包验收准备审查（未制作/安装/发布包）

- 本Goal轮次先用原session2137和PID93071确认百万首轮仍live，处于查询后的5分钟空闲测量，没有因日志静默重启或并发跑验收。
- 已审现有 `package.py`/`gpui_macos_package.py`/`gpui_package_test.py`：真实包需包含外层core/tray和Helpers中的GPUI，验证只读DMG布局/代码签名/包内无用户配置，再测试复制出的程序。构建依赖已由仓库固定为dmgbuild1.6.7/ds-store1.3.3/mac-alias2.2.3，当前Python3.12环境缺失；应在性能之后用隔离构建环境安装，不修改产品依赖或全局Python环境。
- 已核实 `autostart_test.py` 配合当前macOS实现仅写隔离HOME下的LaunchAgents plist，不调用launchctl或修改真实用户的启动项。因此本机包验收可按既有脚本继续，仍不等于实际登录/安装验收；Windows注册表分支需独立Windows用户环境，不能在本机冒报。
- 现有包生命周期脚本的关闭/重开主要通过内部标记驱动，不足以替代U12原生事件；其 `--wait-for-cmd-q` 可配合CUA实际Cmd+Q。后续需以真实包的隔离实例补原生关闭/重开/退出，确认core run正常flush与同一core复用，不能用内部标记或文件存在当鼠标/键盘事件证明。
- 包脚本会重新作本机ad-hoc签名，须分别记录源码构建程序和包内程序的完整SHA256及签名/内容对应关系，不把重新签名后的文件错误宣称为原始相同字节。所有私有测试包留本地，不安装、不发布，不升级版本。


## 百万空闲 CPU 失败与等待策略整改（2026-10-17）

- `strict-matrix-reader-cache` 已终态退出1：6/12阶段通过，百万many首轮62通过/1失败，空闲全核心CPU中位数2.980%，未满足<1%。旧核心SHA256为755f597b4a627aafad7b93f9ed862ba4f923a769a6514139d524b6759a164dfc。失败保留，不能把该矩阵或旧六阶段归入后续新程序的通过证据。
- 已复查生产线程：空闲每25ms回到主循环重查reconcile、backlog及settings。新增真实线程回归，250ms稳定空闲窗口的settings快照读取次数必须不超过1，并要求手动增量扫描1秒内完成。旧策略与新测试已封存到忽略目录 `idle-red-project`，红测尚待执行。
- 当前整改按发现/观察/catchup下一期限等待，Condvar中仅以100ms原子检查切片防通知丢失和超长退出，不在切片间重查SQLite；唤醒epoch保证状态变化发生在检查与等待之间仍能处理。归属修订和catchup多批继续立即运行。源码已编辑，尚未通过编译或CPU重验。
- 串行验收队列当前旧核心canonical、GPUI格式/136测试/Clippy/release、核心534测试/Clippy、导入集成、OS故障已PASS，正在完成调度测试；新GPUI源码尚缺原生GUI复验。后续结果以 `post-matrix-acceptance/report.json` 为准。


## 新核心回归与原生Tab反例（2026-10-17）

- 红测 `idle-red-test.log`：旧25ms策略250ms内9次空闲settings快照读取，真实断言失败。首次长等待绿测发现最终discovery slice后已有priority任务却提前等待，已改Retry立即续跑；失败保留 `idle-green-test.log`。修后同一测试通过 `idle-green-test-2.log`。同步租约持有时还需留在原子等待中，不能因observation_requested尚未消费而忙转；已补现有租约测试断言。
- 完整核心535/0（86.34s，租约细化前）和最终runtime17/0（54.17s，覆盖租约细化）、Clippy均通过。新正式release核心SHA256=d457e036e413f5302c982b5817858ffb201da5bf54cd98517e594afc005cd5d8。尚无新核心严格矩阵PASS。
- 旧串行队列真实终态 `FAILED_STEP_REQUIRES_REPAIR`：前面步骤通过，statistics脚本本身42项/退出0，但guard观察真实profile目录元数据漂移而停止；不能归因或抹去。独立复查 `statistics-isolation-recheck.json` 已逐项记录before/after，42项/退出0且全部未变化；旧失败不被覆盖。
- 新串行协调器 `idle_final_validation.py` 固定复制d457...，每项记录完整命令、退出码、日志hash及逐项真实profile元数据前后值。完成普通CLI后等待native-validation-done标记才跑调度和forwarding计时；后续结果看idle-final-validation/report.json。
- 当前原生诊断fixture ui-release-1792225447808524000（core d457...、GPUI 3b4eb000...）真实三来源910/120/790及3.42/1.14/2.28均可见，左右/Home/End有效，但Tab从侧栏直接跳到供应商，未进入来源组。定位GPUI显式FocusHandle不会采用div.tab_index的tab_stop设置，已改句柄构造为tab_index(0).tab_stop(true)；该改动尚待编译及原生复验。这三张图不是完整12图通过。
- 私有DMG已制作，hdiutil checksum有效，未安装/发布；其中GPUI为上述待修焦点旧程序，不是最终包验收。最终GPUI修复后须另制作包并重验签名、布局和生命周期。


## 当前续工检查点：新核心、新UI与私有包实证（2026-10-17）

- 核心固定SHA256 `d457e036e413f5302c982b5817858ffb201da5bf54cd98517e594afc005cd5d8`；GPUI最终SHA256 `1340cd4130fb17116f487f92c8bf2587e967411f3dcd06555c375e1aec360467`。版本仍0.5.6，分支codex/workbuddy-usage-import，HEAD仍6102b779871bab0bb8ad646dac5004892adf76be；没有提交/推送/合并/发布。
- 新核心冻结黄金/全字段51通过；新核心完整导入157通过、OS故障15通过（idle-retry-validation）；适用既有CLI回归已通过到端点测试。错误地在不带helper的固定裸核心副本上跑core_compatibility触发设置缺失modal，保留失败；正确包路径的兼容替换已在下面DMG实测通过，不通过改断言掩盖失败。
- GPUI焦点修复后136测试/Clippy/release通过。原生fixture `ui-release-1792225750280548000` 的12图全验证通过，宽屏一行/窄屏两行；三来源分别910/120/790 Token、3.42/1.14/2.28点。U03原生Tab返回来源组蓝色2px框和tooltip可见；输入框AB经左键再输入x变AxB，统计保持994，证明没有抢占输入箭头。原图原AX与hash保留，2x本机证据不覆盖1x/Windows。
- 同fixture的X45七状态（Off/5/10/30/60/切页/最小化）全部后台增量≤10秒，最慢4196.637708ms，终态10条/994 Token/4120000000纳积分。Off页面保持910；5秒更新934；10秒946；切页返回982。最小化使用原生按钮，之后AX无focused element；不把缓存截图当在屏证明。原生fixture已清理。
- 真正完整私有DMG `private-package-focus-final/workbuddy-proxy-0.5.6-import-review-focus-final-private-macos-arm64.dmg` SHA256=248c25071dbd07296ffe0ac4bf3cc094a56046063697195b88cc603fed874757。`package-focus-final-validation/report.json` 退出0且真实profile元数据未变：只读挂载/签名/Finder布局/版本/无用户配置、13种旧核心兼容替换、模型能力、自启动隔离注册、完整包生命周期通过。未安装、未发布；自启动不等于实际登录验收。
- 补充原生包事件：原生关闭使设置进程退出，生产 /_control/show 通过原生通知重开新设置进程且复用同核心，CUA Cmd+Q后host/core退出、stats_runs为normal。为避免既有安装版与验收版共享标识，使用唯一CFBundleIdentifier和本机adhoc签名QA副本；去签名后的核心与GPUI字节均与原DMG对应程序相同（哈希见native-package-lifecycle-env/report.json）。该原生事件轮次真实Proxy配置目录未变，但WorkBuddy真实根目录元数据漂移，故整体结果仍FAIL_PROFILE_METADATA_DRIFT；只计功能完成，不宣称全隔离通过。此前共享标识/仅唯一标识轮次触及真实gpui-wake元数据的反例也保留，未回滚用户文件。
- 已修合成GUI fixture自身隔离：完整HOME/Profile/XDG/TMP及WORKBUDDY_PROXY_CONFIG_DIR，Info.plist也写同一隔离LSEnvironment，防原生app查找额外启动不带CLI参数的副本落入默认配置。语法检查通过。该变化只属于验收工具，不改变产品或正式包元数据。native-package-lifecycle-env中的原生操作证明Proxy元数据稳定；其真实WorkBuddy根元数据漂移仍待独立核实。最后短复验因原生关闭等待超时终态失败，terminal-status.json更正原report异常后残留RUNNING，不能把它当live/PASS。
- 性能队列当前正在跑新核心调度测试 `idle-timing-validation`（工具session58739，协调器PID22910），随后退出与forwarding benchmark。顺序标记仅表示原生工作已结束，**不豁免未决隔离门槛**。`deadline-matrix-coordinator.json`（工具session51227，PID23280）等待这三个计时检查通过后才启动新核心12阶段严格矩阵，目前尚未开始scale core。外置证据目录20261017-deadline-wait-matrix，不复用旧755f核心的六阶段通过。
- 仍需：确认新调度/退出/forwarding与完整12阶段、百万5分钟idle及status轮询开销；核实/补齐原生完整隔离、1x/Windows和安装/登录边界；更新所有AC状态及报告。Goal保持active，不完成、不blocked，不以平台缺口替代尚待本机验收。


## AC048真实观测与最终事件预算接续（2026-10-17）

- 本轮为实质进展：先确认PID22910/23280仍live，之后新d457核心调度27/0、退出30/0、forwarding基准全部退出0，真实profile守卫均未变。随后PID34466实际启动严格矩阵；100k/many的trial-1、trial-2各51/0完整结束。
- 逐项审计发现AC048初版把容量2当观测峰值、把四个store单测总时间当批次耗时，原S06.08已加失效banner。矩阵orchestrator明确SIGSTOP等待当前trial-2结束，再经PID/子进程核验后SIGTERM终止扩展；不是超时误判或用户暂停Goal。旧协调器exit=-15/STRICT_MATRIX_FAILED，原因和两阶段完成值见strict-matrix-deadline-wait/supersession.json。该矩阵终态已停，无当前scale core，不复用这些阶段作为新程序证明。
- 新增runtime/telemetry.rs：真正成功offer/receive的专用队列深度/峰值；生产Work::Batch事务成功/失败、总/最大微秒数、最近32个有界样本；真实source I/O边界核验独立OS worker线程。回滚错误计量但成功回执/offset不提前。status增加runtime_metrics有界诊断，DATA-API已说明；规模脚本和聚合器强制记录/校验实际值，旧报告缺此观测不作新通过。
- 新红测metrics-red.log：旧实现缺实际队列观测断言失败，exit101。新完整核心537/0（84.64s）和Clippy通过，release成功，核心SHA256=ac03b9053930489272719d2419effee3bb18797a9331ca22deffc42019b10adf。telemetry-final-validation/canonical为51/0；随后integration整体152通过/5失败，失败全是metadata守卫；proxy_config_macos在该区间变化，原报告保留，不能把整段称PASS。该串行协调器已因该步停止，后续未跑。
- AC075.8的事件预算新增可选WORKBUDDY_PROXY_UI_EVENT_METRICS=1：Statistics中有界128个内存样本记录实际source_callback/source_repeat、on_next_frame选中态及provider/model搜索条件应用的同步段；不在事件处理里写文件，退出Drop才输出WB_UI_EVENT_METRICS。默认禁用，无常驻测量线程。GPUI137/0、Clippy/release通过，仍有原依赖block0.1.6既存future-incompat提示；最终GPUI SHA256=27367fd49af0675811cffe86c99e79bfd528b31bd0c21f346d4af5c67bc0ab1e。尚未取得该新程序的实际事件trace，不将编译/137测试当<100ms或下一帧实证。
- 进一步隔离工具：workbuddy_import_ui_sandbox.py/可选QA bootstrap C均不改产品二进制和Cargo依赖。sandbox-exec合成6项及编译native bootstrap合成6项自检PASS，验证保护路径读/写及exec子进程写拒绝、隔离路径仍可写，未对真实文件做这些正/负写实验。完整GUI kernel模式已实际启动并有PID保护凭据，但CUA未能发现/绑定该AppKit实例，失败记录于ui-guard-final和ui-sandbox-direct-final（实际采样证明app事件循环运行，不冒称窗口可驱动）。所以--guard-gui保留为显式诊断模式，**常规GUI仅核心受kernel策略保护，GUI仍依赖唯一标识/完整隔离env/LSEnvironment，不能称两者全部kernel保护**。常规GUI模式已再次被CUA发现并显示合成Review/910/3.42。相关测试实例都已终止；未操作真实安装进程。
- 新增ACCEPTANCE-AUDIT.md枚举固定80条件。NOT_RUN表示整条证据复核尚未完成，并非抹去已有实现/测试；不输出虚假完成率。current-baseline-audit.json验证HEAD仍6102b779871bab0bb8ad646dac5004892adf76be、core/UI版本0.5.6、Cargo清单/锁和0001～0006逐字节同HEAD。没有提交/推送/合并/发布、没有更改目标门槛。
- 下一项：最新core ac03+GUI27367的合成fixture已用ui-event-final.log启动（常规GUI模式，工具session在本轮输出）；先核实live和fixture.json，完成最终12图，再在**实际竞争回填**时做来源/输入事件采样，原生关闭使EventMetrics Drop落日志（不要SIGTERM丢trace），查dropped=0、source_callback/input_apply<100000us、next_frame选中匹配。只有数据和原生实际通过才记录。然后重做当前程序私有包/原生隔离及全脚本，最后启动新核心完整12阶段严格矩阵。新UI/newCore哈希变化，旧图/旧包/旧矩阵仅保留历史，不能冒充最终验收。
- 尚需复核：百万status在唯一writer扫描ledger的实际开销；新core全部验收、80项逐条证据、代码风格超目标模块具体审查、1x/Windows/原生x64和安装/登录边界。Goal继续active，不完成、不blocked，不把原生工具/平台缺口用来替代本机功能和性能验收。


## 最终程序原生12图、X45及竞争回填事件预算（2026-10-17）

- 沿用live fixture `ui-release-1792229486062928000`，没有重复启动实例；core ac03b905…/GPUI27367fd4…保持不变。最终12图 `ui-event-final-audit/report.json` 已退出0，3来源×中英文×1440×900/1100×700全部核验；原JPEG、AX、程序及图片hash保留。原图为2x，不覆盖1x或Windows。
- 同一程序X45七状态原生操作与真实数据链通过，`x45/report.json` 为PASS_NATIVE_EVENTS_PLUS_DATA_CHAIN；最慢4051.652083ms，每条都≤10秒。Off页面910；5秒934；10秒946；切页返回982。最小化首次样本的焦点证据不充分，保留原attempt并在明确无焦点后追加唯一样本，后者3567.927750ms通过；因此末态为11条/1006 Token/4220000000纳积分，不能沿用旧10条/994的预期。
- 事件预算第一次150k合成文件session与文件stem不符，生产正确记录150000次session_mismatch并不入事实；`budget-before.json`保留失败尝试。改用匹配session的新300k合成文件，操作前128条，操作后94336条，期间尚未导完且持续推进；`budget-active-before/after.json`为只读SQL证据。
- CUA实际点击内置、Right切供应商、重复供应商、供应商AB+Left+x→AxB、模型QQ+Left+p→QpQ；供应商仍790Token/2.28点。正常原生close后UI/core/fixture全退出，GPUI Drop真实导出WB_UI_EVENT_METRICS，无SIGTERM UI或伪造trace。`event-budget-report.json`全部断言通过：dropped=0，source_callback最大96µs、provider_input_apply8µs、model_input_apply9µs；source_next_frame最大36716µs且全部selection_matches=true。下一帧与同步回调时间分别报告。该300k探针仅证明竞争回填事件预算，不替代严格规模矩阵。
- 当前GUI仍只以完整隔离env/唯一ID/LSEnvironment保护，core有kernel保护；这组结果不自动把AC003标PASS。为CLI重新运行新增metadata可读、内容/写入禁止的继承kernel policy，六项合成自检PASS（cli-protection-selfcheck.json）；真实元数据守卫仍生效，不用权限拒绝导致的None值隐藏漂移。新串行队列`telemetry-protected-validation`已启动，黄金/导入/故障/适用既有CLI/调度/退出/forwarding逐项保留命令、退出码、hash和守卫。后续包/严格12阶段尚待执行。

- 追加隔离纠正：原UI51343正常close且trace已导出后，紧接的CUA getAXState重新启动同fixture可执行副本PID57303；发现时其PPID为1，已核对完整路径后精确SIGTERM清理，未处理真实安装进程。该副本effective env/config未捕获，不能以plist声明当实际隔离证明。清理记录extra-native-launch-cleanup.json保留；以后关闭后只读ps/日志，不再调用会激活App的CUA观测。受kernel保护队列此前integration154通过/3失败全为metadata，整体FAIL保留。当前另起telemetry-protected-complete，对每个独立步骤均记录，不因前项失败隐藏后项；最终若有失败仍返回FAIL。新轮次早期四次元数据守卫已PASS，终态未定。


## 验收工具真实隔离与资源采样修正（2026-10-17）

- 前一telemetry-protected-complete整体FAIL保留：canonical51、integration157、OS故障15、shutdown30及多数既有CLI通过；scheduling脚本27/0，但外层WorkBuddy根mtime变化；native_resource在业务断言前因受sandbox限制不能exec setuid /bin/ps失败。没有把这两个失败改写成PASS。
- macOS资源采样改为系统libproc rusage_info_v0：RSS取真实resident字节，user+system按本机Mach timebase转换为秒。并非固定假设纳秒或arm64比例。libproc-sampler-selfcheck七项PASS：24MiB分配可见，RSS与外部ps同PID接近，0.301428s CPU增量与process_time0.301434s一致，非法PID报错。benchmark只调整macOS采样入口，所有内存/延迟/退出阈值保持原值。两次无setuid ps副本实验失败且未使用，系统ps/签名/全局配置均未修改。
- 对“元数据变化→fixture写真实profile”的误归因，新增可失败的内核权限实证：sandbox_check查询实际PID的受保护目录读内容/写/创建/删除权限，并要求已有合成目录的四个权限均允许；未隔离负对照必须失败。先前使用不存在路径作正对照保守拒绝，first-failure.json保留；修正到已有目录后七项PASS，包括同PID exec后权限仍拒绝。没有真实读写探测，没有仅信环境变量声明。SPI接口参考WebKit官方SandboxSPI.h，ctypes为Apple arm64可变参数显式声明固定三个参数。
- Sandbox的真实before/after及real_profiles_unchanged不伪造、不抹去；只有已验证内核拒绝本测试树读写时，记录kernel isolation通过与外部metadata漂移观察；未被保护时旧元数据不变断言仍是必要条件。GUI经外部原生服务重新启动不属于exec继承范围，AC003的GUI缺口保持。新telemetry-attested-final逐步骤在sandbox中检查实际权限、记录与Popen一致PID，再exec真实脚本；外层不再把外部mtime变化混为测试数据写入。所有业务断言和固定验收门槛不变。
- 新完整串行回归已启动，之后status百万探针等其成功才运行；当前没有严格规模矩阵，不冒报通过。本轮新增/修正脚本语法检查与git diff --check通过。详细自检、失败链、源hash见R09-20261017-QA-ISOLATION.json。


## 当前串行验收与外置临时目录迁移（2026-10-17）

- 最终 ac03 核心的 21 个串行 gate 全部通过：canonical 51、integration 157、真实 OS 故障 15、固定节奏/竞争回填 27、退出 30 和既有 CLI/转发基准。实际 PID 的内核权限凭据随报告保留，见 `evidence/R09-20261017-LOCAL-GATES.json`；过去守卫失败记录不覆盖。
- 百万合成快照的 status 补验 13/0：3×50 次 status P95=28.177/27.322/33.399ms；三轮每轮 200 基线+200 轮询并发流，最大 P95 增量约 0.149ms。见 `evidence/R09-20261017-STATUS-POLLING.json`，不把短轮询窗口扩大成长期运行证明。
- R05 实际 release 保留期限 0/7/30/180 天矩阵 53/0，通过真实一分钟 housekeeping、缩短后延长、墓碑不复活和新唯一身份；历史不可证明的错误排除不自动恢复。见 `evidence/R09-20261017-RETENTION.json`。
- 当前私有 DMG build/verify PASS；唯一标识 QA 副本的原生关闭→生产 show 重开→Cmd+Q 正常退出 PASS，core 复用，所有 stats_runs 正常结束，真实 profile metadata 前后相同且无残留 fixture 进程。见 PACKAGE/NATIVE-PACKAGE 摘要。未安装、未发布；GUI 使用隔离 LSEnvironment，没有内核隔离证明，不能扩大 CLI 的 kernel 保证。
- 用户存储要求已原样落实为主工作区与 worktree 的 AGENTS.md 六条规则。TMPDIR/TMP/TEMP 固定到外置盘会话专用目录，Cargo 后续复用主工作区 target 和 gpui/target。既有 worktree 缓存归属不足，保留，不执行 cargo clean。
- 旧 telemetry 严格矩阵在两个 100k 阶段完成后，按用户要求先停调度、等待阶段退出再终止，原因记录于 `external-tmp-transition.json`；不把主动中止当产品断言失败，也不复用缺 phase 直接 kernel 凭据的报告。
- performance 阶段现补实际 `check_isolated`，SIGTERM/Ctrl+C 进入 finally，等待 fixture 停止再删除临时数据库/配置/复制程序；保留必要快照，逐阶段记录清理与磁盘余量。全新 12 阶段矩阵输出在外置盘 `20261017-external-tmp-telemetry-matrix`，仍在运行，未计 PASS。
- 已按 fixture/原生报告确证的确切路径清理约 771MiB，包括一个已退出的系统临时 fixture。报告/截图/快照/DMG 全保留；未按前缀批删。未知系统临时候选与既有 worktree target 缓存保留，清单、大小和磁盘余量见 `evidence/R09-20261017-STORAGE.json`。
