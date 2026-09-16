# Phase 4d acceptance — live Log tab accepted with check 3 open (deployed 2026-09-16 08:10 UTC; checks run 08:19–08:25 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-4d-live-log` (`49597fd`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Gaming PC:** unchanged by this phase; `qf-helper` was not touched and was not involved in any of these checks.
- **Local gate (at `49597fd`):**
  - Tests pass: `utils` 3 (including the new `core::tests::tail_returns_the_newest_lines_oldest_first_and_the_sink_sees_clean_lines`), qf_core 187, qf-server 19 (4 unit, 15 integration).
  - `78 server commands, 78 used by web, 0 missing` (77 in 4c; `log_tail` is the new one).
  - `pnpm build` (tsc and vite) is clean, built in 670 ms. Only pre-existing `unused import` warnings; nothing new.
- **Deploy:**
  - `rsync -av --delete` moved 156 KB (the dry-run listed the whole tree only because this is a different worktree from the one that deployed 4c, so every mtime differs). The deletion list was 4 entries, identical in the dry-run and the real run: `crates/qf_core/logs/2026-09-14/trader_item.log`, `crates/qf_core/logs/2026-09-14/log.log`, `crates/qf_core/logs/2026-09-14/` (stale in-tree runtime logs from an earlier host-side run) and `web/src/components/Popups/` (an empty leftover directory, emptied in the repo at `34f5f13`). 4d removes no files of its own. `backups/` (1777), `secrets/` (0700) and `.env` were confirmed untouched; `docker compose config -q` valid.
  - **Healthy in 5.2 s:** `StartedAt` 2026-09-16T08:10:36.334Z, first health check passed 08:10:41.560Z.
  - Boot log (17 lines): `[Db:Connect] Database ready` (08:10:37), `Loaded 3840 tradable items`, `[Collector] Started: 3840 items, 27 hot`, `[Housekeeping] Started: tick 60 s, backups in /backups` (08:10:40), `[Startup] Core started`, `quantframe-server listening on 0.0.0.0:8080`, first tick `alerts 0, deleted events 0, backup -`. No panic, no `CRITICAL`, no `ERROR`-level line, **no `Trader started`** at boot. The only line matching `Error` is `[Emit:SendEvent:App:Error] Event: message (0 receivers)` — `clear_error!` clearing the UI error state at startup (`crates/qf_core/src/macros.rs:52-61`), not a failure.
- The trader was in dry-run for the whole acceptance window; nothing was started live and no real order was touched.

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Live Scraper → Log: the last lines appear at once on opening the tab | Pass | The boot lines from 08:10:37 `Database ready` onward were already in the pane when the tab opened |
| 2 | A line arrives live without reloading | Pass | Deviation from the plan text: an idle server logs nothing, and the housekeeping tick only logs when it did work (`run_loop` in `crates/qf_core/src/housekeeping/mod.rs:106` prints the summary only when there were alerts, deletions or a backup), so the 60 s tick never fires a line on a quiet database. The line was driven instead: pressing Start then Stop on the trader in dry-run at 08:19:39–08:19:48 produced `Trader:Start` "Trader started (dry-run)", the dry-run cycle and `Trader:Stop` "Trader stopped: Stop button", all of which appeared in the pane live |
| 3 | Scrolling up pauses auto-scroll and "Jump to latest" resumes it | **Fail, accepted open** | The user reported "the autoscroll doesn't seem to work" and chose to accept the phase with this open ("Move on and pin it for a future session"). Root cause not investigated. Follow-up 1 |
| 4 | Turning Info off hides the Info lines; all toggles off shows the no-match empty state | Pass | Info off hid the Info lines; with every level off the pane read "No lines match the selected levels." (the `no_match` string added in `49597fd`) |
| 5 | Clear empties the pane | Pass | |
| 6 | Leaving the tab and returning re-fetches the tail | Pass | |
| 7 | The pane never exceeds 2000 lines | Covered by review | Not exercisable on this server in the acceptance window (the idle server does not produce 2000 lines). The cap is enforced on both paths in `web/src/pages/live_scraper/Tabs/Log/index.tsx`: the socket append (line 34) and the tail load (line 41) both `.slice(-CAP)` with `CAP = 2000`. Verified by the task and final reviewers |
| 8 | No recursion: `docker compose logs` shows no `Emit:SendEvent` line per log line | Pass | Four readings of `docker compose logs --since 2m`, total lines and `Emit:SendEvent` count: 08:11:27 — 18 / 6; 08:12:29 — 18 / 6; 08:12:35 — 18 / 6; 08:13:14 (window now past the boot burst) — 1 / 0. Flat: neither the emit count nor the total grows, so the sink is not feeding itself. The 6 emits are the boot ones (`App:StartingUp` ×2, `App:Error`, `User:Update`, `Lifecycle:State`, `LiveScraper:UpdateRunningState`) |

Checks 1–2 and 4–6 were confirmed by the user in the browser ("Everything else is a success"). The phase is accepted with check 3 open at the user's request.

## Rulings made during execution

