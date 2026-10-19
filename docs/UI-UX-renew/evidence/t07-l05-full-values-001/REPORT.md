# T07-L05 同数据集逐值前后对照（请求日志每个可见/完整值）

**任务 ID：** UI-T07-L05-FULL-VALUES-001
**结论：** PASS —— 同一隔离合成数据集、同一配置/语言/尺寸下，T07 前后两个二进制对 16 个夹具行的 **224 个表格值** 逐值对照：214 PASS、7 项纯显示截断差异、3 项 OCR 误读（已目视复核为显示正确）、**0 项底层值变化**；**10 个代表行的详情 200 个字段全部 PASS**；产品源码零修改、未提交、未推送。
**平台：** 真实 macOS 桌面会话（macOS 27.0.1，Apple Silicon）。仅 zh-Hans / 1680×944 窗口实测；未验证 Windows，也不声称 Windows 结果。

## 1. 范围

- 仅做 L05：同一数据集对 T07 前后请求日志的每一个可见值（金额/币种、Token 总量与输入输出、缓存命中率与缓存读/写、耗时、速度、状态、时间、来源、点数）以及完整值入口（tooltip、详情）做逐值对照，区分**显示截断**与**底层值变化**。
- 未开始 T08 或其它任务；未提交、未推送、未 `git reset`；未修改产品源码、`IMPLEMENTATION_STATUS.md`、其它文档或主 checkout 内容。
- 全部使用隔离配置与合成数据；未读取或写入真实用户配置、历史库、Key，未发起任何上游请求。

## 2. 基线与保护内容

| 项 | 值 |
|---|---|
| worktree | `/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign` |
| 分支 | `codex/ui-redesign` |
| 起始 / 结束 HEAD | `f39096ea1575958622802c151c7197f106d59f92`（未变，实测核对见 `logs/baseline-and-metadata.txt`） |
| 对照基线（T07 前） | `796fcac86c4acc26d0b5fb51873b3a5de67414b5` |
| T07 改造提交 | `f5e69e719a9b597b1bf57a952373bcb24ffddcdf`（及后续 `192f038`、`f39096e`，仅文档） |
| 产品源码 diff | **空**（`git diff --name-only -- . ':(exclude)docs'` = 0 行） |
| 主 checkout 的 `gpui/src/usage_table.rs` | 未访问、未修改 |
| 未跟踪设计资料（PDF/PNG）与 `IMPLEMENTATION_STATUS.md` 既有修改 | 未触碰（最终 `git status` 仅这些既有条目 + 本任务新目录） |
| 本任务新增 | 仅 `docs/UI-UX-renew/evidence/t07-l05-full-values-001/` |

`gpui/target/`、`target/` 全程复用，未 `cargo clean`、未新建 Cargo target、未清理既有缓存（详见 §10）。

## 3. 环境与构建

- 工具链：`RUSTUP_TOOLCHAIN=stable` → `rustc 1.99.1 (58cf8f9c3 2026-10-13)`；默认 1.88.0 未使用（沿用前序任务结论）。
- `TMPDIR`/`TMP`/`TEMP` 全部指向本任务专用目录：`/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/t07-l05-full-values-001`（`/Volumes/1TB` 未挂载，此为机器本地映射）。
- 命令与退出码（`logs/build-pre-t07.log`、`logs/build-current-restore.log`）：

| 用途 | 命令 | 退出码 | 耗时 |
|---|---|---|---|
| 构建 T07 前 gpui | `git archive 796fcac… \| tar -x -C <tmp>/src-pre-t07` 后 `cargo build --release --manifest-path gpui/Cargo.toml --target-dir <worktree>/gpui/target` | `PRE_BUILD_EXIT=0` | 1m06s |
| 恢复当前 HEAD gpui（同一 target） | 同命令，源为 worktree（先 `touch gpui/**/*.rs` 使 fingerprint 失效） | `CURRENT_BUILD_EXIT=0` | 2m13s |
| core（未重建，复用 19:01 产物） | `target/release/workbuddy-proxy` | — | — |

