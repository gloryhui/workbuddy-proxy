# WorkBuddy Proxy desktop

The maintained desktop application with one shared GPUI interface on macOS and Windows.

The relay and tray controller do not link GPUI. Opening settings starts a separate GPU-backed process; closing settings persists private unsaved drafts and releases that process. The relay continues to run. Use the tray/menu-bar Quit command to stop the entire application.

Providers, model discovery/selection, editable model names and IDs, reasoning settings, WorkBuddy sync/removal, local port settings, bilingual localization (one language at a time), connection diagnostics, and log clear/copy use the existing local authenticated management API.

No bundled browser, WebView, JavaScript runtime, custom fonts, syntax grammars, or full icon catalog. Model and discovery lists use virtual scrolling. The default light theme and spacing are shared across both systems.

Settings drafts live in `gpui-drafts.json` in the private application data directory. They may contain API keys, just like `config.json`. Drafts are discarded when the underlying saved configuration changes externally.

Formal Windows distribution uses a Chinese EXE installer; Mac distribution uses DMG. Package sizes, measured process RSS, renderer compatibility, and package-level checks are recorded separately; CI measurements do not constitute testing every physical Windows GPU or remote-control setup.

GPUI and GPUI Kit are Apache-2.0 software. https://github.com/zed-industries/zed/tree/main/crates/gpui and https://github.com/longbridge/gpui-kit

## Component coverage

All settings-page interactive controls use GPUI Kit 0.7.3: `Button`, `Input` (including mask toggle), `Select`, `Checkbox`, `Sidebar`/`SidebarMenuItem`, virtualized `DataTable`, `ListItem`, `Dialog`/`DialogContent`/`DialogFooter`, `Label`, `Tag`, `TitleBar`. Provider counts appear in a compact tag; the active provider follows the selected skin, with monochrome as the default. Existing models in discovery are checked and disabled, with an Added label.

Layout-only surfaces and spacing use GPUI containers. A GroupBox was evaluated but its nested intrinsic-height wrappers clipped flexible log panels, so the settings surfaces retain flexible layout containers. They are not OS-native input or list controls. Native tray integration remains small and resident; closing settings releases the GPUI process.

The DataTable delegates row selection to the application to preserve Shift ranges, Ctrl/Command toggle, Delete/Backspace, and double-click editing. Both configured and discovered models remain virtualized.

## Interface behavior

Statistics has separate Overview, Usage records and Analysis views. The page header, shared filters and report status stay above the content. Usage records and grouped analysis use a flexible table with a pinned header and bottom pagination; only the table body scrolls vertically. Overview charts and statistics settings have their own content scrolling areas. Pricing, storage and detailed collection/import diagnostics are available through Statistics settings.

The applied view, grouping, relative/custom date range, provider/model, model source, record type, outcome, usage completeness, capture method, test-record inclusion, refresh interval and currency are saved locally in `statistics-view.json` beside the application configuration. Leaving statistics preserves these choices. Relative dates resolve against the current clock when reopening; the initial range, cleared filters, and invalid or expired custom ranges use the past 7 calendar days including today. Corrupt or future preference files fall back safely. Page cursors and in-progress date edits are not restored. This file uses the existing cross-platform atomic configuration writer and contains no API keys.

Record type (model usage, generation tool charges, other standalone charges) is independent of built-in/custom model source. It applies to dashboard totals, time series, request pagination and grouped analysis on the server. Imported records show logged credits as their primary amount; standalone charges show Token as not applicable. Currency estimates and WorkBuddy credits remain separate units.

The development version adds per-model [WorkBuddy capability settings](MODEL-CAPABILITIES.md) to the model editor: images, tools, reasoning options, token/context limits and complete Chat Completions URL mode. Defaults enable image input for DeepSeek V4.1 Flash; explicit per-model overrides survive drafts, saving, duplication and sync. Other media JSON and response bytes remain transparent; this does not add native audio/video APIs. Provider-level protocol selection now also adapts OpenAI Responses, Anthropic Messages and Gemini GenerateContent to the local Chat Completions interface; see [protocol usage](protocol-compat/USAGE.md).

Version 0.5.2 adds an opt-in **Launch at login** checkbox in the header. New configurations and existing configurations without this setting default to off. The authenticated core command persists the selection and updates only this user's Windows `Run/WorkBuddyProxy` value or macOS `~/Library/LaunchAgents/io.github.gloryhui.workbuddy-proxy.autostart.plist`; a registration failure rolls back the saved choice. Login launches use `--autostart --config-dir DIR`, start the resident tray/core without settings, and do not wake an already-running settings window. Normal manual launches still show settings. There is no keep-alive restart after Quit. Windows uninstall removes the app's Run value while retaining user configuration.

Provider and log operations use compact Kit ghost icon buttons with localized tooltips; the sidebar footer no longer applies a whole-toolbar hover background. Sync is the primary action and removal is shortened to Unsync. Local port, the live relay address and provider/model/request counts share one service card.

