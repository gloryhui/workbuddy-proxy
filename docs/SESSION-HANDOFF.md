# 当前交接补充（2026-10-16，v0.5.6）

- 用户已确认发布 **v0.5.6**，授权提交并推送主仓库全部未提交代码和文件。原请求中的 v0.5.1 已正式发布，不覆盖旧标签或旧附件。
- 主仓库现包含 Anthropic 官方／Azure OpenAI 预设与认证、部署发现、Chat／Responses 转发；Azure 无法列出部署时提示手动输入，不回退到基础模型列表。管理协议为 **10**，与自动更新开发分支的协议 9 区分。
- 核心 186 项、GPUI 80 项测试及严格 Clippy／格式检查通过；macOS ARM64 release 测试 DMG 已通过实际 CUA 预设操作、保存重开、Mock JSON／SSE、协议 8／9 核心替换及包生命周期检查。报告目录为 `dist/provider-presets-qa-20261016`，属于本地忽略产物，不作为源码提交。
- 测试工具曾在关窗后无隔离环境重启复制的设置 helper，短暂加载真实配置并重写私有草稿；该进程已停止，真实保存配置的修改时间未变化，未覆盖还原真实草稿。详情在上述目录的 `reopen-validation.json`。
- 自动更新仍仅在 `codex/update-check-feedback` 分支，尚未合入本次发布；原开发 worktree 路径已不存在，分支及提交仍在。v0.5.3 自动更新测试包保留，等待用户反馈。
- 发布需以最终提交的 CI／三平台安装包结果和 GitHub Release 状态为准；本段记录准备状态，不代表发布已经完成。以下保留自动更新工作的历史交接。

---

# 会话交接记忆（2026-10-16）

## 新会话先读

本轮任务已完成：修复更新检测结果不可见，加入更新确认、稍后提醒、自动下载校验、替换重启和失败回滚；已交付本地 **v0.5.3 macOS ARM64 测试包**。目前等待用户测试反馈，没有新的发布授权。不要重新实现、重复全量测试、自动发布或直接合并 main。

本文件在主仓库和更新开发 worktree 各有一份；主仓库副本是便于新会话发现的未跟踪交接文件，不代表功能已经合入 main。此文档是历史上下文，后续用户的新指令优先。

## 工作位置与 Git 状态

- 主仓库：`/Volumes/1TB/Downloads/git/workbuddy-proxy`
- 更新开发 worktree：`/Users/glory/.codex/worktrees/upstream-protocols/workbuddy-proxy`
- 更新开发分支：`codex/update-check-feedback`
- 功能提交：`2d54cbb68853764d3197580c47ee8b232f0c5e40`，`feat(updater): add confirmed automatic updates with rollback`
- 分支基线 / main 当前提交：`21e6a05ed30ff36e08231c0b0000f525d50ba89a`。
- 更新功能仅在开发分支本地提交；没有推送、合入 main、发布或改写标签。
- **交接时主仓库 main 有大量其他未提交改动**，涉及供应商预设、协议选项、配置、代理、测试脚本和文档；还有未跟踪的 `scripts/api_key_providers_test.py`。来源未核实，不属于此次更新交接工作，不要 reset、覆盖、暂存或提交这些改动，不要在 dirty main 直接合并。
- 新会话必须先查看两个目录的 `git status` 和 `git worktree list`；上述状态可能继续变化。

## 用户目标与历史边界

用户要求：检测到更新弹窗显示“更新 / 稍后提醒”；点击更新后自动下载并替换当前程序。最后要求本地测试包版本为 v0.5.3，用于测试升级到现有正式 v0.5.5。

此前统计、价格快照、原生上游协议、GPUI 页面、WorkBuddy 同步及重启确认等功能已在 v0.5.5 正式发布，不要重新开展早期需求。README 截图已更新为两张，先供应商管理、后统计。

正式 v0.5.5：https://github.com/gloryhui/workbuddy-proxy/releases/tag/v0.5.5

正式标签指向 `9b1f74ca97eec52a701683bf6b3a7c9f5b1efc5a`。仓库 Cargo.toml 和 gpui/Cargo.toml 仍为 0.5.5；仅临时源码副本改为 0.5.3 来打测试包，没有回退正式源码版本或覆盖历史标签。

## 根因与已实现行为

旧 v0.5.3 实际已成功检测并缓存 v0.5.5，但 GPUI 成功状态不可见；也没有安装入口，启动自动检查未接通。已发布的 v0.5.5 同样没有本次新更新器。