> 说明：两个版本共用 worktree 的 `gpui/target`，构建旧版后已重建当前版恢复；当前产物 SHA256 与构建前备份**完全一致**（`efcc0a1f…`，见 §4），可证明共享缓存被正确恢复、无持久污染。`touch` 只改 mtime、不改内容（`usage_table.rs` SHA256 与 `git show HEAD:` 一致）。

被验收的二进制（`logs/baseline-and-metadata.txt`）：

| 二进制 | 版本 | sha256 | 大小 |
|---|---|---|---|
| T07 前 gpui（`796fcac` 构建） | 0.5.6 | `05fa35297b3e152f47d66e1ceab44b4d11688c4c84ac0d83ca6f470ea7a454c1` | 14,616,256 B |
| 当前 gpui（`f39096e` 构建） | 0.5.6 | `efcc0a1f44491581d8867b34b1be2fac6f2d01e78df808f8571b7054fb5ed49c` | 14,633,424 B |
| core（两个 GUI 共用） | 0.5.6 | `22cacc663af1293252611001e589b6bc49cde885234e42a5e6a200b53697d4ca` | 8,379,136 B |

源文件 SHA256：T07 前 `usage_table.rs` = `03d7c794…`（与 `git show 796fcac:gpui/src/usage_table.rs` 一致）；当前 = `62e6c6ea…`（与 `git show HEAD:` 一致）。

## 4. 隔离夹具与合成数据集

`fixture.py setup` 在任务 TMPDIR 下新建一次性 config，启动当前 HEAD core（`bin/core-current --headless --config-dir … --port <随机>`），向该 core 自建 SQLite 插入 **16 行**合成 `usage_requests`（14 行 `record_origin=proxy`、1 行 `workbuddy_builtin`、1 行 `workbuddy_custom`），时间全部落在本机当天。

夹具行矩阵（`expected.json`）：

| # | 用例 | 来源 | 状态 | 用量 | Token（in/out/total） | 缓存（读/写） | 耗时/首内容 | 费用状态 | 币种 | 点数 | 其它 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 完整成功·默认零价 | 代理 | 完成 | 完整 | 1200/300/1500 | 0/0 | 500/200 | 未配置价格 | CNY | — | 命中率 0.0% |
| 2 | 有价格·流式 | 代理 | 完成 | 完整 | 1000/500/1500 | 600/0 | 2500/300 | priced | CNY | — | 命中率 60.0%、tok/s 227.3 |
| 3 | 有价格·USD·满命中 | 代理 | 完成 | 完整 | 800/200/1000 | 800/0 | 1000/300 | priced | USD | — | 命中率 100.0% |
| 4 | 极小金额·缓存写 | 代理 | 完成 | 完整 | 10/5/15 | 5/5 | 100/50 | priced ¥0.00005 | CNY | — | `input_breakdown=disjoint` |
| 5 | 零用量 | 代理 | 完成 | 完整 | 0/0/0 | 0/0 | 900/200 | 未配置价格 | CNY | — | 命中率 — |
| 6 | 部分用量 | 代理 | 完成 | 部分 | 1000/—/— | —/— | 1200/400 | 用量不全 | CNY | — | 速度 — |
| 7 | 失败·未定价 | 代理 | 失败 | 缺失 | —/—/— | —/— | 800/300 | 无 cost 行 | — | — | HTTP 429 |
| 8 | 未发上游 | 代理 | 失败 | 不适用 | —/—/— | —/— | 300/100 | 未发上游 | CNY | — | HTTP —、`upstream_started=0` |
| 9 | 进行中 | 代理 | 进行中 | 缺失 | —/—/— | —/— | —/— | 无 cost 行 | — | — | 耗时/速度/状态均未知 |
| 10 | 已取消 | 代理 | 已取消 | 无效 | —/—/— | —/— | 700/300 | 缓存计费数据不足 | CNY | — | HTTP 499 |
| 11 | 内置导入 | 内置 | 已记录 | 完整 | 2000/1000/3000 | 500/0 | —/— | 未定价 | — | 1.14 | HTTP — |
| 12 | 自定义导入 | 自定义 | 已记录 | 部分 | 300/—/— | —/— | —/— | 未定价 | — | 0 | 流式 — |
| 13 | 大数紧凑 | 代理 | 完成 | 完整 | 123456789/98765432/222222221 | 61728394/0 | 5000/1000 | priced ¥2.22 | CNY | — | 1.23亿/9877万/2.22亿 |
| 14 | 中断 | 代理 | 中断 | 部分 | 500/100/600 | 0/0 | 160/80 | 用量不全 | CNY | — | HTTP 500 |
| 15 | 当日最早·低于显示下限 | 代理 | 完成 | 完整 | 42/8/50 | 0/0 | 1000/500 | priced `<0.0001` | CNY | — | HTTP 201 |
| 16 | 历史金额·超长名称 | 代理 | 完成 | 完整 | 999/1/1000 | 500/499 | 2000/300 | 历史金额（来源不明） | CNY | — | 供应商/模型名 60+ 字符 |

