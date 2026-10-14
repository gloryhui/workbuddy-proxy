# GPUI Preview 01 (unreleased)

A separate test build with one shared GPUI interface on macOS and Windows.

The relay and tray controller do not link GPUI. Opening settings starts a separate GPU-backed process; closing settings persists private unsaved drafts and releases that process. The relay continues to run. Use the tray/menu-bar Quit command to stop the entire application.

Providers, model discovery/selection, editable model names and IDs, reasoning settings, WorkBuddy sync/removal, local port settings, bilingual localization (one language at a time), connection diagnostics, and log clear/copy use the existing local authenticated management API.

No bundled browser, WebView, JavaScript runtime, custom fonts, syntax grammars, or full icon catalog. Model and discovery lists use virtual scrolling. The default light theme and spacing are shared across both systems.

Settings drafts live in `gpui-drafts.json` in the private application data directory. They may contain API keys, just like `config.json`. Drafts are discarded when the underlying saved configuration changes externally.

This is a review build, not a formal GitHub Release. Package sizes, measured process RSS, renderer compatibility, and package-level checks are recorded separately; CI measurements do not constitute testing every physical Windows GPU or remote-control setup.

GPUI and GPUI Kit are Apache-2.0 software. https://github.com/zed-industries/zed/tree/main/crates/gpui and https://github.com/longbridge/gpui-kit
