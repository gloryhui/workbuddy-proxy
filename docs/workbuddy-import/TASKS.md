# 固定实施顺序：10 阶段 × 8 子任务

每一项都必须按编号顺序完成。阶段内不得挑容易的先做，不得跨阶段批量实现。只有当前子任务的前置条件、改动边界、测试和证据全部满足才标 DONE。验证失败先修本项；环境缺失标 BLOCKED_EXTERNAL，并只继续不依赖该验证的文档整理，不宣布阶段通过。

每项完成后在 PROGRESS.md 记录：任务号、输入基线、修改文件、实现要点、准确命令、退出码、证据相对路径、风险/遗留。每阶段结束必须检查 diff allowlist。首次修改新的共享文件之前说明它为何属于本项。若不能在下列边界完成，写 DECISIONS.md 的变更提案并等待设计调整，禁止随意扩展。

命令组见 [COMMANDS.md](COMMANDS.md)。所有任务验收同时受 [ACCEPTANCE.md](ACCEPTANCE.md) 的横切要求约束。表内路径为相对 worktree；此处的允许路径不是允许无关重构。

## S01 冻结基线与测试输入

阶段关卡：`baseline`。依赖：设计文档交付与基线核对。

### S01.01 核对开发位置和基线

- 前置：本设计包已交付。

- 操作：只读 git status、rev-parse、worktree list；记录基线指纹与未提交设计文件，确认不会进入 main。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC001`：发现其他业务改动时定位归属；不得 reset/stash 掩盖差异。

- 证据：`evidence/S01.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.02 登记既有入口与风险

- 前置：`S01.01`。

- 操作：逐项核实 baseline.json 所列版本及 writer/query/UI 接线位置；把偏差写 DECISIONS.md。

- 允许修改：`docs/workbuddy-import/DECISIONS.md`。

- 验收 `AC002`：版本偏差未解决不得自选迁移编号或覆盖文件。

- 证据：`evidence/S01.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.03 准备隔离运行目录

- 前置：`S01.02`。

- 操作：建立自动测试临时 HOME/USERPROFILE/config-dir 与关闭公网价格请求的环境；证明实际根目录是临时目录。

- 允许修改：`scripts/workbuddy_import_test.py`。

- 验收 `AC003`：fixture 测试不得读写真实 .workbuddy、配置或安装应用。

- 证据：`evidence/S01.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.04 落实合成黄金样例

- 前置：`S01.03`。

- 操作：复制本设计 fixtures 与预期到 scripts/fixtures/workbuddy_import；增加拆分工具响应、坏行、无换行末尾样例。

- 允许修改：`scripts/fixtures/workbuddy_import/**`。

- 验收 `AC004`：所有样例为合成，无用户正文/账号；明确每项预期而不是生成后自己算自己。

- 证据：`evidence/S01.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.05 定义内部类型

- 前置：`S01.04`。

- 操作：新增 workbuddy_import/types.rs：Candidate/NormalizedUsage/ParseDisposition/ImportBatch/CheckpointAck；状态用枚举、未知用 Option。

- 允许修改：`src/stats/workbuddy_import/types.rs`、`src/stats/workbuddy_import/mod.rs`、`src/stats/mod.rs`。

- 验收 `AC005`：共享 ID/整数限值、禁止 Value 裸传；暂不挂运行线程。

- 证据：`evidence/S01.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.06 建立测试入口与预期清单

- 前置：`S01.05`。

- 操作：建立生产模块测试入口与用例清单；先运行已可执行的类型/身份编码测试。parser行为的红→绿测试留在S03对应子项内完成，不把未实现功能的失败测试接入当前关卡。

- 允许修改：`src/stats/workbuddy_import/tests.rs`。

- 验收 `AC006`：测试必须调用生产模块，不写另一份导入器；不得使用ignore/空断言制造通过，S01结束仓库仍可编译和跑完整测试。

- 证据：`evidence/S01.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.07 固定日志与错误规范

- 前置：`S01.06`。

- 操作：定义固定 error/reason code 和白名单诊断类型；加入敏感 canary 扫描测试辅助。

- 允许修改：`src/stats/workbuddy_import/types.rs`、`scripts/workbuddy_import_test.py`。

- 验收 `AC007`：日志/DB/API 中不得出现合成密钥、prompt、tool arguments 的 canary。

- 证据：`evidence/S01.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S01.08 完成基线验收

- 前置：`S01.07`。