后端交叉证据（同一数据集、与表格相同的 filter，逐视图经 core API 采样，`expected.json.views`）：

| 视图（来源筛选） | source_scope | capture_method | API 返回 | 覆盖行 |
|---|---|---|---|---|
| 全部来源（默认） | `all` | `proxy` | 14 | 1–10、13–16 |
| 内置模型 | `builtin` | `all` | 1 | 11 |
| 自定义模型 | `supplier` | `all` | 15 | 1–10、12–16 |

三个视图合计覆盖全部 16 行；GUI 也按完全相同的三视图顺序采集（§5）。

## 5. 采集与解析方法（真实 GUI，不是模拟）

- 驱动：`capture.py` 依次启动两个二进制，真实点击/滚动/AXPress（Accessibility API），窗口请求 1920×1050、系统实际给到 `[0,30] 1680×944`（AX 限制，两版相同）。
- 三视图 + 每视图 6 屏滚动（步长 250px）共 18 屏/版本，另加 10 个详情对话框与 3 个 tooltip 悬停；`supplement.py` 补齐 current 版首轮未捕获的一项详情；`tooltip_capture.py` 用几何锚点对两版同一行同一列补采 tooltip。
- 逐值提取：`analyze.py` 对每张窗口 PNG 按表头标签推出列区间，**以时间列自身 OCR 文本锚定每行**（截图是窗口坐标而 AX 帧是屏幕坐标，时间文本是唯一不会漂移的锚点），逐 (行 × 列) 裁剪、放大 3× 后用同一 Vision OCR 识别；行身份由时间文本反查夹具，识别失败时按时间单调性在相邻行间插值。
- 判定：`compare.py` 把复合单元（Token 格、费用+点数格、供应商+来源格、状态格）解析成单值，与 `fixture.py` 独立复刻的显示规则（`compact`/`percent`/`amount`/`duration`/`rate_text`/`status_text`/`cost_label`…）逐值比对，输出 `PASS | DISPLAY_TRUNCATED | ONE_SIDE_UNREAD | OCR_LIMITED | VALUE_CHANGED`。
- 详情对照不依赖 OCR：详情卡片每个字段都以 `aria_label("名称：值")` 暴露在 AX 树，直接精确读取后与数据库期望比对。
- 关键截断/误读案例另存放大裁剪图到 `verify/` 并由人眼复核（§7.3）。

