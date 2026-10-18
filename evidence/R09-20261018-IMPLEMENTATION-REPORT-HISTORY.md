# WorkBuddy 用量导入 · 实现与交付报告

- 基线 commit：`6102b779871bab0bb8ad646dac5004892adf76be`
- 分支：`codex/workbuddy-usage-import`（worktree：`worktrees/workbuddy-usage-import/workbuddy-proxy`）
- 设计包：[`docs/workbuddy-import/`](README.md)（含修订 1.1 / WB-RT-001）
- 逐项状态：[`PROGRESS.md`](PROGRESS.md)；逐项证据：[`evidence/`](../../evidence/)
- **版本未升级**（仍为 `0.5.6`），**未制作 Release**，**未提交 / 未推送 / 未合并**

## 当前复核状态（覆盖下方初版记录）

当前候选f402c103…（writer SQLite缓存8MiB）已通过537核心测试、fmt、Clippy、release、21组串行功能门禁和16项接口边界；严格规模矩阵正在串行续跑，最新逐轮计数以机器可读状态为准；全12阶段快照与RSS增长聚合尚未完成。百万首阶段回填552.942秒，20次增量最大4.992秒，查询P95最大323.934ms，真实300秒空闲CPU中位0.143%、未变化正文读取不增长。当前核心状态轮询13/13、保留期限53/53补验通过；其余阶段已串行启动，私有包、原生生命周期、完整GUI及80条证据审计仍待完成。真实签名配对已证明程序非签名字节一致，核心/UI实际进程隔离检查通过；原生操作因Mac锁屏暂缓，未计GUI通过。详见[机器可读状态](../../evidence/R09-20261017-CURRENT.json)及[签名与隔离证据](../../evidence/R09-20261017-CACHE8-GUI-IDENTITY.json)。

> 2026-10-17 ac03历史检查点（不代表当前f402候选）：当前核心 ac03b905… 已通过537测试/Clippy/release，GPUI 27367fd4… 已通过137测试/Clippy/release及最终12原生图、X45和竞争回填事件预算。21个串行release gate、R05保留期限53项、status补验13项、私有DMG及原生关闭/重开/Cmd+Q均已通过。外置TMP全新12阶段严格规模矩阵仍在运行，未计总关卡PASS。AC003 GUI完整隔离证明、AC060结果边界补验、AC072完整异常态/缩放及外部平台仍有缺口；固定80条以[当前审计](ACCEPTANCE-AUDIT.md)为准，不沿用原DONE完成率。未提交、推送、合并、发布。

本机证据摘要：[串行gate](../../evidence/R09-20261017-LOCAL-GATES.json)、[保留期限](../../evidence/R09-20261017-RETENTION.json)、[status成本](../../evidence/R09-20261017-STATUS-POLLING.json)、[UI真实事件](../../evidence/R09-20261017-UI-EVENTS.json)、[私有包](../../evidence/R09-20261017-PACKAGE.json)、[原生生命周期](../../evidence/R09-20261017-NATIVE-PACKAGE.json)、[临时文件清理](../../evidence/R09-20261017-STORAGE.json)。

## 初版历史记录（非当前验收结论）

## 1. 设计与实现对照

