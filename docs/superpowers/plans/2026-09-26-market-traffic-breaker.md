# Market Traffic Breaker, Budget and Change-Only Writes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan with Opus implementers and reviewers. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Never prolong a warframe.market block again (a process-wide circuit breaker on every outbound request, websocket included), comply with the published rules (identified client, a request budget under 3 req/s, no catalogue crawl at full speed), and keep trading throughput (the budget goes to the trader; writes only when a price changes).

**Architecture:** the breaker lives inside the existing `Limiter` (`crates/qf_core/src/market/limiter.rs`) and is fed from the five request sites (collector `fetch.rs`/`backfill.rs`, `game_data`, set cache, and the wf-market gate) plus the websocket builder; the budget is one setting applied to the limiter; the trader's order books are recorded as sweeps so the hot lane idles while trading; the cold lane is paced; `progress_order` compares before it PATCHes.

**Tech Stack:** Rust (tokio, reqwest, serde), the vendored `crates/wf-market` (patched; log each change in `crates/wf-market/PATCHES.md`), React web UI.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` §25 **P21–P24** (read them first); §5.3 for the limiter, §25 P4–P5/P12/P16 for closed mode.

**Evidence the plan rests on** (2026-09-26): collector cold lane 2.4 req/s unpaced + hot 0.42 + closed 0.1 ≈ 2.9 req/s all day (~250 000/day) with no User-Agent; trader adds 1.25–1.6 req/s, 55 % PATCH; after the Cloudflare challenge nothing backed off for 17 h (websocket reconnect every 2.6–5 s, 7 185 failed connects).

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-market-breaker`, branch `market-traffic-breaker` (from `main`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Gate after every task:** `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`; never bare `--workspace`. Output pristine; no new warnings.
- **Exact values:** `BREAKER_STEPS = [15 min, 1 h, 4 h]` (4 h repeats); `TRIP_AFTER_TRANSPORT_ERRORS = 5`; `PROBE_TIMEOUT = 60 s`; `LADDER_RESET_AFTER = 24 h`; `market_requests_per_second` default `2.5`, clamp `0.2..=2.9`; `USER_AGENT = "quantframe-server/<CARGO_PKG_VERSION> (+https://github.com/AI-Ocko/quantframe-server)"`; `cold_pace_s` default `5`; websocket backoff `5 s × 2ⁿ` capped `300 s`, n reset after a connection that lasted `60 s`; `HOT_INTERVAL_S` stays `300`; `CLOSED_PACE_S` stays `10`.
- **Breaker semantics:** collector lanes wait in `acquire`; the wf-market gate fails fast via `try_acquire`; exactly one probe per period; the breaker starts closed at process start; blocked requests count as nothing for item/closed/tax error state.
- `web/public/lang/en.json` by targeted insertion only. RPC allow-list checked by `scripts/check-rpc-commands.py`.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; do not push; never touch `main`.

---

### Task 1: Breaker core in the limiter

**Files:**
- Modify: `crates/qf_core/src/market/limiter.rs` (types, `acquire`, new `try_acquire`, `report`, `outcome_of`, snapshot, tests)
- Modify (callers of the removed API only): `crates/qf_core/src/market/gate.rs`, `crates/qf_core/src/collector/fetch.rs`, `crates/qf_core/src/collector/backfill.rs`, `crates/qf_core/src/game_data/mod.rs`, `crates/qf_core/src/helper_link/trades/sets.rs` — replace `report_429()` with `report(Outcome::RateLimited)` so the crate compiles; their real breaker wiring is Task 2.

**Interfaces (produces):**

```rust
pub enum Outcome { Ok, Challenge, RateLimited, TransportError }
pub struct BreakerOpen { pub until: DateTime<Utc>, pub reason: String }
impl Limiter {
    pub async fn acquire(&self, lane: Lane);                                   // waits through an open breaker
    pub async fn try_acquire(&self, lane: Lane) -> Result<(), BreakerOpen>;    // fails fast while open
    pub fn report(&self, outcome: Outcome);                                    // every request reports once
}
pub fn outcome_of(status: u16, headers: &reqwest::header::HeaderMap) -> Outcome; // 429 → RateLimited; ≥400 with `cf-mitigated: challenge` or a text/html content type → Challenge; other ≥500 → TransportError; else Ok
pub struct BreakerSnapshot { pub state: String /* closed|open|probing */, pub reason: Option<String>, pub opened_at: Option<String>, pub until: Option<String>, pub step: u32, pub trips_total: u64 }
// LimiterSnapshot gains `breaker: BreakerSnapshot`; `paused_ms` derives from `until`.
```

- [ ] **Step 1: Failing tests** (all `#[tokio::test(start_paused = true)]`): `challenge_opens_for_15_min_and_probe_success_closes`; `failed_probes_climb_15m_1h_4h_4h`; `ladder_resets_after_24h_closed`; `only_one_probe_per_period` (two waiters at period end: one granted, the other still waiting until the probe reports); `lost_probe_counts_failed_after_60s`; `five_transport_errors_trip_and_a_response_resets_the_count`; `try_acquire_fails_fast_while_open`; `a_429_trips`; `open_blocks_all_lanes`; `outcome_of_classifies_status_and_headers`. Delete the old `BACKOFF_*` tests. Run `cargo test -p qf_core --lib market::limiter` — RED.
- [ ] **Step 2: Implement.** State: `breaker: Option<Open { reason, opened_at, until, step, probe: Option<Instant> }>`, `transport_errors_in_row`, `last_trip_at`, `trips_total`. `report(Challenge|RateLimited)` opens (or reopens one step up when a probe fails); `report(Ok)` closes a probing breaker and zeroes the transport count; `report(TransportError)` counts and trips at 5. Probe: the first `acquire`/`try_acquire` after `until` is granted as the probe; others wait (or fail) until it reports or `PROBE_TIMEOUT` passes (then it counts as failed). One `warning` on open/reopen, one `info` on probing and on close. Remove `report_429`, `BACKOFF_START/MAX/RESET_AFTER`. Update the callers listed above. Tests GREEN.
- [ ] **Step 3: Gate**, then **Step 4: Commit** `feat(market): circuit breaker inside the request limiter`.

---

### Task 2: Feed the breaker from the qf_core REST sites

**Files:**
- Modify: `crates/qf_core/src/collector/fetch.rs` (`FetchError::Blocked`, `fetch_with_retries`, tests incl. `serve_once` taking headers), `crates/qf_core/src/collector/backfill.rs` (its `fetch_with_retries`, used by closed stats and trade tax), `crates/qf_core/src/game_data/mod.rs`, `crates/qf_core/src/helper_link/trades/sets.rs`, `crates/qf_core/src/collector/runner.rs::sweep`, `crates/qf_core/src/collector/closed.rs::refresh_once`, `crates/qf_core/src/collector/trade_tax.rs`.

- [ ] **Step 1: Failing tests.** `http_source_maps_status_codes` gains a 403 with `content-type: text/html` and `cf-mitigated: challenge` → `FetchError::Blocked`; `blocked_is_not_retried_and_trips_the_limiter`; `rate_limited_is_not_retried_and_trips` (replaces the old 5 s pause test); `ok_and_not_found_report_ok` (both `fetch_with_retries`); `blocked_sweep_does_not_count_an_item_error` (no `consecutive_errors` increment; the item must be due again once the breaker closes); `blocked_closed_fetch_writes_no_state`; `blocked_tax_fetch_sets_nothing_aside`. RED.
- [ ] **Step 2: Implement.** Every attempt calls `limiter.report(outcome_of(status, headers))` (or `TransportError` on a reqwest error); `Blocked` and `RateLimited` return immediately without retry. `game_data` and `sets.rs` report the same way. Sweep/closed/tax treat `Blocked`/`RateLimited` as "no attempt": no error state written. GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `feat(collector): report every fetch outcome to the breaker and treat blocks as no attempt`.

---

### Task 3: The wf-market gate fails fast and classifies challenges

**Files:**
- Modify: `crates/wf-market/src/gate.rs` (trait, `is_challenge`), `crates/wf-market/src/client.rs` (`call_api`, around the gate acquire and status reporting), `crates/qf_core/src/market/gate.rs` (`TraderLaneGate`), `crates/wf-market/PATCHES.md` (new numbered change).

**Interfaces:**

```rust
pub trait Gate: Send + Sync {
    fn acquire(&self) -> GateFuture<'_, Result<(), String>>;   // Err(text) = do not send; text becomes the error content
    fn on_response(&self, status: u16, challenge: bool);
    fn on_transport_error(&self);
}
pub fn is_challenge(headers: &reqwest::header::HeaderMap) -> bool;  // `cf-mitigated: challenge` or a text/html content type
```

- [ ] **Step 1: Failing tests** in `gate.rs`: `a_closed_gate_returns_request_error_without_sending` (a local listener sees no connection; error content is "warframe.market unreachable: breaker open until …"); `challenge_responses_are_flagged` (HTML or `cf-mitigated` 403 → true; JSON 403 → false); `transport_error_is_reported`. Extend the existing `every_api_call_passes_the_installed_gate` for the new trait shape. RED.
- [ ] **Step 2: Implement.** Gate `Err` → `ApiError::RequestError` with that content, nothing sent; on a response, `on_response(status, is_challenge(headers))`; on a reqwest error, `on_transport_error()`. `TraderLaneGate` maps `acquire` onto `try_acquire(Lane::Trader)` (formatting `until` as `HH:MM UTC`) and `on_response`/`on_transport_error` onto `report(outcome_of(..))`. Keep wf-market's own per-route 429 handling as is. GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `feat(wf-market): gate fails fast while the breaker is open and flags Cloudflare challenges`.

---

### Task 4: Websocket through the gate with exponential backoff

**Files:**
- Modify: `crates/wf-market/src/types/websocket/ws_client_builder.rs`, `crates/wf-market/PATCHES.md`.

- [ ] **Step 1: Failing tests** (pure): `ws_backoff(n)` for n = 0..=7 equals `[5, 10, 20, 40, 80, 160, 300, 300]` seconds; `backoff_resets_after_a_60s_connection`. RED.
- [ ] **Step 2: Implement.** Before every `connect_async`: `gate.acquire()`; `Err` → sleep `ws_backoff(n)`, n += 1, continue. `Ok(..)` → `on_response(101, false)`, start the 60 s reset timer. `Err(Http(resp))` → `on_response(status, is_challenge(headers))`. Other errors → `on_transport_error()`. The disconnect/reconnect messages carry the real delay. The User-Agent header value comes from `gate::user_agent()` once Task 6 lands; until then keep the current string. GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `fix(wf-market): websocket asks the gate before connecting and backs off exponentially`.

---

### Task 5: The trader stops cleanly when the market is blocked

**Files:**
- Modify: `crates/qf_core/src/trader/lifecycle.rs`, `crates/qf_core/src/trader/controller.rs` (`tick`, `finish`, `checklist`), `crates/qf_core/src/trader/engine.rs` (`run_loop`, `EngineExit`), `web/src/pages/live_scraper/TraderPanel.tsx`, `web/src/types/tauri.type.ts`, `web/public/lang/en.json` (targeted insertion).

- [ ] **Step 1: Failing tests:** `market_blocked_is_the_first_trigger`; `ready_needs_market_reachable`; `run_loop_exits_market_blocked_before_counting_failures` (inject `blocked: impl Fn() -> bool`); `finish_skips_buy_order_deletes_while_blocked` (the fake platform records no delete). RED.
- [ ] **Step 2: Implement.** `StopReason::MarketBlocked(String)` with `describe` "warframe.market is blocking requests (until …)"; `TriggerInput.market_blocked`; `Checklist.market_reachable`; `EngineExit::MarketBlocked`; `finish` logs "Buy orders left in place: warframe.market unreachable" instead of deleting. The trader stays stopped until the user presses Start. UI: the reason string and checklist row. GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `feat(trader): stop with MarketBlocked and leave orders in place while the breaker is open`.

---

### Task 6: Budget setting and User-Agent everywhere

**Files:**
- Modify: `crates/qf_core/src/app/types/settings/live_scraper_general_settings.rs` (`market_requests_per_second: f64`, `#[serde(default = "default_market_rps")]` = 2.5), `crates/qf_core/src/market/limiter.rs` (`set_rate_per_second(f64)` clamped 0.2..=2.9, interval in state), `crates/qf_core/src/startup.rs` and `crates/qf_core/src/commands/app.rs` (apply at startup and on settings save), `crates/qf_core/src/market/mod.rs` (`pub const USER_AGENT`, `pub fn http_client(timeout) -> reqwest::Client` with the UA as default header), the three `reqwest::Client::builder()` sites (`startup.rs`, `collector/runner.rs`, `helper_link/trades/sets.rs`), `crates/wf-market/src/gate.rs` (`set_user_agent(&'static str)`, `user_agent()`), `crates/wf-market/src/client.rs` (default headers) and `ws_client_builder.rs` (header), the web settings form (number input beside `price_source`), `en.json`.

- [ ] **Step 1: Failing tests:** `spacing_follows_set_rate`; `a_live_rate_change_applies_to_the_next_slot`; `rate_is_clamped`; `pre_p22_settings_load_with_2_5`; `every_client_sends_the_user_agent` (a `serve_once` variant that captures request headers; wf-market's gate test extended for the header). RED.
- [ ] **Step 2: Implement.** GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `feat(market): request budget setting and a project User-Agent on every client`.

---

### Task 7: Cold lane pacing

**Files:**
- Modify: `crates/qf_core/src/collector/runner.rs` (`cold_loop` sleeps the pace after every `Ok(Some(_))`; `cold_pace_s` read from the collector settings if a settings struct exists for the collector, otherwise `pub const COLD_PACE_S: u64 = 5` with a one-line note in the report saying which and why).

- [ ] **Step 1: Failing test:** `cold_loop_sweeps_one_item_per_pace` (paused time, 3 items, 12 s → 3 fetches at 5 s pace). RED.
- [ ] **Step 2: Implement.** GREEN. `HOT_INTERVAL_S` unchanged.
- [ ] **Step 3: Gate**, **Step 4: Commit** `fix(collector): pace the cold lane at one item per 5 s instead of crawling`.

---

### Task 8: The trader's order books feed the sweep tables

**Files:**
- Modify: `crates/qf_core/src/collector/orders.rs` (`pub fn from_wfm(o: &OrderWithUser) -> V2Order`), `crates/qf_core/src/collector/runner.rs` (split `sweep` into fetch + `pub async fn record_sweep(&self, target, lane: &str, result, expected_interval_s)`; add `pub async fn ingest_book(&self, item_id, slug, orders: &[V2Order], expected_interval_s)` that claims the in-flight slot and skips if taken; health counters get lane `trader`), `crates/qf_core/src/trader/item.rs::process_items` (after a live `load_orders` and before the filters, call the collector's `ingest_book` with the current cycle length; skip for fake orders and when the collector is off).

- [ ] **Step 1: Failing tests:** `from_wfm_matches_the_v2_parse_of_the_same_fixture` (`orders_small.json` parsed both ways); `ingested_book_writes_the_same_rows_as_a_hot_sweep`; `hot_step_skips_an_item_the_trader_just_ingested`. RED.
- [ ] **Step 2: Implement.** Confirm the wf-market client's `crossplay` header matches the collector's `Crossplay: true` (note it in the report if they differ). GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `feat(collector): record the trader's order books as sweeps so the hot lane idles while trading`.

---

### Task 9: PATCH only on change

**Files:**
- Modify: `crates/qf_core/src/trader/helpers.rs` (`pub fn order_unchanged(o: &Order, price: u32, qty: u32, per_trade: Option<u32>) -> bool`, requires `o.visible`; in the `Update` branch of `progress_order`, look the order up in `orders.cache_orders()`; unchanged → `orders.update_local(..)` + debug log, no request), `crates/qf_core/src/trader/orders.rs` (`pub fn update_local(&self, id, params)`: dry-run book or live cache, never counts success/failure).

- [ ] **Step 1: Failing tests:** `order_unchanged_cases` (price, quantity, per-trade, invisible); `an_unchanged_live_price_is_not_patched` (item.rs harness with the dry-run book: no `update` entry in the log); `a_changed_price_is_patched`. RED.
- [ ] **Step 2: Implement.** GREEN.
- [ ] **Step 3: Gate**, **Step 4: Commit** `fix(trader): send no PATCH when the order is unchanged`.

---

## Rollout (controller + user; not part of the tasks)

1. Keep the container stopped until deploy A is built.
2. **Deploy A = Tasks 1–7.** Live checks while still blocked: one `Market breaker open: Cloudflare challenge` line then one probe per period; `collector_health` shows `breaker.state=open` with `until`; `grep -c "WebSocket connection failed"` rises by at most one per period; no `HTTP 403` sweep warnings accumulate.
3. When the breaker closes: restart once to sign back in; trader off ~1 h; idle traffic ≈ 0.8 req/s (Σ granted deltas / 3600).
4. **Deploy B = Tasks 8–9.** Trader in dry-run ~1 h: Σ granted / 3600 ≤ 2.5; `granted_hot` flat; `sweep_state` candidates show `lane=trader`; `UpdateSuccess` per hour down ≥ 70 %. Then live; record the first live hour in `docs/PHASE-5-ACCEPTANCE.md`.
5. If the breaker trips again: lower `market_requests_per_second` from the settings page; next `cold_pace_s`.

## Self-Review

- Spec coverage: P21 → Tasks 1–5; P22 → Task 6; P23 → Tasks 7–8; P24 → Task 9.
- Interface consistency: `Outcome`, `outcome_of`, `try_acquire`, `report` named identically in Tasks 1–3; `is_challenge` lives in wf-market (Task 3) and `outcome_of` in qf_core (Task 1) — Task 1 implements its own header check and Task 3 does not depend on it. Task 4 uses only the Task 3 trait. Task 6's `user_agent()` is consumed by Task 4's header after both land (Task 4 keeps the old string until then).