**T07 前 vs 当前的结构差异（预期，非缺陷）**：旧版 11 列（时间/来源/供应商/模型/请求标识/Token·缓存命中率/估算费用/耗时/tok\/s/状态/点数）、行高 30px；新版 9 列（来源并入供应商单元格 pill、点数并入估算费用单元格第二行、Token 改两行）、行高 56px。因此"值出现在哪一列"发生变化属设计变更，本报告比较的是**值本身**。

## 6. 逐值对照结果（表格）

机器可读结果：`row-by-row-compare.json`（含每个值的两版原始 OCR 文本与匹配级别）、`row-by-row-compare.csv`。

**总计 16 行 × 14 值 = 224 项**：PASS 214、DISPLAY_TRUNCATED 7、ONE_SIDE_UNREAD 2、OCR_LIMITED 1、**VALUE_CHANGED 0**。

| 值 | PASS | 非 PASS | 说明 |
|---|---|---|---|
| 时间 | 16 | — | 全部 `MM-DD HH:MM:SS`，与 DB 期望逐秒一致 |
| 来源 | 16 | — | 代理/内置/自定义 三态一致（新版为供应商格内 pill） |
| 供应商 | 13 | 3 截断 | 见 §7.1 |
| 模型 | 12 | 2 误读 + 1 截断 + 1 误读 | 见 §7.2、§7.3 |
| Token 总量 | 16 | — | 含 `1.23亿`、`9877万`、`2.22亿` 紧凑格式与 `—` |
| 输入 Token | 16 | — | |
| 输出 Token | 16 | — | 含 `—`（缺失）、`0`（零用量） |
| 缓存读 | 16 | — | 含 0、600、61728394→`6173万`、`—` |
| 缓存命中率 | 15 | 1 截断 | 见 §7.1 |
| 金额/币种 | 15 | 1 截断 | `¥0`、`¥1.2345`、`$0.12345`、`¥0.00005`、`¥<0.0001`、`¥2.22`、`未定价`、`用量不全`、`缓存计费数据不足`、`历史金额（来源不明）`、`未发上游` 逐项一致；1 项截断见 §7.1 |
| 点数 | 16 | — | `—`、`1.14`、`0` 一致（新版在费用格第二行） |
| 耗时 | 16 | — | `500 ms`/`2.50 s`/`1.00 s`/`—` 一致 |
| 速度 tok/s | 15 | 1 截断 | `600.0`、`227.3`、`0.6`、`19753086.4`（两版同样截断）、`—` |
| 状态 | 16 | — | `HTTP · 完成/失败/中断/已取消/进行中/已记录` 与 `— · …` 全部一致 |

**关键点：唯一两类跨版本差异都是"显示截断程度不同"，没有任何一行出现底层值不同。** 唯一 `VALUE_CHANGED` 判定数为 0；2 项 `ONE_SIDE_UNREAD` 与 1 项 `OCR_LIMITED` 经放大目视确认为 OCR 误读（§7.3），两版实际显示与期望一致。

## 7. 差异明细（显示截断 vs 底层值变化）

### 7.1 显示截断差异（7 项，底层值相同）

| 行 | 值 | 期望（DB） | T07 前可见 | 当前可见 | 判定 |
|---|---|---|---|---|---|
| 11 | 供应商 | `WorkBuddy 内置` | `WorkBudd…` | `WorkBuddy内置` | 新版列宽 100→148px，截断更少 |
| 12 | 供应商 | `WorkBuddy 自定义` | `WorkBudd…` | `WorkBuddy自定义` | 同上 |
| 16 | 供应商 | `供应商名称超长测试-ABCDEFGHIJKLMNOPQRSTUVWXYZ-0123456789` | `供应商名称…` | `供应商名称超长测试…` | 同上，目视复核见 `verify/pre-t07-10-all-sources-00-00-供应商.png`、`verify/current-10-all-sources-00-00-供应商.png` |
| 16 | 模型 | `model-extremely-long-name-for-truncation-check-0123456789` | `model-extremely-lon…` | `model-extremely-long…` | 列宽 180→260px |
| 16 | 金额 | `历史金额（来源不明）` | `历史金额（来源不…` | `历史金额（来源不明）` | 费用列 110→132px；目视见 `verify/…估算费用.png` |
| 13 | 缓存命中率 | `50.0%` | `50.(`（旧版 Token 格单行 5 字段溢出被裁） | `50.0%` | 新版 Token 改两行后完整；目视见 `verify/current-…-Token_缓存命中.png` |
| 13 | 速度 | `19753086.4` | `19753…` | `19753…` | **两版同样截断**（tok\/s 列 60/66px 不足），属既有显示限制 |

