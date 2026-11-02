# Studio V2 设计交付与 GPUI 实现契约

本规范落实 [Issue #6](https://github.com/gloryhui/workbuddy-proxy/issues/6)，供设计端 GPT / Figma MCP 与实现端 Codex 共同使用。优先用于新页面与增量设计；已有画板只补影响本次实现的关键规格。风险与验证遵守 [CODEX_FAST_WORKFLOW.md](../CODEX_FAST_WORKFLOW.md)。

## 来源、范围与可靠性

- 设计文件：[WorkBuddy Proxy](https://www.figma.com/design/zxDfIQ4lhNcTXrFT7qk8QZ)，file key `zxDfIQ4lhNcTXrFT7qk8QZ`；页面 `51:2`，名称 `WorkBuddy Proxy · Studio V2 · Rust GPUI`。
- 2026-11-02 通过官方 Figma MCP `use_figma` 只读核对供应商管理 `54:2` 的结构、指定节点属性、实际变量绑定与组件定义；未修改 Figma，也未重新验收全部 40 帧。
- 源码基线：`763f2aefb5e644d84250590aaa3bdc96a90b6e11`（本轮核实与 GitHub main 相同）。下述“当前实现”来自该基线的静态阅读，后续开工须复核当前版本。
- [figma_node_map.md](figma_node_map.md) 继续拥有完整的「状态画板 → Rust 文件」索引和既有验收记录；本文补充关键工程语义，不复制整表、不重置历史签收。
- 产品维持 Rust + GPUI 原生架构、深色侧栏与浅色主区、旧主题兼容。MCP 返回的 React / Tailwind / HTML / CSS 只用于理解设计，不能成为运行时实现或引入 WebView / Electron 的理由。

Figma 数据表明设计目标；源码表明实现方式；运行证据表明实际行为。三者不能互相替代。发现差异须记录设计值、实现值、影响与待决定项，不能根据截图猜测后悄悄覆盖。

## 设计端可复用交付规范

每次新增或修改设计，按以下顺序增量交付；不要求为既有画板批量补齐所有排列组合。

1. **先发现和复用**：确认目标 page / frame、已有组件、Variables、Styles 和资产来源，再决定是否新增。保留稳定 node ID；确需替换时更新映射与替换原因。
2. **原生结构和命名**：交付 Frame、Auto Layout、文本、形状、矢量及组件实例；采用 `Sidebar/ProviderList`、`Provider/Action/Add`、`Dialog/ConfirmDelete` 等语义名。禁止整屏 PNG 充当可编辑稿，禁止用 `Rectangle 123` 等无意义名称交付。
3. **Tokens 绑定**：颜色、背景、文字层级、边框、半径、间距、字号、行高、字重、控件尺寸用语义 Variables / Styles 表达，并绑定真实属性。记录名称、单位、模式及 Rust 对应项；只写旁注不算完成绑定。字体家族是设计参考，不代表必须嵌入字体。
4. **组件和状态**：复用按钮、输入框、Select、表格、卡片、Toast、Tooltip、Dialog、导航项与图标按钮。按适用性表达 Default、Hover、Pressed/Active、Focused、Disabled、Loading、Selected、Error、Empty；区分样式轴与状态轴，不强制生成全组合。不可只交正常态，把交互留给实现者猜测。
5. **布局约束**：相关子元素使用 Auto Layout；交付方向、padding、gap、对齐、Fixed / Fill / Hug、最小最大宽高、锚点与溢出。绝对坐标仅用于有明确理由的装饰或叠层。注明每块独立滚动区、固定操作栏、表头是否吸顶、列表是否虚拟化、文字换行/省略/Tooltip。
6. **窗口与平台**：以 1440×900 和最小 1100×700 逻辑 px 表达宽窄布局，可以用约束和断点推导窄窗。区分截图物理像素与逻辑 px；标注 Windows / macOS 原生标题栏、系统按钮及应用工具栏的避让，不能照搬装饰交通灯。
7. **行为和数据**：每个业务操作注明触发动作、输入约束、状态流转、数据来源、保存时机、取消/失败/重试语义。覆盖加载、空列表、成功、错误、禁用、长文本、极值与删除确认；安全字段仅用合成数据，API Key 保持掩码。
8. **输入和可访问性**：用简短行为契约表达 Tab 顺序、Enter / Esc / Space、焦点恢复、IME 组合输入及可访问名称；不必每个键位绘制一帧。弹窗遮罩须阻断背景操作，失败信息及主要动作可达。
9. **资源和追溯**：图标优先真实 SVG / 矢量，记录原尺寸、用途、node ID 与导出路径；仅必要图像使用 PNG。沿用 [资产追溯 README](../../gpui/assets/studio/README.md)，不重复导出相同资源，不栅格化文字、业务数值或整个控件。

设计交付至少包含下面示例的数据结构，并明确 `known`、`needs_design_decision` 和 `platform_exception`。未知项显式留空或列为缺口，不虚构节点、变量、组件状态或交互。

## Tokens 与共享原生组件

共享定义由 [theme.rs](../../gpui/src/theme.rs) 和 [theme/skin.rs](../../gpui/src/theme/skin.rs) 管理，实现优先复用 [ui_components.rs](../../gpui/src/ui_components.rs)。下表为本轮从代表节点实际绑定中读取的颜色；只作映射，不成为第二套常量源。

| Figma Variable | 本轮读取值（Studio） | Rust 对应 | 绑定节点示例 |
| --- | --- | --- | --- |
| `studio/color/sidebar` | `#111525` | `color::SIDEBAR` | `54:4` |
| `studio/color/canvas` | `#F7F8FC` | `color::BG_PAGE` | `54:203` |
| `studio/color/ink` | `#161C2C` | `color::TEXT_1` | `54:205` |
| `studio/color/surface` | `#FFFFFF` | `color::BG_BAR` | `54:216` |
| `studio/color/line` | `#E7EAF2` | `color::BORDER` | `54:216`、`54:229` |
| `studio/color/soft` | `#F3F5FA` | `color::BG_TABLE_HEAD` | `54:229`、`54:231` |
| `studio/color/accent` | `#655CF4` | `palette().primary` / `Skin::StudioIndigo` | `54:295` |

`studio/color/accent` 在本轮读到的模式 `62:0` 中另有 `#27272A`，不能把 Studio 的值硬编码到旧主题。其余主题的模式对应关系不在本轮核对范围。

| 属性/组件 | 当前复用入口 | 使用约束 |
| --- | --- | --- |
| 字体角色 | `theme::font`、`theme::line_height` | 保留字号、字重、行高语义；系统字体与 fallback 优化见 [Issue #4](https://github.com/gloryhui/workbuddy-proxy/issues/4)，本文不改变当前嵌入实现。 |
| 尺寸、间距、半径 | `theme::size`、`space`、`radius`、`stroke` | 优先既有 tokens；页面局部值确需共享时再提升，不能按历史表机械覆盖。 |
| 通用按钮 | `action_button`、`button` | 当前标注按钮高 36、圆角 9；`button` 统一处理 disabled / busy / loading，业务事件留在调用者。 |
| 输入与 Select | `text_input`、`masked_input`、`provider_field`、`select` | 保留 Kit 原生编辑、焦点和选择引擎；供应商字段高 40、圆角 10，焦点边框由 `provider_field` 表达。 |
| 系统工具栏 | `system_button` / `system_button_base` | 36×36 热区、20×20 原 SVG、8 gap；保留可访问名称、Tooltip、原生焦点与禁用状态。 |
| 表格、弹窗、提示 | `table`、`dialog_body_sized`、`scrollable_form`、`dialog_footer`、`full_value_tooltip` | 复用虚拟化和原生事件；长值提示有界换行，弹窗滚动与操作区规则按具体节点补充。 |

## 代表画板：供应商管理 `54:2`

以下 JSON 是可由工具解析的最小交付实例。几何来自本轮 MCP 原生节点属性；`rect` 为相对父节点的 `[x, y, width, height]`，单位均为逻辑 px。`implementation` 来自源码，不能当作已存在的 Figma 响应式约束。

```json
{
  "verified_on": "2026-11-02",
  "source_commit": "763f2aefb5e644d84250590aaa3bdc96a90b6e11",
  "figma": {
    "file_key": "zxDfIQ4lhNcTXrFT7qk8QZ",
    "page": "51:2",
    "frame": "54:2",
    "logical_canvas": [1440, 900],
    "containers": [
      {"node": "54:4", "role": "sidebar", "rect": [0, 0, 256, 900]},
      {"node": "54:203", "role": "content", "rect": [256, 24, 1184, 848]},
      {"node": "54:216", "role": "connection", "rect": [32, 198, 755, 338]},
      {"node": "54:246", "role": "models", "rect": [32, 548, 755, 253]},
      {"node": "54:297", "role": "runtime", "rect": [804, 198, 348, 603]}
    ],
    "toolbar": {"instance": "103:523", "component": "103:442", "layout": "HORIZONTAL", "size": [300, 36], "gap": 8},
    "save_button": {"instance": "54:295", "component_set": "51:63", "style": "Primary", "size": [107, 36]},
    "assets": [
      {"node": "54:264", "path": "gpui/assets/studio/model-edit.svg"},
      {"node": "54:299", "path": "gpui/assets/studio/runtime-activity.svg"}
    ]
  },
  "implementation": {
    "min_window": [1100, 700],
    "sidebar_width": 256,
    "column_gap": 17,
    "runtime_width": {"below_1280": 300, "otherwise": 348},
    "model_footer_compact_below": 1320,
    "scroll_regions": ["provider-list", "provider-editor-scroll", "runtime-rail-scroll"],
    "model_table": "Kit DataTable virtualization"
  },
  "needs_design_decision": [
    "主要容器 layoutMode=NONE，窄窗与滚动语义须结合源码，不能声称已有 Auto Layout 约束",
    "按钮组件集仅有 Style 轴，缺少独立交互 State 轴",
    "标题 54:205 为 Noto Sans SC Bold 28/41，当前共享标题 token 为 27/39"
  ],
  "platform_exception": ["原生标题栏与工具栏避让", "系统字体 fallback 与 HiDPI 栅格差异"]
}
```

### 布局恢复与当前差异

- **MCP 已明确**：左侧栏宽 256；内容左右边距可由节点的 32 与 `1184 - (804 + 348)` 恢复为 32；连接与模型面板之间为 12；左右内容列间距由 `804 - (32 + 755)` 恢复为 17。连接卡圆角 16、边框 1；供应商名称与 Key 字段分别为 349×40、350×40，圆角 10。这些值来自结构，无需取色或量截图。
- **当前代码补充**：[ui.rs](../../gpui/src/ui.rs) 的 `render` 用 flex 分配主区，[ui/providers.rs](../../gpui/src/ui/providers.rs) 的 `center` 负责连接/模型区独立滚动；[ui/relay.rs](../../gpui/src/ui/relay.rs) 的 `rail_width` 在窗口小于 1280 时从 348 收到 300。模型操作底栏在小于 1320 时启用 compact 排列。1100×700 由 [main.rs](../../gpui/src/main.rs) 的 `window_min_size` 限定；不能把主区误改为整页固定宽 1184，也不能把窄窗行为臆写成所有区域纵排。
- **滚动边界**：[ui/sidebar.rs](../../gpui/src/ui/sidebar.rs) 的供应商列表独立滚动，操作栏在列表外；右侧运行区独立纵向滚动，日志内部实现留在 `ui/relay.rs`；[model_table.rs](../../gpui/src/model_table.rs) 的 Kit DataTable 负责模型表虚拟化、表头与滚动。不新增一套手写表格或重复全局滚动器。
- **设计缺口**：本轮读取的主要容器 `layoutMode=NONE`、Fixed / Fixed，不能从 Figma 自动恢复真实 Fill / Hug、断点或固定操作栏语义。后续改相关布局时，设计端增量补约束；本轮通过上述源码规则补充契约，不重画既有 40 帧。
- **文字差异**：标题节点 `54:205` 的原生字体属性为 28px / 41px 行高，当前共享 `PAGE_TITLE` 为 27px / 39px。节点外框高度 20 不能被当作行高。保留此差异供相关设计任务决定，本轮不自动覆盖共享 tokens，也不声称标题像素一致。

### 组件状态、业务与数据契约

| 场景/状态 | 当前实现与应保留语义 | Rust 落点 / 设计关联 |
| --- | --- | --- |
| 保存：Default → Loading → 成功/错误 | `54:295` 引用组件集 `51:63`，当前仅 `Style=Primary/Secondary/Subtle/Danger`，未定义完整交互状态轴。运行时使用共享按钮、busy/loading 与真实保存反馈；不能把静态“已保存”标签当作成功依据。 | `ui/providers.rs::model_directory`、`ui.rs::save` / `save_steps` |
| 测试连接 | `test` 先构建 `save_steps` 再追加 test 请求，可能保存草稿；不能描述成纯读取操作。busy 或无模型时禁用，反馈反映当前供应商/模型的真实请求。 | `ui.rs::test`、`provider_connection.rs`、`ui/providers.rs::connection_badge` |
| 新增/复制草稿、编辑和取消 | 新增/复制先进入独立草稿；按各弹窗规则取消或返回草稿，持久化配置需明确操作。草稿恢复与业务配置保存须区分，不能声称所有取消都删除草稿或所有编辑立即落库。 | `ui/provider_new.rs`、`ui/provider_edit.rs`、`state.rs`；关联 `78:364`、`78:605`、`78:843` |
| 删除确认、取消与失败 | 先锁定删除对象，取消不提交删除；遮罩阻断背景操作，明确确认后由现有控制 API 删除与持久化。错误保留真实失败反馈。 | `ui/provider_delete.rs`；关联 `78:1080`、`80:864` |
| 空列表、搜索无匹配与满载 | 无供应商时展示新增入口；无匹配单独反馈，清空搜索可恢复；64 供应商及全配置合计 256 模型上限，发现候选上限 4096，不能因设计示例改变限制。 | `ui/sidebar.rs`、`ui/providers.rs::center`、`provider_filter.rs`、`ui/model_picker.rs`、`draft_discovery.rs`、`src/config.rs` |
| Key、协议与输入焦点 | Key 默认掩码，显隐只改展示；遵守原有四协议与字段校验，错误不得隐藏。输入保留 IME、粘贴、选中、复制和 Tab，焦点边框由原生输入状态决定。 | `ui/providers.rs`、`ui_components.rs::provider_field`、`ui/protocol_dialog.rs`；关联 `54:231`、`61:289/561/838/1111` |
| 长文本、统计与运行数据 | 长名称/ID 保留完整值可达性，按现有省略/提示规则处理；模型来自配置/草稿，状态、计数、日志来自真实 Core，不能硬编码示例模型与数字。统计页继续使用 Usage/Stats，筛选和分页沿用既有接口。 | `model_table.rs`、`ui/relay.rs`、`statistics/overview.rs`、`stats_backend.rs` |

上表中的 `ui/...` 等路径相对 `gpui/src/`；`src/config.rs` 相对仓库根目录。具体键盘路由沿用 `main.rs`、`ui.rs` 和相关弹窗；涉及变动时明确 Enter / Space 激活、Esc 取消、焦点归还和 IME 组合优先级，不能用新增全局快捷键覆盖原生输入行为。

## 平台差异与验收边界

- 原生 `TitleBar` 保留真实系统按钮、拖动与双击行为；Figma 的装饰交通灯不参与实现。当前 `NATIVE_TITLEBAR=24`；`SYSTEM_TOOLBAR_TOP` 在 macOS 为 8，在 Windows 为 24+8，以避开右侧原生按钮。不得为匹配画板将 Windows 工具栏强推回顶部 8。
- 新设计需注明中英文、数字、货币/标点、超长值与其他字符的系统 fallback 要求，允许系统字体度量和栅格差异。字体策略见 #4；不因此重新打包完整字体、禁用非中英字符或改变主题。
- 视觉改动按实际风险检查受影响画面；全局字体、主题与共享控件扩展到代表场景。1100×700、1×/2×、Windows/macOS 的实测仅在相关风险或发布验收时要求；交叉编译不能代替 GUI、IME 或辅助技术验证。
- 本轮仅完成文档、映射及代表节点结构核对；未新运行 GUI、HiDPI、输入或跨平台验收，不修改既有验收结论。

## 实现端读取与交付协议

1. 读任务、最新 `AGENTS.md`、工作流规范与本文；从映射表定位相关 frame 和 Rust 文件。每次只读需要的节点，不批量加载所有画板或历史 QA。
2. 通过 Figma MCP 定向读取 design context、必要 variables / assets；纯规范核对可用只读结构属性。截图作为视觉参考及必要回归，不从截图猜已有结构化数值。遵守所用 MCP 的技能前置要求。
3. 在动代码前列清数据已明确、需要设计决定、平台例外；核对设计和当前代码的差异。不能访问 Figma 时明确未验证，保留缺口，不编造已完成的结构验收。
4. 用原生 GPUI 布局、共享 tokens / 组件和现有状态/事件实现；业务数据及校验继续由现有模块拥有。不修改用户 Figma 原稿，除非当前任务明确授权设计写入。
5. 同一批变动集中验证，按最低充分风险级别运行；文档首轮只做 diff、Markdown/链接及映射检查。截图不证明保存成功，数据契约不证明视觉正确；未覆盖项分别记录。
6. 交付必要 diff、关联 Issue、代表节点/资产来源、变化的契约及验证限制；文档只在语义变化时更新，不逐像素刷新全量报告。提交 PR 和推送按用户授权执行，不自动合并或发布。

后续设计交付检查清单：

- [ ] 目标节点真实存在，原生可编辑结构、语义命名、Variables / Styles 绑定与组件复用可追溯。
- [ ] 布局约束、滚动、最小窗口、必要状态及业务/取消/错误语义明确；缺口显式记录。
- [ ] 至少一个相关节点可由 MCP 结构与本契约恢复关键布局、资产、响应式及状态，不依赖截图猜数值。
- [ ] 与现有 tokens、组件、数据源及节点映射一致，无虚构路径或 ID；不为旧设计无关重建。
- [ ] 完成对应风险的最低充分验证，明确平台限制；保留安全规则、强制 CI、原生架构与发布授权边界。