| 设计要点 | 实现位置 | 证据 |
|---|---|---|
| 后台固定 5 秒增量同步 + 新文件发现，**与页面刷新完全无关**（WB-RT-001） | `reader::SYNC_INTERVAL`、`mod.rs::IMPORT_TICK_MS`、`SyncSchedule` | [S04.01](../../evidence/S04.01.md)、[S06.05](../../evidence/S06.05.md)、[S06.06](../../evidence/S06.06.md)、[S09.06](../../evidence/S09.06.md)、[S10.03](../../evidence/S10.03.md) |
| 只读 `projects/<一级项目>/<会话>.jsonl`，不读 traces / 工具结果 / 外部 symlink | `reader::is_allowed_relative`、`ReaderRoot` | [S04.02](../../evidence/S04.02.md)、[S04.03](../../evidence/S04.03.md) |
| 身份键：长度前缀 SHA256(`namespace`,`sessionId`,`providerData.messageId`) → 64 位小写十六进制 | `parser::entry_key`、`types::ENTRY_KEY_DOMAIN` | [S03.03](../../evidence/S03.03.md) |
| "未知即 None"，不写 0 | `types::NormalizedUsage`、`dto` 的 `Option` 列 | [S03.04](../../evidence/S03.04.md)、[S09.04](../../evidence/S09.04.md) |
| 台账状态机与 `request_id` 不变量 | `store`（`imported` 必有 `request_id`，其余必为 NULL） | [S05.01](../../evidence/S05.01.md)、[S05.06](../../evidence/S05.06.md) |
| 镜像去重：代理已有的记录只计一次 | `store::commit_batch`、`ReplayOutcome` | [S06.03](../../evidence/S06.03.md)、[S10.01](../../evidence/S10.01.md) |
| 归属分类：`proxy` / `builtin` / `direct` / `pending`，区间半开 `[from, to)` | `classify::{attribute, attribute_with_intervals}` | [S05.04](../../evidence/S05.04.md)、[S05.05](../../evidence/S05.05.md) |
| 归属确认与撤销：预览 → 一次性令牌 → 确认；代理证据不可强改直连 | `store::{preview_mapping, claim_mapping_task, revoke_mapping_rows}`、`MappingDialog` | [S05.07](../../evidence/S05.07.md)、[S09.07](../../evidence/S09.07.md) |
| schema 6→7、API 2→3、控制协议 10→11 | `store::SCHEMA_VERSION`、`dto::API_VERSION`、`main::CONTROL_PROTOCOL` | [S02.01](../../evidence/S02.01.md)、[S08.01](../../evidence/S08.01.md)、[S08.07](../../evidence/S08.07.md) |
| 来源组三个互斥选项 + 筛选条窄屏布局 | `source_filter::{SourceFilter, layout}` | [S09.01](../../evidence/S09.01.md)、[S09.02](../../evidence/S09.02.md) |
| 三类来源可区分；缺 HTTP/速率不显示 0/200 | `usage_table::{origin_text, status_text, http_text}` | [S09.05](../../evidence/S09.05.md) |
| 导入开关失败回滚、暂停不删历史 | `workbuddy_import::ImportPanel` | [S09.06](../../evidence/S09.06.md) |
| 10 秒可查、X45～X47 | 端到端脚本 | [S10.03](../../evidence/S10.03.md) |
| 故障注入：无静默丢失、恢复幂等 | 端到端脚本 | [S10.02](../../evidence/S10.02.md) |
| schema 6→7 升级与回滚 | 端到端脚本 | [S10.06](../../evidence/S10.06.md) |

## 2. AC 状态（按阶段，逐项证据见 PROGRESS）

| 阶段 | 项数 | 状态 | 证据入口 |
|---|---|---|---|
| baseline | S01.01–S01.08 | 全部 DONE | [S01.08](../../evidence/S01.08.md) |
| schema | S02.01–S02.08 | 全部 DONE | [S02.08](../../evidence/S02.08.md) |
| parser | S03.01–S03.08 | 全部 DONE | [S03.08](../../evidence/S03.08.md) |
| reader | S04.01–S04.08 | 全部 DONE | [S04.08](../../evidence/S04.08.md) |
| classification | S05.01–S05.08 | 全部 DONE | [S05.08](../../evidence/S05.08.md) |
| runtime | S06.01–S06.08 | 全部 DONE | [S06.08](../../evidence/S06.08.md) |
| lifecycle | S07.01–S07.08 | 全部 DONE | [S07.08](../../evidence/S07.08.md) |
| api | S08.01–S08.08 | 全部 DONE | [S08.08](../../evidence/S08.08.md) |
| ui | S09.01–S09.08 | S09.01–S09.07 DONE；**S09.08 BLOCKED_EXTERNAL** | [S09.08](../../evidence/S09.08.md) |
| release_gate | S10.01–S10.08 | S10.01–S10.04、S10.06 DONE；**S10.05 BLOCKED_EXTERNAL**；S10.07/S10.08 见 PROGRESS | [S10.05](../../evidence/S10.05.md) |

**两项阻塞都不是实现缺陷**，且都附带了可执行的解除条件：

- **S09.08（UI 关卡）**：本机 GPUI 进程存活但窗口不创建（`System Events` 报 `count of windows = 0`），且 GPUI 的 `cargo test` 目标编译时 `rustc` SIGBUS。脚本 `scripts/workbuddy_import_ui_test.py` 已就绪，窗口一可用即可产出 12 张截图与事件证据。
- **S10.05（多平台）**：四个目标平台中只有 macOS ARM64 的自动部分在本机有证据；macOS x64 / Windows x64 / Linux 需要各自执行环境，CI matrix 与 ubuntu 作业已就绪。