另有第 13 行旧版 Token 格整格可见文本 `输入1.23亿｜输出9877万｜缓存读6173万｜总 Token 2.22亿｜命中率50.(`：前 4 个值可读、第 5 个被列宽裁掉；新版两行完整显示 `总 Token 2.22亿 / 输入 1.23亿 / 输出 9877万 / 命中率 50.0% / 缓存读 6173万`（含命中率进度条）。

### 7.2 缓存写（两端一致的"不在表格显示"）

两版表格与 tooltip 均不显示缓存写（源码 `token_cell` 只渲染 输入/输出/缓存读/总Token/命中率）；缓存写只在**详情**的 `Token：↓… ↑… R… W… Σ…` 中出现。第 4、16 行 `cache_write = 5 / 499` 在两版详情中均逐值一致 → 一致，非变化。

### 7.3 OCR 误读（3 项，已目视复核为显示正确）

| 行 | 值 | 期望 | OCR 读数 | 目视结论 |
|---|---|---|---|---|
| 1 | 模型 | `model-01` | 旧版 `LO-lapow` | 裁剪图清晰显示 `model-01`（`verify/pre-t07-10-all-sources-01-11-模型.png`） |
| 4 | 模型 | `model-04` | 两版 `to-lapow` | 同一 Vision 系统性误读，非产品差异 |
| 9 | 模型 | `model-09` | 当前 `60-lapow` | 同上 |

请求标识列（`t07l05-0001-…`）OCR 普遍不可靠（灰字 + 省略号，读数如 `t07105-00..`），因此该值不在表格判定中使用，改由详情 AX 的精确值验证（§8）。

## 8. 详情对照（10 个代表行 × 20 字段 = 200/200 PASS）

对"完整成功（有/无价格）、零用量、部分用量、失败/未定价、进行中、已取消、导入点数（内置/自定义）、历史金额/超长名称"各选代表行打开真实详情对话框，直接读 AX 的 `名称：值`（无 OCR 误差），与数据库期望逐字段比对：**10/10 行、200/200 字段全部 PASS**（`detail-compare.json`、`detail-compare.csv`）。

覆盖字段：来源、采集方式、时间依据、总量口径、WorkBuddy 点数、流式响应、请求标识、时间、供应商/路由、请求模型、上游模型、上游协议、响应声明模型、状态（`HTTP … · …`）、用量（`完整/部分/缺失/无效/不适用 · 原因`）、Token（`↓… ↑… R… W… Σ…`，含缓存写与未知名 `—`）、性能（总/首内容/TTFB）、缓存命中率、tok/s（含长提示）、估算费用（`CNY ¥…` / 状态文案）。

两版详情字段集合一致（`detail-compare.json` 的 `extra_pre`/`extra_current` 为空）。

## 9. 格式、精度、时区、未知值、Tooltip 一致性

