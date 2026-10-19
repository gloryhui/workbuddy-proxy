# T08 成本定价、采集存储与价格弹窗（含 Codex 复核修复）

**任务 ID：** UI-T08-RESIDUAL-003（修复轮 TMP：t08-residual-004）
**结论：** **PASS（R01—R06；本轮四项复核修复均完成）**。修复了定价表“来源/映射”丢失 label、分页总量误导文案、1100 工具栏可达性（已验证无裁切）、以及 R03/R04 输入未生效的补证（四个价格框可输入，R03 保存非零 output，R04 修改价格/备注并核对快照）。产品源码仅改 `gpui/src/statistics.rs`，未改根 `src/`、数据库、计价/快照、设置 revision 或危险操作语义；未提交、未推送。
**平台：** macOS 27.0.1，Apple M1，1920×1080 非 Retina；中文；窗口 1440×900 与 1100×700。未测 Windows、英文、其它 DPI。

## 1. 范围与基线

| 项 | 值 |
|---|---|
| worktree / 分支 | `/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign` / `codex/ui-redesign` |
| HEAD | `7da26855805aa7e0877fda149b8a576efcade27c`（与任务给定一致） |
| 产品源码改动 | `gpui/src/statistics.rs`（唯一文件，保留上一轮未提交改动） |
| 保护内容 | `IMPLEMENTATION_STATUS.md`、未跟踪设计 PDF/PNG、根 `src/`、主 checkout 均未触碰 |
| 修复证据 | `docs/UI-UX-renew/evidence/t08-residual-003/`（新增 `*-004` 文件与 `api-verification-004.json`） |

## 2. 实现摘要（含复核修复）

1. **成本定价卡**：设计表格（模型/规则 ID、来源/映射、币种 tag、四项价格摘要 + 计价模式、编辑/删除）；工具栏为搜索框 + 新建/刷新/手动更新 + 右侧更新摘要；底部 Pager。所有原控件 id、回调与禁用逻辑保留。
2. **来源/映射列（修复）**：第一行显示 `PriceRuleDto.label`（现有来源语义，如 `models.dev · opencode/...`）；label 为空时显示 **“手工 / Manual”**，不再留空；第二行显示 `configured_providers` 中的**可读供应商名称** + 上游模型映射（找不到名称时只显示模型）。未新增/伪造后端字段。证据：`1440-pricing-labels-004.png`、`1100-toolbar-004.png`（同屏含 `models.dev` 与 `manual`）。
3. **分页文案（修复）**：无后端总数时改为准确的 **“第 N 页 · M 条” / “Page N · M rows”**（M=当前页条数），不再写“共 M 条”。证据：`1440-page2-004.png`、`1100-toolbar-004.png`（第 2 页 · 5 条）。
4. **工具栏可达性（修复）**：工具栏改为 `flex_wrap` 两组（搜索+三按钮；更新摘要），窄窗自动换行而不裁切。1100×700 实测：搜索框 `[310,216 240x26]`、新建 `[556,219 94x19]`、刷新 `[656,219 74x19]`、手动更新 `[736,219 82x19]`、右侧摘要完整显示到 x≈1060，全部在 1100×700 视口内（`logs/ax-1100b-004.txt`、`1100-toolbar-004.png`）。
5. **采集与存储卡**：两列（默认币种/摘要采集/保留天数；供应商采集模式表 + 刷新 + Pager），<980 px 堆叠；危险操作底部分隔行。保留全部 id、revision、禁用逻辑。
6. **价格弹窗**：计价方式/币种 60/40；四个价格输入带当前币种符号前缀（切换币种时同时更新）；保存按钮对勾；保留身份锁定、错误行、Enter=保存、成功才关闭。

## 3. 隔离环境与合成数据（修复轮）

- 新 TMP：`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t08-residual-004`（TMPDIR/TMP/TEMP 全部指向）。
- `fixture.py setup`：一次性 config + 当前 core + 本地价格目录 mock（仅 127.0.0.1）；25 供应商、20 行统计、25 条价格规则；mock 支持 ok/slow/fail。
- 修复轮通过真实 GUI paste 输入验证四个价格与备注；未读取真实配置/Key/历史库，未访问上游。

## 4. R01—R06 结果

