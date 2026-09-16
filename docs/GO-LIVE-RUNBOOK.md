# Go-live runbook

Turning global dry-run off is the last step of phase 5 (spec §21). It is a manual, one-time action. Do not do it before **2026-09-22**: the spec requires at least 7 days of dry-run on the server (dry-run started 2026-09-15) and items only become `warm` (7 days of history and at least 10 probable trades in 7 days) from about that date. Before that, nothing routes live even with the flag off, and the dry-run log has nothing to review.

The trader routes per item: with global dry-run **off**, a warm item is traded live and a not-warm item is still simulated (`forced_by = not_warm`). So the Dry-run log keeps filling after the flip. That is expected.

## 1. Pre-flight (all must hold)

- [ ] It is 2026-09-22 or later.
- [ ] **Market Data → Price history** shows the green **Warm** badge for the items you expect to trade (a not-warm item reads **Warming up (N of 7 days)** instead).
- [ ] **Live Scraper → Dry-run log → Summary**, with the selector on **Last 7 days**, shows `create` and `update` rows in **By action** (not only `delete`), and the **Busiest items** min/max prices are plausible next to the **Median (7 d)** figures on Market Data → Price history for the same items. The only `delete` rows should be `AutoDelete` on Start (see follow-up 2 in `docs/PHASE-4D-ACCEPTANCE.md`) and `Knapsack`; a delete count far above the create count means the settings need a look before going live.
- [ ] **Settings → Live Scraper → General → Auto Delete** is **off**. The Trader panel checklist row *"auto_delete is off (on, it deletes every non-blacklisted order on a live start)"* is green. (Decision 2026-09-16: the first live start adopts the existing real orders rather than deleting them. A live start is refused while it is on.)
- [ ] **Trader panel → Delete buy orders on stop** is set the way you want it for live (off keeps buy orders on warframe.market when the trader stops; on deletes them).
- [ ] **Settings → Notifications → On Alert (failed applies, backups)** has the Discord webhook, and **On Trader Stopped** as well.
- [ ] Today's backup exists on ockohome: `ls ~/stacks/quantframe-server/backups/quantframe-$(date -u +%F).sqlite`.
- [ ] The checklist rows *"qf-helper connected (heartbeat in the last 30 s)"* and *"Warframe running on the gaming PC"* are both green. Keep them that way for the first hour.
- [ ] The warframe.market token does not expire within 7 days. The expiry is not shown in the UI; the **On Sign-in Expiring** notification covers it, so the check is that no such alert has arrived and the checklist row *"warframe.market sign-in valid"* is green.

## 2. The flip

1. Open **Live Scraper**. In the Trader panel turn **Dry-run** off. The badge must read **Ready**; if it reads **Offline**, a checklist row is red — fix it, do not force anything.
2. Press **Start**. Open the **Log** tab.
3. Within the first cycle you should see, in this order: `Trader started (live)`; `Checking items...`; `Processing Item: … | Route: Live` for warm items and `Route: DryRun(NotWarm)` for the rest. No `Simulated delete of order` or `Deleted order with ID` lines are expected because **Auto Delete** is off. Setting the warframe.market status to `ingame` happens on Start but is not logged — check your warframe.market profile shows you as in-game instead.
4. Open your warframe.market profile in another tab. The first live `create` or `update` from the log must be visible there within a minute. Note its order id for the acceptance record.

## 3. First hour

- Watch the **Log** tab for `Trader stopped:`. The reason is in the Trader panel (**Last stop:**) and on Discord (**On Trader Stopped**). `OrderFailures(5)` means five consecutive warframe.market write failures; `Critical` means a parsing or bad-request error. Either way: read the lines before it, fix the cause, and Start again.
- The Dry-run log summary keeps growing under `not_warm` in the **Forced by** column; that is normal. `global` rows must not appear after the flip.
- Compare the **Trades** tab and your warframe.market order list once: every live create/update in the log should match an order there.

## 4. Rollback

1. Press **Stop**. Turn **Dry-run** on. Press **Start** if you want simulation to continue.
2. Orders the trader created live stay on warframe.market. Either delete them by hand on the site, or set **Delete buy orders on stop** on, Start once with **Dry-run** off, and Stop: the stop sequence deletes the real cached buy orders only when that setting is on *and* the run was live (spec §16 C9). Sell orders are not touched by that setting.
3. If the database looks wrong, restore last night's backup with the README's **Restore** steps.

## 5. Record it

Append the outcome to `docs/PHASE-5-ACCEPTANCE.md` under "The flip": date and time (UTC), the first live order id, the count of `Route: Live` items in the first cycle, whether the trader stopped in the first hour and why, and any manual order cleanup you did.
