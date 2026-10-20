# 2026-10-20 用户决策补充（当前有效）

最新产品基线`d831f37`，Codex直接实现/复核。D01/D16/D15分别本地提交；D04/D09按用户确认关闭选择。见[决策记录](../DECISIONS_20261020.md)及[最新核查JSON](decisions-verification.json)。

D15/D16最终各139个现有GPUI测试及release通过，当前隔离GUI实测范围见各报告。D16的修改后最小窗口仍未取得有效证据。Windows、英文、Retina/IME及旧T05/T06验收缺口保留。受保护文件哈希无变化，所有本轮CAP已清理，tmp为空。

下方是初次T10审计历史记录，其旧D04/D16待用户确认状态已由本次决策记录替代；旧“未运行测试/GUI”只指该次文档审计。

---

# T10 只读验证记录（审计快照 1dbebac，2026-10-20）

## 本轮性质

- **文档审计**：只更新 `FINAL_REPORT.md` 与 `logs/verification.md`；这是审计快照后的文档更新，产品代码基线仍为 `1dbebac`，文档由 Codex 复核后本地提交。
- **未构建、未运行测试/GUI、未创建临时目录、未启动进程、未做清理**；不声称验证新代码。
- 旧机 `/Volumes/1TB`、`/Volumes/git` 是另一台电脑/迁移归档的旧路径，不是本机路径、缓存或阻塞原因。

## 工作区与 git

```
$ git rev-parse --abbrev-ref HEAD
codex/ui-redesign
$ git rev-parse HEAD   # 审计快照时（产品代码）
1dbebac8b245d2ad237ae43943f3748293361dd8
$ git rev-list --count cbb3a91..1dbebac
25
$ git diff --check
exit=0
$ git diff --name-only cbb3a91..1dbebac -- src | wc -l
3
$ git diff --stat cbb3a91..1dbebac -- gpui/src
12 files changed, 3971 insertions(+), 1545 deletions(-)
$ git diff --stat cbb3a91..1dbebac -- src
3 files changed, 91 insertions(+), 5 deletions(-)
$ git diff --stat cbb3a91..1dbebac -- Cargo.toml gpui/Cargo.toml gpui/Cargo.lock
（空）
```

`git diff 1dbebac -- gpui/src src` 为空；审计期间产品源码未改。

审计快照时 status（受保护；T10 两文件为快照后的文档更新）：

```
 M docs/UI-UX-renew/IMPLEMENTATION_STATUS.md
?? docs/UI-UX-renew/ACCEPTANCE.md
?? docs/UI-UX-renew/AGENT_TASK.md
?? docs/UI-UX-renew/DESIGN_SPEC.md
?? docs/UI-UX-renew/IMPLEMENTATION_PROMPT.md
?? docs/UI-UX-renew/TASK_BREAKDOWN.md
?? docs/UI-UX-renew/*.png / *.pdf（未跟踪设计资料）
```

## 报告链接与状态核对

- 链接检查：Codex 独立核对 **88 个 Markdown 相对链接、0 缺失**；DSH 脚本（仅 inline `](...)`）67 个、0 缺失（口径较窄）。
- T07 L03/L04/L05：各自报告为已闭合；本轮未重跑。
- T08 R06：B 的修复与复核文件存在（`t08-r06-error-recovery-fix-001/REPORT.md`、`CODEX_REVIEW.md`）；复核为 PASS（macOS 隔离夹具）。
- T09 X03：`t09-x03-focus-fix-001/REPORT.md`、`CODEX_REVIEW.md` 存在；结论 PASS 仅限 macOS 中文、非 Retina、1440×900 的 OpenAI Chat 协议弹窗。
- T09 X05：`t09-x05-repeat-001/REPORT.md`、`CODEX_REVIEW.md`、`corrected/` 存在；权威 corrected 10 轮逐轮 AX+KPI，旧预备测量已标 superseded；PASS 限 macOS 合成数据 10 轮，无性能阈值/长期 soak/无泄漏结论。
- macOS 总体仍 PARTIAL；Windows 全矩阵/DPI BLOCKED；英文/Retina/IME NOT RUN；T05 仅同步/重启、T06 A02/A04/A05、D04/D16、F01 provenance、F03 迁移版本对应性仍 PARTIAL。
- F03 相邻迁移归档：433 文件、T04 61/T05 38/T06 50、errors 空、SHA256SUMS 15/15 OK；旧 manifest 为 `/Volumes/git`、T04 record 为 `6f19c42`，T05/T06 无 HEAD，仍不能对应审计基线 `1dbebac`。

## 临时目录与空间

- 本次只读审计未创建/清理任何临时目录。
- CAP 父目录 `/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp`：**空**。
- X05 corrected 任务的清理记录：CAP 1,196 KiB / 24 文件 → 196 KiB / 7 文件 → 删除；无 stray mock；详见 `t09-x05-repeat-001/corrected/logs/cleanup-size.txt`、`cleanup.log`。
- 系统盘约 **193 GiB** 可用。

## 提交与 trailer

- 近 3 个提交 `1dbebac`、`344b896`、`29e834a`：各恰 1 条 `Co-authored-by: Codex <codex@openai.com>`，位于末尾。
- `cbb3a91..1dbebac` 共 25 个提交；`36c5853`、`bf811b9`、`2c91060` 缺该 trailer，不重写历史，记录为规范缺口。

## 未执行项

- 构建、`cargo test`/`clippy`/`fmt`、GUI、Windows/macOS 全矩阵、DPI/英文/Retina/IME、X03 其它 Dialog/Shift+Tab 回归、X05 长时 soak：**本轮均未执行**（只读文档审计）。
- 无临时文件/清理字节可报告；历史测试结果未冒充本轮重跑。
