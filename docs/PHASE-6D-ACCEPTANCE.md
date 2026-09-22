# Phase 6d acceptance — closed-trade price source (deployed 2026-09-20 18:48 UTC; follow-ups deployed through 2026-09-21 07:17 UTC; accepted 2026-09-22 00:58 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-6d-closed-price-source` (`8256177` deployed, merge base with main `0c62a6c`). Web UI origin `http://quantframe.cgcorp.internal`.
- **Local gate (at `8256177`):**
  - Tests pass: `utils` 3, `qf_core` 253, `qf-server` 19 (4 unit, 15 integration). Phase 6c had `qf_core` 209.
  - `89 server commands, 89 used by web, 0 missing`. Phase 6c had 88. The new one is `market_price_sources`.
  - `pnpm build` is clean. Rust warning counts are unchanged from `main` (`qf_core` 32, `utils` 1, `entity` 5, `service` 3, `migration` 3).
- **Deploys:** five, all run by the user with `!` commands, each with an empty deletion preview and each bringing the container to `healthy` within 25 seconds with `Db:Connect` ready, `Collector Started: 3840 items` and no panic.

| Deploy (UTC) | Commit | What it carried |
|---|---|---|
| 2026-09-20 18:48 | `9704579` | Tasks 1–4 and the final-review fix wave |
| 2026-09-20 22:10 | `7169b3f` | Task 6: max-rank buy candidates, language-file revalidation |
| 2026-09-21 00:21 | `9d61ab9` | Task 7: orphaned buy order sweep |
| 2026-09-21 01:27 | `956a704` | Task 8: the sweep's log line names the item |
| 2026-09-21 07:17 | `8256177` | Tasks 9–11: profit basis, candidate limit setting, trade tax fetch (adds migration `m20260922_000001`) |

- **Pre-deploy settings check (P12):** the user read **150 000** and **Price shift threshold = −1** from the web UI. **Correction, 2026-09-21:** the 150 000 is almost certainly **Max Total Price Cap**, not Trading Tax Cap: the desktop settings the server imported hold `max_total_price_cap = 150000` and `trading_tax_cap = -1`, and the two fields sit next to each other in the form. It does not matter which, because **the trading-tax filter is inert on the server** (see "Defect found after acceptance" below).
- ockohome now runs `956a704`. **Price source is `closed`, global dry-run is ON, Auto Delete is OFF** (user, 2026-09-20). The phase 5 flip is still pending, on or after 2026-09-22; closed mode needs 48 h of reviewed dry-run first, and that clock started 2026-09-20 21:45 UTC, less four restarts.

