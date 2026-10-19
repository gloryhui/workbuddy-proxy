# T07-L03 per-page size acceptance

## Result

The requested page-size recalculation slice passes on macOS with an isolated 60-row synthetic request fixture. Changing from 20 to 50 rows per page resets the visible page to **1/2**; moving to the final 50-row page shows **2/2** and disables Next. Changing from that page to 100 rows per page resets to **1/1** and disables both page controls. No product source files were changed.

This is a scoped result for `UI-T07-L03-PAGESIZE-001`, not a claim that all of L03 passed. The row-detail interaction was outside this task and remains unverified.

## GUI evidence

All screenshots are 1440×901. The relevant captured states are:

| Evidence | Observed state |
|---|---|
| `shots/00-page1-20.png` | 20/page, page 1/3; first visible request IDs descend from `0060`. |
| `shots/02-page3-20.png` | 20/page, page 2/3; visible IDs descend from `0040`. The filename is historical; the image shows page 2/3. |
| `shots/03-prev-page2-20.png` | Previous returns from page 2/3 to page 1/3. |
| `shots/04-page3-20-again.png` | Next returns to page 2/3. |
| `shots/05-page1-50.png` | After opening the page-size selector and choosing 50, the UI resets to page 1/2. |
| `shots/06-page2-50.png` | Page 2/2, Next disabled. |
| `shots/07-page1-100.png` | After choosing 100, the UI resets to page 1/1 and both page controls are disabled. |

The first Next-page action's snapshot (`shots/01-page2-20.png`) still showed page 1/3. A later Next action showed page 2/3. This is recorded as an AX snapshot timing caveat; the initial screenshot is not counted as evidence of a page advance. The GUI run did not capture page 3/3.

## API evidence

`core-api-samples.json` records 60 total rows. At page sizes 20, 50, and 100 it returned 20, 50, and 60 rows respectively. The 20-row cursor walk returned three pages: IDs `0060–0041`, `0040–0021`, and `0020–0001`, with the cursor absent on the last page. The 50-row response retained a cursor for the remaining 10 rows; the 100-row response had no next cursor.

The API walk covers all three 20-row pages and the last-page cursor boundary. The GUI screenshots independently show page-size reset behavior and the 50-row last-page disabled state.

## Build and environment

The task captured `logs/build-core.log` and `logs/build-gpui.log`; both application binaries were launched from the worktree's existing `target/` and `gpui/target/` caches with an isolated config directory. No `cargo clean` was run. The isolated core and GPUI processes were stopped before cleanup.

The session-owned temporary directory was `/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp`. After confirming both application processes had exited and no open files referenced the directory, it was removed (5,615,498 file bytes; approximately 6.5 MiB allocated). No files remain at that session path. The system volume reports 200 GiB available. The build caches remain in place.

## Provenance

Worktree branch: `codex/ui-redesign`; HEAD at task start and review: `f5e69e719a9b597b1bf57a952373bcb24ffddcdf`. This round added only acceptance evidence under this directory. Existing `docs/UI-UX-renew` changes and design artifacts were left untouched.
