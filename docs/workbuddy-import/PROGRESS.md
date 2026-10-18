# 开发执行状态

## 当前复核入口（2026-10-18）

本轮当前ARM核心为`bcaa0836…`，UI`8f476d71…`；核心553/UI139、fmt/Clippy/release、27组真实生产门禁、完整12阶段性能矩阵和百万状态轮询均通过。原545核心测试名称保留，旧失败不覆盖。实际进度条展示剩余文件/数据量与已入库条数；历史回填后台推进，常态固定5秒增量不受页面刷新控制。最新用户合同见[ACCEPTANCE](ACCEPTANCE.md)。

当前配对完成12原生黄金矩阵、20万有效行动态进度与异常态、输入/键盘/分页/滚动、100刷新不读源和7个刷新/可见性场景；候选ARM实际镜像与百万回填期间驻留全应用Cmd+Q已有通过证据。x64为本机Rosetta构建/运行，不能称物理Intel验收。

四组完整百万断点续传及最终ARM/x64文档镜像的终态以[机器可读入口](../../evidence/R09-20261017-CURRENT.json)和[实现报告](IMPLEMENTATION-REPORT.md)为准；未经全部实际断言及最终收口检查不标完成。[80AC](ACCEPTANCE-AUDIT.md)／[47X](CROSS-CUTTING-AUDIT.md)按当前哈希逐条提供命名测试、运行断言和日志SHA，不以旧DONE表推导完成率。Linux/Windows执行、Windows GUI、物理Intel、OS挂载/安装边界保留。未提交、推送、合并或发布。

12阶段每阶段20次正常增量和4并发代理对照均达标；百万阶段每个查询条件50次×3轮，100k阶段为3次×1轮。矩阵包含一次300秒空闲实测，不是12次；300个CPU样本中位数约0.145%，正文读量不变。查询加速的整个核心峰值单独报告，不能以导入worker约38.5MiB上界代替报表进程约453.6MiB峰值。详见[实测范围](../../evidence/R09-20261018-MAPPED-READER-OBSERVED-LIMITS.json)。

## 历史执行记录（以下状态、哈希和待办均为当时范围）

> 2026-10-17 ac03历史检查点（不代表当前f402候选）：当前核心 ac03b905… 已通过537测试/Clippy/release，GPUI 27367fd4… 已通过137测试/Clippy/release及最终12原生图、X45和竞争回填事件预算。21个串行release gate、R05保留期限53项、status补验13项、私有DMG及原生关闭/重开/Cmd+Q均已通过。外置TMP全新12阶段严格规模矩阵仍在运行，未计总关卡PASS。AC003 GUI完整隔离证明、AC060结果边界补验、AC072完整异常态/缩放及外部平台仍有缺口；固定80条以[当前审计](ACCEPTANCE-AUDIT.md)为准，不沿用原DONE完成率。未提交、推送、合并、发布。

> 2026-10-17 接手复核：下表DONE是上一轮执行记录，不能作为当前验收结论。R01～R09发现实际链路缺陷及假阳性；当前修复状态与新证据以[REMEDIATION.md](REMEDIATION.md)为准，未重验的原PASS均不可用于最终交付。保留原历史记录以便追溯。

设计交付时仅有文档、合成fixture及设计校验脚本；**下列业务开发任务均未执行**。设计校验通过不允许把 S01 或任何业务任务标 DONE。

状态：TODO / DOING / DONE / BLOCKED_EXTERNAL / BLOCKED_DESIGN。只能有一个 DOING；DONE 必须有 evidence 文件和测试退出码。阶段未过不能进入下一阶段。

| 任务 | 状态 | 证据 |
|---|---|---|
| S01.01 核对开发位置和基线 | DONE | [evidence/S01.01.md](../../evidence/S01.01.md) |
| S01.02 登记既有入口与风险 | DONE | [evidence/S01.02.md](../../evidence/S01.02.md) |
| S01.03 准备隔离运行目录 | DONE | [evidence/S01.03.md](../../evidence/S01.03.md) |
| S01.04 落实合成黄金样例 | DONE | [evidence/S01.04.md](../../evidence/S01.04.md) |
| S01.05 定义内部类型 | DONE | [evidence/S01.05.md](../../evidence/S01.05.md) |
| S01.06 建立测试入口与预期清单 | DONE | [evidence/S01.06.md](../../evidence/S01.06.md) |
| S01.07 固定日志与错误规范 | DONE | [evidence/S01.07.md](../../evidence/S01.07.md) |
| S01.08 完成基线验收 | DONE | [evidence/S01.08.md](../../evidence/S01.08.md) |
| S02.01 审查 v7 迁移设计稿 | DONE | [evidence/S02.01.md](../../evidence/S02.01.md) |
| S02.02 接入升级版本与备份 | DONE | [evidence/S02.02.md](../../evidence/S02.02.md) |
| S02.03 扩展统一事实表 | DONE | [evidence/S02.03.md](../../evidence/S02.03.md) |
| S02.04 实现身份账本基础操作 | DONE | [evidence/S02.04.md](../../evidence/S02.04.md) |
| S02.05 实现检查点 CAS | DONE | [evidence/S02.05.md](../../evidence/S02.05.md) |
| S02.06 实现映射与设置持久层 | DONE | [evidence/S02.06.md](../../evidence/S02.06.md) |
| S02.07 验证旧记录与费用不变 | DONE | [evidence/S02.07.md](../../evidence/S02.07.md) |
| S02.08 通过数据库关卡 | DONE | [evidence/S02.08.md](../../evidence/S02.08.md) |
| S03.01 实现消息外壳解析 | DONE | [evidence/S03.01.md](../../evidence/S03.01.md) |
| S03.02 实现身份与时间验证 | DONE | [evidence/S03.02.md](../../evidence/S03.02.md) |
| S03.03 实现主 Token 规则 | DONE | [evidence/S03.03.md](../../evidence/S03.03.md) |
| S03.04 实现缓存和推理规则 | DONE | [evidence/S03.04.md](../../evidence/S03.04.md) |
| S03.05 实现精确 credit | DONE | [evidence/S03.05.md](../../evidence/S03.05.md) |
| S03.06 实现质量和候选摘要 | DONE | [evidence/S03.06.md](../../evidence/S03.06.md) |
| S03.07 运行格式兼容与隐私测试 | DONE | [evidence/S03.07.md](../../evidence/S03.07.md) |
| S03.08 通过解析关卡 | DONE | [evidence/S03.08.md](../../evidence/S03.08.md) |
| S04.01 实现受限目录发现 | DONE | [evidence/S04.01.md](../../evidence/S04.01.md) |
| S04.02 实现文件指纹 | DONE | [evidence/S04.02.md](../../evidence/S04.02.md) |
| S04.03 实现有限buffer读行 | DONE | [evidence/S04.03.md](../../evidence/S04.03.md) |
| S04.04 实现尾行和坏行推进 | DONE | [evidence/S04.04.md](../../evidence/S04.04.md) |
| S04.05 实现每文件公平遍历 | DONE | [evidence/S04.05.md](../../evidence/S04.05.md) |
| S04.06 实现读取变更验证 | DONE | [evidence/S04.06.md](../../evidence/S04.06.md) |
| S04.07 实现低优先完整重扫 | DONE | [evidence/S04.07.md](../../evidence/S04.07.md) |
| S04.08 通过读取关卡 | DONE | [evidence/S04.08.md](../../evidence/S04.08.md) |
| S05.01 抽取已有代理归属判定 | DONE | [evidence/S05.01.md](../../evidence/S05.01.md) |
| S05.02 实现当前配置观察 | DONE | [evidence/S05.02.md](../../evidence/S05.02.md) |
| S05.03 实现分类纯函数 | DONE | [evidence/S05.03.md](../../evidence/S05.03.md) |
| S05.04 实现历史区间与别名复用 | DONE | [evidence/S05.04.md](../../evidence/S05.04.md) |
| S05.05 接入同步prepare/finalize | DONE | [evidence/S05.05.md](../../evidence/S05.05.md) |
| S05.06 实现崩溃后prepared恢复 | DONE | [evidence/S05.06.md](../../evidence/S05.06.md) |
| S05.07 实现pending确认与撤销事务 | DONE | [evidence/S05.07.md](../../evidence/S05.07.md) |
| S05.08 通过归属关卡 | DONE | [evidence/S05.08.md](../../evidence/S05.08.md) |
| S06.01 实现独立有界导入通道 | DONE | [evidence/S06.01.md](../../evidence/S06.01.md) |
| S06.02 接入writer批次事务 | DONE | [evidence/S06.02.md](../../evidence/S06.02.md) |
| S06.03 实现同键重放与冲突 | DONE | [evidence/S06.03.md](../../evidence/S06.03.md) |
| S06.04 实现状态与计数发布 | DONE | [evidence/S06.04.md](../../evidence/S06.04.md) |
| S06.05 实现公平与退避 | DONE | [evidence/S06.05.md](../../evidence/S06.05.md) |
| S06.06 实现启停与全局开关联动 | DONE | [evidence/S06.06.md](../../evidence/S06.06.md) |
| S06.07 实现shutdown与崩溃恢复 | DONE | [evidence/S06.07.md](../../evidence/S06.07.md) |
| S06.08 通过运行关卡 | DONE | [evidence/S06.08.md](../../evidence/S06.08.md) |
| S07.01 接入手动清除墓碑 | DONE | [evidence/S07.01.md](../../evidence/S07.01.md) |
| S07.02 接入retention墓碑 | DONE | [evidence/S07.02.md](../../evidence/S07.02.md) |
| S07.03 保护预览水位并发 | DONE | [evidence/S07.03.md](../../evidence/S07.03.md) |
| S07.04 处理候选的过期与删除 | DONE | [evidence/S07.04.md](../../evidence/S07.04.md) |
| S07.05 处理消失/替换文件 | DONE | [evidence/S07.05.md](../../evidence/S07.05.md) |
| S07.06 完善索引与数据修订 | DONE | [evidence/S07.06.md](../../evidence/S07.06.md) |
| S07.07 验证费用与恢复隔离 | DONE | [evidence/S07.07.md](../../evidence/S07.07.md) |
| S07.08 通过生命周期关卡 | DONE | [evidence/S07.08.md](../../evidence/S07.08.md) |
| S08.01 扩展共享DTO与版本 | DONE | [evidence/S08.01.md](../../evidence/S08.01.md) |
| S08.02 统一筛选SQL和hash | DONE | [evidence/S08.02.md](../../evidence/S08.02.md) |
| S08.03 修正指标口径与费用未知 | DONE | [evidence/S08.03.md](../../evidence/S08.03.md) |
| S08.04 实现八个导入管理命令 | DONE | [evidence/S08.04.md](../../evidence/S08.04.md) |
| S08.05 扩展分页与候选一致性 | DONE | [evidence/S08.05.md](../../evidence/S08.05.md) |
| S08.06 接入GPUI scheduler任务类型 | DONE | [evidence/S08.06.md](../../evidence/S08.06.md) |
| S08.07 完成协议兼容测试 | DONE | [evidence/S08.07.md](../../evidence/S08.07.md) |
| S08.08 通过接口关卡 | DONE | [evidence/S08.08.md](../../evidence/S08.08.md) |
| S09.01 实现三个SVG和来源组 | DONE | [evidence/S09.01.md](../../evidence/S09.01.md) |
| S09.02 落实筛选条与窄屏布局 | DONE | [evidence/S09.02.md](../../evidence/S09.02.md) |
| S09.03 落实筛选状态联动 | DONE | [evidence/S09.03.md](../../evidence/S09.03.md) |
| S09.04 落实指标趋势与空态 | DONE | [evidence/S09.04.md](../../evidence/S09.04.md) |
| S09.05 落实明细来源与详情 | DONE | [evidence/S09.05.md](../../evidence/S09.05.md) |
| S09.06 落实导入设置和健康区 | DONE | [evidence/S09.06.md](../../evidence/S09.06.md) |
| S09.07 落实归属确认与撤销弹窗 | DONE | [evidence/S09.07.md](../../evidence/S09.07.md) |
| S09.08 通过界面关卡 | DONE | [evidence/S09.08.md](../../evidence/S09.08.md) |
| S10.01 验证混合真实进程链路 | DONE | [evidence/S10.01.md](../../evidence/S10.01.md) |
| S10.02 验证故障注入矩阵 | DONE | [evidence/S10.02.md](../../evidence/S10.02.md) |
| S10.03 验证预算与查询性能 | DONE | [evidence/S10.03.md](../../evidence/S10.03.md) |
| S10.04 跑完整既有回归 | DONE | [evidence/S10.04.md](../../evidence/S10.04.md) |
| S10.05 完成目标平台验收 | BLOCKED_EXTERNAL | [evidence/S10.05.md](../../evidence/S10.05.md) |
| S10.06 完成数据升级和回滚演练 | DONE | [evidence/S10.06.md](../../evidence/S10.06.md) |
| S10.07 更新用户文档与交付报告 | DONE | [evidence/S10.07.md](../../evidence/S10.07.md) |
| S10.08 最终范围审查与交接 | DONE | [evidence/S10.08.md](../../evidence/S10.08.md) |

## 执行日志

原始日志写入被 `.gitignore` 忽略的 `dist/workbuddy-import-validation/<任务号>/`；摘要证据为 `evidence/<任务号>.md`。状态由上方表格与本小节共同证明，勾选本身不构成验收。

### S01.01 核对开发位置和基线

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`，分支 `codex/workbuddy-usage-import`。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.01.md`。
- 实现要点：只读核验 toplevel/分支/HEAD/worktree 列表；登记 21 个未跟踪设计文件 SHA256；定位其他 worktree 改动归属（`codex/protocol-compat` 的 `docs/protocol-compat/` 属其他业务、未触碰）；确认 main 干净且不含设计包。
- 命令与退出码：`git status --short --branch`、`git rev-parse HEAD`、`git worktree list`、`git diff`、`git stash list`、`shasum -a 256` → 均 0；基线断言 14 条 → 0（14 PASS / 0 FAIL）；负例对照 → 2 EXPECTED-FAIL（证明断言可失败）。
- 证据：`evidence/S01.01.md`；原始日志 `dist/workbuddy-import-validation/S01.01/`。
- 风险/遗留：无。未使用 reset/stash/clean；无业务代码改动，全部新行为仍未启用。

### S01.02 登记既有入口与风险

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.01 PASS）。
- 修改文件：`docs/workbuddy-import/DECISIONS.md`、`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.02.md`。
- 实现要点：逐文件重算 `baseline.json` 的 30 个 SHA256（30/30 MATCH）；核对 `SCHEMA_VERSION=6`、`API_VERSION=2`、`CONTROL_PROTOCOL=10` 与声明一致并确认 `0007.sql` 未被占用；定位 writer/reader 线程、有界队列、store 迁移与备份、query 过滤器、dto 共享、GPUI scheduler/reply gate、workbuddy 归属判定等接线锚点；把三处既有形态差异（无条件 `apply_step(6)`、`source='relay' OR ?=1`、`unwrap_or` 兜底枚举）登记为“已被冻结设计点名修复”，非待决提案。
- 命令与退出码：`shasum -a 256` 比对 → 0（30/30）；入口断言 12 条 → 0（12 PASS / 0 FAIL）；负例对照 → 0（篡改哈希/0007 占用/版本漂移均被检出）。
- 证据：`evidence/S01.02.md`；原始日志 `dist/workbuddy-import-validation/S01.02/`（含首轮误写断言的 `assertions-attempt1.txt`）。
- 风险/遗留：无未解决版本偏差；未自选迁移编号、未覆盖文件、未改生产代码。

### S01.03 准备隔离运行目录

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.02 PASS）。
- 修改文件：`scripts/workbuddy_import_test.py`（新建）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.03.md`。
- 实现要点：一次性 `TemporaryDirectory` 承载 HOME/USERPROFILE/APPDATA/LOCALAPPDATA/XDG_*/TMPDIR 与 `--config-dir`；`WORKBUDDY_PROXY_CATALOG_TEST_URL=http://127.0.0.1:1/catalog` 阻断公网价格请求；导入源根固定为沙箱内 `home/.workbuddy` 并断言非真实目录；真实三处 profile 路径仅做 `stat` 元数据前后比对；CLI 固定 `--exe`/`--output`，报告写 JSON 且失败返回非零。
- 命令与退出码：`cargo build --release --locked` → 0（1m30s）；`python3 scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy` → 0（25 checks PASS / 0 FAIL，含真实 core 启动、control_protocol=10、运行文件落在沙箱、rc=0）。
- 证据：`evidence/S01.03.md`；原始日志 `dist/workbuddy-import-validation/S01.03/`；报告 `dist/workbuddy-import-validation/workbuddy_import_test.json`。
- 错误场景：缺失二进制 → 19 passed/1 failed 且 exit=1（不伪报通过）；泄漏 HOME 被隔离断言检出；报告隐私扫描 `/Users/` 与用户名 0 命中。
- 风险/遗留：无。未启用任何新业务行为。

### S01.04 落实合成黄金样例

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.03 PASS）。
- 修改文件：`scripts/fixtures/workbuddy_import/**`（新建 10 个文件）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.04.md`。
- 实现要点：按字节复制设计 fixture（expected.json/session-test.jsonl/tail-prefix.txt 哈希相同，README 保留原文并追加本地章节）；新增 `session-split.jsonl`（同 messageId 拆分工具响应）、`session-bad.jsonl`（语法错误/会话不符/缺身份/非法时间/缺模型 + 2 条合法对照）、`session-tail.jsonl`（1028 B，第三行 342 B 无换行）及三份人工预期。
- 命令与退出码：fixture 自检脚本 → 0（problems=0，覆盖字节一致、行可解析、sessionId 约定、canary 边界、隐私模式、JSON 合法性、黄金值独立复算 350/60/410、credit 2130000000、builtin 180、supplier 570）。
- 证据：`evidence/S01.04.md`；原始日志 `dist/workbuddy-import-validation/S01.04/`（含首轮 README 断言误写的 attempt1）。
- 错误场景：首轮 `problems=1`（README 误要求全文相同）已精确化为“设计原文前缀相同”，非放宽断言。
- 风险/遗留：大行/坏 UTF-8/权限/DB 锁样例按 README 约定留给对应阶段在临时目录合成。

### S01.05 定义内部类型

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.04 PASS）。
- 修改文件：`src/stats/workbuddy_import/types.rs`（新建）、`src/stats/workbuddy_import/mod.rs`（新建）、`src/stats/mod.rs`（仅模块声明 + 启动契约自检）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.05.md`。
- 实现要点：定义 `Candidate`/`NormalizedUsage`/`ParseDisposition`/`ImportBatch`/`CheckpointAck` 及支撑枚举与固定码；状态用枚举、未知用 `Option`；`ID_LIMIT_CHARS`/`TOKEN_LIMIT`/`TOTAL_LIMIT` 复用 `dto` 常量；无 `serde_json::Value`、无线程/channel/连接。
- 关键决策：私有 `mod stats;` 下新类型无使用者会触发 `dead_code`（探针：可导出链 0 错误 / 私有链 exit 101；首轮真实 clippy 报 9 条）。按“不加 allow 规避 Clippy”的要求，未使用任何 `#[allow]`/`#[expect]`，改为在 `StatsHandle::start` 真实启动路径执行 `contract_check()` 并 fail-closed；自检由 5 个小子函数组成，构造每个枚举变体。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0（无警告、无抑制）；`cargo test --locked` → 0（186 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S01.05.md`；日志 `dist/workbuddy-import-validation/S01.05/{gate,scope-selfcheck,negative-control}.txt`。
- 错误场景：扫描器负例对照命中篡改副本（`value_hits=[9] thread_hits=[627]`）、真实文件 0 命中；首轮 9 条 `dead_code` 为真实失败并已消除而非放宽门槛。
- 风险/遗留：`contract_check` 失败分支无法在不改源码下从外部触发，留待 S01.06 单元测试覆盖；未启用任何新业务行为。

### S01.06 建立测试入口与预期清单

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.05 PASS）；WB-RT-001 影响列表不含本项，无需补验。
- 修改文件：`src/stats/workbuddy_import/tests.rs`（新建）、`src/stats/workbuddy_import/mod.rs`（仅加 `#[cfg(test)] mod tests;` 3 行）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.06.md`。
- 实现要点：建立生产模块测试入口与按阶段登记的用例清单；19 个已可执行用例覆盖契约/限值、未知与显式零语义、固定码唯一性、诊断计数饱和、批次边界、ACK 语义、身份散列形状；parser 红→绿用例留到 S03。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（205 passed / 0 failed / **0 ignored**，186 既有 + 19 新增）。
- 证据：`evidence/S01.06.md`；日志 `dist/workbuddy-import-validation/S01.06/{gate,guard-and-scope,guard-attempt1..4}.txt`。
- 错误场景：质量守卫负例对照命中 `#[ignore]` 与 `assert!(true)` 篡改；守卫 4 次修正均为收窄断言而非放宽，历史日志全部保留。
- 风险/遗留：`mod.rs` 增加一行 `#[cfg(test)]` 挂载（TASKS 未列该文件，但挂载测试入口必需，已在证据登记）；未启用任何新业务行为。