- 操作：执行 G00 记录原有核心与 GPUI 测试状态；旧失败先解释且留证据，不能把旧失败改断言消失。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC008`：基线报告包含命令、退出码、环境、日志路径；所有新行为仍未启用。

- 证据：`evidence/S01.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S02 数据库迁移与事务基础

阶段关卡：`schema`。依赖：前一阶段全部通过。

### S02.01 审查 v7 迁移设计稿

- 前置：`S01.08`。

- 操作：在临时 v6 数据库执行 schema-v7.sql，核对列、约束、视图、费用 FK；与 DATA-API 一致后复制为0007.sql。

- 允许修改：`src/stats/migrations/0007.sql`。

- 验收 `AC009`：旧0001～0006字节完全不变；禁止对真实库执行设计稿。

- 证据：`evidence/S02.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.02 接入升级版本与备份

- 前置：`S02.01`。

- 操作：SCHEMA_VERSION=7、include新增迁移；FK OFF只在必要重建步骤外，任何返回路径恢复 ON。 当前无条件apply_step(6)改为version<6分支，再追加version<7；防v6重复建表。

- 允许修改：`src/stats/store.rs`。

- 验收 `AC010`：0/1/2/3/4/5/6→7和7→7全部通过；错误迁移原库可恢复且版本不假升。

- 证据：`evidence/S02.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.03 扩展统一事实表

- 前置：`S02.02`。

- 操作：落实 proxy/import 的互斥 CHECK，导入 nullable transport与recorded状态；逐列复制保持原 id/request_id。

- 允许修改：`src/stats/migrations/0007.sql`、`src/stats/store.rs`。

- 验收 `AC011`：非法proxy缺run/stream、非法import带HTTP/费用断言拒绝。

- 证据：`evidence/S02.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.04 实现身份账本基础操作

- 前置：`S02.03`。

- 操作：新增通过借用 Transaction 操作 ledger 的函数及同键查询；无独立连接，禁止 replace。

- 允许修改：`src/stats/workbuddy_import/store.rs`。

- 验收 `AC012`：同键插入只一行、冲突不覆盖代理、tombstone不丢；FK检查为0行。

- 证据：`evidence/S02.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.05 实现检查点 CAS

- 前置：`S02.04`。

- 操作：文件generation/revision/offset条件更新，事务与ACK绑定；支持未变文件和重读。

- 允许修改：`src/stats/workbuddy_import/store.rs`。

- 验收 `AC013`：错误revision、旧generation、负offset、跨namespace拒绝且无部分写。

- 证据：`evidence/S02.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.06 实现映射与设置持久层

- 前置：`S02.05`。

- 操作：落实半开区间、verified重叠触发器、单例设置revision；启用才生成namespace且永久复用。

- 允许修改：`src/stats/workbuddy_import/store.rs`、`src/stats/store.rs`。

- 验收 `AC014`：相邻区间合法，重叠拒绝；关闭再开namespace不变。

- 证据：`evidence/S02.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.07 验证旧记录与费用不变

- 前置：`S02.06`。

- 操作：构造v6多协议、进行中/失败/成功/不同币种/价格快照，迁移前后逐字段比对。

- 允许修改：`src/stats/store.rs`。

- 验收 `AC015`：行数、ID、tokens、price snapshots、cost金额逐项相等；并验证索引和视图仍可查询。

- 证据：`evidence/S02.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S02.08 通过数据库关卡

- 前置：`S02.07`。

- 操作：执行G01迁移专项和核心测试；人工检查SQL无清库、无隐藏重建回退。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC016`：故障注入、外键、integrity_check通过，提交证据后才进入解析。

- 证据：`evidence/S02.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S03 白名单解析与用量规范化

阶段关卡：`parser`。依赖：前一阶段全部通过。

### S03.01 实现消息外壳解析

- 前置：`S02.08`。

- 操作：只接受允许type/role；providerData用typed serde并忽略正文；单行字节限制由reader保障。

- 允许修改：`src/stats/workbuddy_import/parser.rs`。

- 验收 `AC017`：message assistant与无role function_call纳入，user/reasoning/tool result全部排除。

- 证据：`evidence/S03.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.02 实现身份与时间验证

- 前置：`S03.01`。

- 操作：实现length-prefix SHA256键、session匹配文件名、messageId必需、整数毫秒时间验证。

- 允许修改：`src/stats/workbuddy_import/parser.rs`、`src/stats/workbuddy_import/types.rs`。

- 验收 `AC018`：同会话不同message分开；跨文件相同消息同键；conversationRequestId不能去重。