| 检查项 | 结果 |
|---|---|
| 时间格式/时区 | 两版均 `MM-DD HH:MM:SS` 本机本地时间；机器时区 `Asia/Shanghai`（`/etc/localtime → /var/db/timezone/zoneinfo/Asia/Shanghai`，`date` = `CST +0800`）。16 行时间与 DB 期望逐秒相等，含跨日最早 00:00:05 |
| 金额精度 | 逐值一致：`¥1.2345`、`$0.12345`、`¥0.00005`、`¥<0.0001`（低于显示下限）、`¥0`、`¥2.22`、`¥2200` 等；两版金额格式规则同源（`format_amount_text`） |
| Token 紧凑精度 | `1.23亿/9877万/2.22亿/6173万`、`1500`、`0` 逐值一致 |
| 百分比/速率精度 | 命中率与 tok\/s 均 1 位小数，逐值一致（`0.0%`…`100.0%`、`600.0`、`227.3`、`19753…`） |
| 未知值 | `—` 一致：`未定价`、`用量不全`、`进行中` 行的 Token/耗时/速度/状态、导入行 HTTP、无点数行 `—` |
| Tooltip 完整值入口 | 两版 token tooltip 都给出完整 `输入/输出/缓存读/总 Token/命中率`（同一行 R16：`输入 999 / 输出 1 / 缓存读 500 / 总 Token 1000 / 命中率 50.1%`）；费用 tooltip 含 `点数 <值>` 与"点数来自日志记录值"说明；`tooltip-compare.json` |
| 详情完整入口 | 见 §8，两版一致 |

## 10. 临时路径归属、清理与空间

| 路径 | 归属 | 处理 |
|---|---|---|
| `…/tmp/t07-l05-full-values-001/src-pre-t07`（22 MiB） | 本任务：`git archive 796fcac` 导出树，用于构建 T07 前二进制 | 停止全部进程后删除（可复现：`git archive 796fcac \| tar -x`） |
| `…/tmp/t07-l05-full-values-001/crops`（46 MiB） | 本任务：逐单元格裁剪图缓存 | 删除（关键案例已复制到证据 `verify/`） |
| `…/tmp/t07-l05-full-values-001/bin`（36 MiB） | 本任务：两个 GPUI 二进制副本 + core 副本 + `axdrive`/`ocr` 工具 | **保留**，供 Codex 复核复跑（删除会失去逐值复现能力） |
| `…/tmp/t07-l05-full-values-001/run`（9.0 MiB） | 本任务：隔离 config、合成 `usage.sqlite3`、core/gpui 运行日志 | **保留**（隔离数据，供复核；含一次性 local_key，非真实凭据） |
| `…/tmp/t07-l05-full-values-001/{logs,ref,tmp}`（<1 MiB） | 本任务：构建/运行日志、前后源码副本、TMP 暂存 | 保留；`tmp/` 为空 |
| 证据目录（27 MiB） | 本任务交付物 | 保留 |
| 其他会话/用户文件、`target/`、`gpui/target/`、主 checkout | — | 未触碰、未清理 |

- 停止的进程：core `66120`（控制接口 `/_control/stop` → `OK`）、两个 GPUI（每个采集脚本 `finally` 内 SIGTERM，退出码 -15）。清理后 `ps` 无本任务进程。
- 释放：约 **68 MiB**（`src-pre-t07` 22 MiB + `crops` 46 MiB）；任务临时目录 113 MiB → **45 MiB**。
- 系统盘：清理前后均 **200 GiB 可用**（`/dev/disk3s3s1`，460 GiB，已用 13 GiB）——本轮占用未影响系统盘水位。
- `/tmp` 与系统临时目录：无本任务遗留（已核查）。
- 完整记录：`logs/cleanup.txt`。

## 11. GUI 实测范围与不能证明的项

**实测（真实窗口、真实二进制）**：用量明细三来源视图 × 6 屏滚动、10 个详情对话框、3 组 tooltip 悬停，共 41 张（T07 前）/ 40 张（当前）窗口 PNG + 对应 AX 树；所有判定基于这些真实像素与 AX 输出。