## 3. 已知限制

1. **导入范围**：只读 `<HOME>/.workbuddy/projects/<一级项目>/<会话>.jsonl`；traces、工具结果、外部符号链接一律不读。
2. **超长行**：单行超过 4 MiB 会被跳过，同文件其他记录不受影响（[S10.02](../../evidence/S10.02.md) 场景 4）。
3. **改名文件**：解析器要求文件 stem 等于 `sessionId`，改名后该文件被拒（这正是"改名不复活已计数记录"的实现方式）。
4. **不可读文件**：权限不足的文件被跳过，核心继续服务（[S10.02](../../evidence/S10.02.md) 场景 5）。
5. **磁盘写失败未注入**：需要把数据库放到写入会失败的介质，超出测试脚本能力，**未用相似场景冒充**（[S10.02](../../evidence/S10.02.md)）。
6. **性能门槛未全测**：1M 条 / ≥1 GiB 语料、Mock 上游 4 并发的 P95 影响、**按线程**的 worker RSS 峰值、首次回填 10 分钟预算均未在本机执行，以缺口形式登记（[S10.03](../../evidence/S10.03.md)）。已测部分全部达标，其中增量 **20/20 次 ≤10 秒**。
7. **归属确认的交互入口**：`MappingDialog` 的渲染元素与点击接线尚未落地（状态机与语义已完成）。
8. **导入区渲染**：`ImportPanel` 的开关/计数/检查按钮的渲染元素待补（模块与状态机已完成）。
9. **`core_compatibility.py`** 端到端运行需要桌面会话，本机未跑通（[S08.07](../../evidence/S08.07.md)）。

## 4. 文件清单

**新增（核心）**：`src/stats/workbuddy_import/{mod,types,parser,normalize,reader,classify,store}.rs`、`src/stats/migrations/0007.sql`

**新增（界面）**：`gpui/src/{source_filter,workbuddy_import}.rs`

**新增（脚本与证据）**：`scripts/workbuddy_import_test.py`、`scripts/workbuddy_import_bench.py`、`scripts/workbuddy_import_ui_test.py`、`scripts/fixtures/workbuddy_import/*`、`evidence/S01.01.md` … `evidence/S10.06.md`

**修改（核心）**：`src/stats/{mod,store,dto,query}.rs`、`src/{main,frontend}.rs`

**修改（界面）**：`gpui/src/{assets,main,statistics,stats_backend,stats_controller,usage_table}.rs`

**修改（工程）**：`.github/workflows/{ci,desktop}.yml`、`README.md`、若干 `scripts/*.py`（控制协议 11 的一行更新）

**未改动**：`Cargo.toml` / `Cargo.lock`（未新增依赖）、`0001.sql`–`0006.sql`（未改历史迁移）

## 5. 复现命令

```bash
# 核心：格式、lint、测试
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked

# 界面
cargo fmt --manifest-path gpui/Cargo.toml --check
cargo clippy --manifest-path gpui/Cargo.toml --locked --all-targets -- -D warnings
cargo build --manifest-path gpui/Cargo.toml --release --locked

# 端到端（隔离沙箱，价格目录指向闭合端口，不发公网调用）
export WORKBUDDY_PROXY_CATALOG_TEST_URL=http://127.0.0.1:1/catalog
export RUST_TEST_THREADS=1
cargo build --release --locked
python3 scripts/workbuddy_import_test.py --exe target/release/workbuddy-proxy \
    --output dist/workbuddy-import-validation/manual
python3 scripts/workbuddy_import_bench.py --exe target/release/workbuddy-proxy \
    --records 20000 --incremental-samples 20

# 界面关卡（需要可用的窗口会话）
python3 scripts/workbuddy_import_ui_test.py
```

## 6. 交付边界

- **未提交、未推送、未合并**，工作树保留全部改动供审查。
- **未升级版本**（`Cargo.toml` 仍为 `0.5.6`），**未制作 Release**，**未打包安装程序**。
- 两项 BLOCKED_EXTERNAL 已如实登记，**未计入完成**。

## 7. 最终范围审查（S10.08）

审查命令与输出：`dist/workbuddy-import-validation/S10.08/review.txt`。