- 证据：`evidence/S03.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.03 实现主 Token 规则

- 前置：`S03.02`。

- 操作：按固定优先级解析input/output/total，checked_add只用于缺total，冲突/非法值拒绝。

- 允许修改：`src/stats/workbuddy_import/normalize.rs`。

- 验收 `AC019`：零/缺失/负数/小数/字符串/上限/溢出/冲突分别有明确断言。

- 证据：`evidence/S03.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.04 实现缓存和推理规则

- 前置：`S03.03`。

- 操作：执行缓存别名优先级、子集约束、alias diagnostics；未知字段不猜。

- 允许修改：`src/stats/workbuddy_import/normalize.rs`。

- 验收 `AC020`：高优先4096低优先0得到4096；缓存不重复加输入；推理不超过输出。

- 证据：`evidence/S03.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.05 实现精确 credit

- 前置：`S03.04`。

- 操作：读取原始数字字面量转整数nano，最多9位小数，非法credit不破坏合法Token。

- 允许修改：`src/stats/workbuddy_import/normalize.rs`。

- 验收 `AC021`：1.14→1140000000，0明确保留，缺失null，科学计数/负值/超精度拒绝。

- 证据：`evidence/S03.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.06 实现质量和候选摘要

- 前置：`S03.05`。

- 操作：输出complete/partial/missing/invalid，序列化白名单candidate且≤16KiB；无需业务正文。

- 允许修改：`src/stats/workbuddy_import/normalize.rs`、`src/stats/workbuddy_import/types.rs`。

- 验收 `AC022`：normalized semantic hash不依赖JSON顺序/正文；冲突比较不含文件路径或时间。

- 证据：`evidence/S03.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.07 运行格式兼容与隐私测试

- 前置：`S03.06`。

- 操作：为未知格式、额外巨型字段、UTF-8/BOM、ID控制字符写测试；不做任意格式fallback。

- 允许修改：`src/stats/workbuddy_import/tests.rs`。

- 验收 `AC023`：未知格式有诊断且继续文件；canary不出现在候选、日志、API、数据库。

- 证据：`evidence/S03.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S03.08 通过解析关卡

- 前置：`S03.07`。

- 操作：执行G02，黄金样例与人工预期逐字段一致；记录支持的WorkBuddy格式边界。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC024`：不能用本机“正好能读”代替合成错误用例；无HTTP路径变化。

- 证据：`evidence/S03.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S04 文件发现与有界增量读取

阶段关卡：`reader`。依赖：前一阶段全部通过。

### S04.01 实现受限目录发现

- 前置：`S03.08`。

