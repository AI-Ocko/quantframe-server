# Phase 4b acceptance — trade events (deployed 2026-09-15 17:05 UTC; in-game checks pending)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-4b-trade-events` (`bb5ce4e`). The web UI's public origin is `http://quantframe.cgcorp.internal` (the plan's `http://ockohome:8080` is the container port behind it).
- **Gaming PC:** this desktop (`omarchy`, Arch Linux, Warframe under Proton). `qf-helper` 0.1.0 (release build of `bb5ce4e`) in `~/.local/bin`, run by the `systemd --user` unit `qf-helper.service`. `ee_log_path` is not set; the default under `$HOME` resolves to the real EE.log.
- **Local gate (at `bb5ce4e`):**
  - Tests pass: qf_log_parser 20 (11 unit, 9 fixture), qf-helper 23, wf-market 2, qf_core 166, qf-server 18 (3 unit, 15 integration).
  - All 77 RPC commands used by the web exist.
  - `pnpm build` (tsc and vite) is clean.
- **Deploy:**
  - rsync deleted nothing (28 new files, 28 changed, the rest timestamp-only).
  - The container was healthy within 30 s, the migration applied (`Database ready`), and the first hourly maintenance line already reports `deleted events 0`.
  - There was no panic and no trader start at boot.
  - `POST /helper/trade` answered 401 with no key.
  - The server was deployed before the helper was restarted, so no queued event could hit a 404.
- **Helper install:** the service restarted at 17:06 UTC and logged `heartbeat accepted` and `watching <EE.log> from byte 625151 (0 queued trade(s))`. `qf-helper --parse` on the live EE.log returned an empty array (no trade this session).
- Device keys and the web password never appeared in the conversation, logs or repo. Acceptance RPC calls were made on `ockohome` with a session created from the container's password secret; the session file was deleted afterwards.

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Real sale in game: `server: applied`, green toast, stock down, Sale transaction at game time, real WFM sell order closed or lowered, listed under Applied | **Pending** | Needs the user in game. This is the only end-to-end run of the parser → helper → server path on a real dialog |
| 2 | Real purchase in game: `server: applied`, item in stock or wish-list row bought, exactly one Purchase transaction | **Pending** | Needs the user in game. This is the only coverage of `HandlerApplier`'s wish-list-first purchase branch. A purchase that matches a wish-list row writes the transaction and decrements the wish list but creates no stock row (existing `wish_list_bought` behaviour) |
| 3 | Unresolved name from the queue, applied from Review | Pass | Queued `943e9e1f…` (`Paryy` for 1p, player `AcceptanceTest`). Journal: `trade detected: sale 1p with AcceptanceTest, 1 items; server: needs_review`; the row had reason `unresolved: Paryy`, direction sale, no items. `helper_trade_apply` with `parry` rank 0 ×1 @1p → `applied`, `matched_by: review`, `reviewed_at 18:29:54Z`; the server logged `HandleTransaction:SetDate → 2026-09-15T12:00:00Z` and emitted the three refresh events. Transaction 4 (`parry`, sale, 1p, `created_at 2026-09-15T12:00:00Z`) was then deleted; there was no stock row and no WFM order for Parry, so nothing else to restore |
| 4 | Replay returns `duplicate` | Pass | Same line queued again: journal `server: duplicate`; the unfiltered list shows the id once, still `applied` |
| 5 | No platinum side is ignore-only | Pass | Queued `d8561e1e…` (`Paryy` for `Adaptation`). Journal `trade detected: unknown 0p with AcceptanceTest, 2 items; server: needs_review`; reason `no_platinum_side`, direction null (Review disabled). `helper_trade_ignore` → `ignored`, reason `reviewed`, `18:31:30Z`. The server log shows only the toast; no handler ran |
| 6 | Kill switch | Pass | `auto_trade` set false through `app_update_settings` (confirmed on `/data/settings.json`). Queued `d6c593d0…` (`Parry`, correct spelling, 1p): landed `needs_review` with reason `auto_trade_off` and a fully resolved item (`parry`, `matched_by: name`). Ignored at `18:33:20Z`; `auto_trade` restored to true and confirmed on disk |
| 7 | Settings → Advanced → Log no longer shows the EE.log path; settings save; Helper devices and Dry-run log tabs load | Partial | Static evidence: `ee_log_path` appears in none of the 64 built chunks, the Log panel source has no reference, the served `en.json` carries `on_trade_event.applied/needs_review` and `pages.live_scraper.trades`, and the live_scraper chunk contains the review modal. **The browser walk-through is pending** (user) |
| 8 | `docker compose restart`: the Trades tab still lists every event; the helper's journal shows no dropped lines | Pass | Restarted at 18:34 UTC, healthy in 6 s, `Database ready`. `helper_trades` (all) after a fresh login: total 3 (`943e` applied, `d8561e` ignored, `d6c593` ignored). The helper journal shows no failures and no dropped lines across the restart; the queue is empty |

Event table after the checks: `applied` 1, `ignored` 2, `needs_review` 0.

## Follow-ups

1. **Pending acceptance.** Checks 1, 2 and the browser part of 7 need the user; update this record when they run. If check 2 matches a wish-list row, confirm that "transaction, no stock row" is the wanted meaning.
2. **Import the user's existing desktop trading data** (user request, 2026-09-14). Source `~/.local/share/dev.kenya.quantframe/quantframeV2.sqlite` (`transaction` 280 rows, `stock_item` 16 rows). Not scheduled yet.
3. **`auto_delete` is still on** in the server settings. Live, it deletes every non-blacklisted real order when the trader starts. Decide before phase 5.
4. **Idle cycle cost** (phase 3 follow-up 4) still stands.
5. **Buy, sell and wish-list decisions haven't run on live data yet** (phase 3 check 5, limited). Re-check once items turn warm (from about 2026-09-22).
6. **Phase 2 time-based checks** 3–5, 7 and 8 are still open.
7. ~~`log_settings.ee_log_path` leftover setting~~ — closed: the field was removed from Settings → Advanced → Log in 4b (the struct field stays so old settings files still load).
8. **Known limitation.** Presence and trades assume one gaming PC ("last heartbeat wins"); revisit if a second helper is added.
9. **Known limitation.** A partial `apply_failed` followed by a review apply re-applies the items that had already succeeded. The warning log names them so stock can be fixed by hand.
10. **Known limitation.** `helper_events` retention runs inside the collector's hourly maintenance, so it does not run when the collector is disabled (`QF_COLLECTOR=off`).
11. **Test-fidelity note.** The fixtures `wrapped_mod_name.log` and `split_end_marker.log` contain no actual split; the chunk-size invariance test (1, 7, 64 bytes and whole) is the real coverage of split writes.
12. **Fixed in 4b, no longer a limitation.** `handle_wish_list` now passes the caller's flags to `handle_transaction`, so a purchase that matches a wish-list row is dated at game time (`SetDate`) like the stock path.
13. **Deferred minors** from the per-task and final reviews (all judged follow-ups, none merge-blocking): `Queue::push` without `sync_all`; `SetCache::save` not atomic; `parse_line` prices an unparsable platinum amount as 1; the modal's `equalSplit` puts the whole remainder on row 0 while the server distributes it; `ItemIndex` name collisions are first-wins without a warning; `set_candidates`' prefix rule can add a superset root (one extra WFM round trip); `HandlerApplier`'s purchase branch has no automated test (needs the global database).
14. **Operational note.** Deploy the server before starting or restarting the helper: a 404 from `POST /helper/trade` makes the helper drop the queued event by design.