| 检查 | 命令 | 结果 |
|---|---|---|
| 空白错误 | `git diff --check` | exit 0，**无输出** |
| 变更清单 | `git status --short` | **34 项**（24 个已跟踪文件被修改 + 10 项未跟踪新增） |
| 依赖锁 | `git status --short Cargo.toml Cargo.lock` | **空** —— 两者均未改动，**未新增依赖** |
| 版本 | `grep '^version' Cargo.toml` | `0.5.6` —— **未升级** |
| 旧迁移指纹 | `git status --short src/stats/migrations/000{1..6}.sql` | **空** —— 历史迁移一行未改 |
| Git hooks | `ls .git/hooks/`（排除 `.sample`） | **空** —— 未安装任何 hook |
| 隐私扫描 | 在改动的已跟踪文件里搜真实 home 路径与密钥模式 | **无命中** |
| 作者身份 | `git config user.name` / `user.email` | `gloryhuis` / `quchaohui@126.com` —— **未修改** |
| 提交状态 | `git rev-parse HEAD` | `6102b779871bab0bb8ad646dac5004892adf76be` —— **与基线一致，未提交** |

### 变更清单（按类别）

**核心**：`src/stats/{dto,mod,query,store}.rs`、`src/{frontend,main,proxy,workbuddy}.rs`
**界面**：`gpui/src/{assets,main,statistics,stats_backend,stats_controller,usage_chart,usage_table}.rs`
**工程**：`.github/workflows/{ci,desktop}.yml`、`README.md`、`scripts/{api_key_providers_test,core_compatibility,gpui_package_test,native_protocol_test,statistics_test,workbuddy_endpoint_check_test}.py`
**新增（未跟踪）**：`docs/workbuddy-import/`、`evidence/`、`gpui/src/{source_filter,workbuddy_import}.rs`、`scripts/fixtures/`、`scripts/workbuddy_import_{test,bench,ui_test}.py`、`src/stats/migrations/0007.sql`、`src/stats/workbuddy_import/`

### 提交建议（需批准，未执行）

建议按关注点拆成若干提交，而不是一个巨型提交：

1. `feat(stats): add WorkBuddy JSONL usage import` —— `src/stats/workbuddy_import/`、`0007.sql`、`src/stats/{store,dto,query,mod}.rs`
2. `feat(gpui): add source filter and import panel` —— `gpui/src/{source_filter,workbuddy_import,statistics,assets,main,stats_backend,stats_controller,usage_chart,usage_table}.rs`
3. `test(scripts): add import integration, bench and UI harnesses` —— `scripts/workbuddy_import_*.py`、`scripts/fixtures/`、协议 11 的一行更新
4. `ci: run the import integration checks` —— `.github/workflows/{ci,desktop}.yml`
5. `docs: document the import feature and delivery report` —— `README.md`、`docs/workbuddy-import/`、`evidence/`

**待批准项**：

- 本次Codex已实质参与开发，用户已明确署名规则：若后续人工提交这些成果，消息末尾必须且只能有一次 `Co-authored-by: Codex <codex@openai.com>`。保持当前Git Author/Committer，不再另问是否署名；本轮仍不提交。
- 是否把 `evidence/` 与 `docs/workbuddy-import/` 一并纳入版本库（当前为未跟踪状态）。
- 两项 BLOCKED_EXTERNAL 是否在提交信息或 PR 描述中单独说明。

### 交付边界（重申）

**未执行** `git commit` / `git push` / 合并到 main / `--force` / `--amend` / 安装 hook。工作树保留全部改动，HEAD 仍在基线，供人工审查后决定。

## 当前新增横切验证缺口

AC043暂列NOT_RUN：现有去重单测重复同一checkpoint，黄金脚本重复同一JSONL行，不能取代真实跨项目复制的reader→writer链。新增 `scripts/workbuddy_import_edge_probe.py` 通过实际源文件验证两个项目各自EOF、同消息不同顶层ID/时间冻结、BOM/CRLF/损坏UTF-8、FIFO/socket/目录/外部symlink不读取、205条相同时间戳稳定分页和canary边界。当前仅完成语法检查，排在严格规模矩阵之后执行，不能算作已通过。规模矩阵后串行队列为 `dist/workbuddy-import-review/run_cache8_after_matrix.py`，随后验证当前私有DMG的实际签名代码身份及自动生命周期，原生关闭/重开/CmdQ仍需独立CUA事件证据。当前80项结论见[当前二进制审计](../../evidence/R09-20261017-CACHE8-AC-AUDIT.json)，旧失败与旧版本证据均保留。
