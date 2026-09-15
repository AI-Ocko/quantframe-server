# Phase 3 acceptance — dry-run accepted (deployed 2026-09-15 06:43 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-3-trader` (`be70b32`)
- **Origin:** `http://ockohome:8080/live_scraper`
- **Local gate:** wf-market 2, qf_core 120, qf-server 9 tests pass; 71/71 RPC commands used by the web exist; `pnpm build` (tsc + vite) clean
- **Deploy:** rsync removed nothing; container healthy within 34 s; no panic, no `CRITICAL`, no `Trader started` at boot. The settings loader printed `Missing property: notifications.on_trader_stopped` once while filling the new notification settings with their defaults.

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | After deploy the panel is Offline or Ready, never Trading; Dry-run badge on | Pass | Confirmed by the user. Logs: `Core started` at 06:43:01, no trader start until the user's first Start at 06:46:59 |
| 2 | Helper override off: "Helper connected" is ✕ and Start is disabled | Pass | Confirmed by the user |
| 3 | Helper override on: every checklist item ✓, state Ready, Start enabled | Pass | Confirmed by the user |
| 4 | Start: state Trading, warframe.market status stays invisible | Pass | Confirmed by the user. Logs: `Trader started (dry-run)` at 06:46:59, 06:50:35 and 06:52:34 |
| 5 | Dry-run log shows rows with `forced_by = global`; real orders unchanged | Pass (limited) | Confirmed by the user; real orders unchanged on warframe.market. `dry_run_log` holds 150 rows, all `delete` / `AutoDelete` / `global`: `auto_delete` removed the user's 50 real orders from the simulated book on each of the 3 starts. No buy, sell or wish-list decisions ran live (0 `Processing Item` in 310 cycles): see follow-up 1 |
| 6 | Helper override off while trading: stops within 5 s with "Helper unavailable" | Pass | Logs: `Trader stopped: Helper unavailable (the helper override needs dry-run)` at 06:50:17 |
| 7 | Start again, then Stop: last stop "Stop button"; Discord message if a webhook is set | Pass | Confirmed by the user. Logs: `Trader stopped: Stop button` at 06:52:09 and 06:53:12; `trader_state.last_stop_reason = 'Stop button'` |
| 8 | `docker compose restart`: panel Offline or Ready, not Trading | Pass | Confirmed by the user. Logs: `Core started` at 06:53:49 with no trader start after it; `trader_state` kept `dry_run = 1`, `helper_override = 1` |
| 9 | Settings → Notifications shows the two new tabs and saves | Pass | Confirmed by the user |

## Follow-ups

1. **Live decisions not exercised yet.** At acceptance the collector had run for about 2 hours: 8,632 `item_stats` rows, none warm, highest volume 0.86/day against `wtb.volume_threshold` 15, so there were no buy candidates. The server also has no stock or wish-list items. The buy, sell and wish-list logic is covered by the golden tests in `trader::item`. Re-run the trader in dry-run once volumes build up (items turn warm from about 2026-09-22) or after follow-up 2, and check the Dry-run log for `create` and `update` rows.
2. **Import the user's existing desktop trading data** (user request, 2026-09-14). Source: `~/.local/share/dev.kenya.quantframe/quantframeV2.sqlite` on the desktop, holding `transaction` 280 rows and `stock_item` 16 rows (`wish_list`, `trade_entry` and `stock_riven` were empty). Rivens are out of scope. Not scheduled into a phase yet. Stock items change what the trader sells, so import them while dry-run is on.
3. **`auto_delete` is on in the server settings.** In dry-run it only simulated deleting the 50 real orders. Live, the same setting deletes every non-blacklisted real order when the trader starts (upstream behaviour). Decide whether to keep it before phase 5 turns dry-run off.
4. **Idle cycle cost.** With nothing to process, a cycle takes about 1 s, and each one reloads all `item_stats` rows plus stock and wish list (310 cycles in about 6 minutes). Consider a longer pause after a cycle that processed no items.
5. **Phase 2 check 10 evidence.** The restart at 06:53 showed the collector resuming (`Collector Started: 3840 items`) with `item_stats` kept. The other time-based phase 2 checks (3–5, 7, 8) are still open.