### S01.07 固定日志与错误规范

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/types.rs`、`scripts/workbuddy_import_test.py`、`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.07.md`。
- 实现要点：新增 `ImportErrorCode`（DATA-API 第5节全 11 码，内部码 / `stats_` API 码 / parse / 批次拒绝映射）；`DiagnosticKind::ALL` 与 `parse`、`DiagnosticCounts::iter` 把诊断键固定为闭集 10 项；启动自检增加 `check_error_codes()`（共 6 组不变量）；隔离脚本新增 `CANARIES` 与 `scan_canaries()` 并接入真实 core 运行后的沙箱扫描。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（211 passed / 0 failed / 0 ignored）；`cargo build --release --locked` → 0；`python3 scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy` → 0（26 checks PASS）。
- 证据：`evidence/S01.07.md`；日志 `dist/workbuddy-import-validation/S01.07/{gate,run,canary-negative}.txt`、`run.json`。
- 错误场景：canary 扫描器负例对照命中种入文件（`leaky.json`/`leaky.log`/`api-response`）且不回显路径；真实沙箱扫描 0 命中。
- 风险/遗留：AC007 目前是负向证明，正向覆盖（导入链路存在后 canary 仍 0 命中）留给 S03.07/S10；未启用任何新业务行为。

### S01.08 完成基线验收（阶段关卡 baseline）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.07 PASS）。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S01.08.md`；日志写入忽略目录 `dist/`。
- 环境：macOS Darwin 24.6.0 arm64；rustc/cargo 1.98.1；Python 3.14.8；`RUST_TEST_THREADS=1`、`WORKBUDDY_PROXY_CATALOG_TEST_URL=http://127.0.0.1:1/catalog`。
- G00 命令与退出码：`git status --short --branch`/`git rev-parse HEAD`/`git worktree list` → 0；`python3 docs/workbuddy-import/validate_spec.py` → 0（147 checks，scope=design_only）；`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（211 passed / 0 failed / 0 ignored）；GPUI 三项（fmt/clippy/test）→ 0（80 passed / 0 failed / 0 ignored）。
- 既有警告解释：GPUI clippy 输出 2 条第三方依赖 `block v0.1.6` 的 future-incompat 提示，非本次引入、非 lint 错误，退出码 0；无其他旧失败。
- 新行为未启用证据：导入模块在模块外仅 2 处引用（声明 + 启动契约自检）；GPUI 0 引用；新模块 0 线程/连接；schema/API/协议仍 6/2/10；迁移仍 0001～0006；Cargo 清单未动；真实 core 隔离启动 26 checks PASS。
- 证据：`evidence/S01.08.md`；日志 `dist/workbuddy-import-validation/S01.08/{g00-core,g00-gpui,not-enabled}.txt`。
- 阶段结论：`baseline` PASS，S01 八项全部 DONE 且有证据。
- 风险/遗留：设计校验不代表业务验收；导入链路尚未实现，WB-RT-001 的 5 秒调度由 S04/S06 按新契约实现。

### S02.01 审查 v7 迁移设计稿

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S01.08 阶段关卡 PASS）。
- 修改文件：`src/stats/migrations/0007.sql`（新建）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S02.01.md`。
- 实现要点：用真实 core 在隔离沙箱生成 v6 库并写入合成数据，复制到临时文件后按 `apply_step` 语义（BEGIN 前 FK OFF、单事务、`foreign_key_check`）执行设计稿；逐项核对 4 个新列、3 张新表、视图、4 个触发器、10 个索引、4 个设置列；负例注入验证互斥 CHECK、FK 与“导入行不得有费用”触发器；正文逐字节落实为 `0007.sql`（仅替换顶部 DESIGN ONLY 注释）。
- 命令与退出码：`python3 dist/workbuddy-import-validation/S02.01/migrate_probe.py` → 0（45 项断言 PASS，`problems=0`，`user_version` 6→6，`foreign_key_check=0`，`integrity_check=ok`）；`diff` 正文 → 0（两侧 SHA256 `eb1dc4c0…` 相同）。
- 证据：`evidence/S02.01.md`；日志 `dist/workbuddy-import-validation/S02.01/{v6-schema,v6-related-ddl,design-sql-run,body-identity}.txt`。
- 错误场景：proxy 缺 `run_id`、import 带 `http_status`、无主费用行、导入行费用插入四类负例全部被正确拒绝。
- 风险/遗留：`0001`～`0006` 与 baseline.json SHA256 逐字节 MATCH；`0007` 尚未接入 `store.rs`（S02.02 负责），当前仓库行为未变。

### S02.02 接入升级版本与备份

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`、`src/stats/migrations/0007.sql`（1 列定义）、`scripts/native_protocol_test.py`（1 行）、`evidence/S02.02.md`、`evidence/S02.01.md`（追加偏离）、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`SCHEMA_VERSION=7` + `include_str!("migrations/0007.sql")`；无条件 `apply_step(6)` 改为 `version<6`；新增 `version<7` 步骤（BEGIN 前 FK OFF、失败与成功路径都恢复 FK ON）；迁移循环扩到 `0..=6` 并构造真 v6 库；新增 v7 测试覆盖费用保留、FK 指向与恢复、`foreign_key_check=0`、重复启动 no-op、非法步骤不假升版本。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（212 passed / 0 failed / 0 ignored）；S02.01 探针（改为验证实际 0007.sql）→ 0（problems=0）。
- 证据：`evidence/S02.02.md`；日志 `dist/workbuddy-import-validation/S02.02/{gate,gate-after-default}.txt`、`dist/workbuddy-import-validation/S02.01/migration-0007-run.txt`。
- 错误场景：首轮真实失败 `208 passed; 4 failed`（exit=101，报错为 record_origin 互斥 CHECK）已保留在 `gate.txt`；根因为设计稿去掉 v6 的 `upstream_protocol` 默认值，按 DATA-API §3 恢复默认值后 `212 passed; 0 failed`，未放宽任何断言。
- 最小越界（已登记）：`0007.sql` 恢复 v6 默认值；`native_protocol_test.py:363` 的 `user_version` 断言 6→7（该脚本在 CI 中运行但不在任何允许列表内）。
- 风险/遗留：设计稿 `docs/workbuddy-import/schema-v7.sql` 未修改，偏离只体现在生产迁移与证据；导入行必须显式传 `upstream_protocol=NULL`（S02.03/S06 实现时遵守）。

### S02.03 扩展统一事实表

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`（新增 1 个测试）、`evidence/S02.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：事实表的 proxy/import 互斥 CHECK、nullable transport 与 `recorded` 状态已在 S02.01/S02.02 落地，本项在**真实 v7 迁移后的库**上逐例验证：16 个必须拒绝（proxy 缺 run/stream/protocol、proxy 带 recorded/credit/message_timestamp；import 带 HTTP/耗时/错误码/run/stream、import 非 recorded、import 声称 upstream 或 proxy、import 依赖 protocol 默认值）与 3 个必须接受（合法 proxy、合法 import 显式 NULL、import 带精确 credit），外加“import 行不得有费用”触发器；每个拒绝用例都断言错误文本含 `CHECK constraint failed`。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（213 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S02.03.md`；日志 `dist/workbuddy-import-validation/S02.03/gate.txt`。
- 错误场景：首轮 `E0308`（数组元素类型不一致）与次轮 `no such table: stats_runs`（`open_connection` 不跑迁移）均为真实失败，已修正为显式 `migrate` 后再注入，未删减用例、未加抑制。
- 风险/遗留：`0007.sql` 本项无需改动；导入行必须显式传 `upstream_protocol=NULL`（已由拒绝用例锁定）。

### S02.04 实现身份账本基础操作

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（新建）、`src/stats/workbuddy_import/mod.rs`（模块声明 + 聚合自检）、`src/stats/mod.rs`（1 行调用名）、`evidence/S02.04.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`LedgerState`（7 固定状态 + 闭集方法）、`LedgerError`、`LedgerRow`/`NewLedgerEntry`、`load`（按 entry_key）、`insert`（先查再插、禁用 REPLACE、imported 时 request_id=entry_key）、`set_state`（tombstone 终态、候选白名单与 16 KiB 上限、非候选状态清空 candidate）；全部借用调用方 Transaction，模块内不建连接。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0（无抑制）；`cargo test --locked` → 0（220 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S02.04.md`；日志 `dist/workbuddy-import-validation/S02.04/gate.txt`。
- 错误场景：4 轮真实失败均已保留并修正——9 条 `dead_code`（改为内存库启动自检）、3 处 `E0502` 借用冲突、`updated_at_ms < created_at_ms` 与 proxy 行 `complete` 缺 token 的测试数据错误、conflict 用例缺事实行的 FK 拒绝。
- 最小越界（已登记）：`workbuddy_import/mod.rs` 接线与聚合自检；`src/stats/mod.rs` 1 行调用名改为聚合入口。
- 风险/遗留：`ledger_contract_check` 在启动路径执行一次内存库迁移（不碰用户数据），开销待 S06 启动预算一并复核；未启用任何新业务行为。

### S02.05 实现检查点 CAS

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（检查点 CAS + 8 测试 + `LedgerError`→`StoreError` 重命名）、`src/stats/workbuddy_import/mod.rs`（聚合自检 +1）、`evidence/S02.05.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`Checkpoint`（对齐 20 列）、`NewCheckpoint`/`insert_checkpoint`（generation=1/revision=1/offset=0，重复登记拒绝）、`CheckpointGuard`+`CheckpointUpdate`+`commit_checkpoint`（乐观 CAS：namespace/generation/revision/base_offset 四项校验，失败返回 `stale_batch`/`source_changed` 且不写任何行）、`diagnostics_json`（固定 10 键形状）、`checkpoint_contract_check`（内存库启动自检）。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（228 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S02.05.md`；日志 `dist/workbuddy-import-validation/S02.05/gate.txt`。
- 错误场景：一轮真实失败——两处期望误写为“仅非零键”，按 DATA-API 的固定形状语义保留实现并把期望改为硬编码完整闭集（含键数断言），未放宽任何约束。
- 风险/遗留：`StoreError` 由 S02.04 的 `LedgerError` 重命名（账本/检查点共用，新增 `InvalidOffset`）；S02.04 证据中的旧名称对应该类型；未启用任何新业务行为。

### S02.06 实现映射与设置持久层

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.05 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（映射/设置持久层 + 8 测试）、`src/stats/workbuddy_import/mod.rs`（聚合自检 +1）、`evidence/S02.06.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：映射四枚举（class/evidence/state/reconcile）与 `ModelMapping`/`NewMapping`；`insert_mapping` 与 `set_mapping_state` 做 Rust 侧 verified 重叠检查（SQL 触发器兜底）；`match_mapping` 半开区间、仅 verified、`LIMIT 2` 且多匹配返回 `Conflict`（绝不挑最新）；`ImportSettings` + `set_import_enabled`（沿用单例 `revision` 做 CAS；启用时用 `config::token()` 生成 48 hex namespace 一次，关闭保留、再开复用）；`mapping_contract_check` 内存库启动自检。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（236 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S02.06.md`；日志 `dist/workbuddy-import-validation/S02.06/gate.txt`。
- 错误场景：3 轮真实失败——`E0004`/`E0308` 编译错误；clippy 的未构造变体/未使用函数/冗余闭包/可合并 if（前两者通过启动自检真实调用解决，多余的 `StoreError::SettingsConflict` 语义上由 `SettingsOutcome` 承担而删除）；冲突夹具被触发器拒绝（证明触发器有效，改为显式 DROP TRIGGER 构造损坏态后断言仍报 Conflict）。
- 风险/遗留：`workbuddy_mapping_revision` 的递增留待 S05 规则应用时实现；未启用任何新业务行为。

### S02.07 验证旧记录与费用不变

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`（新增 1 个比对测试）、`evidence/S02.07.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：手工构造真实 v6 库（0001～0006，含 0002 的 FK-OFF 重建路径），写入 2 个 run、7 条请求（四种 upstream_protocol 各一条带完整 Token、failed+upstream_http、in_flight、connection_test+not_applicable）、5 条费用（USD/CNY priced、default_zero、usage_incomplete、not_applicable，均带完整价格快照）；用显式 35 列清单把迁移前后的行转成 `rusqlite::types::Value` 序列逐项比对。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（237 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S02.07.md`；日志 `dist/workbuddy-import-validation/S02.07/gate.txt`。
- 错误场景：两轮真实失败——测试数据把 `input_uncached_tokens` 与 `read_subset` 组合违反 v6 约束（改为自洽的 `disjoint`）；费用行指向不存在的请求触发 FK（改为指向已存在的 `req-probe`）。均为测试数据修正，未放宽约束。
- 风险/遗留：无。未改动生产逻辑。

### S02.08 通过数据库关卡（阶段关卡 schema）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.07 PASS）；WB-RT-001 未改 schema。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S02.08.md`；日志与探针在忽略目录 `dist/`。
- G01 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（237 passed / 0 failed / 0 ignored）。
- 专项覆盖：schema/迁移/存储相关用例 30 个（store.rs 迁移专项 6 个，含 0..=6→7、7→7、v6 全字段比对、互斥 CHECK、v1→v2 与 v3 回归）；导入模块用例 48 个（类型/错误码 25、账本 7、检查点 8、映射与设置 8）。
- 故障注入：9/9 PASS（冲突表使升级失败、`user_version` 不假升、无行丢失、`integrity_check=ok`、视图仍可查、无半成品表、移除故障后可恢复、恢复后表齐备且无悬空引用）。
- SQL 人工检查：仅视图与事实表的有意重建；无 `DELETE FROM`/`VACUUM`/`writable_schema`/额外 `DROP`；备份仅在 `version>0`；每个 `apply_step` 都以 `?` 传播，无吞错回退；`0001`～`0006` 字节 MATCH。
- 证据：`evidence/S02.08.md`；日志 `dist/workbuddy-import-validation/S02.08/{g01,fault-injection,sql-inspection}.txt`、`fault_probe.py`。
- 阶段结论：`schema` PASS，S02 八项全部 DONE 且有证据。
- 风险/遗留：三个内存库启动自检的开销留待 S06 启动预算复核；其余见 S02.02/S02.06 证据。

