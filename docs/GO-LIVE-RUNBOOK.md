# Go-live runbook

Turning global dry-run off is the last step of phase 5 (spec §21). It is a manual, one-time action. Do not do it before **2026-09-22**: the spec requires at least 7 days of dry-run on the server (dry-run started 2026-09-15) and items only become `warm` (7 days of history and at least 10 probable trades in 7 days) from about that date. Before that, nothing routes live even with the flag off, and the dry-run log has nothing to review.

The trader routes per item: with global dry-run **off**, a warm item is traded live and a not-warm item is still simulated (`forced_by = not_warm`). So the Dry-run log keeps filling after the flip. That is expected.

## 1. Pre-flight (all must hold)

- [ ] It is 2026-09-22 or later.
- [ ] **Market Data → Price history** shows the green **Warm** badge for the items you expect to trade (a not-warm item reads **Warming up (N of 7 days)** instead).
- [ ] **Live Scraper → Dry-run log → Summary**, with the selector on **Last 7 days**, shows `create` and `update` rows in **By action** (not only `delete`), and the **Busiest items** min/max prices are plausible next to the **Median (7 d)** figures on Market Data → Price history for the same items. `delete` rows carry the reason that caused them: `AutoDelete` (only on a Start with Auto Delete on), `Knapsack`, or the operation set that replaced an order (`Update,Delete,…`, normal when an item hits max stock or a price rule). A delete count far above the create count means the settings need a look before going live.
- [ ] **Settings → Live Scraper → General → Auto Delete** is **off**. The Trader panel checklist row *"auto_delete is off (on, it deletes every non-blacklisted order on a live start)"* is green. (Decision 2026-09-16: the first live start adopts the existing real orders rather than deleting them. A live start is refused while it is on.)
- [ ] **Settings → Live Scraper → General → Price source** is the one you have reviewed. `Inferred` needs nothing more. `Closed trades` needs at least **48 h of dry-run in that mode** with the Dry-run log Summary reviewed, and **Market Data → Price source** showing the closed statistics fetched for nearly all items with `failed` near 0. Change the source only while global **Dry-run** is on: switching to Closed trades makes every item whose closed statistics are warm eligible for live routing at once.
- [ ] **Settings → Live Scraper → Item → WTB**: **Price shift threshold** holds a value you chose on purpose; it applies in Closed trades mode, and only exactly `-1` disables it. **Trading tax cap has no effect yet**: the server does not load trade tax, so the trader also bids on items taxed at a million credits, as the desktop app does with its cap at `-1`. **Max Total Price Cap** (150 000) is the knapsack budget across all buy orders.
- [ ] **Settings → Advanced**: the debugging list of live item entries is empty. A debug entry list replaces the whole candidate list, and the orphan sweep (§3) would then delete every other buy order after 30 minutes.
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

- **About 30 minutes after a live Start** the Log tab shows `Deleted buy order <id> for <item> … its item is no longer a candidate` (reason `NotCandidate` in the Dry-run log while simulating). That is the orphan sweep: a buy order whose item has been neither a buy candidate nor on the wish list for 30 minutes is removed, so no bid is left standing at a stale price. Sell orders are never touched, and an item blacklisted for buying or for the wish list is left alone. On 2026-09-21 the dry run listed nine such adopted orders, all judged stale by the user: Akbolto Prime Set, Arcane Nullifier, Equilibrium, Volt Prime Neuroptics Blueprint, Akjagara Prime Barrel, Synoid Gammacor, Secura Penta, Hystrix Prime Set, Augur Reach. To keep bidding on an item the trader does not pick, put it on the wish list. A cycle with no buy candidates at all never sweeps; if the candidate list ever reads empty, fix that first.
- **Keep Max Buy Candidates about 30 below your account's order cap.** warframe.market limits how many orders an account can hold (about 150 on 2026-09-23). With no candidate limit the buy orders filled every slot, `has reached the order limit. Skipping.` appeared about a hundred times a cycle, and bought stock could not be listed for sale until a slot freed. Settings → Live Scraper → Item → WTB → Max Buy Candidates; 120 worked. The orphan sweep removes the surplus bids 30 minutes after the change.
- Expect cycles of about three minutes instead of two: the trader rewrites each of its orders every cycle, and live those rewrites share the three-requests-a-second limit with the order-book fetches. `has reached the order limit. Skipping.` means warframe.market's per-account order cap is binding below the 150-candidate list; it is harmless.
- Watch the **Log** tab for `Trader stopped:`. The reason is in the Trader panel (**Last stop:**) and on Discord (**On Trader Stopped**). `OrderFailures(5)` means five consecutive warframe.market write failures; `Critical` means a parsing or bad-request error. Either way: read the lines before it, fix the cause, and Start again.
- The Dry-run log summary keeps growing under `not_warm` in the **Forced by** column; that is normal. `global` rows must not appear after the flip.
- Compare the **Trades** tab and your warframe.market order list once: every live create/update in the log should match an order there.

## 4. Rollback

0. If the problem appeared after switching **Price source** to Closed trades, set it back to **Inferred**; it takes effect on the next trader cycle, with no restart.
1. Press **Stop**. Turn **Dry-run** on. Press **Start** if you want simulation to continue.
2. Orders the trader created live stay on warframe.market. Either delete them by hand on the site, or set **Delete buy orders on stop** on, Start once with **Dry-run** off, and Stop: the stop sequence deletes the real cached buy orders only when that setting is on *and* the run was live (spec §16 C9). Sell orders are not touched by that setting.
3. If the database looks wrong, restore last night's backup with the README's **Restore** steps.

## 5. Record it

Append the outcome to `docs/PHASE-5-ACCEPTANCE.md` under "The flip": date and time (UTC), the first live order id, the count of `Route: Live` items in the first cycle, whether the trader stopped in the first hour and why, and any manual order cleanup you did.