- 操作：仅projects一级项目目录/*.jsonl，惰性迭代与每slice100项，不follow symlink。目录发现接受每5秒后台调度，覆盖运行中新建项目目录并支持分片恢复。

- 允许修改：`src/stats/workbuddy_import/reader.rs`。

- 验收 `AC025`：中文空格路径可读；traces/工具结果/外部symlink完全不读。新目录/新小文件达到X46增量门槛。

- 证据：`evidence/S04.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.02 实现文件指纹

- 前置：`S04.01`。

- 操作：保存prefix长度/锚点范围及散列，元数据比较发现追加/截断/替换。

- 允许修改：`src/stats/workbuddy_import/reader.rs`。

- 验收 `AC026`：小文件增长不误判；替换/截断generation增加，账本不删。

- 证据：`evidence/S04.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.03 实现有限buffer读行

- 前置：`S04.02`。

- 操作：64KiB读块，LF/CRLF/BOM，4MiB最大行；不read_to_end整文件。

- 允许修改：`src/stats/workbuddy_import/reader.rs`。

- 验收 `AC027`：4MiB边界和50MiB单行内存有界，完整行可被后续处理。

- 证据：`evidence/S04.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.04 实现尾行和坏行推进

- 前置：`S04.03`。

- 操作：未换行末尾不提交offset；坏完整行计一次诊断推进；oversized排空状态可重启恢复。

- 允许修改：`src/stats/workbuddy_import/reader.rs`。

- 验收 `AC028`：分三次写入同一JSON只入一次；坏行后合法行仍可见。

- 证据：`evidence/S04.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.05 实现每文件公平遍历

- 前置：`S04.04`。

- 操作：维护有限活动文件窗口和轮转；8MiB/50ms任一达限让出，文件新旧都可进展。回填/full每slice服务到期增量及目录发现，优先已追平文件追加，回填仍轮转推进。

- 允许修改：`src/stats/workbuddy_import/reader.rs`、`src/stats/workbuddy_import/mod.rs`。

- 验收 `AC029`：一个持续追加大文件不能永久饿死其他文件。回填其他文件时已追平文件增量不饥饿。

- 证据：`evidence/S04.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.06 实现读取变更验证

- 前置：`S04.05`。

- 操作：提交前核对已读文件的身份/范围；旧generation批次重读，权限失败不推进offset。

- 允许修改：`src/stats/workbuddy_import/reader.rs`。

- 验收 `AC030`：注入rename/truncate/权限错误，不能半批旧数据入库或丢行。

- 证据：`evidence/S04.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.07 实现低优先完整重扫

- 前置：`S04.06`。

- 操作：独立scan游标、每日核对时机与手动full；按同一解析和账本处理。

- 允许修改：`src/stats/workbuddy_import/reader.rs`。

- 验收 `AC031`：同大小/mtime中间修改在full被发现；full不把incremental游标清0。

- 证据：`evidence/S04.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S04.08 通过读取关卡

- 前置：`S04.07`。

- 操作：执行G03，记录读取字节和最大缓冲而非只计文件条数。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC032`：未变化文件常规轮次不重读正文，所有checkpoint预期可重放。

- 证据：`evidence/S04.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S05 历史归属与防重复分类

阶段关卡：`classification`。依赖：前一阶段全部通过。

### S05.01 抽取已有代理归属判定

- 前置：`S04.08`。

- 操作：将managed_signature/installation的必要非敏感判断抽成共享小函数，原同步行为用回归锁住。

- 允许修改：`src/workbuddy.rs`、`src/stats/workbuddy_import/classify.rs`。

- 验收 `AC033`：没有第二套按名称/localhost猜测的检测器；旧同步测试仍通过。

- 证据：`evidence/S05.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.02 实现当前配置观察

- 前置：`S05.01`。

- 操作：仅提取当前模型ID/marker/route，不保存key/url；观测时刻起建立历史区间。

- 允许修改：`src/stats/workbuddy_import/classify.rs`、`src/stats/workbuddy_import/mod.rs`。

- 验收 `AC034`：当前直连配置不倒推历史；无法证明旧区间进入pending。

- 证据：`evidence/S05.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.03 实现分类纯函数

- 前置：`S05.02`。

- 操作：顺序固定proxy→已验证内置格式→已确认direct→pending；全套表驱动测试。

- 允许修改：`src/stats/workbuddy_import/classify.rs`。

- 验收 `AC035`：custom-local并非全部proxy；同响应模型来自两路线可区分；已知proxy缺记录也排除。

- 证据：`evidence/S05.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.04 实现历史区间与别名复用

- 前置：`S05.03`。

- 操作：关联模型ID精确匹配、区间边界、route重命名/删除、人工规则撤销。

- 允许修改：`src/stats/workbuddy_import/classify.rs`、`src/stats/workbuddy_import/store.rs`。

- 验收 `AC036`：direct→proxy同alias时间不同分类正确；不按现有名字覆盖过去。

- 证据：`evidence/S05.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.05 接入同步prepare/finalize

- 前置：`S05.04`。

- 操作：在现有mutation序列内准备映射再写文件后确认；不跨await持config Mutex。

- 允许修改：`src/workbuddy.rs`、`src/proxy.rs`、`src/stats/mod.rs`。

- 验收 `AC037`：prepare失败零WorkBuddy文件变更；finalize失败报告已同步但归属未确认。

- 证据：`evidence/S05.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.06 实现崩溃后prepared恢复

- 前置：`S05.05`。

- 操作：启动对照实际非敏感配置完成/冲突化prepared；关闭导入维持原同步可用性。

- 允许修改：`src/stats/workbuddy_import/mod.rs`、`src/stats/workbuddy_import/store.rs`。

- 验收 `AC038`：在写前/写后/ACK前崩溃都不会把不确定历史当direct计入。

- 证据：`evidence/S05.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.07 实现pending确认与撤销事务

- 前置：`S05.06`。

- 操作：预览冻结水位和revision，应用/撤销分批；规则不能覆盖已知代理区间。

- 允许修改：`src/stats/workbuddy_import/store.rs`。

- 验收 `AC039`：过期/重复token、冲突revision拒绝；tombstone不复活；撤销只影响workbuddy行。

- 证据：`evidence/S05.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S05.08 通过归属关卡

- 前置：`S05.07`。

- 操作：执行G04加原WorkBuddy同步/端点测试；审查日志/快照无凭据。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC040`：代理排除、历史pending、规则确认/撤销逐一提供前后计数。

- 证据：`evidence/S05.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S06 导入事务与运行调度

阶段关卡：`runtime`。依赖：前一阶段全部通过。

### S06.01 实现独立有界导入通道

- 前置：`S05.08`。

- 操作：使用专用容量2 channel与最多1个待ACK批次；实现运行state，不挤原事件队列。

- 允许修改：`src/stats/workbuddy_import/mod.rs`、`src/stats/mod.rs`。

- 验收 `AC041`：队列满只让worker退让，Proxy begin/finish不阻塞。

- 证据：`evidence/S06.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.02 接入writer批次事务

- 前置：`S06.01`。

- 操作：借用现有writer连接，统一提交ledger/fact/checkpoint，当前revision再校验。

- 允许修改：`src/stats/workbuddy_import/store.rs`、`src/stats/mod.rs`。

- 验收 `AC042`：任一SQL失败全批rollback；offset不得领先事实。

- 证据：`evidence/S06.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.03 实现同键重放与冲突

- 前置：`S06.02`。

- 操作：同hash no-op、不同hash撤回workbuddy明细置conflict；proxy不能被删除/覆写。

- 允许修改：`src/stats/workbuddy_import/store.rs`。

- 验收 `AC043`：ACK丢失/重复批/复制文件/重复工具响应都计一次。

- 证据：`evidence/S06.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.04 实现状态与计数发布

- 前置：`S06.03`。

- 操作：缓存status DTO所需快照，累计数来自ledger/检查点，运行状态不冒充全部完成。

- 允许修改：`src/stats/workbuddy_import/mod.rs`、`src/stats/mod.rs`。

- 验收 `AC044`：restart后累计数可信，错误文件未处理时状态仍需处理。

- 证据：`evidence/S06.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.05 实现公平与退避

- 前置：`S06.04`。

- 操作：实时事件/恢复优先，导入一批后让出，1～30秒退避且不增加全局SQLite超时。单调时钟固定5秒调度增量及目录发现；tick/手动检查合并至多一个待办，不重入、不逐批等待5秒。

- 允许修改：`src/stats/mod.rs`、`src/stats/workbuddy_import/mod.rs`。

- 验收 `AC045`：持续回填时代理事件仍被写入；DB锁超时不忙等、不丢未ACK批。通过X45～X47，页面状态不改变同步周期。

- 证据：`evidence/S06.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.06 实现启停与全局开关联动

- 前置：`S06.05`。

- 操作：默认off；独立import_enabled和effective_enabled；暂停ACK后不再提交。开启/恢复/有效开启的核心重启立即调度；系统唤醒合并漏过tick，不绑定页面生命周期。

- 允许修改：`src/stats/mod.rs`、`src/stats/workbuddy_import/mod.rs`。

- 验收 `AC046`：关闭后源文件读取字节不增长，已有SQL查询仍可用，恢复断点继续。页面刷新关闭/切页/隐藏窗口不暂停同步。

- 证据：`evidence/S06.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.07 实现shutdown与崩溃恢复

- 前置：`S06.06`。

- 操作：使用原4秒总预算，当前事务完成或回滚；worker句柄可回收，未提交offset不保存。

- 允许修改：`src/stats/mod.rs`、`src/main.rs`、`src/stats/workbuddy_import/mod.rs`。

- 验收 `AC047`：中断后重启无丢失无重复，不等待历史全导完才退出。

- 证据：`evidence/S06.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S06.08 通过运行关卡

- 前置：`S06.07`。

- 操作：执行G05和并发Mock转发；此时仍无用户UI入口，隔离测试显式启用。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC048`：证明worker没在Tokio/UI线程读文件；记录队列峰值与批次耗时。

- 证据：`evidence/S06.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S07 清除、保留与数据生命周期

阶段关卡：`lifecycle`。依赖：前一阶段全部通过。

### S07.01 接入手动清除墓碑

- 前置：`S06.08`。

- 操作：原clear冻结范围内，事务先deleted_manual再删明细；保存账本。

- 允许修改：`src/stats/store.rs`、`src/stats/workbuddy_import/store.rs`。

- 验收 `AC049`：重扫同消息不复活，代理原清除语义不变。

- 证据：`evidence/S07.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.02 接入retention墓碑

- 前置：`S07.01`。

- 操作：已入库过期导入→deleted_retention；首次过期候选→excluded_retention。

- 允许修改：`src/stats/store.rs`、`src/stats/workbuddy_import/store.rs`。

- 验收 `AC050`：保留期限缩短再延长不恢复已删除条目；不限期0正确。

- 证据：`evidence/S07.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.03 保护预览水位并发

- 前置：`S07.02`。

- 操作：清除过程中暂停/串行化导入分类提交，确认as_of外新记录不误删。

- 允许修改：`src/stats/mod.rs`、`src/stats/store.rs`。

- 验收 `AC051`：预览后新增/晚到记录留存，删除数量和预览边界一致。

- 证据：`evidence/S07.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.04 处理候选的过期与删除

- 前置：`S07.03`。

- 操作：pending确认时再次查当前retention、ledger终态；清理候选JSON的持久时机。

- 允许修改：`src/stats/workbuddy_import/store.rs`。

- 验收 `AC052`：候选变旧不能绕过retention，deleted状态不可被mapping apply覆盖。

- 证据：`evidence/S07.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.05 处理消失/替换文件

- 前置：`S07.04`。

- 操作：文件消失保留事实；30天不可见checkpoint可回收，ledger不回收。

- 允许修改：`src/stats/workbuddy_import/reader.rs`、`src/stats/workbuddy_import/store.rs`。

- 验收 `AC053`：删除源文件不删统计；同文件恢复不重复。

- 证据：`evidence/S07.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.06 完善索引与数据修订

- 前置：`S07.05`。

- 操作：为origin/time和ledger pending/time创建索引；撤回/删除提升维护revision。

- 允许修改：`src/stats/migrations/0007.sql`、`src/stats/query.rs`、`src/stats/mod.rs`。

- 验收 `AC054`：EXPLAIN无每页JSONL读取或全量候选反序列化；旧游标返回expired。

- 证据：`evidence/S07.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.07 验证费用与恢复隔离

- 前置：`S07.06`。

- 操作：导入永不创建cost row、永不进入proxy orphan recovery；源错误不改proxy health。

- 允许修改：`src/stats/store.rs`、`src/stats/workbuddy_import/store.rs`。

- 验收 `AC055`：恢复只处理proxy in_flight；旧费用快照未变。

- 证据：`evidence/S07.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S07.08 通过生命周期关卡

- 前置：`S07.07`。

- 操作：执行G06覆盖清除/保留/重扫/重启的组合顺序。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC056`：给出每种组合操作后的事实/账本/费用行数，不能只检查HTTP200。

- 证据：`evidence/S07.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S08 API、筛选与报表统一

阶段关卡：`api`。依赖：前一阶段全部通过。

### S08.01 扩展共享DTO与版本

- 前置：`S07.08`。

- 操作：加入origin/source_scope/capture_method/recorded/nullable元数据/点数；API3、control11。 仅为新enum/nullable签名更新上述模块的穷尽匹配和row mapping，不改Collector解析行为或代理状态判定。

- 允许修改：`src/stats/dto.rs`、`src/main.rs`、`src/frontend.rs`、`gpui/src/usage_table.rs`、`gpui/src/statistics.rs`、`src/stats/query.rs`、`src/stats/store.rs`、`src/stats/collector.rs`、`src/stats/event.rs`、`src/stats/mod.rs`。

- 验收 `AC057`：所有enum match编译；只做旧UI必要适配，不提前新布局；proxy数值原样。

- 证据：`evidence/S08.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.02 统一筛选SQL和hash

- 前置：`S08.01`。

- 操作：每个报表/候选入口复用origin与capture过滤；include_tests只针对测试请求。

- 允许修改：`src/stats/query.rs`、`src/stats/dto.rs`。

- 验收 `AC058`：all=builtin+supplier；supplier=proxy+direct（同过滤、排除tests）；非法组合拒绝。

- 证据：`evidence/S08.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.03 修正指标口径与费用未知

- 前置：`S08.02`。

- 操作：增加proxy/import计数、已知状态分母、导入unpriced和独立credit合计。

- 允许修改：`src/stats/query.rs`、`src/stats/dto.rs`。

- 验收 `AC059`：混合和纯导入结果准确；不同币种与credit绝不相加。

- 证据：`evidence/S08.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.04 实现八个导入管理命令

- 前置：`S08.03`。

- 操作：按DATA-API表完整实现status/settings/scan/pending/mapping preview/apply/revoke preview/revoke。

- 允许修改：`src/stats/mod.rs`、`src/stats/workbuddy_import/mod.rs`、`src/stats/dto.rs`。

- 验收 `AC060`：仍受管理鉴权/1MiB限制；扫描异步入队，错误码固定无SQL路径。

- 证据：`evidence/S08.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.05 扩展分页与候选一致性

- 前置：`S08.04`。

- 操作：请求/分组/filter options/pending cursor覆盖新筛选与revision，冻结as_of水位。

- 允许修改：`src/stats/query.rs`、`src/stats/dto.rs`。

- 验收 `AC061`：切来源旧cursor拒绝；并发追加不重复分页；归属撤回旧cursor失效。

- 证据：`evidence/S08.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.06 接入GPUI scheduler任务类型

- 前置：`S08.05`。

- 操作：新增导入read/write job；read去重，写操作严格排队，离页不丢已授权写。

- 允许修改：`gpui/src/stats_backend.rs`、`gpui/src/stats_controller.rs`。

- 验收 `AC062`：慢旧回复不能覆盖新source；scan轮询不形成无限请求。

- 证据：`evidence/S08.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.07 完成协议兼容测试

- 前置：`S08.06`。

- 操作：更新所有协议10硬编码断言到目标11并保留旧协议case；验证frontend替换旧core链。

- 允许修改：`scripts/core_compatibility.py`、`src/frontend.rs`、`gpui/src/stats_backend.rs`、`scripts/gpui_package_test.py`、`scripts/statistics_test.py`。

- 验收 `AC063`：不把旧core伪判为兼容；不靠吞反序列化错误显示空页。

- 证据：`evidence/S08.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S08.08 通过接口关卡

- 前置：`S08.07`。

- 操作：执行G07和原statistics/pagination/price测试；生成固定fixture的完整API快照。

- 允许修改：`scripts/workbuddy_import_test.py`、`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC064`：SQL汇总、API响应和fixture期望逐项一致，不能只验证接口存在。

- 证据：`evidence/S08.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S09 三图标界面与导入交互

阶段关卡：`ui`。依赖：前一阶段全部通过。

### S09.01 实现三个SVG和来源组

- 前置：`S08.08`。

- 操作：按U03新增资产分支和独立source_filter组件；默认all，三个互斥选择。

- 允许修改：`gpui/src/assets.rs`、`gpui/src/source_filter.rs`、`gpui/src/main.rs`。

- 验收 `AC065`：图标不是fallback加号；hover/focus/键盘/选中a11y齐全。

- 证据：`evidence/S09.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.02 落实筛选条与窄屏布局

- 前置：`S09.01`。

- 操作：按U04宽度/顺序/断点；新增高级capture下拉，保留日期默认当天。

- 允许修改：`gpui/src/statistics.rs`、`gpui/src/source_filter.rs`、`gpui/src/stats_controller.rs`。

- 验收 `AC066`：A=1012/1011/560/559布局测试；来源组永远最前且不拆开。

- 证据：`evidence/S09.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.03 落实筛选状态联动

- 前置：`S09.02`。

- 操作：按U06先invalidates再reset分页/filter，再一次发起报表和候选查询。

- 允许修改：`gpui/src/statistics.rs`、`gpui/src/stats_controller.rs`。

- 验收 `AC067`：旧响应/错误/loading不覆盖；同选项点击零新请求。

- 证据：`evidence/S09.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.04 落实指标趋势与空态

- 前置：`S09.03`。

- 操作：按U07/U08更新计数/成功率/完整率/费用/点数文案和9指标布局。

- 允许修改：`gpui/src/statistics.rs`、`gpui/src/usage_chart.rs`。

- 验收 `AC068`：导入未知显示—，费用无伪零线；纯导入成功率为—。

- 证据：`evidence/S09.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.05 落实明细来源与详情

- 前置：`S09.04`。

- 操作：时间后新增来源列、末尾点数列；recorded标签和时间依据详情；保留列宽拖动/分页。

- 允许修改：`gpui/src/usage_table.rs`、`gpui/src/statistics.rs`。

- 验收 `AC069`：proxy/内置/直连可区分，缺HTTP/速率不显示0/200。

- 证据：`evidence/S09.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.06 落实导入设置和健康区

- 前置：`S09.05`。

- 操作：新模块渲染开关/检查/full/状态/六计数，与原存储设置接线。显示固定5秒后台同步说明；保留原页面刷新选项和默认关闭，不新增实时选项或同步周期设置。

- 允许修改：`gpui/src/workbuddy_import.rs`、`gpui/src/statistics.rs`、`gpui/src/main.rs`。

- 验收 `AC070`：开关失败回滚，暂停不删历史，普通刷新不读JSONL，首次开启无多余确认。页面刷新与后台同步完全独立，见X45。

- 证据：`evidence/S09.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.07 落实归属确认与撤销弹窗

- 前置：`S09.06`。

- 操作：按U11分页候选、区间、无默认选择、preview token确认和撤销；实现中英文。

- 允许修改：`gpui/src/workbuddy_import.rs`、`gpui/src/stats_controller.rs`。

- 验收 `AC071`：过期/冲突重预览；不能把proxy证据强改direct；无裸内部错误。

- 证据：`evidence/S09.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S09.08 通过界面关卡

- 前置：`S09.07`。

- 操作：执行G08与真实隔离GPUI操作，保存规定截图和事件证据。

- 允许修改：`scripts/workbuddy_import_ui_test.py`、`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC072`：3来源×2尺寸×2语言截图齐全，额外异常态/键盘/缩放有证据；截图不能替代实际数据断言。

- 证据：`evidence/S09.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

## S10 全链路验证、性能与交接

阶段关卡：`release_gate`。依赖：前一阶段全部通过。

### S10.01 验证混合真实进程链路

- 前置：`S09.08`。

- 操作：启动隔离Proxy+Mock上游，加入builtin/direct日志和同一次proxy镜像日志，从真实API/UI核对。

- 允许修改：`scripts/workbuddy_import_test.py`。

- 验收 `AC073`：三类集合正确且代理只计一次；不发公网付费调用。

- 证据：`evidence/S10.01.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.02 验证故障注入矩阵

- 前置：`S10.01`。

- 操作：自动化DB锁/ACK丢失/杀进程/rename/truncate/磁盘写失败/权限/大行。

- 允许修改：`scripts/workbuddy_import_test.py`。

- 验收 `AC074`：数据无静默丢失，恢复幂等，代理继续响应；固定种子可复现。

- 证据：`evidence/S10.02.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.03 验证预算与查询性能

- 前置：`S10.02`。

- 操作：固定100k/1M条及1GiB合成集，记录导入吞吐/RSS/idle读取/转发P95/SQL延迟。执行X45～X47及最坏tick相位测试，记录源落盘/commit/查询可见时间。

- 允许修改：`scripts/workbuddy_import_bench.py`。

- 验收 `AC075`：达到ACCEPTANCE硬门槛；达不到修实现，不擅改门槛。常态增量每次≤10秒可查，不以UI刷新或中位数替代。

- 证据：`evidence/S10.03.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.04 跑完整既有回归

- 前置：`S10.03`。

- 操作：执行G09全部核心/GPUI/脚本/包验证；将新集成测试加入既有CI适当步骤。

- 允许修改：`.github/workflows/ci.yml`、`.github/workflows/desktop.yml`、`scripts/workbuddy_import_test.py`。

- 验收 `AC076`：旧断言不放宽，无新warning；新脚本必须真的执行生产二进制。

- 证据：`evidence/S10.04.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.05 完成目标平台验收

- 前置：`S10.04`。

- 操作：Windows x64、macOS ARM64/x64；Linux核心；记录各平台自动与GUI证据。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC077`：缺平台标BLOCKED_EXTERNAL，不把编译通过算GUI或打包通过算安装。

- 证据：`evidence/S10.05.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.06 完成数据升级和回滚演练

- 前置：`S10.05`。

- 操作：临时v6库升级真实新二进制、故障回滚、旧版本拒绝新库、停止后恢复备份。

- 允许修改：`scripts/workbuddy_import_test.py`、`docs/workbuddy-import/IMPLEMENTATION-REPORT.md`。

- 验收 `AC078`：代理历史数据/价格/配置前后摘要一致；无真实库/应用替换。

- 证据：`evidence/S10.06.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.07 更新用户文档与交付报告

- 前置：`S10.06`。

- 操作：补README用量导入章节、设计实现对照、已知限制、AC状态、文件清单和复现命令。

- 允许修改：`README.md`、`docs/workbuddy-import/IMPLEMENTATION-REPORT.md`、`docs/workbuddy-import/PROGRESS.md`。

- 验收 `AC079`：每项完成必须有证据链接；没有发布授权就不升版本或制作Release。

- 证据：`evidence/S10.07.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。

### S10.08 最终范围审查与交接

- 前置：`S10.07`。

- 操作：git diff --check、allowlist、依赖锁/版本/旧迁移指纹、隐私扫描；整理提交建议与待批准项。

- 允许修改：`docs/workbuddy-import/PROGRESS.md`、`docs/workbuddy-import/IMPLEMENTATION-REPORT.md`。

- 验收 `AC080`：不自动提交/推送/合main；真实作者和工具贡献准确，禁force/amend/hooks。

- 证据：`evidence/S10.08.md`（详细说明+日志链接）；记录到 PROGRESS 后继续下一项。