**不能证明（明确不在本轮范围）**：
- Windows 及非 macOS 平台；非 zh-Hans 语言；1680×944 以外的窗口尺寸与缩放比例（含 Retina 高 DPI 差异）。
- 列宽用户拖拽后的截断行为、快速滚动/并发切换中的中间态、网络/数据库异常路径。
- 像素级颜色、字号、间距的逐像素比对（仅做文本值与结构对照）；表格单元格文本未通过 AX 暴露，故表格值依赖 OCR + 目视，存在 §7.3 类误读风险。
- tooltip 的显示时序/位置（只验证其内容与存在性）；tooltip OCR 会与底层表格文本混读，解析取关键词行。

## 12. 缺陷判定

**未发现 L05 范围内的产品缺陷，未修改任何产品源码**（非 docs 路径 `git diff` 为空）。T07 前后所有底层值一致；7 项跨版本差异全部可归因为"列宽/行高与单元格布局变化导致的显示截断程度不同"，其中 1 项（tok\/s `19753…`）两版同样截断，属既有显示限制而非本轮回归。若后续需要"长名称/大数值不被截断"，应作为独立设计项（列宽或 tooltip 可读性）由 Codex 决定，本轮只留证据，未改源码。

## 13. 无真实数据与无外网声明

- 未连接或读取任何真实用户配置、历史统计库、Key 或上游；全部数据由 `fixture.py` 在一次性隔离目录内合成，`local_key` 为任务内固定假值。
- 全程无外网请求：core 以 `--headless` 在 `127.0.0.1` 随机端口监听，仅本机 `/_control/action` 与 `stats_*` 调用；GUI 只连本机 core。

## 14. 证据清单（本目录）

| 文件 | 内容 |
|---|---|
| `REPORT.md` | 本报告 |
| `row-by-row-compare.json` / `.csv` | **逐行逐值前后对照机器可读结果**（224 表格值，含两版原始 OCR 文本、匹配级别、判定） |
| `detail-compare.json` / `.csv` | 10 个代表行 × 20 详情字段对照（AX 精确值） |
| `tooltip-compare.json` | token/费用 tooltip 完整值入口对照 |
| `expected.json` | 合成夹具 16 行 × 三视图期望值 + core API 三视图采样 |
| `observed-pre-t07.json` / `observed-current.json` | 逐 (行 × 列) 的 OCR 观测与列布局 |
| `cells-pre-t07.json` / `cells-current.json` | 每屏每行每列的原始单元格文本（人工复核用） |
| `compare-summary.json` | 采集覆盖与视图清单 |
| `fixture.py` | 隔离配置/合成数据/期望与 API 采样脚本（setup/status/stop/cleanup） |
| `regen_expected.py` | 仅重读同一 DB 与 core 重写 `expected.json`（不触碰数据） |
| `capture.py` | 两版固定 GUI 序列采集（三视图 × 6 屏 + 详情 + tooltip） |
| `supplement.py` / `tooltip_capture.py` | 补齐 current 版一项详情；按几何锚点补采两版 tooltip |
| `analyze.py` | 表头推列区间 + 时间锚定行 + 逐单元格裁剪 OCR |
| `compare.py` | 复合单元解析、显示规则复刻、逐值判定 |
| `tools/axdrive.swift`、`tools/ocr.swift` | AXPress/截图/AX dump（含 resize/move/escape）与 Vision OCR 源码 |
| `raw/pre-t07/`、`raw/current/` | 每版约 40 张窗口 PNG（`shots/`）+ 对应 OCR JSON（`ocr/`）+ AX 树（`ax/`）+ `capture.json` + 运行日志 |
| `verify/*.png` | 关键截断/误读案例的放大裁剪图（人工复核证据） |
| `logs/` | 基线元数据、构建日志、采集/分析/对照日志、清理记录 |

复现路径（供 Codex 独立复核）：`fixture.py setup` →（如需旧版）`git archive 796fcac | tar -x` 后按 §3 命令共享 `gpui/target` 构建 → `capture.py <bin> <label>` → `analyze.py` → `compare.py`。
