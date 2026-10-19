# T09 跨平台验收矩阵

**HEAD：** `6533bf1605ad4ab395d896d5cc215142d96b3a99`（`codex/ui-redesign`）
**结论：** **BLOCKED** —— Windows 平台不可用（本机无 Windows 主机/虚拟机/远程；无 Windows 构建或运行证据）。macOS 只完成了本机可行子集；**不得**把本矩阵读作“双平台通过”。

## 1. 环境

| 项 | 值 |
|---|---|
| 主机 | macOS 27.0.1（26A434），Apple M1，8 核，1920×1080 @75Hz，**非 Retina，scale=1.0** |
| Windows | **不存在**：无 Parallels/VMware/VirtualBox/UTM/Wine，无 Windows 远程目标 |
| `/Volumes/1TB` | **未挂载**（`mount` 无记录；仅 `Macintosh HD`、`Recovery`） |
| 构建 TMP | 依 AGENTS.md 必须指向 `/Volumes/1TB`；不可用 → **本轮未构建、未跑 cargo check/test/clippy**（复用 T08 提交前记录） |
| 已构建二进制 | GPUI `gpui/target/release/workbuddy-proxy-gpui`，14,649,920 B，sha256 `89049d01…`，mtime 2026-10-20 01:02；core `target/release/workbuddy-proxy` sha256 `22cacc66…` |
| 运行隔离 | L05 夹具 **副本** 到 `evidence/t09-cross-platform/runtime/`（16 行，config language=zh，providers=[]）；未触碰 L05 原始保留目录 |
| 截图尺寸 | 全部 PNG 物理尺寸见 `logs/shot-sizes.txt`（1100×700、1280×800、1440×900） |

## 2. 主矩阵（SC01—SC12）

标记：**NEW**=本轮重跑；**REUSE**=既有证据（注明版本）；**NOT RUN**=本机条件不足；**BLOCKED**=Windows 缺失。

| 画面 | Windows | macOS 本轮 | macOS 复用 | 英文 | 判定 |
|---|---|---|---|---|---|
| SC01 数据统计 | **BLOCKED** | **NEW** 1440×900 / 1280×800 / 1100×700 中文（16 行，KPI/图表/日志可见；`mac-*-sc01.png`） | L01 三尺寸、L03/L04/L05 明细/分组/全值（中文） | NOT RUN | 仅 macOS 中文部分 |
| SC02 供应商管理 | **BLOCKED** | NOT RUN（providers 为空；无安全夹具新建条件） | 无 | NOT RUN | 未覆盖 |
| SC03 选择模型弹窗 | **BLOCKED** | NOT RUN（无模型） | 无 | NOT RUN | 未覆盖 |
| SC04 上游协议下拉 | **BLOCKED** | NOT RUN | 无 | NOT RUN | 未覆盖 |
| SC05 协议 OpenAI（含 Responses none） | **BLOCKED** | NOT RUN | 无 | NOT RUN | 未覆盖 |
| SC06 协议 Anthropic | **BLOCKED** | NOT RUN | 无 | NOT RUN | 未覆盖 |
| SC07 协议 Gemini | **BLOCKED** | NOT RUN | 无 | NOT RUN | 未覆盖 |
| SC08 成本定价/采集存储展开 | **BLOCKED** | REUSE T08（中文 1440/1100；`t08-residual-003/shots/1440-storage-settings.png`、`1440-pricing-open.png`） | 同左 | NOT RUN | 仅 macOS 中文部分 |
| SC09 新建价格 | **BLOCKED** | REUSE T08（中文；`1440-r03-pricenew-empty.png`、`1100-r03-input-filled-004.png`） | 同左 | NOT RUN | 仅 macOS 中文部分 |
| SC10 编辑价格 | **BLOCKED** | REUSE T08（中文；`1440-r04-priceedit.png`、`1100-r04-edited-004.png`） | 同左 | NOT RUN | 仅 macOS 中文部分 |
| SC11 编辑模型 | **BLOCKED** | NOT RUN（无供应商/模型） | 无 | NOT RUN | 未覆盖 |
| SC12 同步 WorkBuddy 确认 | **BLOCKED** | NOT RUN（无模型/确认入口） | 无 | NOT RUN | 未覆盖 |

**X01：BLOCKED。** 12 画面中 macOS 仅 4 个有（部分）证据；Windows 0 个；英文 0 个；Retina 环境缺失。

## 3. DPI 扩展（X02）