- Tab label key is `pages.live_scraper.tabs.log.title` rather than the plan's plain-string `tabs.log` — every sibling under `tabs` is `{ title }`.
- The tab subscribes to the raw `log` channel with `listen()` from `@api/socket` rather than `useTauriEvent`, which is typed to the `TauriTypes.Events` enum and does not cover `log`.
- Spec L6 was corrected in prose rather than gating `console: false` lines out of the sink and `tail` — threading the console flag into `CachedLogEntry` is more change than the risk warrants for a single-operator deployment.
- The broadcast buffer went from 1024 to 8192 slots because log frames now share it with UI events; lag would otherwise evict trader and stock frames.
- The minor "all levels off looks empty" was folded into the fix wave rather than deferred — it would have read as a broken socket during the check 4 filter test. The other eight minors stay deferred.
- Acceptance closes with check 3 open at the user's request.

## Note found during acceptance

The dry-run trader start logged fifty `Trader:Item:Delete` "Deleted order with ID: `<24-hex warframe.market id>` N/50" lines in about 40 ms, with no `DELETE` API line between them. These are simulated book deletes: under global dry-run `Orders::in_book` (`crates/qf_core/src/trader/orders.rs:162-163`) is true for every id, so `delete` removes the entry from the seeded in-memory copy and never calls warframe.market. No real order was touched. The wording is the problem, not the behaviour — it prints real order ids without saying "simulated" (follow-up 2).

## Follow-ups

1. **Auto-scroll in the Log tab does not work** (check 3, open, pinned for a future session). *Closed 2026-09-16: the user re-tested and confirmed it works; no code change was made.* Two candidates from the review notes: the scroll effect's deps do not include `levels`, so toggling a level can leave the view mid-buffer unpaused (`Log/index.tsx:54-57`); and the pane's `calc(100vh - 320px)` box (`Log/index.tsx:98`) may not be the element that actually scrolls. Neither was confirmed — the root cause was not investigated.
2. **Dry-run delete log wording.** A dry-run trader start prints "Deleted order with ID: `<real wfm id>`" for simulated book deletes (see the note above). Say "simulated" in the line, or suppress it under dry-run.
3. **`RequestError.content` is logged unmasked** and therefore appears in full in the second (full-context) line of a large error in the Log tab. This is the masking item the plan asked to add; pre-existing, not changed by 4d.
4. **Spec L6 wording nit** from the re-review: L6 says "the sink and `tail` both read `CACHED_LOGS`"; the sink is fed directly by `dolog` and does not read the cache.
5. **Deferred minors from the 4d reviews** (all judged follow-ups, none merge-blocking): the `log_tail` rpc test's `len() <= 5` assertion is trivially true on an empty cache; the events log-frame test drains with `try_recv().ok()`, which also stops on `Lagged`; lines logged between subscribe and the tail returning appear twice in the overlap; a rejected `log_tail` only reaches `console.error` with no user-visible signal; `pages.live_scraper.log.title` ("Server log") is defined but never rendered; the auto-scroll effect deps lack `levels`; index keys rewrite every row once the cap shifts; there is no re-tail after a socket reconnect; and every browser receives every log frame whether or not the Log tab is open (spec-level).
6. **`auto_delete` is still on** in the server settings; a live start is now refused until it is turned off (H1). Decide before go-live whether the first live start should be a clean slate.
7. **Idle cycle cost** (phase 3 follow-up 4) still stands.
8. **Buy, sell and wish-list decisions haven't run on live data yet** (phase 3 check 5). Re-check once items turn warm (from about 2026-09-22).
9. **Phase 2 time-based checks** 3–5, 7 and 8 are still open.
10. **Known limitation.** Presence and trades assume one gaming PC.
11. **Partly closed.** `apply_failed` and stranded applies now alert; a review re-apply after a partial apply can still double-apply the items already written, and the modal says so.
12. **Test-fidelity note** from 4b stands: `wrapped_mod_name.log` and `split_end_marker.log` contain no actual split.
13. **Unexercised by a real trade:** a purchase matching a wish-list row; closing or lowering a real WFM order on a trade.
14. **Configure the `On Alert` Discord webhook** (Settings → Notifications → On Alert) — per-installation; without it alerts are toast and log only.
15. **Backups live on ockohome only** (no off-site copy); the folder is world-writable (1777) because the deploy user has no passwordless sudo for `chown 10001`.
16. **Deferred minors from the 4c reviews:** `needing_alert`'s skip warning does not name the event id; `list`/`get` still fail on a corrupt event row (the Trades tab page containing it); a prune error after a good backup is reported as a backup failure; only `'` is guarded in the backup path; a `.tmp` may linger if removing a bad copy fails; `Gates` reset on a supervised restart; backup failure logs at error level (no `critical()` helper exists); `alert_variables`' `<KIND>` else-branch; `apply_reviewed` does not re-stamp `applying` before re-applying, so a crash mid-re-apply of an ordinary-reason row is never swept.
17. **Repo hygiene:** add `rustfmt.toml` with `max_width = 150` (default `cargo fmt` rewrites 62 files).
18. **From the same-day layout hotfix (branch `hotfix-live-scraper-layout`, deployed):** the Live Scraper table header no longer sticks at page size 50/100 (mantine-datatable 8 has no sticky-header option); the same `calc(100vh - var(--offset))` pattern remains on the Trade Messages, Trading Analytics and Debug pages.
