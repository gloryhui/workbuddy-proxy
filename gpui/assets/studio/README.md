# Studio V2 system toolbar

Seven original 20×20 SVG exports from Figma file `zxDfIQ4lhNcTXrFT7qk8QZ`, component `103:442`, retrieved through the official Figma MCP on 2026-10-28. Export root dimensions and viewBoxes are preserved. They are embedded through `Assets`, with no runtime network dependency.

Order and layer slots: skin `103:444`, autostart `103:447`, updates `103:451`, language `103:455`, restore `103:459`, GitHub `103:462`, feedback `103:466`. `ui_components::system_button` renders each vector at 20×20 in a 36×36 native Kit button; `Proxy::app_controls` uses 8px gaps (300px total). The appearance trigger uses the same wrapper inside its existing popover. Actions remain owned by the existing handlers.

`provider-search.svg` is the original 17×17 export from search layer `80:460` (supplier rail `80:456`, parent frame `80:418`), also shared by the 64-provider search state `79:438`. It is embedded at its native dimensions as the existing Kit input's prefix.

The sidebar exports added from node `54:4` on 2026-10-28 are `brand.png`, `nav-{chart,server,settings}.svg` (18px), `relay-activity.svg` (18px), `relay-link.svg` (15px), `supplier-{selected,default}.svg` (7px), and `supplier-{add,copy,delete}.svg` (16px). `ui/sidebar.rs` uses each at its original slot and SVG dimensions. Native OS controls continue to be real TitleBar controls rather than the design's decorative traffic-light vectors.

Provider content `54:203` supplies `profile-edit.svg` (16px, layer `75:408`) and `model-edit.svg` (15px, model row edit slot `54:264`). They are embedded in native buttons in `ui/providers.rs` and `model_table.rs`, with their SVG dimensions preserved.

Runtime center `54:298` supplies `runtime-activity.svg` (19px, layer `54:299`) and `runtime-copy.svg` (16px, layer `54:305`). Logs `54:320` supplies `log-terminal.svg` (19px, layer `54:321`). These original exports are embedded in native GPUI elements; the status, address, counters and log text come from the existing core snapshot.

Statistics filter row `52:87` supplies `stats-refresh.svg` (18px, layer `52:104`) and `stats-chevron.svg` (16px, layers `52:90/94/98/102`). They are embedded in the existing refresh and date/interval controls; the native model/provider Select controls keep their own keyboard and focus engine.

Overview KPI grid `52:107` supplies `kpi-grid.svg`, `kpi-shield.svg`, and `kpi-activity.svg` (18px each; layers `52:130`, `52:139`, `52:146`). The main Token surface and cards are native GPUI. The hero's bar area uses real recent dashboard time buckets; it never embeds the design's fixed decorative values as usage data.