Model dialog Enter uses the same validation/submission as Confirm; missing IDs keep the dialog open. The bottom status bar is removed; only a muted tray hint remains at the lower right, while failures appear beside settings. The model table has display-name and model-ID columns plus a fixed-width edit-icon column. Clicking an edit icon opens that row's editor without changing selection; double-click and keyboard editing remain available. The Kit trailing placeholder is overridden to zero width; widths come from the actual table container.

Log entries use single newlines and a 16px line height. New content scrolls to the bottom after layout; polls without new content preserve the viewport. The existing core keeps its latest 100 entries rather than growing memory without a bound.

The shared 36px title toolbar contains Skins, Launch at login, Check updates, Language, Restore default size, GitHub and Feedback, in that order. It uses the page background without a separator or duplicate application title. macOS retains its left traffic lights; Windows retains its right minimize/maximize/close controls. The action buttons are excluded from the title-bar drag region. Language uses a globe menu; the version appears as muted text at the end of the sidebar subtitle. Connection settings offer a searchable [provider preset catalog](PROVIDER-PRESETS.md) above the name and key fields. Presets fill the form and remain editable until saved.

## Windows installation

Use `workbuddy-proxy-0.5.6-windows-x64-setup.exe`. The wizard and uninstaller use Simplified Chinese on all Windows display languages. The directory selection page is always shown. Setup defaults to `%LOCALAPPDATA%\Programs\WorkBuddy Proxy`, installs for the current user, creates a Start Menu entry, and selects the desktop shortcut by default (users can uncheck it). Both shortcuts launch only `workbuddy-proxy.exe`.

The settings helper is named `internal/workbuddy-settings.exe`; do not launch or move it separately. Only the main executable is exposed through installation shortcuts; internal binaries must remain with the installation.

Close the old app using the tray Quit command before upgrading. The installer registers a stable uninstall entry. Uninstall removes installed program files and shortcuts, retaining `%APPDATA%\WorkBuddyProxy` configuration/logs. Installing to a protected directory requires adequate permissions; the per-user installer does not request elevation.

Inno Setup 6.5+ is a Windows build dependency only; neither portable nor installed applications need it or Python/Rust. The Chinese translation is vendored from the upstream Inno Setup `Files/Languages/ChineseSimplified.isl` (retrieved 2026-10-14, translator attribution retained). No compiler language-pack fallback is allowed. CI checks the active Chinese language, translated task text, desktop shortcut creation without a `/TASKS` override, a custom directory containing Unicode and spaces, repeat installation, shortcut targets, installed binary hashes, application lifecycle and uninstall/config retention. Interactive wizard and physical Windows GPU acceptance remain separate checks.

Automatic model capabilities are resolved by the core from provider metadata, the models.dev catalog and built-in fallback rules. Refresh runs in the background; forwarding never consults the catalog. The editor reads cached results asynchronously, displays the main source, and preserves manual overrides. See [MODEL-CAPABILITIES.md](MODEL-CAPABILITIES.md) for precedence, cache limits and offline behavior.

统计页默认显示过去 7 天（含今天，从 6 天前的本地零点到当前时刻）。首次打开、清除筛选及筛选配置异常时均使用此范围；正常保存过的时间范围和筛选条件仍优先恢复。

趋势图在相邻有效数据之间绘制实线，跨越缺失时段时用虚线作为连接指引，并显示说明；悬浮缺失时段可查看具体区间。虚线不代表期间的实际用量或费用，缺失值和总计保持不变，不补零、不外推。各 Token 系列及当前币种独立判断；只有一个数据点时保留圆点并显示明确提示。

统计页的页签、排序和分页操作统一使用 32 逻辑像素高的按钮；筛选框为 36 逻辑像素，正文 13、辅助文字 12、标签 11 逻辑像素。按钮显式指定尺寸，避免 `small()` 随基础字号缩小；Windows 与 macOS 复用同一套尺寸并由 GPUI 处理系统缩放。

全应用共用控件遵循同一尺寸：按钮 32、输入框与筛选框 36、普通表格行 36、正文 13、说明文字 12、标签 11 逻辑像素。下拉选项和复选框采用正文尺寸；按钮图标作为固定 16 像素内容绘制，避免组件库按按钮高度重新放大。日期面板沿用 Kit 的 CalendarState 与键盘/选择逻辑，单独指定日期网格文字和点击尺寸。模型选择弹窗高度随窗口调整，供应商页窄工具栏允许换行。

标题栏在“开机启动”前提供衣服图标的“皮肤设置”。预设黑白（默认）、蓝白、紫藤、玫瑰、琥珀、青绿六套浅色主题，点击后即时应用，并将选择原子保存到配置目录下的 `appearance.json`。下次启动恢复选择；缺失、损坏或不支持的配置回退到黑白。外观设置独立于供应商配置和统计筛选，成功/警告/错误与图表系列保持语义颜色。Windows 与 macOS 复用相同的主题和 SVG 图标实现。
