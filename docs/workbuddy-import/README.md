# WorkBuddy 本地用量导入：软件设计与受控开发说明书

设计基线版本：1.0；日期：2026-10-16。当前业务实现已接线，按2026-10-18用户修订继续验收；完整状态见[实现报告](IMPLEMENTATION-REPORT.md)和[机器可读入口](../../evidence/R09-20261017-CURRENT.json)。

本说明书用于将 WorkBuddy 内置模型、用户直连自定义模型的本地 JSONL 用量增量写入 WorkBuddy Proxy 的 SQLite，并与代理自有记录统一展示。原任务交由 DeepSeek 编码；2026-10-17 用户明确授权 Codex 开启 Goal 接手复核修复。当前执行状态见 REMEDIATION.md；原 DeepSeek 的 DONE/PASS 记录保留追溯，不能替代本轮重新验收。

## 工作位置与基线

- 原仓库：本机主检出目录，通过 `git worktree list --porcelain` 核实，保持原分支和工作区。
- 开发 worktree：通过 `git worktree list --porcelain` 核实 `codex/workbuddy-usage-import` 对应的本机目录，不要求固定盘符或绝对路径。
- 开发分支：`codex/workbuddy-usage-import`。
- 固定基线：`6102b779871bab0bb8ad646dac5004892adf76be`，v0.5.6。
- 基线 SQLite schema=6，统计 API=2，管理协议=10；目标分别为 7、3、11。
- [baseline.json](baseline.json) 记录关键文件 SHA256 和只读调查结论。
- 不在 main 上编码；不自行切换基线、合并其他功能分支、改版本号、发布或推送。

## 阅读与执行顺序

1. 当前 worktree 根目录 `AGENTS.md`。
2. [DESIGN.md](DESIGN.md)：范围、技术路线、数据准确性和资源预算。
3. [DATA-API.md](DATA-API.md)：字段、数据库、接口和状态机的固定契约。
4. [UI-SPEC.md](UI-SPEC.md)：三个来源图标、各区域布局、交互与双语文案。
5. [TASKS.md](TASKS.md)：10 个阶段，每阶段 8 个子任务，共 80 项；严格顺序执行。
6. [ACCEPTANCE.md](ACCEPTANCE.md)：功能、异常、性能、兼容和平台验收。
7. [DEEPSEEK-PROMPT.md](DEEPSEEK-PROMPT.md)：交给编码模型的入口指令。
8. [PROGRESS.md](PROGRESS.md)：当前复核入口及逐项历史执行记录；初始状态仅代表设计交付起点。

文档约束优先级：用户最新明确要求 > 根 AGENTS.md > 本目录冻结契约；契约之间出现冲突必须记录并停止受影响任务，不自行选一种解释。不得把任务完成勾选当作验收证据。

## 已固定的产品决定

- 代理数据始终优先。属于代理的 WorkBuddy 消息只记去重结果，不生成第二条统计明细；代理没有记录时也不偷偷回填。
- 所有报表只查本产品的 SQLite。页面刷新不得扫描 WorkBuddy JSONL。
- 一个统一明细表 `usage_requests`，增加来源等字段；辅助使用导入文件、身份账本、历史模型归属三张表，导入设置放在现有 `stats_settings`。三张辅助表分别解决增量读取、重扫/清除后去重、代理归属；不得省略身份账本。
- 首次开启由用户操作；默认关闭导入。开启后历史回填和后续增量都在后台执行。
- 筛选条最前方固定三个图标：**全部来源 / 内置模型 / 供应商模型**。供应商模型包括用户直连与代理；两者用高级筛选“采集方式”细分，不增加第四个来源图标。
- 无法证明归属的历史自定义模型进入“待确认”，默认不计入总数。允许用户按模型 ID 和明确时间区间确认历史归属；不能靠名称猜供应商。
- 导入费用显示未知；WorkBuddy credit 单独显示为点数，不换算为人民币/美元，不套当前价格追算历史。
- 不保存聊天正文、工具参数、图片、密钥、URL 查询参数、完整原始 JSONL 行。

## 交付边界

原设计交付阶段（2026-10-16）仅创建 worktree 和设计包；2026-10-18 接手实现与验收的当前范围见 [IMPLEMENTATION-REPORT](IMPLEMENTATION-REPORT.md)。`validate_spec.py` 仅检查设计的一致性、任务/验收覆盖和合成示例，不实现生产导入器；它通过不代表业务开发完成。

开发完成必须提供：代码、迁移、自动化测试、隔离环境 GUI 验收、性能报告、平台状态、升级/回滚说明。只有文档或单元测试通过不得标为全部完成。提交/推送/发布按用户当时授权执行；不得沿用旧交接文档中的发布授权。

设计包检查命令（在开发 worktree 执行）：

```bash
python3 docs/workbuddy-import/validate_spec.py
```

## 设计交付验证

[设计校验结果](DESIGN-VALIDATION.json) 仅覆盖文档/SQL设计稿/合成fixture。业务任务状态以 PROGRESS.md 为准，tasks.json 的 status 是初始计划快照，不是验收记录。

[布局预览 PNG](UI-WIREFRAME.svg.png) 与 [SVG 原稿](UI-WIREFRAME.svg) 展示宽屏、窄屏和三个来源状态。

本设计包仍是当前worktree内未提交文档；交给DeepSeek时应保留这些文件，不执行清理未跟踪文件。

修订1.1 / WB-RT-001：后台固定每5秒调度增量及新文件发现，与页面刷新完全独立。施工中的接入与补验顺序见DECISIONS.md；保留现有PROGRESS。