| ID | 结果 | 证据与说明 |
|---|---|---|
| R01 | **PASS** | 折叠/展开（`1440-settings-open.png`、`1440-pricing-open.png`、`1440-storage-settings.png`）；搜索 `model-05`（旧轮 `1440-r01-search-model05.png`）；刷新（`1440-r01-refresh.png`）；价格分页第 2 页新文案 **“第 2 页 · 5 条”**（`1440-page2-004.png`）；供应商模式第 1/2 页；1100 工具栏全部可达（`1100-toolbar-004.png`）。 |
| R02 | **PASS（更新中/成功）**；失败恢复 **PASS** | slow → “正在更新价格…”（`1440-r02-updating.png`）→ 成功恢复并显示更新摘要（`1440-r02-success.png`）；fail → 按钮恢复、健康行“数据更新失败”（`1440-r02-fail-error.png`）。 |
| R03 | **PASS（输入补证）** | 非法 `abc` → 弹窗保留并报 `input: amount must be a plain decimal number`（`1440-r03-invalid.png`）；取消不落库；本轮新建 `r03-input-004`，通过粘贴填入四价 **0.7 / 0.8 / 0.09 / 0.10** 与备注 `r03-paste`（`1100-r03-input-filled-004.png`），切换 USD 后四个 `$` 前缀同步（`1100-r03-usd-prefix-004.png`），保存成功（`1100-r03-saved-004.png`）。API：`input=700000000, output=800000000, read=90000000, write=100000000, currency=USD`，**output 非零**。 |
| R04 | **PASS（输入补证）** | 编辑 `model-22`：修改输入价 0.32→**0.35**、备注 manual→**r04-edited**（`1100-r04-edited-004.png`），保存后 API 返回 `input_rate_nanos=350000000, label=r04-edited, revision=2`（`1100-r04-saved-004.png`）；身份字段锁定；切 CNY 后四个 `¥` 前缀同步（`1100-r04-cny-prefix-004.png`）。历史 20 条 `usage_costs` 保存前后均为 `default_zero/0`，快照未重算（`api-verification-004.json`）。 |
| R05 | **PASS**（沿用上一轮证据） | 默认币种 USD、保留 365 天、摘要采集关闭、Provider B 模式 observe，API revision 4（`1440-r05-settings.png`、`1100-settings-narrow.png`、`api-verification.json`）。本轮新库未重跑 R05；旧证据保留。 |
| R06 | **PASS（预览/确认/完成）**；失败路径 NOT RUN | 预览显示 20 行且确认前 DB 仍 20；确认后 DB 0（`1440-r06-preview.png`、`1440-r06-done.png`）。失败（token 过期/维护错误）未模拟。 |

## 5. 历史快照与设置语义

- 价格保存只改当前规则；`usage_costs` 历史行未被重算（R04 修复轮前后均 `default_zero/0`）。
- 设置仍经 `stats_settings_update`（expected_revision）；供应商模式经 `stats_provider_settings_update`；清除历史仍走 preview→token→maintenance，仅隔离库执行。

## 6. 限制与失败尝试

- **前台激活**：未用 `System Events` 置前时折叠/滚动/点击无效；所有 PASS 均在激活后采集。
- **输入方式**：原生 CGEvent 逐字符 `type` 在价格/备注输入框焦点不稳（上一轮 output=0 的原因）；本轮改用**点击输入框 + ⌘A + 剪贴板 ⌘V**，四个价格与备注均成功写入。该限制只影响自动化手法，不影响产品；未验证项不再标 PASS。
- **OCR**：表头/页文本偶发误读；结论以截图 + AX frame + API/DB 三方对照。
- 未测：Windows、macOS 英文、Retina/非 100% 缩放；R02 失败详情（仅健康行）、R06 失败路径；新建规则供应商下拉的纯键盘可达性。

## 7. 构建与检查

| 检查 | 结果 | 日志 |
|---|---|---|
| `cargo check --manifest-path gpui/Cargo.toml --locked`（修复轮） | PASS（exit 0） | `logs/check-004.log` |
| `cargo fmt --check`（修复轮） | PASS（exit 0） | `logs/fmt-004.log` |
| `cargo test`（上一轮 138 passed；本轮仅展示层小改，未重跑） | PASS（上轮） | `logs/test.log` |
| `cargo clippy --all-targets -- -D warnings`（上一轮） | PASS（上轮） | `logs/clippy.log` |
| `cargo build --release`（修复轮） | PASS | `logs/build-gpui-004.log` |

## 8. 证据索引（修复轮新增）

- `shots/1440-pricing-labels-004.png`：来源/映射显示 label（models.dev/manual）
- `shots/1440-page2-004.png`：第 2 页“第 2 页 · 5 条”
- `shots/1100-toolbar-004.png`：1100 工具栏全可见 + 第 2 页 + manual/models.dev 同屏
- `shots/1100-r03-input-filled-004.png`、`1100-r03-usd-prefix-004.png`、`1100-r03-saved-004.png`
- `shots/1100-r04-edited-004.png`、`1100-r04-saved-004.png`、`1100-r04-cny-prefix-004.png`
- `api-verification-004.json`、`logs/check-004.log`、`logs/fmt-004.log`、`logs/build-gpui-004.log`
- 旧轮证据（R01/R02/R05/R06、Settings 对照）继续保留。

## 9. 清理

见 `logs/cleanup-004.txt`：停止 GUI/core/mock 并等待退出，核查 `ps`/lsof 后只删除本任务 TMP 004；检查 `/tmp`；共享 `target/`、`gpui/target/` 与 L05 保留目录未清理。
