# WorkBuddy Proxy desktop

The maintained desktop application with one shared GPUI interface on macOS and Windows.

The relay and tray controller do not link GPUI. Opening settings starts a separate GPU-backed process; closing settings persists private unsaved drafts and releases that process. The relay continues to run. Use the tray/menu-bar Quit command to stop the entire application.

Providers, model discovery/selection, editable model names and IDs, reasoning settings, WorkBuddy sync/removal, local port settings, bilingual localization (one language at a time), connection diagnostics, and log clear/copy use the existing local authenticated management API.

No bundled browser, WebView, JavaScript runtime, custom fonts, syntax grammars, or full icon catalog. Model and discovery lists use virtual scrolling. The default light theme and spacing are shared across both systems.

Settings drafts live in `gpui-drafts.json` in the private application data directory. They may contain API keys, just like `config.json`. Drafts are discarded when the underlying saved configuration changes externally.

Formal Windows distribution uses a Chinese EXE installer; Mac distribution uses DMG. Package sizes, measured process RSS, renderer compatibility, and package-level checks are recorded separately; CI measurements do not constitute testing every physical Windows GPU or remote-control setup.

GPUI and GPUI Kit are Apache-2.0 software. https://github.com/zed-industries/zed/tree/main/crates/gpui and https://github.com/longbridge/gpui-kit

## Component coverage

All settings-page interactive controls use GPUI Kit 0.7.3: `Button`, `Input` (including mask toggle), `Select`, `Checkbox`, `Sidebar`/`SidebarMenuItem`, virtualized `DataTable`, `ListItem`, `Dialog`/`DialogContent`/`DialogFooter`, `Label`, `Tag`, `TitleBar`. Provider counts appear in a compact tag; the active provider uses the shared teal theme. Existing models in discovery are checked and disabled, with an Added label.

Layout-only surfaces and spacing use GPUI containers. A GroupBox was evaluated but its nested intrinsic-height wrappers clipped flexible log panels, so the settings surfaces retain flexible layout containers. They are not OS-native input or list controls. Native tray integration remains small and resident; closing settings releases the GPUI process.

The DataTable delegates row selection to the application to preserve Shift ranges, Ctrl/Command toggle, Delete/Backspace, and double-click editing. Both configured and discovered models remain virtualized.

## Interface behavior

The development version adds per-model [WorkBuddy capability settings](MODEL-CAPABILITIES.md) to the model editor: images, tools, reasoning options, token/context limits and complete Chat Completions URL mode. Defaults enable image input for DeepSeek V4.1 Flash; explicit per-model overrides survive drafts, saving, duplication and sync. Other media JSON and response bytes remain transparent; this does not add native audio/video APIs or protocol conversion.

Version 0.5.2 adds an opt-in **Launch at login** checkbox in the header. New configurations and existing configurations without this setting default to off. The authenticated core command persists the selection and updates only this user's Windows `Run/WorkBuddyProxy` value or macOS `~/Library/LaunchAgents/io.github.gloryhui.workbuddy-proxy.autostart.plist`; a registration failure rolls back the saved choice. Login launches use `--autostart --config-dir DIR`, start the resident tray/core without settings, and do not wake an already-running settings window. Normal manual launches still show settings. There is no keep-alive restart after Quit. Windows uninstall removes the app's Run value while retaining user configuration.

Provider and log operations use compact Kit ghost icon buttons with localized tooltips; the sidebar footer no longer applies a whole-toolbar hover background. Sync is the primary action and removal is shortened to Unsync. Local port, the live relay address and provider/model/request counts share one service card.

Model dialog Enter uses the same validation/submission as Confirm; missing IDs keep the dialog open. The bottom status bar is removed; only a muted tray hint remains at the lower right, while failures appear beside settings. The model table has display-name and model-ID columns plus a fixed-width edit-icon column. Clicking an edit icon opens that row's editor without changing selection; double-click and keyboard editing remain available. The Kit trailing placeholder is overridden to zero width; widths come from the actual table container.

Log entries use single newlines and a 16px line height. New content scrolls to the bottom after layout; polls without new content preserve the viewport. The existing core keeps its latest 100 entries rather than growing memory without a bound.

Global header controls are ordered Launch at login, Check updates, Language. The language selector has a visible label and compact width; the version appears as muted text at the end of the subtitle. Connection settings offer a searchable [provider preset catalog](PROVIDER-PRESETS.md) above the name and key fields. Presets fill the form and remain editable until saved.

## Windows installation

Use `workbuddy-proxy-0.5.3-windows-x64-setup.exe`. The wizard and uninstaller use Simplified Chinese on all Windows display languages. The directory selection page is always shown. Setup defaults to `%LOCALAPPDATA%\Programs\WorkBuddy Proxy`, installs for the current user, creates a Start Menu entry, and selects the desktop shortcut by default (users can uncheck it). Both shortcuts launch only `workbuddy-proxy.exe`.

The settings helper is named `internal/workbuddy-settings.exe`; do not launch or move it separately. Only the main executable is exposed through installation shortcuts; internal binaries must remain with the installation.

Close the old app using the tray Quit command before upgrading. The installer registers a stable uninstall entry. Uninstall removes installed program files and shortcuts, retaining `%APPDATA%\WorkBuddyProxy` configuration/logs. Installing to a protected directory requires adequate permissions; the per-user installer does not request elevation.

Inno Setup 6.5+ is a Windows build dependency only; neither portable nor installed applications need it or Python/Rust. The Chinese translation is vendored from the upstream Inno Setup `Files/Languages/ChineseSimplified.isl` (retrieved 2026-10-14, translator attribution retained). No compiler language-pack fallback is allowed. CI checks the active Chinese language, translated task text, desktop shortcut creation without a `/TASKS` override, a custom directory containing Unicode and spaces, repeat installation, shortcut targets, installed binary hashes, application lifecycle and uninstall/config retention. Interactive wizard and physical Windows GPU acceptance remain separate checks.

Automatic model capabilities are resolved by the core from provider metadata, the models.dev catalog and built-in fallback rules. Refresh runs in the background; forwarding never consults the catalog. The editor reads cached results asynchronously, displays the main source, and preserves manual overrides. See [MODEL-CAPABILITIES.md](MODEL-CAPABILITIES.md) for precedence, cache limits and offline behavior.