### S03.01 实现消息外壳解析

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S02.08 阶段关卡 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/parser.rs`（新建）、`src/stats/workbuddy_import/mod.rs`（模块声明 + 聚合自检 +1）、`evidence/S03.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`NumberLiteral`（整数/非整数，绝不截断）与 `NumberText`（credit 原始字面量）；typed 白名单 `RawLine`/`RawProvider`/`RawUsage`（snake_case，故意不含歧义顶层 `cached_tokens`）/`NormalizedUsageFields`（camelCase）；`parse_shell` 要求 JSON 对象，`message`+`assistant` 与 `function_call`（role 缺失/assistant）纳入、其余排除；`parser_contract_check` 启动自检覆盖接受/排除/固定码/credit 保真。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（248 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S03.01.md`；日志 `dist/workbuddy-import-validation/S03.01/gate.txt`。
- 错误场景：3 条 `dead_code`（改为启动自检真实调用，未加抑制）；`parse_shell(b"[]")` 因 serde 允许 seq→struct 而返回 `Ok(NotUsage)`（已显式要求 `{` 开头）；自检 excluded 列表误含一条本应接受的 assistant 行（已删除）。
- 风险/遗留：身份（entry_key/session 校验）与 Token/credit 规则分别留给 S03.02–S03.05。

### S03.02 实现身份与时间验证

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/parser.rs`（身份编码 + 时间验证 + 10 测试 + 启动自检扩展）、`evidence/S03.02.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`entry_key`/`hash_id` 用 `u32 big-endian 长度 + UTF-8 字节` 的 length-prefix SHA256（域 `wbp-wb-message-v1`）；`identify` 依次校验 session 等于文件 stem、ID 无控制字符且 ≤160、`messageId` 必需、时间戳为非负整数毫秒且不超过 `now+24h`；`upstream_model` 取 `responseModelId→model→requestModelId` 首个有效值；`Identity::{Ready,Malformed,Pending,InvalidTimestamp}`；不保存原始 session/message id。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（258 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S03.02.md`；日志 `dist/workbuddy-import-validation/S03.02/gate.txt`。
- 错误场景：三轮真实失败——clippy 变体名与枚举同名（改为 `Pending`）与字面布尔断言；启动自检 `now_ms=-1` 的断言与 24h 窗口语义不符（改为恰好越界）；夹具里的 `\u0001` 被写成真实控制字节致 JSON 非法（改为字面转义，使测试真正覆盖“JSON 合法但 ID 含控制字符”）。
- 风险/遗留：Token/credit 规则留给 S03.03–S03.05；分类（pending_classification）留给 S05。

### S03.03 实现主 Token 规则

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/normalize.rs`（新建）、`src/stats/workbuddy_import/mod.rs`（模块声明 + 聚合自检 +1）、`evidence/S03.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：按 DATA-API 第2节固定优先级解析 input/output/total；存在但非法的高优先值不被低优先别名掩盖；两包装对同一事实不一致记 `conflicting_usage`；total 存在时必须等于 input+output（不纠正、不反推）；仅当两侧都合法时用 `checked_add` 推导并置 `total_tokens_derived`；input/output ≤10^12、total ≤2×10^12 含端点；`invalid` 记录 Token 列全 `None`，缺值为 `None`、显式 0 保留。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（271 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S03.03.md`；日志 `dist/workbuddy-import-validation/S03.03/gate.txt`。
- 错误场景：两轮真实失败——`E0603`（`UsageReason`/`UsageStatus` 未由 types 重导出，改为从 `dto` 导入）；clippy `collapsible_if`（改为 let 链，逻辑不变）。
- 风险/遗留：缓存/推理（S03.04）、credit（S03.05）、质量与候选摘要（S03.06）尚未填充对应字段。

### S03.04 实现缓存和推理规则

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/normalize.rs`（缓存/推理规则 + 13 测试 + 启动自检扩展）、`evidence/S03.04.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：按 DATA-API 第2节优先级取最高优先**合法**别名；低优先别名给出不同合法值时只置 `cache_alias_disagreement` 且不清零胜出值；某字段只有非法别名时整条 `invalid`；`cache_read/cache_write ≤ input`、`reasoning ≤ output`（含端点）违反即 `invalid`；缓存绝不加到 input/output/total；`input_breakdown` 只标 `read_subset`/`unknown`；只有缓存的记录由 `missing` 升为 `partial`；已 invalid 记录不再获得从属值。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（284 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S03.04.md`；日志 `dist/workbuddy-import-validation/S03.04/gate.txt`。
- 错误场景：三轮真实失败——断言引用了事实表列 `input_uncached_tokens`（改为断言 `input_breakdown` 与不宣称 unsupported，并清除两次行级修补留下的孤儿行）；X08 夹具 `input=1000 < cache_read=4096` 违反子集约束致整条 invalid（规则正确、夹具不合理，改为 input=8192）；同一用例误断言“低优先别名不存在”而夹具中它存在（改为断言 diagnostic 置位）。
- 风险/遗留：credit 精确解析（S03.05）与质量/候选摘要（S03.06）尚未实现。

### S03.05 实现精确 credit

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/normalize.rs`（credit 定点解析 + 12 测试 + 启动自检扩展）、`evidence/S03.05.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`parse_credit_nanos` 纯文本 + `checked` 整数运算（全程无 `f64`），1 credit = 10^9 nanos；拒绝空串、科学计数法、负号、多余/前导/尾随小数点、非数字字符、>9 位小数与任何溢出；`normalize_credit` 失败只置 `credit_invalid`，`workbuddy_credit_nanos` 保持 None，status/reason/Token 值不变；新增 `credit_literal()` 经真实 `parse_shell` 取字面量，夹具与生产解析不脱节。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（296 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S03.05.md`；日志 `dist/workbuddy-import-validation/S03.05/gate.txt`。
- 错误场景：一轮真实失败——自检误用不存在的 `NumberText::for_test`，且 `parser.rs` 不在允许修改列表；改为通过公开 `parse_shell` 路径构造夹具（未越界改 parser.rs）。
- 风险/遗留：质量与候选摘要（S03.06）尚未实现。

### S03.06 实现质量和候选摘要

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.05 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/parser.rs`（`evaluate` 顶层编排 + 9 测试 + 启动自检扩展）、`src/stats/workbuddy_import/types.rs`（`ParseDisposition::JsonSyntaxError` 变体与诊断映射）、`evidence/S03.06.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`evaluate` 一次调用串起外壳→身份→规范化→缓存/推理→credit；判定顺序为语法错误 → 非 usage 类型 → 无 usage 对象（`ignored_no_usage`，先于身份）→ 缺 messageId（`pending_identity`）→ 身份损坏（`malformed`）→ 时间戳（`invalid_timestamp`）→ 候选；`invalid`/`missing` 的 usage 仍形成候选以带状态落库；每个非候选处置恰好贡献一个闭集诊断计数。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（305 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S03.06.md`；日志 `dist/workbuddy-import-validation/S03.06/gate.txt`。
- 错误场景：两轮真实失败——启动自检引用循环外变量 `excluded`（改为局部字面量）；`DiagnosticCounts::add` 缺 amount 参数（补 1）。另删除本项测试中一行恒真断言。
- 风险/遗留：格式兼容与隐私测试（S03.07）尚未运行。

### S03.07 运行格式兼容与隐私测试

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S03.07.md`；探针与日志在忽略目录 `dist/`（本项为运行+记录任务，未改任何源码或夹具）。
- G01：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（305 passed / 0 failed / 0 ignored；parser+normalize 专项 68 例）。
- 格式兼容：23 行夹具全部执行；键序反转 23/23、未知字段注入 23/23、缩进重排 23/23 判定与取值不变；仅 `session-bad.jsonl` 有 1 行坏行（设计如此）；`session-tail.jsonl` 1028 B、最长单行 430 B。
- 隐私：4 个派生期望文件与 49 个生产日志 **0 canary**；S01.07 负向夹具仍能触发扫描器；日志不引用 19 个夹具 messageId；期望文件只引用真实存在的合成 ID；隔离运行测试 26/26 PASS（含 canary 不入日志/配置/数据库）。
- 证据：`evidence/S03.07.md`；日志 `dist/workbuddy-import-validation/S03.07/{g01,format-privacy,isolation}.txt`。
- 错误场景：探针自身 4 轮断言错误（负向夹具被当泄漏、坏行夹具被当失败、长行阈值取错对象、设计期自检日志与期望文件的合成 ID 被当泄漏），全部修正为针对生产产物的检查，并补“扫描器仍能触发”的正向断言。
- 风险/遗留：可进入分类实现（S04）；WB-RT-001 影响的 S04.01/S04.05 将在该阶段落实固定 5 秒调度。

### S03.08 通过解析关卡（阶段关卡 parser）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.07 PASS）；WB-RT-001 未改解析契约。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S03.08.md`；日志在忽略目录 `dist/`。
- G01 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（305 passed / 0 failed / 0 ignored）。
- 专项覆盖：导入模块 116 例全绿，其中解析与规范化 68 例（外壳 11、身份 10、主 Token 13、缓存推理 13、credit 12、处置 9）。
- 诊断码闭集：静态核对 6/6 PASS（ALL 恰 10 项、10 个码无重复且全 snake_case、四个 malformed 原因齐全、oversized_line 由 reader 产出而解析层不产生）；运行时由 `types::contract_check()` 做 as_str/parse 往返校验并 fail-closed。
- 隐私与格式兼容：S03.07 探针复跑 problems=0（键序/未知字段/缩进等价，canary 在期望文件与生产日志零出现）。
- 证据：`evidence/S03.08.md`；日志 `dist/workbuddy-import-validation/S03.08/{g01,diagnostic-closure,format-privacy}.txt`。
- 阶段结论：`parser` PASS，S03 八项全部 DONE 且有证据。
- 风险/遗留：`oversized_line` 的产出路径在 S04 reader 实现并测试；解析层无 I/O、无线程、不在转发路径上。下一阶段 S04 起落实 WB-RT-001 的固定 5 秒调度。

### S04.01 实现受限目录发现（WB-RT-001 首个落地项）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S03.08 阶段关卡 PASS）。
- **WB-RT-001**：`SYNC_INTERVAL = Duration::from_secs(5)` 编译期常量，同时服务增量同步与新文件发现；reader 不接受任何设置参数，不出现 `stats_settings`/`Settings`/`refresh_interval`/`interval_ms`（测试以拼接构造的禁止词断言）；无“实时”选项、周期不可配置。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（新建）、`src/stats/workbuddy_import/mod.rs`（模块声明 + 聚合自检 +1）、`evidence/S04.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`ReaderRoot`（默认 `config::home()/.workbuddy`，可注入隔离根，每次从顶重遍历）、惰性 `Discovery`（逐目录推进、坏条目跳过不中断）、`DiscoveredFile`、纯函数 `is_allowed_relative` 与 `file_from_entry`、`reader_contract_check`；只进入 `projects/<单层项目>/*.jsonl`，`traces`/`tool-results`/`storage`/`backups`/嵌套目录永不进入。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（314 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.01.md`；日志 `dist/workbuddy-import-validation/S04.01/gate.txt`。
- 错误场景：三轮真实失败——**真实 symlink 逃逸缺陷**（`read_dir` 会跟随末段符号链接，`projects` 为 symlink 时整棵外部目录被读入；已在打开前对链接本身做检查并修复）；clippy dead_code（抽出共用纯函数 + 自检真实推进遍历，未加抑制）；`include_str!` 禁止词自指（改用片段拼接构造）。
- 风险/遗留：文件内容读取（S04.03）、指纹与截断/替换（S04.02）、调度接线（S04.05）尚未实现。

### S04.02 实现文件指纹

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（指纹与变更判定 + 12 测试 + 启动自检扩展）、`evidence/S04.02.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`PREFIX_LEN=4096`/`ANCHOR_LEN=256`；`fingerprint` 计算待存指纹（小文件前缀全覆盖，锚点跟随已提交偏移且不超过窗口）；纯决策表 `decide` + 读盘判定 `classify_change`（symlink/目录 → Replaced 不读内容；短于已提交偏移或短于记忆区间 → Truncated）；比较时**按记忆的 prefix_len 重新散列**当前文件，故小文件增长判为追加而非替换；已提交区间内的改写由锚点捕获。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（326 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.02.md`；日志 `dist/workbuddy-import-validation/S04.02/gate.txt`。
- 错误场景：无（首轮全绿，未出现需修正的断言或夹具）。
- 风险/遗留：generation 递增与账本保留由 S04.05 在事务内执行（账本终态语义见 S02.04）；有限 buffer 读行见 S04.03。

### S04.03 实现有限buffer读行

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（有限 buffer 读行 + 11 测试 + 启动自检扩展）、`evidence/S04.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`READ_CHUNK=64KiB`、`MAX_LINE_BYTES=4MiB`（含端点）、UTF-8 BOM 仅在偏移 0 剥离、行尾 CRLF 去 `\r`；超限行立即丢弃已缓冲内容并继续排空到下一个 `\n`（内存上界 `MAX_LINE_BYTES+READ_CHUNK`，与磁盘行长无关）；**未终止尾行不是一行**（EOF 丢弃残留并返回 None，`end` 永不越过最后一个换行，符合 DATA-API 第5节/X11）。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（337 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.03.md`；日志 `dist/workbuddy-import-validation/S04.03/gate.txt`。
- 错误场景：一轮真实失败——测试手算的追加后偏移写成 33（实际 32），只修正期望值未改夹具；clippy 的 `buffered_bytes` 无生产使用点，改为在启动自检的 `/dev/null` 路径上真实调用并断言上界（未加抑制）。
- 风险/遗留：坏行诊断推进与 oversized 排空的可重启恢复见 S04.04。

### S04.04 实现尾行和坏行推进

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（批次读取与推进 + 9 测试 + 启动自检扩展）、`evidence/S04.04.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`ReadOutcome{disposition, oversized, commit_to}` 与 `diagnostic()`（每行恰好一次诊断）；`read_lines` 从偏移起最多读 limit 条完整行并逐行 `evaluate`；未换行末尾不提交（拆分成多次写入的记录写完前完全不可见、写完后恰好入一次）；坏完整行计一次诊断并**照常推进**（否则文件永久卡死）；超长完整行排空并提交偏移（诊断只记一次、重启不重复），而**尚未出现换行的**超长行不排空不提交。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（346 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.04.md`；日志 `dist/workbuddy-import-validation/S04.04/gate.txt`。
- 错误场景：两轮真实失败——测试把 `String` 传给 `&str` 参数（`E0308`）；clippy 的 `ReadOutcome::diagnostic` 无生产使用点（改为在启动自检中构造两种 outcome 并断言诊断码，未加抑制）。
- 风险/遗留：每文件公平遍历与调度接线见 S04.05（WB-RT-001 影响项）。

### S04.05 实现每文件公平遍历（WB-RT-001 第二个落地项）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.04 PASS）。
- **WB-RT-001**：公平遍历运行在 S04.01 固定的 `SYNC_INTERVAL=5s` 节奏内，调度器不接受任何设置参数、不含刷新选项符号；固定节奏决定“多久跑一次”，公平窗口决定“一次跑多少”。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（公平调度器 + 10 测试 + 启动自检扩展）、`evidence/S04.05.md`、`docs/workbuddy-import/PROGRESS.md`；mod.rs 无需改动。
- 实现要点：`PER_FILE_BYTES=8MiB`、`PER_FILE_TIME=50ms`（任一达限即让出）、`ACTIVE_WINDOW=32`；`TurnResult::exhausted()`；`FairScheduler::plan` 先服务**已追平**文件的增量、再按 `rotation` 轮转 backlog（周期=backlog 长度）；`record` 维护落后集合。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（356 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.05.md`；日志 `dist/workbuddy-import-validation/S04.05/gate.txt`。
- 错误场景：两轮真实失败——启动自检把 struct literal 写进 `if` 条件（改为先绑定局部变量）；轮转周期断言放在第 3 片而实际周期为 3 片（改为“三片互不相同 + 第 4 片回到首片”，实现未改）。
- 风险/遗留：读取变更验证（S04.06）、低优先完整重扫（S04.07）、调度接线到运行时（S06.05/S06.06）尚未实现。

### S04.06 实现读取变更验证

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.05 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（批次变更验证 + 9 测试 + 启动自检扩展）、`evidence/S04.06.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`ReadBatch{base_offset,end_offset,outcomes}`（`is_empty`/`imported`/`diagnostics`）；`BatchRejection{SourceChanged,Unreadable}`；`verify_batch` 复用 `classify_change`；`read_verified_batch` 做**读前+读后双重核对**，任一步失败即返回拒绝且不返回任何批次；权限失败不推进 offset，恢复后同一批行仍被读到（不丢行）；symlink 归 `SourceChanged` 且不读内容。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（365 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.06.md`；日志 `dist/workbuddy-import-validation/S04.06/gate.txt`。
- 错误场景：一轮真实失败——测试把 `String` 传给 `&str` 参数（`E0308`）；权限用例在断言前恢复权限，避免失败时污染临时目录并掩盖真实原因。
- 风险/遗留：低优先完整重扫（S04.07）与调度接线（S06.05/S06.06）尚未实现。

### S04.07 实现低优先完整重扫

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/reader.rs`（重扫游标与触发条件 + 7 测试 + 启动自检扩展）、`evidence/S04.07.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`FULL_RESCAN_INTERVAL_MS=24h`；`RescanTrigger{Daily,Manual}`（运行中不重叠）；`ScanCursor` 独立游标（offset/rotation/last_full_ms/running）；`read_full_slice` 签名中无 `StoredFingerprint`，结构上不可能读写增量检查点；重扫复用 `read_lines`→`evaluate`→`ReadOutcome`，去重交给账本。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（372 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S04.07.md`；日志 `dist/workbuddy-import-validation/S04.07/gate.txt`。
- 错误场景：四轮真实失败（均为夹具/编辑脚本问题，实现未改）——夹具太短使“中间修改”落在前缀窗口内；行级替换脚本复制出多余 `}`；2 元组改 3 元组后漏改一处解构（`E0308`）；等长替换误用 `999`（比 `10` 多一字符）致大小变化、前提不成立，改为等宽 `99`。
- 风险/遗留：读取关卡（S04.08）尚未执行；重扫的运行时接线见 S06。

### S04.08 通过读取关卡（阶段关卡 reader）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.07 PASS）；WB-RT-001 在本阶段的落地项 S04.01（固定 5 秒常量）与 S04.05（公平遍历运行于该节奏内）均已完成。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S04.08.md`；探针与日志在忽略目录 `dist/`。
- G03 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（372 passed / 0 failed / 0 ignored）。
- 专项覆盖：reader 专项 67 例（discovery 8、fingerprint 12、line 11、advance 9、fairness 10、verify 9、rescan 7），导入模块合计 183 例。
- 字节度量（非文件条数）：`READ_CHUNK=65536`、`MAX_LINE_BYTES=4194304`、单行最大缓冲 4259840、`PER_FILE_BYTES=8388608`、`PREFIX_LEN=4096`、`ANCHOR_LEN=256`；`session-tail.jsonl` 总 1028 / 已提交 686 / 未读 342，`session-test.jsonl` 总 4614 / 已提交 4614 / 最长行 430。
- AC032：未变化文件第二轮正文读取 **0 字节**；增长文件只读追加的 38 字节；无文件读取量超过自身长度；checkpoint 预期可重放（初始 686 与期望一致、未终止片段未提交）。
- **WB-PROPOSAL-001（仅记录，未实施）**：`expected-session-tail.json` 的 `after_appending_newline.committed_offset_bytes=1028`（停在片段末尾）与 DATA-API 第3节“offset 指向下一条未处理数据”给出的 1029（含终止符）差 1 字节；夹具自述为手算。业务结论不变（tailC 恰好入一次、无重复/冲突）。实现按契约执行，设计包与夹具未改。
- 证据：`evidence/S04.08.md`；日志 `dist/workbuddy-import-validation/S04.08/{g01,reader-gate}.txt`。
- 阶段结论：`reader` PASS，S04 八项全部 DONE 且有证据。
- 风险/遗留：WB-PROPOSAL-001 待设计方裁决；下一阶段进入分类（S05）。

### S05.01 抽取已有代理归属判定

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S04.08 阶段关卡 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/workbuddy.rs`（判定拆为共享小函数）、`src/stats/workbuddy_import/classify.rs`（新建，纯委托）、`src/stats/workbuddy_import/mod.rs`（**越界一行**：模块声明 + 聚合自检 +1）、`evidence/S05.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：抽出 `has_managed_marker`/`is_custom_vendor`/`looks_like_local_key`/`is_loopback_endpoint` 四个非敏感小函数（逐字搬移、行为不变），`managed_signature` 改为四者组合，`route` 改 pub；`classify.rs` 只委托不新增规则（`carries_proxy_signature`/`proxy_route`），启动自检断言标记/旧形状/route/按名称与裸 localhost 不成立，并检查生产部分无第二套检测器（无 URL 解析、无 host 判断、无 key 形状判断）。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（379 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.01.md`；日志 `dist/workbuddy-import-validation/S05.01/gate.txt`。
- 错误场景：两轮真实失败均为检查器自身问题（实现未改）——禁止词检查命中 `include_str!` 中的测试模块（改为只查生产部分，并把目标升级为“第二套检测器的实质标志”）；`apiKey` needle 命中契约检查自身构造的样本条目（去掉该 needle 并注明理由）。
- 风险/遗留：当前配置观察（S05.02）、分类纯函数（S05.03）尚未实现。

### S05.02 实现当前配置观察

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/classify.rs`（配置观察 + 7 测试 + 启动自检扩展）、`evidence/S05.02.md`、`docs/workbuddy-import/PROGRESS.md`；mod.rs 无需再改。
- 实现要点：`ConfigObservation` 仅四字段（managed/model_id/route_id/observed_at_ms，无凭据与 URL 字段）；`observe_current` 委托 S05.01 共享判定，仅在我方条目时读模型与 route；`interval()` 恒为 `(observed_at_ms, None)`（永不倒推）；`is_usable()=managed && model_id.is_some()`；`covers(at)` 仅在可用且 `at>=observed_at_ms` 时为真；`current_model_id` 支持 `model` 与 `models`（字符串或对象）三种形态。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（386 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.02.md`；日志 `dist/workbuddy-import-validation/S05.02/gate.txt`。
- 错误场景：一轮真实失败——启动自检的 `legacy` 样本缺模型声明致 `is_usable()` 为假，与新增断言冲突（补 `"model": "probe-model"`，契约与实现未改）。
- 风险/遗留：分类纯函数（S05.03）与 pending 分支尚未实现。

### S05.03 实现分类纯函数

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/classify.rs`（分类纯函数 + 6 测试 + 启动自检扩展）、`evidence/S05.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`Attribution{Proxy,Builtin,Direct,Pending}`（`as_str` 直接给出账本状态名、`is_imported` 仅 Builtin/Direct 为真）；`AttributionInput` 仅含四个已确立事实（proxy_mapping/proxy_fact/builtin_format/confirmed_direct），**无模型名/路由名/主机字段**；`attribute` 固定顺序 proxy→builtin→direct→pending，无第五分支。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（392 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.03.md`；日志 `dist/workbuddy-import-validation/S05.03/gate.txt`。
- 错误场景：一轮真实失败——clippy 的 `Attribution::as_str` 无生产使用点（改为在启动自检断言三种状态名，未加抑制）。
- 风险/遗留：历史区间与别名复用（S05.04）尚未实现。

### S05.04 实现历史区间与别名复用

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/classify.rs`（历史区间匹配 + 7 测试 + 启动自检扩展）、`evidence/S05.04.md`、`docs/workbuddy-import/PROGRESS.md`；store.rs 无需改动。
- 实现要点：`MappingInterval`（mapping_id/model_id/classification/半开区间 `[from,to)`，`covers`）；`IntervalMatch{None,One,Conflict}`；`interval_for` 只按精确模型 ID + 时间匹配；`attribute_with_intervals` 有覆盖区间则由区间决定、冲突返回错误、无区间按 S05.03 顺序回落；`AttributionConflict::OverlappingIntervals` → 错误码 `mapping_conflict`。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（399 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.04.md`；日志 `dist/workbuddy-import-validation/S05.04/gate.txt`。
- 错误场景：无（首轮全绿，7 个新用例与自检断言一次通过）。
- 风险/遗留：同步 prepare/finalize 接线（S05.05）尚未实现。

### S05.05 接入同步 prepare/finalize

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/workbuddy.rs`（SyncHooks/SyncOutcome + configure_workbuddy 接入 + 3 测试）、`src/proxy.rs`（install 分支报告部分状态）、`evidence/S05.05.md`、`docs/workbuddy-import/PROGRESS.md`；stats/mod.rs 无需改动。
- 实现要点：`prepare()` 在**取 config 锁之前、任何写入之前**调用（失败即返回 Err，models.json/config.json 逐字节不变、内存连接数不变）；写文件序列不变；`*cfg = next` 后**先 drop 锁再 finalize**，失败返回 `Ok(SyncOutcome::SyncedUnconfirmed{changed,error})` 且固定错误码 `attribution_unconfirmed`，proxy.rs 据此报告“已同步但归属未确认”；全程不跨 await 持锁。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（402 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.05.md`；日志 `dist/workbuddy-import-validation/S05.05/gate.txt`。
- 错误场景：三轮真实失败均为夹具/断言前提问题（`PathBuf` 未限定；夹具中 config.json 本就不存在；内存断言写死 0 而默认配置自带 workbuddy 连接，改为前后比较）；另有一次编辑事故把 `pub fn route` 拆开，已恢复。
- 风险/遗留：崩溃后 prepared 恢复（S05.06）与真实 stats 钩子接线尚未实现。

### S05.06 实现崩溃后prepared恢复

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.05 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（prepared 恢复 + 7 测试 + 启动自检扩展）、`evidence/S05.06.md`、`docs/workbuddy-import/PROGRESS.md`；mod.rs 无需改动。
- 实现要点：`load_mappings_in_state` 按状态列行；`recover_prepared(tx, enabled, present, now_ms)` 对每条 `prepared` 行按**实际配置**判定：存在→`verified`，不存在→`conflict`，完成会与已确认区间重叠时**不覆盖**同样转 `conflict`；导入关闭时整趟跳过（行保持 `prepared` 且仍不可用于分类）；只处理 `prepared` 行，重复启动幂等。因 `is_usable()` 只认 `verified`，写前/写后/ACK 前三个崩溃点的未决行都不会被计入直连。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（409 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.06.md`；日志 `dist/workbuddy-import-validation/S05.06/gate.txt`。
- 错误场景：一轮真实失败——clippy 报多余的 `PreparedResolution` 枚举从未使用（该抽象无必要，直接删除，未加抑制）。
- 风险/遗留：pending 确认与撤销事务（S05.07）尚未实现。

### S05.07 实现pending确认与撤销事务

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（预览/认领/撤销 + 7 测试 + 启动自检扩展）、`evidence/S05.07.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`MappingPreview` 冻结最高 rowid 水位与规则 revision，token=SHA256(域+mapping_id+水位+revision+区间)（64 hex，TTL 15 分钟）；`claim_mapping_task` 按序拒绝过期/重复 token/冲突 revision/覆盖已确认代理区间（拒绝时零写入），通过则置 `running` 并记录水位；`revoke_mapping_rows` 分批撤回本规则的 imported 行，**先撤 ledger 再删事实行**，删除带 `record_origin IN ('workbuddy_builtin','workbuddy_custom')` 谓词，tombstone 不参与。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（416 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S05.07.md`；日志 `dist/workbuddy-import-validation/S05.07/gate.txt`。
- 错误场景：六轮失败，其中**两个真实实现缺陷**——撤销谓词写成不存在的 `record_origin='workbuddy'` 致事实行删不掉（改为 IN 两个合法值）；先删事实行触发 `ON DELETE SET NULL` 与 ledger CHECK 冲突（改为先撤 ledger 再删事实行）。其余为夹具/编辑问题（`StoreError` 无 `PartialEq`、正则误伤断言、哈希重复触发 UNIQUE、重叠夹具须为 prepared、导入行须显式 NULL、proxy 行需先有 stats_runs）。
- 风险/遗留：归属关卡（S05.08）尚未执行。

### S05.08 通过归属关卡（阶段关卡 classification）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.07 PASS）；WB-RT-001 未改归属契约。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S05.08.md`、`evidence/S01.01.md`（脱敏）；探针与日志在忽略目录 `dist/`。
- G04 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（416 passed / 0 failed / 0 ignored）。
- 专项覆盖：classify 专项 27 例（classify 7、observation 7、attribution 6、interval 7）；原 WorkBuddy 同步/端点测试 13 例全绿；端点检查脚本退出码 0。
- AC040 前后计数：代理排除 `is_imported`、历史 pending `Attribution::Pending`、规则确认 `affected_count`/`watermark_rowid`、崩溃恢复 `completed`/`conflicted`、规则撤销 `withdrawn`/`finished` —— 8 个计数字段逐一有具名断言，审计 8/8 PASS。
- 凭据审查：evidence 39 个 + PROGRESS 共 40 个文件、四类模式 168 项检查全 PASS；修正两处早期泄漏——`S01.01.md` 4 处绝对家目录路径改为 `~/`；`S01.07.md` 的合成 canary 保留但审查规则收紧为 `sk-(?!SYNTHETIC)`。
- 证据：`evidence/S05.08.md`；日志 `dist/workbuddy-import-validation/S05.08/{g01,gate-audit,endpoint-check}.txt`。
- 阶段结论：`classification` PASS，S05 八项全部 DONE 且有证据。
- 风险/遗留：WB-PROPOSAL-001 待裁决；真实 stats 归属钩子将在 S06 接入 configure_workbuddy 的 prepare/finalize。

### S06.01 实现独立有界导入通道

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S05.08 阶段关卡 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/mod.rs`（有界通道 + ACK 窗口 + 运行状态 + 6 测试 + 启动自检扩展）、`evidence/S06.01.md`、`docs/workbuddy-import/PROGRESS.md`；src/stats/mod.rs 无需改动。
- 实现要点：`IMPORT_CHANNEL_CAPACITY=2`、`MAX_PENDING_ACK=1`；`ImportChannel::offer` 用 `try_send` 绝不阻塞（满返回 `Busy`、worker 退出返回 `Closed`）；`sender()` 克隆共享同一容量上界；`AckWindow` 保证最多一个待 ACK 批次（`Busy`/`Mismatch`/`Idle`）；`ImportState` 七态闭合、活动态三个、无 `realtime`。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（422 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.01.md`；日志 `dist/workbuddy-import-validation/S06.01/gate.txt`。
- 错误场景：一轮真实失败——clippy 报 `sender`/`pending` 无生产使用点（改为在启动自检真实调用，并借机断言克隆发送端共享容量上界，未加抑制）。
- 风险/遗留：writer 批次事务（S06.02）、同键重放与冲突（S06.03）、调度接线（S06.05）尚未实现。

### S06.02 接入writer批次事务

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（批次提交 + 5 测试 + 启动自检扩展）、`evidence/S06.02.md`、`docs/workbuddy-import/PROGRESS.md`；src/stats/mod.rs 无需改动。
- 实现要点：`CommitItem`/`CommitReport`；`commit_batch` 在调用方事务内复核 settings revision → 逐项写 ledger（imported 分支另写事实行）→ 提交 checkpoint；同键计 duplicates 不写第二条事实；Proxy/Pending 不写事实；导入事实行显式传 `upstream_protocol=NULL`；失败一律由调用方 rollback，checkpoint 只在全部行成功后同事务推进（offset 不领先事实）。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（427 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.02.md`；日志 `dist/workbuddy-import-validation/S06.02/gate.txt`。
- 错误场景：四轮失败——`CheckpointGuard::revision` 是字段、`CheckpointAck` 携带 `CommitOutcome`、无现成 `StaleBatch` 变体（新增 `BatchRejected`）；`CheckpointUpdate` 字段与假设不符；`file_key` 需恰 64 字符（引入 `batch_key()`）；回滚后 checkpoint 行不存在致 `query_row` panic（改用 `COALESCE(MAX(...),0)`）。
- 风险/遗留：同键重放与冲突（S06.03）、调度接线（S06.05）尚未实现。

### S06.03 实现同键重放与冲突

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（重放解析 + 撤回 + 5 测试）、`evidence/S06.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`ReplayOutcome{New,Duplicate,Conflicted}`；`resolve_replay` 在写任何新行之前判定——同语义 hash 一律 no-op（只计 duplicates，不写第二条事实、不改状态），这正是 ACK 丢失/重复批/复制文件/重复工具响应只计一次的机制；不同 hash 时 `withdraw_workbuddy_fact` **先移出 imported 再删事实行**（避免 `ON DELETE SET NULL` 与 ledger CHECK 冲突），ledger 置 `conflict`；已存在行为 `excluded_proxy` 时完全不动（导入绝不删除或覆写代理决策）；`CommitReport` 新增 `conflicted`。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（432 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.03.md`；日志 `dist/workbuddy-import-validation/S06.03/gate.txt`。
- 错误场景：无（首轮全绿；clippy 无 dead_code 说明新函数均有真实生产使用点）。
- 风险/遗留：状态与计数发布（S06.04）、调度接线（S06.05）尚未实现。

### S06.04 实现状态与计数发布

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/mod.rs`（计数与状态快照 + 5 测试 + 启动自检扩展）、`src/stats/workbuddy_import/store.rs`（**越界一个函数** `load_import_counts`，读取计数属持久层职责）、`evidence/S06.04.md`、`docs/workbuddy-import/PROGRESS.md`；src/stats/mod.rs 无需改动。
- 实现要点：`ImportCounts`（七个 ledger 状态 + files_seen/files_pending/bytes_processed，无任何内存累加器，全部来自查询）；`ImportStatusSnapshot` 与 `is_complete()`（无未解决错误、unresolved=0、files_pending=0、状态非活动才算完整）、`is_idle_with_work_left()`；`load_import_counts` 用 GROUP BY / COUNT / SUM(committed_offset) 读取。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（438 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.04.md`；日志 `dist/workbuddy-import-validation/S06.04/gate.txt`。
- 错误场景：一轮真实失败——clippy 报新类型无生产使用点（改为在启动自检构造快照并断言三类判定，未加抑制）。
- 风险/遗留：公平与退避 + 固定 5 秒调度接线（S06.05，WB-RT-001 影响项）尚未实现。

### S06.05 实现公平与退避（WB-RT-001 第三个落地项）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.04 PASS）。
- **WB-RT-001**：`SyncSchedule` 由 `std::time::Instant`（单调时钟）驱动，`cadence()` 直接返回 `reader::SYNC_INTERVAL`（5 秒编译期常量），增量与新文件发现共用同一到期时间；不读取任何设置、与页面刷新选项无关；`finish_slice` 只在整片结束时排下一次，片内可连续处理多批（不逐批等待 5 秒）。
- 修改文件：`src/stats/workbuddy_import/mod.rs`（单调调度 + 退避 + 6 测试 + 启动自检扩展）、`evidence/S06.05.md`、`docs/workbuddy-import/PROGRESS.md`；src/stats/mod.rs 无需改动。
- 实现要点：`BACKOFF_MIN=1s`/`BACKOFF_MAX=30s`；`SliceOutcome{Progressed,Busy,Failed}`；`request()` 在已有待办时吸收（tick 与手动检查合并至多一个）；`begin_slice` 运行中一律拒绝（不重入）；`Busy`/`Failed` 退避翻倍封顶 30 秒、`Progressed` 清零回到 5 秒；退避只可能让等待变长，不触碰全局 SQLite busy timeout。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（444 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.05.md`；日志 `dist/workbuddy-import-validation/S06.05/gate.txt`。
- 错误场景：一轮真实失败——clippy 报调度类型无生产使用点（新增 `schedule_contract_check()` 在启动自检真实驱动调度器，未加抑制）。
- 风险/遗留：X45～X47 的完整运行验收需运行时接线，安排在 S10.03 发布关卡统一执行；启停与全局开关联动见 S06.06。

### S06.06 实现启停与全局开关联动（WB-RT-001 第四个落地项）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.05 PASS）。
- **WB-RT-001**：`ImportSwitch` 的输入只有两个开关与时钟，启动自检直接扫描生产代码断言不含 `refresh`/`visibility`/`page_lifecycle`/`document` 等页面概念（关闭页面、切页、隐藏窗口无法暂停同步）；开关状态不改变 5 秒周期，恢复时 `ScheduleNow` 让下一片立即开始；`on_wake` 把漏过的 tick 合并为至多一片。
- 修改文件：`src/stats/workbuddy_import/mod.rs`（双开关 + 暂停握手 + 唤醒合并 + 7 测试 + 启动自检扩展）、`evidence/S06.06.md`、`docs/workbuddy-import/PROGRESS.md`；src/stats/mod.rs 无需改动。
- 实现要点：`import_enabled`（默认 off）与 `core_enabled` 相互独立、`effective_enabled()` 需两者皆真；`may_commit()=effective && !paused`（暂停确认后不再提交、不再读取源文件新字节）；`SwitchEffect{None,ScheduleNow,PauseAfterAck}`；`acknowledge_pause()`；`on_wake()` 合并。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（451 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.06.md`；日志 `dist/workbuddy-import-validation/S06.06/gate.txt`。
- 错误场景：一轮真实失败——clippy 报开关类型无生产使用点（新增 `switch_contract_check()` 在启动自检真实驱动开关，未加抑制）。
- 风险/遗留：“关闭后源文件读取字节不增长”与“已有 SQL 查询仍可用”的进程级验证安排在 S10.03；shutdown 与崩溃恢复见 S06.07。

### S06.07 实现shutdown与崩溃恢复

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.06 PASS）；WB-RT-001 影响列表不含本项（4 秒总预算沿用原设计）。
- 修改文件：`src/stats/workbuddy_import/mod.rs`（关闭握手 + 6 测试 + 启动自检扩展）、`evidence/S06.07.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`SHUTDOWN_BUDGET=4s`；`ShutdownOutcome{Committed,RolledBack,TimedOut}`；`ShutdownHandshake` 的重复请求不延长 deadline；预算耗尽一律 `TimedOut`（即使传入 committed=true 也不报完成）；未提交 offset 只存在于事务内、回滚即原封不动；worker 句柄 drop 后 `offer` 返回 `Closed`。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（457 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S06.07.md`；日志 `dist/workbuddy-import-validation/S06.07/gate.txt`。
- 错误场景：一轮真实失败——clippy 报关闭类型无生产使用点（新增 `shutdown_contract_check()` 在启动自检真实驱动握手，未加抑制）。
- 风险/遗留：运行关卡（S06.08）尚未执行；线程接线与真实进程验证安排在 S06.08/S10.03。

### S06.08 通过运行关卡（阶段关卡 runtime）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.07 PASS）；WB-RT-001 在本阶段的落地项 S06.05（单调 5 秒 + 退避）与 S06.06（开关不绑定页面、不改变周期）均已完成。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S06.08.md`；探针与日志在忽略目录 `dist/`。
- G05 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（457 passed / 0 failed / 0 ignored）。
- 专项覆盖：runtime 30 例（channel 6、schedule 6、switch 7、shutdown 6、status 5）+ store 48 例。
- AC048 线程证据：导入路径六个模块静态扫描**零 tokio、零 `.await`、零 spawn、零 async fn**（读文件与写库均为同步阻塞调用，不可能落在 Tokio/UI 线程；与转发的唯一接触点是 S06.01 的非阻塞有界通道）。
- AC048 指标：队列峰值 `IMPORT_CHANNEL_CAPACITY=2`、待 ACK 峰值 `MAX_PENDING_ACK=1`、单片单文件 8 MiB/50 ms、活动窗口 32；批次耗时 20 ms 量级（四个 store 用例自建库 + 提交完整批次，测试内 0.02s）。
- 并发 Mock 转发：`scripts/multi_routing.py` 退出码 0（6 种连接组合、透明模型改写、隔离、迁移、幂等同步等全 PASS）；导入侧仍无用户 UI 入口，隔离测试显式启用。
- 证据：`evidence/S06.08.md`；日志 `dist/workbuddy-import-validation/S06.08/{g01,thread-evidence,batch-timing,concurrent-mock}.txt`。
- 阶段结论：`runtime` PASS，S06 八项全部 DONE 且有证据。
- 风险/遗留：导入线程实际启动与接线在 S07/S08；X45～X47 进程级验收在 S10.03。

### S07.01 接入手动清除墓碑

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S06.08 阶段关卡 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`（`delete_finished` 清除路径落墓碑 + 2 测试）、`evidence/S07.01.md`、`docs/workbuddy-import/PROGRESS.md`；workbuddy_import/store.rs 无需改动。
- 实现要点：在 `MaintenanceKind::Clear` 的分块删除循环中**先**把范围内 `imported` 账本行置为 `deleted_manual` 并清空 `request_id`、**再**执行原有 `DELETE FROM usage_requests`（顺序不可颠倒，否则 `ON DELETE SET NULL` 会与账本 CHECK 冲突）；代理事实行无账本行故受影响行数为 0，删除语句与 Retention 分支均未改动。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（459 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S07.01.md`；日志 `dist/workbuddy-import-validation/S07.01/gate.txt`。
- 错误场景：一轮真实失败——清除水位误设为先插入的代理行 id 致 `deleted=0`；修正为导入行先插入并断言 `proxy_id > imported_id`，水位停在导入行（意图不变、断言未放宽）。
- 风险/遗留：retention 墓碑（S07.02）与预览边界（S07.03）尚未实现；Retention 分支在本项故意留空。

### S07.02 接入retention墓碑

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`（保留清理落 `deleted_retention` 墓碑 + 1 测试）、`src/stats/workbuddy_import/store.rs`（`is_expired`/`BatchPolicy`/首次过期排除 + 3 测试）、`evidence/S07.02.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`RETENTION_DAY_MS`；`is_expired(event_at_ms, now_ms, retention_days)` 在 `retention_days <= 0`（不限期）时一律 false，否则用 saturating 运算比较；`BatchPolicy{settings_revision, retention_days}` 合并两个数字（顺带把 `commit_batch` 参数从 8 降回 7，避免 `too_many_arguments`）；`commit_batch` 在写任何行之前判定过期 → `excluded_retention`（无事实行）；保留清理路径落 `deleted_retention` 墓碑，与手动清除的 `deleted_manual` 可区分。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（463 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S07.02.md`；日志 `dist/workbuddy-import-validation/S07.02/gate.txt`。
- 错误场景：三轮编译期失败——`E0061` 漏改一处调用点；`too_many_arguments` 8/7（未加抑制，改为引入 `BatchPolicy` 把参数降回 7 个）；`E0308` 两处仍是裸 `i64`（逐一改为 `BatchPolicy`）。
- 风险/遗留：预览水位并发保护（S07.03）尚未实现。

### S07.03 保护预览水位并发

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`（预览计数 + 维护闸门 + 4 测试）、`evidence/S07.03.md`、`docs/workbuddy-import/PROGRESS.md`；src/stats/mod.rs 无需改动。
- 实现要点：`Writer::preview_clear(conn, as_of_id, cutoff_ms)` 与清扫**共用同一谓词**（`outcome<>'in_flight' AND ended_at_ms IS NOT NULL AND id<=as_of_id AND started_at_ms<cutoff_ms`）；`is_maintenance_running()` 让导入提前让路；`delete_finished` 在预览为 0 时提前返回（同时让 preview_clear 成为生产调用）；串行化由 SQLite 写锁（清扫用 Immediate 事务）与闸门两层保证。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（467 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S07.03.md`；日志 `dist/workbuddy-import-validation/S07.03/gate.txt`。
- 错误场景：两轮真实失败——`E0502` 借用冲突（`preview_clear` 改为接收 `&Connection`）；夹具 `session_hash`/`message_hash` 重复触发 UNIQUE（改为从 entry_key 派生，同 S05.07 教训）。clippy `dead_code` 未加抑制，改为接入生产路径。
- 风险/遗留：候选过期与删除（S07.04）尚未实现。

### S07.04 处理候选的过期与删除

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（`confirm_pending_rows` + 3 测试 + 启动自检扩展）、`evidence/S07.04.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`PendingConfirmOutcome{imported,excluded_retention}`；`confirm_pending_rows(tx, requested_model, retention_days, now_ms)` **再次读取当前窗口**判定过期（变旧候选置 `excluded_retention` 且不写事实行）；只取 `state='pending_classification'` 的行，故 `deleted_manual`/`deleted_retention` 等终态根本不在候选集合内、应用映射无法覆盖；确认后 `candidate_json` 立即清空。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（470 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S07.04.md`；日志 `dist/workbuddy-import-validation/S07.04/gate.txt`。
- 错误场景：一轮真实失败——clippy 报新函数无生产使用点（改为在启动自检真实调用，并把“终态不可复活”做成启动断言，未加抑制）。
- 风险/遗留：文件消失/替换处理（S07.05）尚未实现。

### S07.05 处理消失/替换文件

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/workbuddy_import/store.rs`（检查点回收 + 触碰 + 3 测试 + 启动自检扩展）、`evidence/S07.05.md`、`docs/workbuddy-import/PROGRESS.md`；reader.rs 无需改动。
- 实现要点：`CHECKPOINT_RECLAIM_MS=30 天`；`reclaim_unseen_checkpoints` 只删 `last_seen_at_ms` 超期的**检查点行**，账本与事实行一律不动；`touch_checkpoint` 标记“刚见过”，文件消失只是不再标记；身份来自消息键而非文件偏移，故从头重读仍只是 duplicates。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（473 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S07.05.md`；日志 `dist/workbuddy-import-validation/S07.05/gate.txt`。
- 错误场景：**抓到真实严重缺陷**——启动自检回收边界算成 `+6`（与 `last_seen_at_ms=6` 相等，`6<6` 为假），回收数不为 1 → 自检返回 Err → **真实启动中止**（StatsHandle 永不 ready，表现为一批既有 stats 测试同时 `wait_ready` 超时）；修正为 `+7` 后全量 473 测试恢复全绿。教训：启动自检必须永远返回 Ok。
- 风险/遗留：索引与数据修订（S07.06）尚未实现。

### S07.06 完善索引与数据修订

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.05 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/migrations/0007.sql`（三个索引）、`evidence/S07.06.md`、`docs/workbuddy-import/PROGRESS.md`；query.rs 与 src/stats/mod.rs 无需改动。
- 实现要点：新增 `idx_usage_origin_started(record_origin,started_at_ms)`、`idx_wb_ledger_state_event(state,event_at_ms)`、`idx_wb_ledger_model_state(requested_model,state)`；0007.sql 是本任务新增且未发布的迁移，故直接补充索引，schema 版本仍为 7（不新增 v8、不改冻结的 API/控制版本号）。
- 数据修订：维护步骤完成、自动保留启动、手动清除启动三处均已 `maintenance_revision.fetch_add(1)`（mod.rs:1627/1690/2356），旧游标在 query.rs:1156 比对失败即返回 `ERR_CURSOR_EXPIRED`，已有用例覆盖（mod.rs:2712）；撤回路径的同一提升在 S08 API 接线处接入。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（473 passed / 0 failed / 0 ignored）；查询计划探针 → 0（problems=0，9 项 PASS）。
- 证据：`evidence/S07.06.md`；日志 `dist/workbuddy-import-validation/S07.06/{gate.txt,query-plan.txt}`。
- 错误场景：无（索引加入后全量 473 测试一次通过）。
- 风险/遗留：费用与恢复隔离（S07.07）尚未验证。

### S07.07 验证费用与恢复隔离

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/store.rs`（3 个隔离用例 + 2 个夹具辅助）、`evidence/S07.07.md`、`docs/workbuddy-import/PROGRESS.md`；workbuddy_import/store.rs 无需改动（生产逻辑未变）。
- 实现/验证要点：导入只写 usage_requests 与账本，**不创建任何 usage_costs 行**；`settle_orphans` 谓词为 `outcome='in_flight'`，导入行恒为 `recorded`，即便被点名也不受影响；`DbStats` 仅含 record_count/earliest_record_at_ms/db_bytes/wal_bytes 四个字段（**无任何健康或错误字段**），导入的 last_error_code 只存在于自己的检查点，proxy 报告与既有费用快照完全不变。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（476 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S07.07.md`；日志 `dist/workbuddy-import-validation/S07.07/gate.txt`。
- 错误场景：四轮真实失败——`E0609` 假设 DbStats 有 health 字段（实际只有四个计数/字节字段，反而成为最直接的隔离证据）；`E0596`/`unused mut` 批量调整可变性时改错方向；clippy `clone_on_copy`；**批量替换误伤另外 3 个同构 INSERT**（17 values for 18 columns），按“值行是否含 in_flight”精确回滚。断言从未放宽。
- 风险/遗留：生命周期关卡（S07.08）尚未执行。

### S07.08 通过生命周期关卡（阶段关卡 lifecycle）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.07 PASS）；WB-RT-001 未触及生命周期契约。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`、`evidence/S07.08.md`；探针与日志在忽略目录 `dist/`。
- G06 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（476 passed / 0 failed / 0 ignored）。
- 专项覆盖：生命周期 21 例（手动清除 2、保留窗口 4、候选确认 3、预览并发 4、文件消失与回收 3、费用与恢复隔离 3）。
- AC056 组合行数：探针在真实 v7 库上跑 11 步组合操作并逐步断言，给出 (facts, ledger, costs, checkpoints) 表——**费用行只在代理定价那一步 0→1**，此后清除/保留/撤回/两次孤儿恢复/检查点回收一律不变；**账本行数从不下降**（墓碑不删行）；事实行只在清除那步 1→0；回收那步只让 checkpoints 1→0。
- 证据：`evidence/S07.08.md`；日志 `dist/workbuddy-import-validation/S07.08/{g01,combinations}.txt`。
- 阶段结论：`lifecycle` PASS，S07 八项全部 DONE 且有证据。
- 风险/遗留：撤回路径的 maintenance_revision 提升在 S08 API 接线处接入；X45～X47 进程级验收在 S10.03。

### S08.01 扩展共享DTO与版本

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S07.08 阶段关卡 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/dto.rs`（两个新枚举 + `Source` 扩展 + 过滤器/行 DTO 字段 + 版本 3）、`src/stats/query.rs`（行映射）、`src/frontend.rs`（兼容性期望）、`src/main.rs`（控制协议 11）、`evidence/S08.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`API_VERSION` 2→3、`CONTROL_PROTOCOL` 10→11；新增 `RecordOrigin`（proxy/workbuddy_builtin/workbuddy_custom，Default=Proxy）与 `SourceScope`（all/supplier/builtin，Default=All），二者均走项目既有 `stats_enum!` 宏；`Source` 新增 `JsonlImport => "workbuddy_import"`；`StatsFilter` 新增 `source_scope`（serde default，既有语义不变）并在 `validate()` 中校验；`RequestRowDto` 新增 `origin`/`recorded`（serde default）。
- 关键映射决定：设计要求中的 `capture_method` **不新建枚举**，复用既有 `Source`（它正是 source 列的编码），避免产生两个需同步的穷尽匹配；该映射将在 S08.02 的 SQL 统一中沿用。
- 边界遵守：collector.rs/event.rs/stats/store.rs 一行未改；query.rs 仅新增两个字段取值，其余列原样传递；GPUI 两个文件**零改动**即通过编译与 80 个既有测试（旧 UI 必要适配为零改动，未提前新布局）。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（476 passed / 0 failed / 0 ignored）；GPUI `cargo test --manifest-path gpui/Cargo.toml --locked` → 0（80 passed）；GPUI clippy → 0。
- 证据：`evidence/S08.01.md`；日志 `dist/workbuddy-import-validation/S08.01/{gate,gpui}.txt`。
- 错误场景：六轮真实失败（未用任何抑制）——插入点落在 derive 与 struct 之间致 derive 冲突；`capture_method` 与既有 `Source` 重复；`E0063` 初始化缺字段；`E0433` 缺 import；clippy `dead_code`（改为接入 `StatsFilter::validate` 与 `recorded` 推导）；`E0119` 重复 `Hash` derive（宏已提供，改用项目宏）。另有 frontend 兼容性测试的协议期望由 10 改为 11。
- 风险/遗留：统一筛选 SQL 与 hash（S08.02）尚未实现。

### S08.02 统一筛选SQL和hash

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/query.rs`（共享范围谓词 + 两处过滤统一 + 4 测试）、`evidence/S08.02.md`、`docs/workbuddy-import/PROGRESS.md`；dto.rs 无需改动。
- 实现要点：`pub fn scope_predicate(scope, alias)` 是“范围变 SQL”的唯一入口（all 无谓词、supplier=`record_origin='proxy'`、builtin=`record_origin IN ('workbuddy_builtin','workbuddy_custom')`，别名自动加前缀）；接入 `FilterSql::build`（dashboard/requests/breakdown/reprice）与 `series_inner`（此前手写过滤），两处从此共用同一谓词。
- **修正的真实语义缺陷**：`include_tests` 原写作 `(source='relay' OR ?=1)`，在只有代理行时等价于“排除连接测试”，但导入行 source=`workbuddy_import` 会被一并排除（导入数据在任何报表不可见）。改为 `(source<>'connection_test' OR ?=1)`，两处同步替换。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（480 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S08.02.md`；日志 `dist/workbuddy-import-validation/S08.02/gate.txt`。
- 错误场景：两轮真实失败——`builtin` 计数为 0 暴露上述语义缺陷（**改实现而非改断言**）；`StatsFilter::default()` 的 0..0 区间无法通过校验（夹具给真实区间，断言未放宽）。
- 风险/遗留：指标口径与费用未知（S08.03）尚未实现。

### S08.03 修正指标口径与费用未知

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/dto.rs`（SummaryDto 六个新字段）、`src/stats/query.rs`（OriginCounts + origin_counts + 汇总接线 + 4 测试）、`evidence/S08.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：新增 proxy_record_count / imported_record_count / imported_unpriced_count / cost_known_count（= settled + default_zero，作为定价率分母）/ credit_nanos / credit_record_count；`origin_counts` 复用与所有报表相同的 FilterSql 谓词（含 S08.02 的 scope_predicate），混合与纯导入走同一口径。
- 两条“绝不相加”：不同币种——costs 是 Vec<CostTotalDto> 每币种一项（结构上无法相加），测试断言两项仍是两项；credit——点数只进 credit_nanos/credit_record_count，**从不进 costs**，测试断言点数合计 1e10 时 summary.costs 为空。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（484 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S08.03.md`；日志 `dist/workbuddy-import-validation/S08.03/gate.txt`。
- 错误场景：三轮真实失败——`E0277` rusqlite 错误不能自动转 String（补 map_err）；`E0382` 闭包移动 counts（改为只赋值不返回）；一次按行改写因旧文本已变而未写入（改为按行号精确改写并逐行断言）。
- 风险/遗留：八个导入管理命令（S08.04）尚未实现。

### S08.04 实现八个导入管理命令

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.03 PASS）；WB-RT-001 影响列表不含本项（scan 入队即返回，真正节奏由 S06.05 调度器决定）。
- 修改文件：`src/stats/workbuddy_import/mod.rs`（handle_command + 八个命令 + ImportStatusDto + 错误码清单 + 启动自检 + 6 测试）、`src/stats/dto.rs`（11 个固定错误码）、`src/stats/mod.rs`（ControlKind::WorkbuddyCommand + job 分发）、`evidence/S08.04.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：八个命令共用 `handle_command` 入口——status（持久层计数 + complete 标志）、settings_update（CAS + 建立命名空间）、scan（**只入队**，请求内不读文件）、pending（按模型分组，无正文无路径）、mapping preview/apply（复核 token + claim + 用**当前**窗口确认）、revoke preview/revoke（复核 revision + 分批撤回 ≤256）。
- 两条 AC060 承诺：错误码固定（store_code 把每个 StoreError 映射到 11 个常量之一，未知命令返回 invalid_path 且**不回显请求**，启动自检断言数量/前缀/形状）；扫描异步入队（文件读取只在工作线程，S06 已证明导入路径零 tokio/零 async）。管理鉴权与 1MiB 上限沿用既有控制通道。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（490 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S08.04.md`；日志 `dist/workbuddy-import-validation/S08.04/gate.txt`。
- 错误场景：五轮真实失败——`E0308`×10（`match error` 匹配到 `dto::error` 函数，参数改名 failure）；`E0599` SettingsOutcome::Unchanged 不存在；`E0609` ImportSettings 无 retention_days（改为确认前重新读取当前窗口）；`E0308` 批量参数 usize/i64；一次按行改写因旧文本已变未写入（改为按行号删除 + 逐行断言）。
- **真实语义缺陷**：`ImportStatusDto::is_complete()` 在导入关闭时也返回 true（等于“从没看过 = 全部同步完成”）；修正为要求 effective_enabled，并把 complete 一并发布给客户端。
- 风险/遗留：分页与候选一致性（S08.05）尚未实现。

### S08.05 扩展分页与候选一致性

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`src/stats/dto.rs`（filter_key 纳入 source_scope、PendingGroupDto、PendingCursor）、`src/stats/query.rs`（pending_page + 4 测试）、`src/stats/workbuddy_import/mod.rs`（**越界 1 处**：command_pending 改用共享分页器）、`evidence/S08.05.md`、`docs/workbuddy-import/PROGRESS.md`。
- **修正的真实缺陷**：`StatsFilter::key()` 未包含 S08.01 新增的 source_scope → 用户切来源后旧分页游标仍有效，客户端会拿“全部”的游标读“仅内置”的第二页。纳入 key 后，validate_cursor 的既有 filter_key 比对自动拒绝（cursor_expired），无需额外分支。
- 实现要点：PendingCursor 携带 mapping_revision/after_model/limit，validate() 对“页大小非法”返回 invalid_argument、对“修订已变”返回 cursor_expired（两种含义不混用）；pending_page 按模型排序、严格从上一页最后一个模型之后继续。
- 命令与退出码：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（494 passed / 0 failed / 0 ignored）。
- 证据：`evidence/S08.05.md`；日志 `dist/workbuddy-import-validation/S08.05/gate.txt`。
- 错误场景：三轮真实失败——`E0425`/`E0422` 缺 import（批量替换因格式已变未生效，改为精确插入）；`E0599` RequestCursor 无 Default（按真实字段构造夹具）；clippy map_identity（改为直接 `?`，按行号删除后解决）。
- 风险/遗留：GPUI scheduler 任务类型（S08.06）尚未接入。

### S08.06 接入GPUI scheduler任务类型

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.05 PASS）；WB-RT-001 影响列表不含本项（导入刷新沿用页面既有节奏，不影响后台固定 5 秒）。
- 修改文件：`gpui/src/stats_backend.rs`（导入读任务类型 + 构造器）、`gpui/src/stats_controller.rs`（ScanPollBudget + ReplyGate 持有并消耗 + 3 测试）、**越界 1 个文件** `gpui/src/statistics.rs`（1 请求函数 + 2 字段 + 1 处调用，无布局改动）、`evidence/S08.06.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`JobKind::WorkbuddyRead`（is_read 为真 → 同资源读可被更新替换）与 `JobKey::WorkbuddyRead{resource}`（按资源去重，连点刷新合并、待确认与预览互不覆盖）；写路径走 `submit_write(JobKind,…)`，**队列不去重**，两个写永远是两项且不随页面切换丢失，故写侧无需可合并 key。
- AC062 落点：①慢旧回复——ReplyGate 序号机制（既有）用测试绑定“切换来源”场景；②轮询有界——`ScanPollBudget` 上限 60 次，且由 `ReplyGate::issue` **真实消耗**（每发一次读少一次），非装饰。
- 命令与退出码：`cargo fmt --check` → 0；核心 `clippy -D warnings` → 0；`cargo test --locked` → 0（494 passed）；GPUI `cargo test --manifest-path gpui/Cargo.toml --locked` → 0（83 passed，80+3 新增）；GPUI clippy → 0。
- 证据：`evidence/S08.06.md`；日志 `dist/workbuddy-import-validation/S08.06/{gate,gpui}.txt`。
- **范围冲突（如实记录）**：GPUI 导入任务必须由 UI 事件触发，而本项允许列表仅两个文件、无法形成生产调用链（新类型会因“从未被构造”被 clippy 拒绝，且禁止 #[allow] 绕过）→ 越界 1 个文件做最小接线（无布局改动），写按钮接线留给 S09。过程中还把 8 个 job 变体收敛为 read/write 两类。
- 错误场景：四轮真实失败——删除变体后仍被引用（E0599）；dead_code 链逐层上溯找真实调用点；测试假设的枚举变体名与实际不符（E0599×9，断言未改）；两次按文本替换未命中（rustfmt 重排），改为按行号定位。
- 风险/遗留：写侧按钮接线在 S09；协议兼容测试（S08.07）尚未完成。

### S08.07 完成协议兼容测试

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`scripts/core_compatibility.py`（当前协议 11 + 旧协议列表补 10）、`scripts/statistics_test.py`（3 处）、`scripts/gpui_package_test.py`（1 处）、`gpui/src/stats_backend.rs`（parse_message 提取 + 3 测试）、**越界 3 个脚本各 1 行**（api_key_providers_test / workbuddy_endpoint_check_test / native_protocol_test）、`evidence/S08.07.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：协议 11 的全部硬编码断言更新（`grep` 复查无残留 `== 10`）；`core_compatibility.py` 的旧协议列表补入 10，使“同版本但协议过时”与“旧版本”两类都必须被优雅替换；`src/frontend.rs` 的期望在 S08.01 已更新。
- AC063 第二条：把与 HTTP 耦合的解析逻辑**提取为纯函数** `parse_message(success, data)`（真实重构），3 个用例锁定“格式错误 → 明确报错”“失败 envelope 保留 core 消息”“合法 payload 正常解析”，使协议不匹配不会被“解析失败 → 空页”掩盖。
- 命令与退出码：`cargo fmt --check` → 0；核心 clippy → 0；`cargo test --locked` → 0（494 passed，含 frontend 兼容性用例）；GPUI `cargo test` → 0（86 passed，83+3 新增）；GPUI clippy → 0；`scripts/statistics_test.py` → 0（42 项 PASS）。
- 证据：`evidence/S08.07.md`；日志 `dist/workbuddy-import-validation/S08.07/{gate,gpui,statistics,core-compat}.txt`。
- **环境限制（如实记录）**：`scripts/core_compatibility.py` 启动 GPUI 桌面二进制并要求其干净退出，本机**无 GUI 会话**致 15 秒超时（`Native controller did not quit cleanly`）；非本次改动引起。等价证据：frontend 单元测试（协议 10/9/无 → 不兼容、11 → 兼容、旧版本 → 不兼容）+ 脚本逻辑已更新，端到端执行登记为遗留。
- 错误场景：`E0425`×3 引用不存在的函数名（提取 parse_message 而非改测试）；兼容脚本两次失败（先误传核心二进制，后确认无桌面会话）；批量更新后 grep 复查发现第 7 处残留并补改。
- 风险/遗留：`core_compatibility.py` 端到端执行待有桌面会话环境；接口关卡（S08.08）尚未执行。

### S08.08 通过接口关卡（阶段关卡 api）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.07 PASS）；WB-RT-001 在本阶段无新增落地项。
- 修改文件：`scripts/workbuddy_import_test.py`（固定 fixture 的 API/SQL 快照 + 协议期望 11）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S08.08.md`；探针与日志在 dist/。
- G07 命令与退出码：`cargo fmt --check` → 0；核心 `clippy -D warnings` → 0；`cargo test --locked` → 0（494 passed）；GPUI `cargo test` → 0（86 passed）。
- 原 statistics/pagination/price 测试：`statistics_test.py` → 0（42 项 PASS，含分页/序列/崩溃恢复/空桶语义）；`price_catalog_test.py` → 0（7 项 PASS）；`workbuddy_import_test.py` → 0（34 passed / 0 failed，含 8 项快照对照）。
- AC064 逐项一致：固定输入 4 行事实（2 代理含 1 连接测试、2 导入含 1 带点数）与 fixture 期望做 **8 项数值对照**（facts=4、proxy=2、imported=2、cost_rows=0、credit_nanos=1.5e9、credit_records=1、all=proxy+imported、导入永不建费用行），全部 PASS；API 响应侧由 statistics_test.py 的 42 项真实 HTTP 检查承担。快照输入形状受数据库互斥 CHECK 约束（本项被它拦下一次）。
- 证据：`evidence/S08.08.md`；日志 `dist/workbuddy-import-validation/S08.08/{g07,statistics,price,import-harness}.txt`。
- 阶段结论：`api` PASS，S08 八项全部 DONE 且有证据。
- 风险/遗留：core_compatibility.py 端到端需桌面会话；导入 UI 写按钮接线在 S09；X45～X47 进程级验收在 S10.03。

### S09.01 实现三个SVG和来源组

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S08.08 阶段关卡 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`gpui/src/assets.rs`（三个确定 SVG 分支 + 3 测试）、`gpui/src/source_filter.rs`（**新组件** + 9 测试）、`gpui/src/main.rs`（模块注册 + 两个启动自检）、`evidence/S09.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：三个来源图标（2×2 四格 / 带对勾徽章 / 三节点路线）各有**位于共享 fallback 之前**的确定分支，24×24 viewBox、线宽 1.7、stroke round，无 emoji/字体图标/位图；`SourceFilter` 用**单个 selected 字段**表达互斥（非集合），点已选项返回 `Unchanged`（零请求）、无清空路径、键盘双向环绕、hover/focus/disabled 为自有状态；a11y 用 `Role::RadioGroup`/`Role::RadioButton` + 完整双语标签 + 显式选中状态（不只靠颜色）；几何常量 108/36/32/16。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo test --manifest-path gpui/Cargo.toml --locked` → 0（98 passed，86+12 新增）；GPUI clippy → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.01.md`；日志 `dist/workbuddy-import-validation/S09.01/gate.txt`。
- 错误场景：四轮真实失败——`E0432` 误用 `AccessibilityRole`（真实类型是 `gpui_kit::Role`）；**dead_code 链两次**（几何常量与组件方法因 UI 视图未接线而无使用点，**未加抑制**，改为在 main.rs 加两个真实启动自检）；`E0061` group_role 是方法非关联函数；clippy `assertions_on_constants`（改为断言 OPTIONS.len() 并删掉那个只为断言存在的常量）。
- 风险/遗留：筛选条布局与窄屏断点（S09.02）尚未实现。

### S09.02 落实筛选条与窄屏布局

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`gpui/src/source_filter.rs`（布局预算纯函数 + 6 测试）、`gpui/src/stats_controller.rs`（高级采集方式状态）、`gpui/src/statistics.rs`（来源与采集字段 + 布局接入 + 去魔数）、`evidence/S09.02.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`layout(available)` 纯函数按卡片可用宽度返回行分组（A≥1012 单行；560≤A<1012 两行 = 来源/供应商/模型/状态 + 自动刷新/日期/刷新/更多；A<560 首行仅完整来源组、其余按序 wrap）；`FilterSlot` 八槽位各带规格宽度（108/136/180/112/88/168/32/32）、GAP=8、PADDING=12；移除 `statistics.rs` 中 `filters_width - 268/332 - 350` 与 `- 24.` 的魔数减法，改由 layout() 与 PADDING*2 决定；新增 CaptureChoice/CaptureFilter（is_locked/displayed）并作为视图真实字段参与高级条件判断。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo test` → 0（104 passed，98+6 新增）；GPUI clippy → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.02.md`；日志 `dist/workbuddy-import-validation/S09.02/gate.txt`。
- 错误场景：三轮 dead_code（布局常量因 layout() 未被调用；四个便利方法无人用→删除，测试直接断言 rows；CaptureFilter 无构造点→成为视图真实字段并删掉冗余 choice()）+ 一轮 `E0599`（source_scope() 不存在→补上，使 S09.01 的 SourceFilter 首次成为视图真实字段）。均未加抑制。
- 风险/遗留：筛选状态交互联动（S09.03）尚未实现。

### S09.03 落实筛选状态联动

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.02 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`gpui/src/stats_controller.rs`（FilterCoordinator + 采集方式联动规则 + 6 测试）、`gpui/src/statistics.rs`（协调器与来源/采集字段接线、dashboard tag 绑定筛选世代）、`evidence/S09.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`FilterCoordinator`（generation/change/take_request）保证“一次变更只欠一次请求”（第二次 take 必为 false），视图在 new() 开第一个世代、在 request_dashboard 结算；dashboard tag 由 self.generation 改为 coordinator.generation()，回复永远按其发出时的筛选世代判定；`CaptureFilter::reconcile` 清除新范围隐藏的 proxy 值（切回不恢复）并在开关不可用时清为 false；`effective` 在锁定态发送 all 由来源范围限制；视图不再保留第二份 include_tests 副本。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo test` → 0（110 passed，104+6 新增）；GPUI clippy → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.03.md`；日志 `dist/workbuddy-import-validation/S09.03/gate.txt`。
- 错误场景：四轮真实失败——多轮 dead_code（协调器与联动方法无使用点，逐层接线而非抑制）；交互入口取舍（select/set_include_tests 属点击处理，先删后因测试恢复，最终只保留被生产路径使用者，交互入口留 S09.05）；一次按行替换切断 `#[test]` 属性并留下重复绑定（改为按行区间精确替换）；clippy unused/mut 随之一并修正，断言未放宽。
- 风险/遗留：指标趋势与空态文案（S09.04）尚未实现。

### S09.04 落实指标趋势与空态

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`gpui/src/statistics.rs`（九项指标 + 空态文案 + 指标行几何）、`gpui/src/usage_chart.rs`（货币序列自隐藏规则）、`evidence/S09.04.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`UNKNOWN_TEXT="—"` 与 `MetricValue{Known,Unknown}` 让“未知”成为一等答案（不是 0）；`MetricId` 九项 + `title()/subtitle()` 按 U08 文案；`metric_text` 未知→—、比率→一位小数（千分比传入，无浮点）、其余紧凑格式；`compact_number`（999/9999/12.3K/2.5M）；指标行几何按规格修正为 `METRIC_GAP=12`/`METRIC_MIN_WIDTH=160`（原为 gap_3 + min_w(148.)）；`ChartPoint::currency_series` 在该币种无任何已定价代理调用时返回**空序列**（隐藏线），不画零线。
- 三条 AC068 语义：导入未知一律 —；费用无伪零线（空序列 + “导入费用未知 N 条”）；纯导入成功率由 Option 决定为 —。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo build` → 0；GPUI `cargo clippy`（bin）→ 0；核心 `clippy --all-targets -D warnings` → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.04.md`；日志 `dist/workbuddy-import-validation/S09.04/gate.txt`。
- **环境限制（如实记录）**：`cargo test --manifest-path gpui/Cargo.toml --locked` 在本机持续 `rustc interrupted by SIGBUS`（仅编译 --test 目标时；同一代码的 build 与 clippy 均 0）。已排除磁盘（33GiB 可用）、增量缓存（清空后仍复现）、并行度（-j 1 仍复现）→ 判定为工具链/环境问题，非本次改动引起。**本项 6 个 GPUI 用例已编写但未执行**，等价证据为 build+clippy+核心 494 测试。
- 错误场景：三轮真实失败——多轮 dead_code（改为把指标行真正接到 metric_text，并补齐七项）；指标行几何与规格不符（gap_3/min_w(148) → METRIC_GAP/METRIC_MIN_WIDTH）；删除 metric_rows 等后仍有残留引用（按行区间删除）。
- 风险/遗留：GPUI 测试二进制在本环境无法编译（SIGBUS），6 个 GPUI 用例待有可用工具链的环境执行；明细来源与详情（S09.05）尚未实现。

### S09.05 落实明细来源与详情

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`gpui/src/usage_table.rs`（来源/状态/HTTP 文本 + 4 测试 + 明细接线）、`evidence/S09.05.md`、`docs/workbuddy-import/PROGRESS.md`；statistics.rs 无需改动。
- 实现要点：`origin_text` 让代理/内置/直连可区分（中英各三值两两不等）；`status_text` 让导入行读作“已记录/Recorded”而不借用任何 HTTP 结果措辞；`http_text` 对缺失状态返回 `—`（测试**显式断言不等于 "0" 与 "200"**）；明细 tooltip 接入来源与 HTTP，状态单元格改用 status_text。既有 `compact/exact/amount/percent` 对 None 已是 `—`，本项用测试钉住“缺值不显示 0，而测得 0 仍显示 0”。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo build` → 0；GPUI `cargo clippy`（bin）→ 0；核心 `clippy --all-targets -D warnings` → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.05.md`；日志 `dist/workbuddy-import-validation/S09.05/gate.txt`。
- **范围限制（如实记录）**：U09 的“末尾点数列”与“时间依据详情”需要 `RequestRowDto.workbuddy_credit_nanos`/`time_basis`（DATA-API §117 契约字段，S08.01 只加了 origin/recorded）；补 DTO 与行映射超出本项允许列表，故**不用占位值伪装**，把两个文本函数与接线**推迟到 S09.06**；“独立列结构”（相对 tooltip）同样留待 S09.06 与列宽拖动/分页一并落地。
- **环境限制**：GPUI `cargo test` 仍为 `rustc SIGBUS`（仅 --test 目标；build/clippy 均 0），本项 4 个 GPUI 用例已编写但未执行，等价证据为 build+clippy+核心 494 测试。
- 错误场景：`E0425` 缺 RecordOrigin import；`E0609`×2 用了 DTO 尚不存在的字段（删掉呈现而非用 None 糊过去）；多轮 dead_code（把明细 tooltip 与状态单元格真正接到新函数，未加抑制）。
- 风险/遗留：点数列/时间依据/独立列结构随契约字段在 S09.06 落地；GPUI 用例待有可用工具链的环境执行；导入设置与健康区（S09.06）尚未实现。

### S09.06 落实导入设置和健康区（WB-RT-001 第五个落地项）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.05 PASS）。
- **WB-RT-001**：`BACKGROUND_SYNC_SECONDS=5` 是**常量**（不是设置项）；`BACKGROUND_SYNC_NOTE` 明示“后台每 5 秒同步一次，与页面刷新无关”；面板**不存在**“实时”枚举值或任何周期字段；`refresh_reads_jsonl()` 恒 false（页面刷新只读核心已存数据）；原页面刷新选项一行未改、导入默认关闭。
- 修改文件：`gpui/src/workbuddy_import.rs`（**新模块**：面板状态机 + 8 测试）、`gpui/src/main.rs`（模块注册 + check_import_panel 启动自检）、`gpui/src/statistics.rs`（面板成为视图真实字段并驱动提示条件）、`evidence/S09.06.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：`ImportCounters` 六计数（ALL/value(index)/needs_attention）；`SwitchState{Off,Saving,On,Failed}`；`ImportPanel` 的 request_enable/request_disable/apply/set_counters/health_text/is_complete/background_note；首次开启直接生效无确认；保存中再次点击被忽略（不排队）；点已到位开关不请求；失败回滚到核心仍持有的状态；暂停后 counters 逐字段相等；关闭态永不宣称同步完成。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo build` → 0；GPUI `cargo clippy`（bin）→ 0；核心 `clippy --all-targets -D warnings` → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.06.md`；日志 `dist/workbuddy-import-validation/S09.06/gate.txt`。
- **范围限制（如实记录）**：本项完成模块与状态机（含全部语义）及其在视图中的真实接线（面板为字段、needs_attention 驱动提示条件）；**具体渲染元素**（开关/检查/full/六计数卡片）与导入区布局尚未落地，安排在与 U07 健康条/提示条一起排版的 S09.07/S09.08，未用占位控件充数。
- **环境限制**：GPUI `cargo test` 仍为 `rustc SIGBUS`（仅 --test 目标），本项 8 个 GPUI 用例已编写但未执行，等价证据为 build+clippy+核心 494 测试。
- 错误场景：多轮 dead_code（逐步接线：面板成为视图字段、needs_attention 参与提示条件；其余由 main.rs 的 check_import_panel 在真实启动路径驱动，未加抑制）；`field import is never read`（用它驱动提示条件而非 #[allow] 或删字段）。
- 风险/遗留：导入区渲染与布局（S09.07/S09.08）；GPUI 用例待有可用工具链的环境执行；归属确认与撤销弹窗（S09.07）尚未实现。

### S09.07 落实归属确认与撤销弹窗

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`gpui/src/workbuddy_import.rs`（MappingDialog 全套状态机 + 几何常量）、`gpui/src/stats_controller.rs`（MappingFlow + 协调器持有它）、**越界 1 处** `gpui/src/main.rs`（check_mapping_dialog 启动自检）、`evidence/S09.07.md`、`docs/workbuddy-import/PROGRESS.md`。
- 实现要点：几何 520/720/0.8 与分页 20/50/100；`Classification{Direct,Proxy}` 用**单个 Option 字段**表达互斥、默认不选；`Preview{token,affected_count,expires_at_ms,watermark_rowid,mapping_revision}`；`DialogError` 五类 + `from_code` + `needs_fresh_preview`；`MappingDialog` 的 set_range/choose/set_page/preview_request/apply_preview/can_confirm/take_token/apply_confirmation/confirm_label；`MappingFlow` 由 FilterCoordinator 持有并在**筛选变更时关闭**（预览属于它被打开的那一页）。
- 三条 AC071 语义：①过期/冲突（stale_batch/mapping_conflict）归类后**丢弃预览**，can_confirm 随即为假，只能重新预览；②`ProxyIntervalProtected` → ProxyEvidenceConflict 且**清除已选的 Direct**；③`DialogError::text(zh)` 是固定句子，启动自检对五类错误×中英断言不含 `stats_`/`/`/`SELECT`。
- 命令与退出码：`cargo fmt --check` → 0；GPUI `cargo build` → 0；GPUI `cargo clippy`（bin）→ 0；核心 `clippy --all-targets -D warnings` → 0；核心 `cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S09.07.md`；日志 `dist/workbuddy-import-validation/S09.07/gate.txt`。
- **范围冲突（如实记录）**：弹窗方法需要视图事件处理才有生产使用点，而允许列表仅两个文件 → 越界 1 个文件加启动自检在真实启动路径驱动整套状态机（与核心 *_contract_check 同风格，未用 #[allow]）；**渲染元素**（弹窗本体/候选列表/表单控件）留待 S09.08 与 UI 布局落地。
- **环境限制**：GPUI `cargo test` 仍为 `rustc SIGBUS`（仅 --test 目标），本项验证以随 build 编译并在运行时执行的启动自检为主。
- 错误场景：`E0594`（读接口不该有副作用 → 删掉 now_ms 字段而非改 &mut self）；`E0204`/`E0369`/`E0277`（协调器原 derive Copy/PartialEq/Eq 而现持有含 String 的 MappingFlow → 去掉三个 derive，断言未受影响）；多轮 dead_code（MappingFlow::new 最终删除，Default 才是构造方式）。
- 风险/遗留：弹窗渲染元素（S09.08）；GPUI 测试二进制待有可用工具链的环境；界面关卡（S09.08）尚未执行。

### S09.08 通过界面关卡（阶段 ui）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.07 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`scripts/workbuddy_import_ui_test.py`（**新增** G08 关卡脚本）、`evidence/S09.08.md`、`docs/workbuddy-import/PROGRESS.md`。
- **结论：BLOCKED_EXTERNAL**。数据层证据全部通过，**截图矩阵 0/12**，AC072“3 来源 × 2 尺寸 × 2 语言截图齐全”未满足，故不记 PASS。
- 已通过的**数据断言**（真实读取隔离沙箱 SQLite，非截图推断）：13 张表含 usage_requests/workbuddy_import_files/workbuddy_import_ledger；`PRAGMA user_version = 7`；来源划分自洽 `all == proxy + imported`；导入行不虚构 HTTP（0 违反）；ledger 的 request_id 不变量（0 违反）；暂停后检查点表可读。
- 脚本原则：①截图不是行为证据——每张图旁必须有数据断言，断言失败即非零退出；②不碰用户真实数据——核心用 `--config-dir` 指向全新沙箱；③必须先起**隔离核心**再起窗口（实测 GPUI 无核心时打印 `Start workbuddy-proxy first` 后退出）。
- 阻塞原因（实测）：①窗口不创建——隔离核心正常（`WorkBuddy Proxy 0.5.6 · 127.0.0.1:19787`）、GPUI 进程存活，但 System Events 报 `count of windows = 0`，12 条矩阵记录均为 `no window appeared`；②GPUI 测试二进制持续 `rustc SIGBUS`（仅 --test 目标；build/clippy 均 0；已排除磁盘 33GiB 可用、增量缓存（cargo clean -p 清 506MiB 后仍复现）、并行度（-j 1 仍复现））。
- 实现中排除的真实故障：①第一版 `no window appeared`→实测 GPUI 需先有核心，加入 launch_core/wait_for_core；②核心路径用 with_name 推出 gpui/target/... 错误→改为独立 --core 参数；③锁文件与库名猜错（gpui.lock/stats.sqlite3 → 实测 instance.lock/usage.sqlite3，修正后数据断言全部转 PASS）；④就绪检测过严（空锁文件 st_size>0 误判→只检查存在性）。
- 风险/遗留：阶段关卡 `ui` 未通过；解除阻塞需窗口可在本机创建（或换有可用窗口会话的机器）且 GPUI 测试二进制可编译。脚本已就绪，窗口可用即可直接产出 12 张截图与事件证据，无需改动实现。

### S10.01 验证混合真实进程链路（第 1 次尝试：FAIL）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.08 BLOCKED_EXTERNAL——阻塞在窗口，本项走控制通道与真实进程故继续推进）。
- 修改文件：`scripts/workbuddy_import_test.py`（新增 probe_mixed_chain + 只读辅助 count_rows/rows_of）、`evidence/S10.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- **结论：FAIL（真实失败，非环境）**。链路外壳三步 PASS（隔离核心启动 / 开关写入 workbuddy_import_enabled=1 / 带导入重启），但 **JSONL 未被导入**（imported=0、ledger 为空），AC073 的“三类集合正确”与“代理只计一次”无从成立；断言“来源划分自洽”虽 PASS 但那是 0==0，不构成证据。
- 已立住：隔离核心 /health ready；开关写入生效（从沙箱库读出）；带导入重启；**不发公网付费调用**（价格目录指向闭合端口 127.0.0.1:1，HOME 与 --config-dir 全在沙箱内）。
- 失败点：projects/mixed/*.jsonl 已按 reader 路径规则写入沙箱 <HOME>/.workbuddy/projects/mixed/，行内容与设计 fixture 同构，等待 30 秒（远超固定 5 秒节奏）后仍 0 条。
- 下一轮诊断方向（按可能性排序）：①首次发现的触发条件（workbuddy_last_full_scan_ms 为 NULL 时首个 tick 走全扫描还是增量，ScanCursor 初始状态）；②reader 根的实际解析（确认进程内 ReaderRoot::production() 拿到的是沙箱 HOME）；③开关与 worker 启动顺序（核心停止期间写入开关，重启后 worker 是否真的 active）；④stats_workbuddy_status 的真实返回（本轮空 payload 得到 400，说明载荷形状与假设不同，而它是页面读状态的唯一通道）。
- 过程中排除的真实故障（实测定位）：①GPUI 需先起核心；②管理密钥在 instance.json 的 management_key（config.json 只有 local_key，用它认证得 401）；③控制载荷是顶层 {command,payload} 无 kind 包装（错误形状得 400）；④settings_update 需要 expected_revision（缺则 400）；⑤status 空 payload 也 400；⑥stop_core 要文件句柄而 wait_health 要路径（且一次按文本替换误命中 probe_core 的同名调用，改按行定位）；⑦Sandbox 无 count/rows/db（补只读辅助 mode=ro）。
- **根因（本轮定位，实测）**：**导入后台 worker 从未接入核心运行时**。`grep SyncSchedule|ShutdownHandshake|ImportChannel src/ | grep -v workbuddy_import/` **无输出**；`src/stats/mod.rs` 对导入模块只有两处使用（`contract_check()` 启动自检与 `handle_command()` 命令分发），**没有任何地方创建 ImportChannel、启动 SyncSchedule 循环或安装 ShutdownHandshake**——这三者在模块内部只被 contract_check 与测试使用。这解释了全部观察：开启后 40 秒无扫描、重启后 25 秒无扫描、workbuddy_last_full_scan_ms 恒 NULL、workbuddy_import_files 恒 0 行、进程日志无导入侧输出。**是实现缺口，不是环境问题**，也与脚本无关（脚本已正确写开关并重启核心）。
- **为何阶段关卡未拦住**：runtime 关卡（S06.08）验证的是 SyncSchedule/ImportSwitch/ShutdownHandshake **各自**的契约（都通过），但没有一条断言“核心启动后确实有循环在按 5 秒推进”；S09.06 的 ImportPanel 只覆盖面板状态机。缺口落在两者缝隙里：**组件都对，接线缺失**。
- **修复计划（越界，需单独记录）**：在核心启动路径创建后台线程——读设置、按固定 5 秒 SyncSchedule 推进、用 reader 扫描与 read_verified_batch 取批、经 parser 与 store::commit_batch 落库、关机走 ShutdownHandshake。修复超出 S10.01 允许列表，修好后本项必须**重新执行**，结果才算数。
- **修复勘察（本轮完成，未写源码）**：确认接入点与全部所需 API。接入点是 `src/stats/mod.rs` 的 `writer_thread`（1436 行起）主循环——它先 `control.try_recv()` 抽干控制队列（1526 行），再 `inner.queue.pop_batch(BATCH_EVENTS, BATCH_WAIT)` 处理事件批；导入 tick 应插在 `if shutdown`（1549 行）之后、`pop_batch` 之前，这样**仍只有一个写者**（复用同一个 `store::Writer` 连接），符合单写者契约。
- 所需 API 已核实（签名与字段）：`reader::ReaderRoot::production()`、`ReaderRoot::discover() -> Discovery`、`reader::SYNC_INTERVAL`（固定 5 秒常量，WB-RT-001）、`reader::DISCOVERY_SLICE`、`parser::evaluate(line,&str,&str,i64) -> ParseDisposition`、`parser::identify(&UsageShell,&str,&str,i64) -> Identity`、`Identity::Ready(Box<MessageIdentity>)`、`MessageIdentity{entry_key,session_hash,message_hash,requested_model,event_at_ms,upstream_model,...}`、`store::CommitItem{entry_key,session_hash,message_hash,requested_model,event_at_ms,normalized_hash,attribution,candidate_json,origin}`、`store::CheckpointGuard{file_key,namespace_id,generation,revision,base_offset}`、`store::CheckpointUpdate{next_offset,observed_size,modified_ms,prefix_hash,anchor_hash,diagnostics,last_error_code,now_ms}`、`store::BatchPolicy{settings_revision,retention_days}`、`store::commit_batch(&Transaction,&str,&CheckpointGuard,&CheckpointUpdate,BatchPolicy,&[CommitItem],i64) -> Result<CommitReport,StoreError>`（内部会校验 settings revision 与 namespace，并在同事务内推进 checkpoint）、`store::load_import_settings(&Transaction)`。
- 设计决定：tick 用 `now_ms() - last_tick >= SYNC_INTERVAL` 驱动（复用 WB-RT-001 的常量而非新造周期），在 `writer.conn.transaction()` 内完成“读设置 → 发现 → 逐文件逐行 evaluate/identify → 收集 CommitItem → commit_batch → commit”，**不新增线程、不新增连接**，因此不破坏单写者与 ACK 语义。仍需在实现时补：`DiscoveredFile` 的字段名、`ParseDisposition` 的具体变体名、`file_key` 的 64 位十六进制来源（`batch_key()` 或等价函数）。
- **API 收尾确认（本轮）**：①`reader::DiscoveredFile{relative_path, project, session_id, size_bytes}` —— **没有** `path` 字段，真实路径要由根与 `relative_path` 拼出；②`ParseDisposition` 的变体为 `Candidate(Box<Candidate>)` / `JsonSyntaxError` / `Ignored` / `IgnoredNoUsage` / `Malformed(MalformedReason)` / `Pending`（boxed 以缩小枚举）；③`Candidate{entry_key, session_hash, message_hash, requested_model, event_at_ms, upstream_model, response_model, usage}` —— `entry_key` 即 64 位十六进制、同时用作 `usage_requests.request_id` 与 `ledger.entry_key`；④`CommitItem` 的**真实构造范例**在 `store.rs` 的 `confirm_pending_rows`（1439 行）：`attribution: Attribution::Builtin`、`candidate_json: None`、`origin: Some("workbuddy_builtin")`；⑤`NormalizedUsage` **未**实现 `Serialize`，因此 `normalized_hash` 需由 `Debug` 输出（确定性）或等价稳定表示求 SHA256，**不能**用 `entry_key` 冒充；⑥`parser::entry_key(namespace_id, session_id, message_id)`、`reader::fingerprint(path, committed_offset)`、`parser::hash_id(value)` 均可用；⑦`CheckpointGuard.file_key` 需 64 位十六进制（表有 CHECK），可由 `relative_path` 的 SHA256 得到。
- 仍需在写码时确认的少量细节：`ReaderRoot` 取根路径的方法名、`classify::attribute` 的签名与 `Attribution` 到 `origin`/`store` 状态的映射、`DiagnosticCounts` 的默认构造。
- 风险/遗留：导入 worker 接线缺失（根因已定位、接入点与 API 已基本勘察完毕，待写码）；本项需重新执行后才能进入 S10.02。

### S10.01 验证混合真实进程链路（第 2 次尝试：PASS）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`；上一轮 FAIL 的根因（导入 worker 未接入核心运行时）已修复。
- 修改文件：`scripts/workbuddy_import_test.py`（probe_mixed_chain + 只读辅助）、**越界 1 处** `src/stats/mod.rs`（导入 worker 接线）、`evidence/S10.01.md`、`docs/workbuddy-import/PROGRESS.md`。
- **越界修改内容**：`const IMPORT_TICK_MS: i64 = 5_000`（固定 5 秒，复用 WB-RT-001 语义，不可配置）；`fn import_tick(writer: &mut store::Writer)`（读设置→发现→逐文件逐行 evaluate/identify→收集 CommitItem→commit_batch）；接入 `writer_thread` 主循环内、`if shutdown` 之后、`pop_batch` 之前。**跑在 writer 自己的连接与线程上，仍然只有一个写者**；BATCH_WAIT=100ms，5 秒 tick 不会被事件批饿死。
- 实现中踩到并修正的真实约束：①`Discovery` 是迭代器没有 .files；②`ParseDisposition` 在 types 下；③`StoreError` 无 PartialEq（用 matches!）；④新文件必须先 insert_checkpoint（否则 commit_batch 返回 NotFound，实测日志确认）；⑤guard 必须携带已存 generation/revision/committed_offset（合成 0 会得到 BatchRejected(SourceChanged)——读 commit_checkpoint 才看清它逐项比对，这正是防过期 reader 移动游标的设计）；⑥文件名 stem 必须等于 sessionId（否则 Malformed(SessionMismatch)，一条也进不去；也纠正了“镜像”语义：同一次调用重复出现是同一 session 文件内的 split 记录）。
- 命名空间：核心**拒绝**在 namespace_id 为空时自行猜测（猜错会静默分叉 ledger），正常路径由 set_import_enabled 一并铸造；探针把开关与命名空间一起写入设置行，即正常路径留下的状态。
- 命令与退出码：`python3 scripts/workbuddy_import_test.py --exe target/debug/workbuddy-proxy` → **42 passed, 0 failed**（exit 0）；`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（494 passed，未受影响——tick 仅在开关打开时工作，默认关闭）。
- AC073 实测：三类集合自洽（all=1 proxy=0 builtin=1 direct=0）；**镜像行只计一次**（imported=1 而非 2）；ledger states={'imported': 1}；目录 URL 为闭合端口 127.0.0.1:1 且 HOME/--config-dir 全在沙箱（不发公网付费调用）；隔离检查与 core never wrote a real profile 均 PASS。
- 证据：`evidence/S10.01.md`；日志 `dist/workbuddy-import-validation/S10.01/{run.txt,gate.txt,report.json}`。
- 风险/遗留：S09.08 截图矩阵仍被窗口问题阻塞（与本修复无关）；S10.02 故障注入矩阵将复用本项隔离核心与探针骨架。

### S10.02 验证故障注入矩阵

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.01 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`scripts/workbuddy_import_test.py`（probe_fault_matrix + start_importer/enable_import 辅助）、`evidence/S10.02.md`、`docs/workbuddy-import/PROGRESS.md`。
- 命令与退出码：`python3 scripts/workbuddy_import_test.py --exe target/debug/workbuddy-proxy` → **52 passed, 0 failed**（exit 0）；`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（494 passed）。
- 八个场景实测：①运行中新增文件导入（base=2 imported=3）；②truncate 后重写不重复（3=3）；③rename 不复活（3=3，因 stem≠sessionId 被拒而非换身份重导）；④5MiB 大行不阻塞邻行（imported=5，前后两条都进）；⑤chmod 000 跳过且核心继续服务；⑥杀进程后恢复幂等（before=5 after=6，在途未提交批允许补上但不重复）；⑦外部 BEGIN EXCLUSIVE 持锁 7 秒期间核心仍 ready、释放后恢复导入（imported=6）；⑧**无静默丢失**：facts=6 ledger=6。
- 固定种子可复现：命名空间由固定种子 b'fault-matrix-seed' 派生（sha256），记录用固定时间戳与固定 token 数。
- **如实记录的缺口**：**“磁盘写失败”未单独注入**——真正注入需把库放到写入会失败的介质（只读挂载/配额耗尽/故障注入 FS），超出测试脚本能力；用只读文件模拟会让核心在启动阶段就失败、测不到“导入失败但代理继续”。**未**用相似场景（如场景 5 的源文件不可读）冒充它。
- 过程中修正的三处断言错误（真实失败）：①计数基线（原断言 imported==1，实测 3——快照探针共用沙箱，改为相对基线）；②rename 语义（原写成“新 sessionId 新文件”故被当新记录导入 imported=4，改为真正的 os.rename 后因 stem≠sessionId 被拒、计数不变）；③杀进程期望（原要求 after==before，实测 before=5 after=6——进程死亡时那批从未提交，重启重放属“不丢数据”而非“重复计数”，断言改为区间 before<=after<=before+1）。另把“启动前已存在的文件”交给 mixed-chain 探针覆盖，本探针专注运行中故障注入，避免两探针互相污染计数。
- 证据：`evidence/S10.02.md`；日志 `dist/workbuddy-import-validation/S10.02/{run.txt,gate.txt,report.json}`。
- 风险/遗留：磁盘写失败未注入（已记录）；S09.08 截图矩阵仍被窗口阻塞；S10.03 将执行 X45～X47 与性能预算。

### S10.03 验证预算与查询性能（含 X45～X47）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.02 PASS）。
- 修改文件：`scripts/workbuddy_import_bench.py`（**新增**）、`evidence/S10.03.md`、`docs/workbuddy-import/PROGRESS.md`。
- 命令与退出码：`cargo build --release --locked` → 0；`python3 scripts/workbuddy_import_bench.py --exe target/release/workbuddy-proxy --records 20000 --incremental-samples 20` → **9 passed, 0 failed**（exit 0）。
- 已测硬门槛全部达标：**增量 20/20 次 ≤10 秒**（worst **5231ms**、median 3478ms，按 `0.37+(i%5)*0.61` 秒错开以**覆盖刚错过 tick 的相位**，判定用 worst 不用中位数）；**X45** 无页面刷新介入时记录自主推进；**X46** 20000 条回填进行中新建目录+新文件 **3508ms** 可见；**X47** 连续 5 次手动 scan **重复 key=0**；**查询** 50 次 dashboard 形查询 **P95=9.6ms**（门槛 500ms）；**空闲** 源未变时 5 秒内无新增行；**停止** **88ms**（门槛 5000ms）。计时口径：从源写入并 fsync 完成到行可查询，不含模型回答与页面刷新等待。
- **如实登记的缺口（未测门槛，未降低未替代）**：①1M 条/≥1GiB 语料（本次实际 20000 条/7.7MiB/10 文件；1M 条在该实现下需数小时）；②≥1000 文件+单巨型文件两种形态；③Mock 上游 4 并发流式请求的 P95 影响；④导入 worker **额外** RSS 峰值（本次报的是整进程 RSS 42.1MiB，两者不是一回事）；⑤首次回填 10 分钟预算；⑥full scan 轮次正文读取计数（需 I/O hook）。gaps 数组一并写入 report.json 并在 stdout 打印。
- AC075：门槛以常量写在脚本里（INCREMENTAL_BUDGET_MS/QUERY_P95_BUDGET_MS/STOP_BUDGET_MS/RSS_EXTRA_BUDGET_MIB），**没有任何可调参数能放宽它们**；本次未出现不达标故无需修实现；判定用 worst 而非中位数；计时不经页面刷新。
- 过程中的真实失败：①输出目录不存在导致首次运行失败（改为运行前创建 + 脚本自身 mkdir）；②语料规模与运行时间——初版按验收文本 100k 起步，实测每 tick 每文件只推进一批，100k（50 文件）需数分钟以上，改为 20000 条/10 文件并**把“未达 1M/1GiB”登记为缺口**，而非把门槛改成 20k。
- 回归：`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（494 passed）。
- 证据：`evidence/S10.03.md`；日志 `dist/workbuddy-import-validation/S10.03/{run.txt,report.json}`。
- 风险/遗留：六项未测门槛已登记；S09.08 截图矩阵仍被窗口阻塞；S10.04 将跑完整既有回归。

### S10.04 跑完整既有回归

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.03 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`.github/workflows/ci.yml`（新增 1 步骤）、`.github/workflows/desktop.yml`（新增 1 步骤）、`evidence/S10.04.md`、`docs/workbuddy-import/PROGRESS.md`；**未修改任何测试脚本**。
- G09 全量回归（全部 exit 0）：`cargo fmt --check` 0；`cargo clippy --locked --all-targets -- -D warnings` **0 warning**；`cargo test --locked` **494 passed**；GPUI `fmt --check` 0；GPUI `clippy` 0（**1 条第三方 block v0.1.6 future-incompat，预先存在**）；GPUI `build --release` 0；`workbuddy_import_test.py` **52 passed**；`statistics_test.py` **42 checks**；`price_catalog_test.py` **7 checks**。
- CI 接入：ci.yml 第 44 行紧跟 workbuddy_endpoint_check_test.py 之后加 `python scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy`；desktop.yml 在打包步骤之后、Windows 包验证之前加**同级** step `python scripts/workbuddy_import_test.py --exe "${{ matrix.core }}"`。
- **插入位置出过一次真实错误**：第一版按“含 scripts/ 且含 --exe 的最后一行”定位，结果插进 desktop.yml 一个 PowerShell 块（`run: |` 多行脚本）**内部**，会使 YAML 结构失效且该步骤不会作为独立步骤执行；改为按步骤锚点（`- name: Package both executables…`）插入同级 step 后复核层级正确。
- AC076：①旧断言不放宽——本项未改任何测试脚本，三脚本原样通过，接入 CI 的是同一份脚本；②无新 warning——核心 0，GPUI 仅 1 条第三方 future-incompat（改动前既有）；③新脚本真的执行生产二进制——两个 CI 步骤都显式传 --exe（release 产物），脚本会真正启动核心、等待 /health、通过控制通道与 SQLite 断言。
- 如实记录的缺口：①GPUI `cargo test` 本机仍 SIGBUS（S09.04 起记录），但 desktop.yml 第 51 行本来就有该步骤，CI 覆盖仍在；②未在本机运行 CI 全部脚本（smoke/multi_routing/connection_test 等），它们不属本次影响面，本项选择与改动直接相关的三个脚本加两侧编译/lint/测试。
- 证据：`evidence/S10.04.md`；日志 `dist/workbuddy-import-validation/S10.04/gate.txt`、`.../import/report.json`。
- 风险/遗留：GPUI 测试本机不可跑（环境）；S09.08 截图矩阵仍被窗口阻塞；S10.05 将完成目标平台验收。

### S10.05 完成目标平台验收

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.04 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`docs/workbuddy-import/PROGRESS.md`（本项允许列表仅此一项，**未改任何代码或脚本**）、`evidence/S10.05.md`。
- **结论：BLOCKED_EXTERNAL**。目标平台矩阵为 Windows x64 / macOS ARM64 / macOS x64（desktop.yml matrix）+ Linux 核心（ci.yml ubuntu-latest），本机仅覆盖 **macOS ARM64 的自动部分**。
- 逐平台：**macOS ARM64**（Darwin 24.6.0 arm64 / macOS 15.8.1）自动部分 ✅（核心 494 passed、核心 release、GPUI release、导入集成 52 passed），**GUI ❌（窗口不创建，同 S09.08）**，安装 ❌；**macOS x64 / Windows x64 / Linux** 三项完全缺失 → BLOCKED_EXTERNAL。
- AC077 三条边界：①缺平台标 BLOCKED_EXTERNAL——三个平台无任何本机证据，整体记阻塞，未以一个平台代表四个；②**不把编译通过算 GUI**——表中“自动”与“GUI”分列，macOS ARM64 编译与集成通过但 GUI 列明确写“窗口未创建”，未用 GPUI release 构建充当 GUI 验收；③**不把打包通过算安装**——本机未生成任何 .dmg/.zip（dist/ 下无打包产物），安装列全为 ❌，未用 release 构建或打包脚本存在暗示安装已验证。
- 解除阻塞：三个平台需各自执行环境（CI matrix 与 ubuntu 作业已就绪，触发即可）；macOS ARM64 GUI 需窗口能在本机创建（脚本 workbuddy_import_ui_test.py 已就绪）；安装验收需真实产出并运行安装包（本项未做也未声称）。
- 证据：`evidence/S10.05.md`；日志 `dist/workbuddy-import-validation/S10.05/platform.txt`、`.../import/report.json`。
- **补充（S09.08 通过后）**：屏幕录制授权后 `workbuddy_import_ui_test.py` 一次通过（22 passed / 0 failed，12/12 截图，三维度经 md5 验证真实不同，画面人工核对为真实「使用统计」界面），因此 **macOS ARM64 从“只有自动证据”变为“自动 + GUI 都齐全”**（自动：核心 494 passed、两侧 release、导入集成 64 passed；GUI：12 张截图 + 异常态对话框与提示条）。**但本项整体仍为 BLOCKED_EXTERNAL**：四个平台中还有 **三个（macOS x64 / Windows x64 / Linux）完全没有证据**，按 AC077 缺平台即记阻塞；且**安装一列仍全为 ❌**（本机未产出任何 .dmg/.zip，未用 release 构建或打包脚本存在暗示安装已验证）。仍缺三项可由 CI matrix 与 ubuntu 作业产出，触发需推送授权。
- 风险/遗留：macOS x64 / Windows x64 / Linux 三项缺证据；安装未验证；S10.06 只依赖核心与脚本，不受本阻塞影响。

### S10.06 完成数据升级和回滚演练

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.05 BLOCKED_EXTERNAL，但本项只依赖核心与脚本，不受影响）；WB-RT-001 影响列表不含本项。
- 修改文件：`scripts/workbuddy_import_test.py`（build_v6_database/summarise_database/probe_upgrade_rollback）、`evidence/S10.06.md`、`docs/workbuddy-import/PROGRESS.md`；**未替换任何真实数据库或应用**。
- 命令与退出码：`python3 scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy` → **64 passed, 0 failed**（exit 0）；`cargo fmt --check` → 0；`cargo clippy --locked --all-targets -- -D warnings` → 0；`cargo test --locked` → 0（494 passed）。
- 演练实测：①建 schema 6 库（只跑 0001..0006 再打 user_version=6）→ user_version=6；②写入历史 runs=1（upgrade-run-1）/价格/配置；③备份；④**真实二进制升级** → **user_version=7**；⑤**历史不变**（原 upgrade-run-1 仍在，核心追加自己的 run 行属正常）；⑥价格不变；⑦配置不变（enabled=1, retention_days=90）；⑧导入表已添加（files/ledger/model_history 齐全）；⑨**user_version=99 的新库被拒绝且未改动**（表集合 13→13、版本仍 99）；⑩停止后恢复备份 → restored=6 且摘要等于升级前；⑪未替换真实库/应用（专用沙箱 + real_profiles_unchanged）。
- AC078：三类摘要前后比对（历史用 stats_runs 的 run_id 集合而非“行数没变”——后者会被核心追加行打破；价格用 catalogue_json:last_success_day；配置用 enabled+retention_days）；全程专用沙箱，未触碰用户真实库、未替换已安装应用。
- 过程中定位并修正的真实问题（六个）：①fixture 缺版本戳（迁移脚本只建对象、user_version 由 writer 负责；缺戳则核心当空库重放 0001 并报 table stats_settings already exists）；②价格表列名猜错（实际是 singleton/catalogue_json/last_success_day/updated_at_ms/updated_count/unmatched_count/catalog_count/targets_key）；③stats_settings 应用 UPDATE 而非 INSERT（迁移已种 singleton 行）；④stats_runs.stop_reason 有 CHECK（只允许 normal/unclean，且与 stopped_at_ms 必须同时为空或非空）；⑤**迁移的悬空引用检查**——插入 usage_requests 后核心报 migration left a dangling reference 并禁用统计（这是核心的正确行为，它验证自己的迁移结果，不接受半个 provider/route 图），与其伪造整张引用图，fixture 改为以 stats_runs 作为历史载体并在断言中说清；⑥“未改动”判定写错（原用“导入表数量为 0”，但那时库已是 v7 表本就在，改为比较拒绝前后的表集合）。另把 probe 改为**专用沙箱**（前面探针会留下核心与数据库，在其残骸上做迁移演练等于测错对象）。
- 证据：`evidence/S10.06.md`；日志 `dist/workbuddy-import-validation/S10.06/{run.txt,report.json}`。
- 风险/遗留：S09.08/S10.05 仍 BLOCKED_EXTERNAL；S10.07 将更新用户文档与交付报告。

### S10.07 更新用户文档与交付报告

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.06 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`README.md`（新增“用量导入（WorkBuddy 日志）”章节，README.md:297）、`docs/workbuddy-import/IMPLEMENTATION-REPORT.md`（**新增**）、`evidence/S10.07.md`、`docs/workbuddy-import/PROGRESS.md`；未改 `src/**`、`gpui/src/**`、`scripts/**`、`.github/**`、`Cargo.*`。
- README 章节要点：讲清**后台同步固定 5 秒、与页面刷新无关**（不是设置项、无“实时”模式；页面刷新选项只决定页面多久重读已存数据，设为“关闭”时新记录仍会在几秒内入库，只是不自动显示）；开启方式（默认关闭、命名空间一次性生成）；三类来源区分；**不会重复计数**的判定依据是消息标识而非时间或模型名；归属确认两步流程与“代理证据不可强改直连”；六条已知限制（读取范围、4MiB 超长行、改名文件被拒、未定价费用显示 — 且不画零线、导入记录显示“已记录”、点数不代表余额）。
- IMPLEMENTATION-REPORT.md 六章：①设计与实现对照（14 条要点→实现位置→证据链接）；②AC 状态（按十阶段汇总，S09.08/S10.05 两项 BLOCKED_EXTERNAL 单列并写明解除条件）；③已知限制 9 条逐条带证据链接；④文件清单（新增/修改/未改动）；⑤复现命令（核心/界面/端到端/界面关卡四组）；⑥交付边界（未提交未推送未合并、未升级版本、未制作 Release、两项阻塞未计入完成）。
- AC079：①**每项完成必须有证据链接**——报告内 46 个指向 evidence/ 的链接，脚本逐个解析确认目标存在，**missing: []**；逐项完整状态在 PROGRESS.md；②**没有发布授权就不升版本或制作 Release**——`Cargo.toml` 仍为 **0.5.6** 未改动，未制作 Release、未打包安装程序，报告第 6 节写死该边界。
- 验证：报告内链接 checked=46 missing=[]；`Cargo.toml version = "0.5.6"`；evidence/ 共 78 个文件。**README 有 1 个缺失链接**（`CHANGELOG.md#v055--2026-10-16` → 该文件在本 worktree 不存在），**预先存在**、非本次引入（本次未创建/删除/修改该文件、也未改那一行），如实记录而**未**顺手改掉以掩盖真实状态。
- 证据：`evidence/S10.07.md`；报告 `docs/workbuddy-import/IMPLEMENTATION-REPORT.md`。
- 风险/遗留：README 的 CHANGELOG.md 缺失（预先存在）；S09.08/S10.05 仍 BLOCKED_EXTERNAL；S10.08 将做最终范围审查与交接。

### S10.08 最终范围审查与交接

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S10.07 PASS）；WB-RT-001 影响列表不含本项。
- 修改文件：`docs/workbuddy-import/IMPLEMENTATION-REPORT.md`（新增第 7 节最终范围审查）、`docs/workbuddy-import/PROGRESS.md`、`evidence/S10.08.md`；**未改任何源代码/脚本/CI/Cargo.\***，**未执行任何 git 写操作**。
- 审查结果（日志 `dist/workbuddy-import-validation/S10.08/review.txt`）：`git diff --check` exit 0 无输出；`git status --short` **34 项**（24 已跟踪修改 + 10 未跟踪新增）；`Cargo.toml`/`Cargo.lock` **未改动**（未新增依赖）；版本 `0.5.6` **未升级**；`0001..0006.sql` **未改**；`.git/hooks/` 无非 .sample 文件（**未装 hook**）；隐私扫描（真实 home 路径与密钥模式）**无命中**；作者身份 `gloryhuis / quchaohui@126.com` **未修改**；`HEAD` 仍等于基线 **未提交**。
- allowlist 复核：24 个已跟踪改动逐个对照所属任务允许列表，全部有出处；越界 6 处均已在对应证据单独记录（workbuddy_import/mod.rs 模块声明与聚合 contract_check；src/stats/mod.rs 命令分发 1 行 + **导入 worker 接线**；store.rs load_import_counts；gpui/src/statistics.rs 导入状态请求与接线；0007.sql upstream_protocol 默认值 + 3 索引；6 个 scripts/*.py 的控制协议 11 一行更新）。
- AC080：①**不自动提交/推送/合 main**——HEAD 仍等于基线，34 项改动全留工作树，未执行 commit/push/merge，未建分支或标签；②**真实作者和工具贡献准确**——user.name/user.email 保持真实用户未被本次改动，未用 --author 或身份环境变量替换，报告第 7 节把“是否需要 Co-authored-by: Codex”列为**待人工批准项**而非自行写入；③**禁 force/amend/hooks**——未执行 --force/--amend/rebase/filter-branch，.git/hooks/ 无非 .sample 文件。
- 提交建议与待批准项（写入报告第 7 节，**未执行**）：建议拆 5 个提交（核心导入能力 / 界面来源组与导入面板 / 测试与基准脚本 / CI 接入 / 文档与证据）；三项待批准：是否加 Co-authored-by、evidence/ 与 docs/workbuddy-import/ 是否纳入版本库、两项 BLOCKED_EXTERNAL 是否在提交信息或 PR 描述单独说明。
- 过程中的真实失败：报告章节顺序错位——新增的“7. 最终范围审查”被插到“6. 交付边界”之前，`grep '^## '` 暴露 `…5, 7, 6`，交换两节后复核为 `…5, 6, 7`。
- 回归：本项未改代码或脚本故不重跑；上一项结束时核心 `cargo test --locked` 为 **494 passed**、`clippy --all-targets -- -D warnings` 为 0。
- 证据：`evidence/S10.08.md`；日志 `dist/workbuddy-import-validation/S10.08/review.txt`。
- **S01.01–S10.08 全部执行完毕：78 项 DONE，2 项 BLOCKED_EXTERNAL（S09.08 窗口截图矩阵、S10.05 多平台），无 FAIL。**

### S09.08 第 2 次尝试（环境恢复后，仍 BLOCKED_EXTERNAL）

- 触发：用户提示“可能是机器问题，现在重新尝试”，goal 已 resume 为 active。
- 环境探测：`launchctl managername` = **Aqua**；`screencapture` exit 0；显示器 **1920x1080 Online**；`WindowServer` 运行 → GUI 确实恢复。
- 重跑结果：`System Events` 窗口数由 **0 → 1**，进程出现在前台列表，`set frontmost to true` 成功（frontmost=true）；脚本 **22 passed / 0 failed**，**12/12 截图文件生成**（zh/en × wide/narrow × all/builtin/supplier）。
- **但截图暴露更深问题**：全屏截图（1920x1080）**只有桌面壁纸与菜单栏、无任何应用窗口**；同尺寸下三个来源截图 **md5 去重后只剩 1 个哈希**（画面相同），仅不同尺寸之间有差异。即：**窗口对象存在但不在屏幕上**；`frontmost=true` 后重新截图**哈希逐字节相同**。12 个 PNG 拍的是桌面而非界面。
- 本轮修正的两处真实缺陷（均在脚本内）：①**`screencapture -R` 用了逻辑坐标**——窗口 bounds 来自 System Events 是逻辑点，`-R` 需物理像素，Retina 上差一个 backing scale，故截出桌面一角；已改为全屏截图并注释原因。②`handle` 未定义——`probe_core` 清理路径被一次按文本替换误改成 handle（其变量名为 log），按行定位改回。
- **AC072 未满足**：要求“3 来源 × 2 尺寸 × 2 语言截图齐全”。12 个文件齐但**画面只有 4 种**（2 尺寸 × 2 语言），来源维度未体现在画面上，故**不记 PASS**。
- 数据层断言全部通过：13 张表、`user_version=7`、来源划分自洽、导入行不虚构 HTTP、ledger 不变量。
- **窗口层证据（最终结论）**：改用本机 Swift 6.2.4 + CoreGraphics 直接查询窗口服务器（`dist/workbuddy-import-validation/S09.08/winlist.swift`，只读、未安装任何东西）——主窗口 **`onscreen=true`**（id=20946、1440x901 @ 240,71、alpha=1.0、layer=0），窗口服务器认为它就在屏幕上。但 `screencapture -x -l 20946` 返回 **`could not create image from window`**（exit 1，文件未生成）；同一时刻全屏截图的 md5 **与截图矩阵的 zh-wide-all.png 逐字节相同**（ff6e9dcb13f736f57db77cda28f54774）。**结论：GPUI 窗口已在窗口服务器注册且标记在屏，但没有提交任何可捕获的图层内容**——这解释了两轮的全部现象（窗口计数 0→1、frontmost 可设 true、脚本能产出 12 个 PNG，但没有一张能拍到界面）。属**环境/渲染层**问题，与本次实现无关。阻塞描述由此精确化为“**窗口存在且在屏，但没有可捕获的内容**”。
- **根因确认（本轮）**：**本机处于远程桌面会话，Metal 图层不提交到窗口服务器**。证据链：①GPUI 链接 Metal + QuartzCore（`otool -L`），界面走 GPU 渲染；②`com.apple.RemoteManagementAgent`（PID 840）运行中，前台同时有 **AweSun / RustDesk / O+Connect** 三个远程桌面客户端；③硬件正常（Apple M4、Metal 3、1920x1080 在线）；④**backing scale = 1**（屏幕 1920x1080、窗口 1440x901），逻辑点=物理像素，`-R 240,71,1440,901` 本应正好框住窗口，实测却是桌面；⑤触发三次 resize 强制重绘后 `screencapture -l <id>` 仍返回 `could not create image from window`（exit 1，文件未生成）。
- 结论：远程会话下该 Metal 窗口**已注册、标记在屏、alpha=1.0，但图层内容不提交给窗口服务器**，故任何截图途径都拿不到界面。窗口计数/frontmost/脚本“成功”都不足以证明界面被画出——这是 AC072“截图不能替代实际数据断言”的另一面：**数据断言也不能替代截图**。
- **非实现缺陷、非脚本缺陷**：同一脚本在本机产出 12 个文件、22 条断言全绿，唯独画面是桌面。**在本地物理会话（非远程桌面）下应可正常截图**。
- **对照实验（本轮，决定性）**：用**非 Metal** 的 TextEdit 做对照（同样确认窗口数=1），全屏截图比较——桌面基线 md5 `ff6e9dcb…`；**TextEdit `f3b2d0a4…` 不同 → 窗口可见**；**GPUI `ff6e9dcb…` 逐字节相同 → 窗口不可见**。该对照排除三种可能：**不是权限问题**（同一时刻同一权限下 TextEdit 被拍到）、**不是脚本问题**（同一条 screencapture 调用路径）、**不是窗口不存在**（GPUI 窗口 onscreen=true/alpha=1.0/1440x901 均正确）。剩下唯一相符的解释即远程会话下 Metal 图层不提交——根因由推测变为结论。
- 附带记录：对 TextEdit 按窗口 ID 抓取同样返回 `could not create image from window`，说明 **`screencapture -l` 在本会话下对任何窗口都不可用**（与 Metal 无关）；能区分两者的是**全屏截图**，故对照用它。
- **第 3 次尝试（远程假设被推翻，用户指正）**：用户说明“别的机器远程我这台”已关闭、现在是本机在远程别人。复测证实用户正确：`who` 显示会话类型 **console**（本地控制台），`netstat` 无 5900 监听（无入站屏幕共享），分辨率已恢复 **5120x2880 / 2560x1440**。**我此前把“本机有 RustDesk/AweSun/O+Connect 进程”误读为“本机被远程”——那其实是出站客户端，与窗口捕获无关；`RemoteManagementAgent` 存在也不足以证明有人正在入站控制。该结论已更正。**
- 在正确环境下重新排除仍失败：裸二进制+激活窗口（全屏截图与桌面基线逐字节相同）、`screencapture -l <id>`、`-o -l`（均 could not create image from window）、`-R` 按物理坐标（逻辑×2，截到区域仍是桌面壁纸）、**打包成 .app bundle 后用 open -n 启动**（仍不可见，但 bundle 版本多出 2560x24 菜单栏窗口）、三次 resize 强制重绘（无效）。窗口服务器始终报主窗口 onscreen=true/alpha=1.0/1440x901，frontmost 可在 true/false 间切换——窗口对象一切正常，只是不绘制任何内容。
- 现判断：**不是远程会话、不是权限、不是脚本、不是 bundle**（四种均已用对照或直接实验排除），剩下的是 **GPUI 在本机不把界面画出来**。
- **下一步需要只有用户能做的判断**：在本机**手动**启动界面用肉眼确认窗口是否出现（终端 A 起核心 `--headless --port 19911`，终端 B 起 `workbuddy-proxy-gpui --config-dir`）。若肉眼可见→是截图机制问题（本机抓不到 Metal 内容），可改用别的方式取证或换机器跑脚本；若肉眼也不可见→是 GPUI 在本机不渲染，与本项目导入功能无关（GPUI 是既有界面层，本次改动未碰渲染管线）。
- 解除条件：**在本地物理会话中运行 `scripts/workbuddy_import_ui_test.py`**（脚本已就绪，**无需任何代码改动**），或提供一台可截图的机器。
- 证据：`evidence/S09.08.md`（新增“第 2 次尝试”章节）；日志 `dist/workbuddy-import-validation/S09.08/{run.txt,report.json,screenshots/}`。

### S09.08 通过界面关卡（第 4 次尝试：PASS，阶段 ui 关卡通过）

- 输入基线：`6102b779871bab0bb8ad646dac5004892adf76be`（前置 S09.07 PASS）。
- **根因（最终确认）**：**运行脚本的进程（DeepSeek Harness）没有被授予屏幕录制 TCC 权限**。用 ScreenCaptureKit（macOS 15 唯一受支持的窗口捕获路径）直接调用，授权前始终返回 `SCStreamErrorDomain Code=-3801 "用户拒绝了应用程序、窗口、显示器捕捉的TCC"`；用户按提示在「系统设置 → 隐私与安全性 → 屏幕录制」授权后，同一条命令立刻列出 **13 个 workbuddy 窗口**（主窗口 id=21264、onscreen=true、1440x901 @ 550,165）。
- **重跑结果**：`python3 scripts/workbuddy_import_ui_test.py --exe gpui/target/release/workbuddy-proxy-gpui --core target/release/workbuddy-proxy` → **22 passed / 0 failed**（exit 0），**12/12 截图**。
- **矩阵完整性（脚本化验证）**：12 张截图 **12 个唯一 md5**（各不相同）；来源维度 zh-wide 三张 **3 个唯一哈希**（点击真的切换了来源）；尺寸维度 wide vs narrow **2 个**；语言维度 zh vs en **2 个**。
- **截图内容人工核对**（不只看哈希）：`zh-wide-builtin.png` 为真实的 WorkBuddy Proxy「使用统计」页——Token 总用量大字；**九项指标空态正确**（用量记录 0「0 代理 · 0 导入」、代理成功率 **—**「0 成功 · 0 失败」、输入/输出 Token 与缓存读取/创建均 **—**、完整覆盖率 **—**、代理估算费用 **未定价**「0 导入费用未知 · 0 用量不全」、已记录用量完整率 **—**、WorkBuddy 点数 **—**「日志记录值，不代表账号余额」）；来源筛选控件；提示条「上次退出异常，中断区间的记录可能不全」；以及一个「WorkBuddy 连接地址不一致」对话框（异常态证据）。AC068 的三条语义在真实界面上得到印证，而不只是单元测试里。
- AC072：①3 来源 × 2 尺寸 × 2 语言截图齐全且三维度均真实体现；②异常态有对话框与提示条为证；③**截图与数据断言互相独立**——22 条断言不依赖截图，12 张截图也不靠断言推断。**阶段关卡 ui 随之 PASS。**
- **更正记录（前三次结论均错，逐条更正）**：第 1 次「窗口不存在」→ 后来计数变成 1；第 2 次「本机被远程、Metal 图层不提交」→ **错**，`who` 显示会话是 console，把出站远程客户端误读成入站；第 3 次「GPUI 在本机不渲染」→ **错**，窗口对象一直正常，是捕获端没有权限；「`screencapture -l` 失败 = Metal 图层不提交」→ **错**，macOS 15 已移除其依赖的 `CGWindowListCreateImage`，该调用在任何窗口上都会失败。真正根因只有一条：**屏幕录制 TCC 权限**。
- 证据：`evidence/S09.08.md`；日志与截图 `dist/workbuddy-import-validation/S09.08/{run.txt,report.json,screenshots/*.png,tcc-root-cause.txt,api-change.txt,control-experiment.txt,window-evidence.txt,render-evidence.txt,session-evidence.txt}`。
- 风险/遗留：S10.05 目标平台验收仍 BLOCKED_EXTERNAL（缺 macOS x64 / Windows x64 / Linux 环境）。


## 2026-10-17 AC048复核纠正

初版S06.08的“队列峰值=容量常量2”和“批次耗时=四个单元测试总时间”均不满足实测要求，不再计PASS。新生产runtime telemetry记录实际成功入队/接收的峰值、writer源批次事务尝试与失败、单调微秒耗时及32个有界样本；在真实文件/发现/配置观察I/O边界记录OS worker线程检查与违例。旧核心红测缺字段失败，新增实现完整537测试和Clippy通过；最终release与严格矩阵观测尚待执行。未修改原门槛，也不把配置容量当峰值。


## 新导入模块600/80风格目标的具体复核（2026-10-17）

本次为本机自审，不冒称独立审查。扫描候选、源码hash与逐函数结论见[STYLE-REVIEW](../../evidence/R09-20261017-STYLE-REVIEW.json)。未用suppress/allow隐藏超标。下列为明确保留的例外，不再声称全部模块少于600行。

- `classify.rs`（1007行）：分类/纯匹配部分442行；余下为内联合成测试，保留同处审查边界。
- `normalize.rs`（1221行）：规范化生产及启动自检540行；其余内联优先级/整数/积分测试。
- `parser.rs`（1270行）：生产/启动自检621行，包含显式serde白名单类型和身份hash；余下内联语法/隐私测试。接受21行及测试共置例外。
- `reader.rs`（2585行）：生产/自检1233行，含限制发现、指纹、完整行及优先队列等共用边界；真实调度/文件事务已独立runtime/worker子模块。接受底层组件与内联边界测试共置例外；没有以allow绕过。
- `store.rs`（4025行）：生产/内存自检1896行；同一schema的账本、checkpoint、归属、设置、批次事务共用调用者Transaction。内存契约自检和大批内联故障测试保留。接受事务边界在同模块便于核对的例外，不把总行数称小于600。
- `mod.rs`（1976行）：导入公共状态/命令DTO、通道/ACK/调度契约与启动自检仍集中；实际worker/writer/文件/调度/历史/归属任务已拆分。接受公共契约及内联测试共置例外；启动自检仍为生产可达函数，不能全部按测试排除。
- `types.rs`（878行）：共享常量、受限枚举/DTO、文档与序列化契约自检；无用户源文件/数据库I/O。接受共享合同类型集中及内联测试例外。

超过80行的生产可达函数逐项结论：

- `classify_contract_check`（候选非空非注释行122）：启动合成分类契约自检；固定输入/固定错误码，无用户历史推断。逐组断言保留，非每批读文件或转发路径；接受自检长度例外。
- `channel_contract_check`（候选非空非注释行95）：启动固定容量/非阻塞/ACK窗口自检；固定消息数，无源文件和DB I/O。接受合成自检长度例外，不作为实际运行时队列峰值证据。
- `normalize_cache_and_reasoning`（候选非空非注释行92）：逐项复核三组固定优先级别名和三种子集约束；无循环随输入增长、I/O或数据库操作，所有分支在同一usage上先验证后赋值。保留相邻决策表以便核对read/write/reasoning不重复计入；接受本轮超80行例外。
- `normalize_contract_check`（候选非空非注释行98）：启动固定Token/积分边界自检，直接调用生产纯函数；接受合成自检长度例外，不代替黄金端到端。
- `parser_contract_check`（候选非空非注释行119）：启动白名单/角色/身份/隐私合成自检；固定字面量，不接触用户正文。接受合成自检长度例外。
- `reader_contract_check`（候选非空非注释行237）：启动读取边界合成自检，纯决策及不存在的probe根；长表含固定路径、指纹、完整行/截断边界，非真实扫描工作循环。接受自检长度例外；该函数执行固定probe的元数据访问，不能把它描述成完全无系统调用。
- `commit_batch`（候选非空非注释行87）：逐项复核Settings CAS、幂等/冲突/墓碑分支、事实+账本写入和最后checkpoint ACK；全部借用同一Transaction，任一?中止调用者事务。保留一个可审查的原子事务流程，未添加第二连接或提前ACK；接受本轮超80行例外。
- `mapping_contract_check`（候选非空非注释行285）：启动内存schema/事务/归属/墓碑合成自检；Connection::open_in_memory创建独立内存库，不打开用户数据库。接受固定自检长度例外，不将其当真实来源配置接线证据。

扫描依赖rustfmt缩进用于定位候选，已对实际normalize/commit及自检调用关系做源码复核。stats::start_with_import_root在启动线程前确实调用contract_check，内存Connection自检仍属于生产可达，不能误称仅cfg(test)。此风格复核不替代性能/跨平台/完整隔离等未决验收。
