# T09 双平台验收矩阵、交互回归与问题闭环

**任务：** UI-T09-CROSS-PLATFORM
**HEAD：** `6533bf1605ad4ab395d896d5cc215142d96b3a99`（`codex/ui-redesign`）
**结论：** **BLOCKED**。Windows x64 在本机不存在（无实机、无 VM、无远程、无 Wine），因此第 5.1 节主矩阵的 Windows 行、X01 全量、X02 Windows DPI 均无法完成。macOS 本机只完成可行子集：SC01 三档窗口中文、X04 生命周期 PASS、X03/X05/X06 PARTIAL；SC02—SC07、SC11、SC12 因缺少安全夹具/数据未跑。**本报告不把 macOS 单平台结果称为双平台通过，也不把复用旧截图当作本轮重跑。**

## 1. 环境盘点

| 项 | 结果 |
|---|---|
| OS / CPU | macOS 27.0.1（26A434），Apple M1，arm64，8 核 |
| 显示 | 1920×1080 @75Hz，**非 Retina，scale=1.0**；无其它显示器/缩放环境 |
| Windows | **不可用**：`/Applications` 无 Parallels/VMware/VirtualBox/UTM/Wine；无远程 Windows 目标 |
| `/Volumes/1TB` | **未挂载**（仅 Macintosh HD、Recovery）；`df` 系统盘 195 GiB 可用 |
| Rust / Python | rustc 1.99.1；Python 3.14.6（仅用于只读盘点/副本数据调整，无构建） |
| 当前二进制 | GPUI sha256 `89049d01…`（14,649,920 B，01:02，源码=HEAD）；core sha256 `22cacc66…`（与 L05 core 副本一致） |
| 运行隔离 | L05 夹具**副本**（`evidence/t09-cross-platform/runtime/`）；未触碰 L05 原始保留目录；结束后删除副本 |

## 2. 本轮实际执行（macOS，当前 HEAD 二进制）

1. 复制 L05 隔离 config/DB 到 T09 runtime；启动隔离 core（端口 62044）与当前 GPUI；未读取真实配置/Key/历史。
2. 将副本 16 行请求时间平移到当天（保留行/成本语义），刷新统计，得到有数据的 SC01。
3. 三档窗口截图并核对 PNG 物理尺寸：1100×700、1280×800、1440×900；窗口 bounds 由 `axdrive windows` 记录。
4. X04：AXMinimized true/false 最小化/恢复；Cmd+Q 关闭（GPUI 退出、core 存活）；重开（pid 65419、wid 9751）；第二实例写 `gpui-wake.json` 后退出、窗口数 1。
5. X03：点击日志行打开用量详情 → Escape 关闭；发送 Tab 三次（无可见变化，标记未验证）；剪贴板粘贴中文 `测试中文`（AX value 确认）。
6. X05：详情开关 5 次、供应商统计/模型统计切换、滚动；进程 RSS 136 MB、窗口数 1、无崩溃。
7. X06：静态核对 `workbuddy_restart.rs`、`ui.rs:2960–2977`、`theme.rs`、标题栏 D04；记录自动重启仅 macOS、非 macOS 禁用并提示、Windows 字体候选、标题栏差异。
8. 证据：`MATRIX.md`、`shots/`、`logs/`；截图尺寸/哈希见 `logs/shot-sizes.txt`。

## 3. 结果摘要

| 项 | 结果 |
|---|---|
| X01 主矩阵 | **BLOCKED**：Windows 0 画面；macOS 仅 SC01（NEW）与 SC08/09/10（REUSE T08）；SC02—SC07、SC11、SC12 NOT RUN；英文 0；Retina 0 |
| X02 DPI | **NOT RUN/BLOCKED**：无 Windows；macOS 仅 100% 非 Retina |
| X03 输入法/剪贴板/键盘 | **PARTIAL**：Escape 关闭详情、剪贴板中文 PASS；真实 IME 候选 NOT RUN；Tab 焦点未观察到可见变化（未验证） |
| X04 窗口生命周期 | **PASS（仅 macOS 本机）**：最小化/恢复、关闭重开、多实例唤醒、关闭后 core 运行 |
| X05 重复操作 | **PARTIAL**：5 次详情开关 + tab 切换 + 滚动稳定；无性能基线/长稳 |
| X06 平台分支/标题栏/字体 | **PARTIAL（静态）**：自动重启仅 macOS；非 macOS UI 禁用+说明；Windows 字体候选；D04 标题栏差异已登记，未实测 |