| # | Check (spec §25 P11–P14) | Result | Notes |
|---|---|---|---|
| 1 | Default mode after deploy changes no trading behaviour | Pass | Cycle size was 158 against 86 two days earlier; the nightly backups show the inferred candidate pool growing with history (16, 64, 106, 141 on 17–20 Sep) until it met the 150 cap, so the rise is data accumulation. No `FastDropGuard` in inferred mode |
| 2 | The 90-day import fills the closed table | Pass | `[Backfill] Finished: items 3840, days 516, missing 0, failed 0` at 21:24 UTC, then `[ClosedStats] Pass complete: ok 3840, missing 0, failed 0`. `days 516` counts chart-table rows only (K2 intact). It took 145 minutes, not the planned 70, because it ran beside the dry-run trader |
| 3 | Ash Prime Set agrees with warframe.market | Pass | The tab showed closed volume 46.0; the same figure computed directly from warframe.market for 13–19 Sep was 46.0 (moving average 68.8) |
| 4 | Calibration against the desktop app | Pass, method changed | The desktop UI shows no volume or moving average. Its local cache `ItemPrices.json` (refreshed the same day) holds upstream's price data: every `volume` is a whole number over seven, so upstream's unit is mean trades per day over a week, the unit P3 chose. Ash Prime Set reads 50.4 there against 46.0, a window offset and not a factor |
| 5 | Candidate counts are plausible | Pass | 150 inferred, 150 closed, 134 in both once every item had closed data. Both modes meet the 150 cap, so with the user's thresholds the switch changes candidate membership little; its effect is the price anchor and `warm` |
| 6 | Switch to `closed` in dry-run | Pass | 2026-09-20 21:45 UTC. Cycle 156 items, all `DryRun(Global)`, `warm` true on 97 % of price checks (0 % in inferred mode), moving averages in sevenths, no errors. Over 57 cycles: 7 631 simulated updates, 154 creates, 46 rule deletes, 1 264 not-worth-buying skips |
| 6a | `FastDropGuard` fires rarely and sensibly | Pass | About five items a cycle (3 %), the same handful each time. Spot checks: Primed Convulsion (closed median 130, collector 48 h average 98.8, cheapest live sell 100) and Arcane Rise. 57 of 289 firings also tagged `LowProfit`: the guard stopped a buy the weekly average would have allowed |
| 7 | Switch back to `inferred` | Not run | Optional comparison; the user chose to stay in closed mode |
| 8 | Next day's refresh pass after 08:30 UTC | Pass, estimate corrected | `[2026-09-22 00:42:38] [ClosedStats] Pass complete: ok 3840, missing 0, failed 0`, 0 warnings. The pass took 16 h 12 min, not the spec's "about 11 h": each fetch waits about 5 s for a Cold-lane slot behind the nonstop trader, the ~200-item hot sweep, the cold sweep and (from 07:17 UTC) the trade-tax loop, so the real pace is about 15 s per item. The user read `1138 waiting` at about 20:05 UTC, consistent with that pace. It still finishes well inside the day and hot-set items are refreshed first; recorded as follow-up 9 |
| 9 | Warm-up tab's `tracked` is the collector's universe | Not read | Covered by the blend tests and the `market_warmup` filter review; not read in the browser |
| 10 | Task 6: only max-rank mods are bought | Pass | Verified by price against warframe.market's per-rank closed medians: Critical Delay anchor 20.57 = rank 5 (rank 0 is 14.0), Frostbite 24.57 = rank 3 (rank 0 is 20.57), Blind Rage 71.4, Transient Fortitude 70.6. The server's item list carries `maxRank`, so the fail-open path was not hit |
| 11 | Task 6: labels appear after a deploy | Pass | Confirmed by the user after the language-file revalidation fix. Before it, a hard refresh did not reach the script-initiated fetch |
| 13 | Task 9: profit basis `range` | Pass | Switched on in dry-run 2026-09-21 about 07:30 UTC together with check 14. See the parity table below |
| 14 | Task 10: `max_buy_candidates = -1` | Pass | Cycle size 156 → 204 on the first cycle after the save; a 204-item dry-run cycle takes 2 min 21 s; 0 errors over 445 cycles in the following 17.5 h |
| 15 | Task 11: trade tax fetched | Pass | 0 `TradeTax` warnings over 17.5 h; the tax cap is at −1 (the desktop app's value), so the filter is inert by choice; the server bids on 32 million-credit items, the desktop app on 32 |
| 12 | Tasks 7–8: orphan sweep | Pass | Twice, identically. Start 01:28:34 UTC, first cycle ended 01:30:19, sweep fired by 02:01: nine adopted real buy orders deleted in simulation, reason `NotCandidate`, 0 failures. Akbolto Prime Set 73 p, Arcane Nullifier [rank=5] 45 p, Equilibrium [rank=10] 30 p, Volt Prime Neuroptics Blueprint 25 p, Akjagara Prime Barrel 20 p, Synoid Gammacor 15 p, Secura Penta 15 p, Hystrix Prime Set 12 p, Augur Reach [rank=5] 12 p. The user judged all nine stale |

## Defect found after acceptance (2026-09-21), not fixed

**The trading-tax cap filter added in Task 3 never excludes anything.** `game_data::to_tradable_item` sets `trade_tax: 0` for every item (`crates/qf_core/src/game_data/mod.rs`, with a test asserting it), because warframe.market's v2 item list carries no tax. `ItemPriceInfo.trading_tax` is therefore 0 for every item and `trading_tax ≤ trading_tax_cap` always holds. Found when the dry run turned out to be bidding on 30 items taxed at 1 000 000 credits or more (Arcane Energize, Arcane Grace, the Archon mods, Legendary Fusion Core, …). The controller's claim in P6 that "the tax data was already in the item cache" was checked against the *desktop app's* cache file, which does carry `tradeTax`, and not against the server's loader. No reviewer caught it: each checked that the field had no other reader, none checked that it was populated. Trading impact: none relative to the desktop app, whose `trading_tax_cap` is −1 and whose own candidate list holds 32 such items. It means the setting is dead on the server until tax data is loaded; follow-up 0.

## Parity with the desktop app (2026-09-21)

Method: the desktop app's own cached price data (`~/.local/share/dev.kenya.quantframe/cache/items/ItemPrices.json`, refreshed the same day) with the desktop settings (volume > 15, profit > 10, avg ≤ 1000, no cap) gives "what the desktop app would buy"; one complete server dry-run cycle gives "what the server buys".

| | Closed mode, spread profit, cap 150 (06:00 UTC) | Closed mode, range profit, no cap (07:34 UTC) |
|---|---|---|
| Server buy items | 150 | 202 |
| Desktop app would buy | 213 | 213 |
| In both | 124 | 192 |
| Server only | 26 | 10 |
| Desktop only | 89 | 21 |
| Share of the desktop list covered | 58 % | 90 % |
| Price anchor vs the desktop moving average, median | +1.2 % | +0.9 % |
| Anchors within 5 % | 90 % | 87 % |

Every remaining server-only item is a desktop near miss (profit 7.0 to 9.6 against 10, or volume just under 15); 20 of the 21 desktop-only items are server near misses (volume 15 to 20 a day, or profit 10 to 13). That is the two data pulls' windows being a day apart, not a logic difference. The desktop's average price cap of 1000 against the server's 600 changes nothing today: no desktop candidate is priced in between. The user declined to set `max_total_price_cap` (desktop: 150 000): "no price cap means more trades".

## What the plan got wrong, and how it was caught

The plan and spec were the controller's. Execution corrected them five times; each correction is in the spec (§25 P12–P14) and the plan.

1. **Stale-item query** exempted failed fetches from the daily cutoff. Caught by the Task 2 reviewer.
2. **Shift-filter "off" value**: the shared `is_disabled` (≤ −1) switched every negative threshold off, and shift thresholds are naturally negative. Caught by the plan's own test.
3. **00:30 UTC cutoff and a six-day window**: yesterday's row is missing until an item's refetch, so volume read 6⁄7. Caught by the final review; the first correction still compared the fetch with the *current* cutoff and left the defect between 00:00 and 08:30 UTC, the user's trading hours. Caught by the scoped re-review.
4. **Closed `warm` re-arming the live gate** for items the collector had never seen. Caught by the final review.
5. **Orphan sweep with an empty candidate list** would have deleted every standing bid after 30 minutes. Caught by the Task 7 reviewer.

## Rulings made during execution

- The trading-tax cap applies in both price modes, as P6 says. Cost if wrong: a user who had set a cap sees fewer candidates after deploy. (Moot for now: the filter is inert, see above.)
- No fix round for Task 1's unrun web build: the task changed no web file and the gate binds the branch tip. Cost if wrong: a web break surfaces at Task 4.
- The daily cutoff applies to failed fetches too; the spec beats the plan's SQL. Cost if wrong: one extra request per failed item per day.
- For `price_shift_threshold` only, exactly −1 means disabled. Cost if wrong: −1 p cannot be expressed, and a stored value such as −3 becomes live in closed mode.
- Cutoff 08:30 UTC and the window anchored on the cutoff in force at each item's own fetch, beyond the reviewer's one-constant fix. Cost if wrong: a thin item reads one older day until its refetch.
- Closed `warm` needs only that the collector knows the key, not that the collector's own `warm` is true. Cost if wrong: items route live on closed evidence alone after the user's 48 h review, which is the user's stated aim.
- `market_warmup` counts only collector-known keys. Cost if wrong: none found.
- A second, narrow fix round after the final review's one permitted wave, because the residual fell in the user's trading hours and the session had to stop for the deploy anyway. Cost if wrong: one implementer and one reviewer seat.
- The max-rank rule applies in both price modes and to buy candidates only. Cost if wrong: rank-0 mods the user wanted to flip are no longer bought; one filter reverses it.
- The orphan sweep is always on while Buy mode is on, with a 30-minute in-memory grace, respecting the Buy and WishList blacklists, and with no new setting. Cost if wrong: a buy order placed by hand for an item that is neither a candidate, on the wish list nor blacklisted is deleted after 30 minutes of trader runtime; a stale bid survives up to 30 minutes.
- No sweep on a cycle without buy candidates. Cost if wrong: orphans linger while the list is empty, the pre-P14 behaviour.
- Accepted: a progress-counter under-report on a rare partial import failure; one test whose behavioural RED was claimed, not printed.

## Follow-ups

0. ~~Load trade tax on the server.~~ Done in Task 11 (P18): fetched once per item from warframe.market's item detail. Taxes are never re-fetched (a change at warframe.market needs a manual `DELETE FROM item_trade_tax`), and the stock and wish-list `credits` bookkeeping still reads the always-zero cache field.
1. **Upstream differences (P13), now mostly built.** Upstream's `profit` (the mean daily closed price range) is selectable as `profit_basis = range` (Task 9, P16) and the 150-candidate cap is a setting, `max_buy_candidates` (Task 10, P17); both are on in the user's dry run. Still unbuilt: `profit_margin` (`profit ÷ avg_price × 100`) and the `min_wtb_profit_margin` filter, which the desktop app has off.
9. **The daily closed-statistics pass takes about 16 h, not 11.** Fetch items that have closed trades first (about 1 500 of 3 840) or shorten the pace while behind; live trading will slow it further (estimate about 20 h). Not a correctness issue: hot-set items go first, windows are anchored per fetch, data stays fresh for 3 days.
10. **Deferred minors from the Task 9 to 11 reviews:** the two adjacent profit columns render with different decimals; unchecked `i64` subtraction in `range_profit`; the TypeScript interfaces `ItemWtbSettings` and `ItemWtsSettings` have swapped bodies (pre-existing, cosmetic); `null` `tradingTax` untested; `trade_tax_loop` untested like `closed_stats_loop`.
2. **Live expectations to confirm on the first live run:** cycles of about three minutes (every order is rewritten every cycle, as upstream does); warframe.market's per-account order cap binding below 150 candidates (`has reached the order limit. Skipping.`).
3. **Orphan sweep:** the due list is a pre-loop snapshot, so a badly stale live order cache could spend the five-failure budget in one sweep; the outcome is a trader stop, and the knapsack block has the same exposure.
4. **Desktop app alongside the server:** trades detected from `EE.log` are applied even in dry-run whenever Auto Trade is on, and the desktop app applies them too. To trade with the desktop app, untick Auto Trade on the server and stop its trader first. Worth a guard or a documented mode.
5. **Deferred minors from the reviews:** the retention delete count is not logged; `market_price_sources` blends the item universe three times per call (fine without polling); `StatsPriceSource::load` does two cache lookups per item; `has_shift` is keyed by a reconstructed sub-type string and fails open; the WTB form can store a negative shift threshold other than −1; boundary tests (freshness at exactly three days, shift equal to the threshold, guard with absent averages); `max_rank` serializes as `null` with no TypeScript field; the two new settings inputs carry no `error` prop.
6. **Import duration:** 145 minutes beside a running trader. P11's "about 70 minutes" holds only with the trader stopped.
7. **Uptime Kuma** still has no monitor on `/healthz` (open item in the user's server notes).
8. **Still open from `docs/PHASE-6C-ACCEPTANCE.md`**, carried forward by number.
