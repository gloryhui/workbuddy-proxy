# GPUI Kit Preview 04 (unreleased)

A separate test build with one shared GPUI interface on macOS and Windows.

The relay and tray controller do not link GPUI. Opening settings starts a separate GPU-backed process; closing settings persists private unsaved drafts and releases that process. The relay continues to run. Use the tray/menu-bar Quit command to stop the entire application.

Providers, model discovery/selection, editable model names and IDs, reasoning settings, WorkBuddy sync/removal, local port settings, bilingual localization (one language at a time), connection diagnostics, and log clear/copy use the existing local authenticated management API.

No bundled browser, WebView, JavaScript runtime, custom fonts, syntax grammars, or full icon catalog. Model and discovery lists use virtual scrolling. The default light theme and spacing are shared across both systems.

Settings drafts live in `gpui-drafts.json` in the private application data directory. They may contain API keys, just like `config.json`. Drafts are discarded when the underlying saved configuration changes externally.

This is a review build, not a formal GitHub Release. Package sizes, measured process RSS, renderer compatibility, and package-level checks are recorded separately; CI measurements do not constitute testing every physical Windows GPU or remote-control setup.

GPUI and GPUI Kit are Apache-2.0 software. https://github.com/zed-industries/zed/tree/main/crates/gpui and https://github.com/longbridge/gpui-kit

## Component coverage

All settings-page interactive controls use GPUI Kit 0.7.3: `Button`, `Input` (including mask toggle), `Select`, `Checkbox`, `Sidebar`/`SidebarMenuItem`, virtualized `DataTable`, `ListItem`, `Dialog`/`DialogContent`/`DialogFooter`, `Label`, `Tag`, `TitleBar`. Provider counts appear in a compact tag; the active provider uses the shared teal theme. Existing models in discovery are checked and disabled, with an Added label.

Layout-only surfaces and spacing use GPUI containers. A GroupBox was evaluated but its nested intrinsic-height wrappers clipped flexible log panels, so the settings surfaces retain flexible layout containers. They are not OS-native input or list controls. Native tray integration remains small and resident; closing settings releases the GPUI process.

The DataTable delegates row selection to the application to preserve Shift ranges, Ctrl/Command toggle, Delete/Backspace, and double-click editing. Both configured and discovered models remain virtualized.

## Preview 03 adjustments

Provider and log operations use compact Kit ghost icon buttons with localized tooltips; the sidebar footer no longer applies a whole-toolbar hover background. Sync is the primary action and removal is shortened to Unsync. Local port, the live relay address and provider/model/request counts share one service card.

Model dialog Enter uses the same validation/submission as Confirm; missing IDs keep the dialog open. The bottom status bar is removed; only a muted tray hint remains at the lower right, while failures appear beside settings. The model table has two columns, with the Kit trailing placeholder overridden to zero width; widths come from the actual table container.

Log entries use single newlines and a 16px line height. Polling preserves the viewport scroll offset; the existing core keeps its latest 100 entries rather than growing memory without a bound. An isolated GUI test inserted 150 entries and exercised forward/backward scrolling and refresh while reading history.

## Windows installation (Preview 04)

Use `workbuddy-proxy-0.4.0-gpui-kit-preview.4-windows-x64-setup.exe`. The wizard and uninstaller use Simplified Chinese on all Windows display languages. The directory selection page is always shown. Setup defaults to `%LOCALAPPDATA%\Programs\WorkBuddy Proxy Preview`, installs for the current user, creates a Start Menu entry, and selects the desktop shortcut by default (users can uncheck it). Both shortcuts launch only `workbuddy-proxy.exe`.

The settings helper is named `internal/workbuddy-settings.exe`; do not launch or move it separately. The portable ZIP has only one EXE in its top-level folder. Keep its internal directory with the main executable. Older portable preview layouts are still recognized by the launcher.

Close the old app using the tray Quit command before upgrading. The installer registers a Preview-only uninstall entry. Uninstall removes installed program files and shortcuts, retaining `%APPDATA%\WorkBuddyProxy` configuration/logs. Installing to a protected directory requires adequate permissions; the per-user installer does not request elevation.

Inno Setup 6.5+ is a Windows build dependency only; neither portable nor installed applications need it or Python/Rust. The Chinese translation is vendored from the upstream Inno Setup `Files/Languages/ChineseSimplified.isl` (retrieved 2026-10-14, translator attribution retained). No compiler language-pack fallback is allowed. CI checks the active Chinese language, translated task text, desktop shortcut creation without a `/TASKS` override, a custom directory containing Unicode and spaces, repeat installation, shortcut targets, installed binary hashes, application lifecycle and uninstall/config retention. Interactive wizard and physical Windows GPU acceptance remain separate checks.
