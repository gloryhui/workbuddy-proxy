# Historical Studio font provenance

Studio V2 now inherits the operating system UI font and native glyph fallback.
No Noto Sans SC or Inter font files are embedded, registered, installed or
fetched at runtime. Font selection lives in `gpui/src/theme.rs`; text roles
retain their existing sizes, weights, line heights and tabular figures.

The seven former font assets were removed for Issue #4. [manifest.json](manifest.json)
records their historical sizes, hashes and upstream provenance; it is an archive,
not a build input or a list of files still distributed. The original
[Noto license](Noto-LICENSE.txt) and [Inter license](Inter-LICENSE.txt) are retained
for attribution and repository history. These provenance files are not packaged
by `scripts/package.py`.
