# Studio fonts

The original, unmodified upstream faces used by the Studio V2 Figma typography
are embedded in the native UI. They are loaded once, on first use of the Studio
Indigo skin. Other skins retain their original native family. No fonts are
installed in the user's Fonts directory or registered for other processes.

- Noto Sans SC Regular / Medium / Bold (400 / 500 / 700), from
  [noto-cjk at f8d1575](https://github.com/notofonts/noto-cjk/tree/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/SubsetOTF/SC).
  Embedded copyright: © 2014–2021 Adobe (http://www.adobe.com/).
  The original license is in [Noto-LICENSE.txt](Noto-LICENSE.txt).
- Inter Regular / Medium / SemiBold / Bold (400 / 500 / 600 / 700), from the
  original `extras/ttf/` faces in the
  [Inter 4.1 release](https://github.com/rsms/inter/releases/tag/v4.1).
  Copyright (c) 2016 The Inter Project Authors (https://github.com/rsms/inter).
  The original license is in [Inter-LICENSE.txt](Inter-LICENSE.txt).

[manifest.json](manifest.json) records the original file sizes, hashes, Noto Git
blob identities and Inter distribution archive hash. No fonts were subsetted,
renamed or regenerated. The native UI requires no font downloads at runtime.

Static faces are intentional: the pinned GPUI Apple text system chooses among
loaded faces by weight rather than selecting the `wght` axis of one variable
face. Inter is applied to the inspected sidebar captions/address, overview KPI
figures/date axis/cost/recent time, request time/Token totals, and aggregate
counts/ratios/card figures. Noto supplies headers, names, Chinese labels and
localized numeric units. The remaining frames' explicit font roles and all
rendered geometry still need review. This does not complete typography acceptance.

On macOS, GPUI's memory source alone does not make named fallback faces visible
to CoreText's cascade matching. The UI therefore registers the three embedded
Noto graphics fonts within its own process. The memory API is deprecated on
macOS 15, but the descriptor-based alternative rejects the same memory-only
descriptors with error 303 in the native probe. This compatibility bridge avoids
extracting font copies to disk. Existing registered faces are left intact.

Native font parsing/name/glyph tests do not establish full-frame appearance,
Windows behavior, font-version equality with Figma, or startup/RSS performance.
Those remain separate runtime acceptance checks.