## 4. 复用证据（明确标注，不等于本轮重跑）

- **T08**（同二进制 `89049d01…`、同源码 HEAD）：`docs/UI-UX-renew/evidence/t08-residual-003/` — SC08（`1440-storage-settings.png`、`1440-pricing-open.png`）、SC09（`1440-r03-pricenew-empty.png`、`1100-r03-input-filled-004.png`）、SC10（`1440-r04-priceedit.png`、`1100-r04-edited-004.png`）；均为中文 macOS。
- **L01/L03/L04/L05**（T07 阶段，旧构建/旧夹具）：`docs/UI-UX-renew/evidence/t07-l0*/` — SC01 三尺寸、明细/分组/全值补充；L05 校验见其 `CODEX_REVIEW.md`。
- 复用只覆盖 macOS 中文；**不能**用于 Windows、英文或 Retina 结论。

## 5. 未测 / 阻塞清单

- Windows x64：100%（1280×800 内容区、1440/1100 窗口）、125/150/200% DPI、中文/英文、SC01—SC12 全部 → **BLOCKED**。
- macOS Retina（scale 2.0）与英文 → **NOT RUN**（当前非 Retina；夹具 zh）。
- SC02 供应商管理、SC03 选择模型、SC04—SC07 协议菜单、SC11 编辑模型、SC12 同步确认 → **NOT RUN**（无 providers/models 数据；1TB 未挂载，不能安全新建夹具）。
- 真实输入法（中文/英文候选框、组合输入、光标/候选位置）→ **NOT RUN**；Tab 焦点圈 → 未验证。
- X05 性能/泄漏基线 → **NOT RUN**；不做百分比结论。
- 构建/`cargo test`/`clippy` → **NOT RUN**（AGENTS.md 要求 TMPDIR=/Volumes/1TB；卷未挂载；未在系统盘新建构建 TMP）。复用 T08 提交前记录（138 tests、clippy/fmt/check/build 通过）。

## 6. 缺陷/差异登记（未改代码）

- **D04 标题栏**：macOS 透明标题栏 + 原生红黄绿；Windows 由 Kit 绘制最小化/最大化/关闭（IMPLEMENTATION_STATUS §2.3），设计稿要求“系统原生”。差异已登记；本轮未修改窗口宿主。
- **D02 自动重启**：实现仅 macOS；非 macOS 端口变更弹窗显示“此平台暂不支持自动重启”，确认按钮禁用（`ui.rs` 静态核对）。Windows 行为未实机验证。
- 无新增产品缺陷证据；未做代码修改（T09 未发现可证明需修复的本机缺陷；Windows 问题无法在本机证明）。

## 7. 建议

1. 在具备 Windows x64 实机/VM 与 1TB 卷的环境重跑主矩阵与 DPI 扩展；补英文与 Retina。
2. 为 SC02—SC07、SC11、SC12 建立隔离夹具（供应商/模型/协议/同步确认），在 1TB 可用时执行。
3. 输入法专项：中文/英文 IME 候选框、组合输入、Tab/Enter/Escape、焦点圈。
4. X05 性能基线：同一构建、同一数据量，记录打开/关闭/翻页耗时与 RSS；不虚构百分比。
5. D04/D02 若要求跨平台一致，按设计决策单独评估；本轮不擅自改宿主/平台能力。

## 8. 清理

见 `logs/cleanup.md`：停止 GUI/core、等待退出、核查 `ps`/`lsof`，删除 T09 runtime 副本；检查 `/tmp`、证据大小、共享缓存与 `df`。未删除 L05 原始保留目录、`target/`、`gpui/target/`。