新实现：打开设置后后台检查，手动检查跳过缓存；同一版本“稍后提醒”持久化 24 小时，手动检查仍可触发。更新弹窗不覆盖供应商编辑、同步等确认框。独立后台线程下载官方固定 GitHub Release，校验平台、资源名称、URL、大小和 SHA256；下载阶段保持代理运行。helper 就绪后才退出旧进程，等待锁释放再安装，保留配置和 SQLite；验证新版版本与 Ready，失败尝试恢复旧包并重启。Mac 不提权、不清除系统安全标记、不重启 WorkBuddy。控制协议 8 升到 9。

主要代码入口：

- `src/updater/{mod,dto,install}.rs`：下载、校验、进度、helper、安装、健康检查及回滚。
- `src/main.rs`：`--apply-update PLAN` helper 入口、控制协议 9。
- `src/proxy.rs`：`update_status / update_start / update_apply / update_ack` 本地鉴权管理接口。
- `gpui/src/update_feedback.rs`：检测结果严格解析、提醒、错误文本。
- `gpui/src/update_ui.rs`：后台工作线程、弹窗、进度、排队和提醒持久化。
- `docs/AUTO-UPDATE.md`：详细功能和验证边界，开发入口说明。
- `scripts/auto_update_macos_e2e.py`：隔离真实升级探针。
- `scripts/auto_update_macos_rollback.py`：隔离启动失败回滚探针。

Mac 通过直接执行 app 内核心二进制启动，保留独立测试 HOME；没有使用会丢测试环境的 LaunchServices open。

## 最终交付包

目录：`/Volumes/1TB/Downloads/git/workbuddy-proxy/dist/v0.5.3-auto-update-test`

安装包：`workbuddy-proxy-0.5.3-auto-update-test-macos-arm64.dmg`

约 16 MiB，核心、GPUI 与 bundle 版本均为 0.5.3。

SHA256：`f6646f61dd514039f12d49ba30bfd3d094e097ae651abb936286b25b5e773dc7`

旁边有 `.dmg.sha256` 和 `validation.json`、`package-validation.json`、`update-e2e.json`、`rollback-validation.json`、`reminder-validation.json`。交接时实际 SHA256 与校验文件相符，四项报告均 passed，构建 Rust 源文件与功能提交一致。

用户应先完全退出旧代理，再手动覆盖安装测试包一次，然后用检查更新验证升级。**目标是现有正式 v0.5.5，它不包含本次新更新器；升级后会回到该正式包原有的更新功能。** 将来若要持续使用新更新器，需要用户另行要求发布包含新提交的版本。

## 已验证，不需无故重复

- 核心 191 项测试通过，GPUI 82 项通过；两项目严格 Clippy `-D warnings` 和格式检查通过。
- 原生 GPUI 实际点击：更新/稍后提醒、24 小时提醒持久化、手动检查、下载替换重启。
- 隔离目录真实下载正式 v0.5.5，替换、重启、Ready 验证通过；配置和 SQLite 统计行保留。
- 构造版本校验通过但启动退出的新版，健康检查失败后恢复 0.5.3，通过；原程序字节、配置、统计行保留。
- 回滚错误中文提示、失败版本稍后提醒和重开不再弹窗通过。
- 最终 DMG 只读挂载、布局、签名、版本、协议兼容替换、打包应用生命周期通过。
- 真实 `/Applications/WorkBuddy Proxy.app` 未被替换，真实 WorkBuddy 未重启；测试使用独立 HOME/配置，不读取真实供应商密钥。
- Windows 安装代码存在，但没有 Windows 实机验证；不能把 Mac 测试当作 Windows 验收。

主要日志在 `/tmp/wbp-auto-update-*.log`，可能被系统清理；可长期依赖的摘要在 dist JSON。

## 清理和下一步

之前用户清理过全部项目构建缓存约 25.4 GiB。本次临时核心/GPUI target（约 6.1 GiB）与 v0.5.3 临时源码副本已再次删除；安装包、报告和开发源码保留。重新构建需要重新生成 target，不要误认为现成二进制还在。

下一步以用户测试反馈为准，在 `codex/update-check-feedback` 继续。尚未请求合并或发布；如之后请求合并，先审查并协调 main 的其他未提交开发，不能覆盖。若之后准备发布，检查 README“轻量实现”仍有“没有自动下载安装更新”的旧表述，与新增更新章节不一致；本次交接未修改它，发布文档应统一。

不用 dsh；未获明确要求不启动新 goal、不创建子代理、不向其他会话发送消息。Git 提交遵循 AGENTS.md，保留用户 Author/Committer，并只添加一次 `Co-authored-by: Codex <codex@openai.com>`。不 amend/rebase/force push。UI 操作使用 CUA，验证时继续使用隔离环境，不动真实应用或配置。