| 平台/缩放 | 结果 |
|---|---|
| Windows 100/125/150/200% | **BLOCKED**（无 Windows） |
| macOS 100%（本机） | NEW 三档窗口尺寸；无裁切/无横向溢出（`mac-1100x700-sc01.png` 等） |
| macOS Retina / 其它缩放 | **NOT RUN**（当前显示器非 Retina，无法伪造 scale） |

**X02：NOT RUN/BLOCKED。** 未测字号基线、1px 边框、焦点圈、图标、下拉锚点、IME 候选框位置。

## 4. 交互与生命周期（X03—X06）

| ID | 结果 | 证据 |
|---|---|---|
| X03 输入法/剪贴板/键盘/Tab/Enter/Escape | **PARTIAL** | 详情打开→Escape 关闭（`mac-request-detail.png`、`mac-detail-closed.png`）；剪贴板中文粘贴 `测试中文` 成功（`mac-paste-chinese.png`，AX value 验证）；**真实输入法候选框 NOT RUN**（未切换 IME）；**Tab 焦点遍历未观察到可见焦点变化**（`mac-tab-focus.png` 与关闭态同哈希），标记未验证。 |
| X04 窗口生命周期 | **PASS（macOS 本机）** | 三档 resize；AXMinimized true→窗口从 on-screen 列表消失、false→恢复（`mac-minimized.png`、`mac-restored.png`）；Cmd+Q 后 GPUI 退出、core 仍运行（后台代理）；重开窗口恢复（`mac-after-reopen-sc01.png`，pid/wid 变化）；第二实例写 `gpui-wake.json` 后退出，窗口数仍 1。 |
| X05 固定数据重复操作 | **PARTIAL** | 详情开关 5 次、供应商统计/模型统计切换、滚动（`mac-supplier-stats.png`、`mac-model-stats.png`）；进程 RSS 136 MB、窗口 1、无崩溃/重复窗口；**无性能基线、无长稳/泄漏测量**，不宣称“无性能问题”。 |
| X06 平台分支/标题栏/字体 | **PARTIAL（静态）** | `logs/x06-static.log`：自动重启仅 macOS（`workbuddy_restart.rs`）；非 macOS 端口变更显示“此平台暂不支持自动重启”且确认按钮禁用（`ui.rs:2960–2977`）；Windows 字体候选 Segoe UI Variable/Segoe UI/Microsoft YaHei UI（`theme.rs`）；标题栏 D04：macOS 透明标题栏+原生红黄绿，Windows 由 Kit 绘制控件（IMPLEMENTATION_STATUS §2.3），设计稿要求“沿用原生”→ 差异已登记，未在 Windows 实测。 |

## 5. 关键限制

1. **Windows 缺失**：无实机/虚拟机/远程；无 Windows 编译或运行证据 → T09 不能 DONE，只能 BLOCKED。
2. **`/Volumes/1TB` 未挂载**：按 AGENTS.md 不能把新构建/测试 TMP 放系统盘；本轮未构建，未跑 cargo test/clippy，未新建夹具。SC02—SC07、SC11、SC12 因无数据/无法安全建夹具而 NOT RUN。
3. **英文界面**：现有夹具 language=zh；未切换，英文 NOT RUN。
4. **Retina/DPI**：本机非 Retina、无 Windows 缩放环境 → X02 NOT RUN。
5. **复用的边界**：T08 证据来自同一 GPUI 二进制 sha256 `89049d01…` 与同一源码提交；L01/L03/L04/L05 为 T07 阶段中文 macOS 证据。它们只作 macOS 补充，不代表 Windows，也不替代本轮未跑画面。
6. **没有破坏性操作**：未清空真实库、未写真实用户数据；运行使用 L05 副本，结束后删除。

## 6. 建议（不改用户状态文档）

- 在具备 Windows x64 实机/VM 与 1TB 卷后重跑 X01/X02：SC01—SC12 × {Windows 100%, 1440/1100} × {zh,en}；Windows 125/150/200% 的 SC02/04/06/09/11。
- macOS 补 Retina 实机（scale 2.0）与英文；补 SC02—SC07、SC11、SC12 的代表数据夹具。
- 输入法：在中文/英文 IME 下验证候选框位置、组合输入、Tab/Enter/Escape 与焦点圈；本轮 IME 与 Tab 未验证。
- 性能：如需 X05 结论，使用同一构建、同一数据量做基线（打开/关闭详情、翻页、滚动耗时/RSS），不以单次观察代替。
- D04 标题栏若需与设计稿“系统原生”完全一致，应在 Windows 侧评估 Kit 绘制控件与原生装饰的取舍；D02 自动重启的 Windows 限制已在 UI 明确说明。
