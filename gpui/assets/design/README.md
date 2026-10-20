# Statistics header artwork

Extracted from the supplied `数据统计@2x.png` (2560 × 3080), SHA-256:
`3f828467222313b3d44bcd15c817b38e8cecfe57794a471acdbf608bf5b56179`.

Each PNG is 32 × 32, rendered at 16 logical pixels on both macOS and Windows.
The original stroke pixels define the glyph mask; the flat background is removed
in linear sRGB, retaining antialiasing. Dark glyphs use the sampled `#5c6679`;
selected source glyphs use white. No image generation or icon substitution is used.

Original crop origins (top-left, 32 × 32):

| Asset | x | y |
| --- | ---: | ---: |
| header-power | 2124 | 32 |
| header-refresh | 2192 | 32 |
| header-globe | 2260 | 32 |
| header-fullscreen | 2328 | 32 |
| header-github | 2396 | 32 |
| header-user | 2464 | 32 |
| source-all | 1893 | 161 |
| source-builtin | 2099 | 161 |
| source-custom | 2305 | 161 |

Regenerate with `scripts/extract-design-header.swift` using the original PNG.
The source's page/segment/selection colors decode to `#f7f8f9`, `#f1f2f5`,
`#00b29c`. Toolbar hit areas are 32 logical pixels with a 2-pixel gap.

Per the requested terminology, source labels are 全部模型 / 内置模型 / 自定义模型.
There is no client dropdown. Existing action semantics remain: launch at login,
check updates, language, restore window size, GitHub project, feedback.
