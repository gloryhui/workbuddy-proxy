# GPUI Kit Preview 02 (unreleased)

A separate test build with one shared GPUI interface on macOS and Windows.

The relay and tray controller do not link GPUI. Opening settings starts a separate GPU-backed process; closing settings persists private unsaved drafts and releases that process. The relay continues to run. Use the tray/menu-bar Quit command to stop the entire application.

Providers, model discovery/selection, editable model names and IDs, reasoning settings, WorkBuddy sync/removal, local port settings, bilingual localization (one language at a time), connection diagnostics, and log clear/copy use the existing local authenticated management API.

No bundled browser, WebView, JavaScript runtime, custom fonts, syntax grammars, or full icon catalog. Model and discovery lists use virtual scrolling. The default light theme and spacing are shared across both systems.

Settings drafts live in `gpui-drafts.json` in the private application data directory. They may contain API keys, just like `config.json`. Drafts are discarded when the underlying saved configuration changes externally.

This is a review build, not a formal GitHub Release. Package sizes, measured process RSS, renderer compatibility, and package-level checks are recorded separately; CI measurements do not constitute testing every physical Windows GPU or remote-control setup.

GPUI and GPUI Kit are Apache-2.0 software. https://github.com/zed-industries/zed/tree/main/crates/gpui and https://github.com/longbridge/gpui-kit

## Component coverage

All settings-page interactive controls use GPUI Kit 0.7.3: `Button`, `Input` (including mask toggle), `Select`, `Checkbox`, `Sidebar`/`SidebarMenuItem`, virtualized `DataTable`, `ListItem`, `Dialog`/`DialogContent`/`DialogFooter`, `Label`, `Tag`, `TitleBar` and `StatusBar`. Provider counts appear in a compact tag; the active provider uses the shared teal theme. Existing models in discovery are checked and disabled, with an Added label.

Layout-only surfaces and spacing use GPUI containers. A GroupBox was evaluated but its nested intrinsic-height wrappers clipped flexible log panels, so the settings surfaces retain flexible layout containers. They are not OS-native input or list controls. Native tray integration remains small and resident; closing settings releases the GPUI process.

The DataTable delegates row selection to the application to preserve Shift ranges, Ctrl/Command toggle, Delete/Backspace, and double-click editing. Both configured and discovered models remain virtualized.
