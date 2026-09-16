# Phase 4c: Hardening Before Go-Live — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the gaps that would make a live start unsafe or a failure invisible: refuse a live start while `auto_delete` is on, make the trader tolerate rows a trade removed mid-cycle, alert on failed or stranded trade applies, fsync the helper queue and write the set cache atomically, run retention independently of the collector, and take a nightly integrity-checked backup off the docker volume with a rehearsed restore.

**Architecture:** One new always-on `qf_core::housekeeping` task (started from `startup::start`, supervised like the collector loops) ticks every 60 s and owns three jobs: the alert sweep (`trades::sweep_alerts`), the hourly `helper_events` retention, and the daily `VACUUM INTO` backup with 7-daily/4-weekly pruning. The Start gate is one more `Checklist` field evaluated by a new `ready_for(dry_run)`. The trader fix is a return-type change in `ItemEntry` plus three `let Some(..) else` skips. Alerts reach the browser through `notify_gui!` and Discord through a new `notifications.on_alert` setting, behind a new `TradeEnv::alert` seam so every test uses the existing fakes.

**Tech Stack:** Rust (tokio, sea-orm 0.12 over sqlx SQLite 3.44, chrono, serde), React 19 + Mantine 9 + TanStack Query 5, pnpm 11.3.0, Docker Compose on the homelab.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§19 (H1–H9)** first; §5.7, §8, §18 (E5, E8) for context. §19 takes precedence.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-4c`, branch `phase-4c-hardening` (from `main` at `704ee90`). All paths are relative to it. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target` to reuse the build cache.
- **Tests:** `cargo test -p qf_core --lib`, `cargo test -p qf-server`, `cargo test -p qf-helper`, `cargo test -p migration`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Test output must be pristine (no new warnings).
- **Timings (H3, H5):** housekeeping tick **60 s**; a row with `reason = applying` is stranded after **180 s** (`STUCK_AFTER_S = 180`); retention hourly; backup once per UTC day.
- **Exact strings:** new reason `apply_interrupted`; migration `m20260920_000001_add_helper_events_alerted_at`; notification key `on_alert`; toast keys `on_trade_event.alert` and `on_backup.failed`; checklist key `auto_delete_off`; env `QF_BACKUP_DIR` (default `<data_dir>/backups`); backup file name `quantframe-<YYYY-MM-DD>.sqlite` (UTC date).
- **`auto_delete` semantics are unchanged.** Only the Start gate is added (H1). Nothing in this plan touches what it deletes.
- **`en.json`** does not round-trip through `json.dump`: edit it with targeted insertions only, then confirm it still parses with `python3 -c "import json;json.load(open('web/public/lang/en.json'))"`.
- **Commits:** conventional commits, one per task unless a step says otherwise, ending with exactly this trailer:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```
- **Pushes:** `git push` after each task (origin's push URL is SSH and works). Never push `main` from this plan.
- **Docker** runs on ockohome only (`ssh -o ClearAllForwardings=yes christopher@ockohome`, stack `~/stacks/quantframe-server`, container uid **10001**). Tasks 1–9 do not touch ockohome; Task 10 does.
- **Live state:** global dry-run stays on throughout. Task 10's H1 check flips it off for one status read with nothing started, then back on.

## File Structure (end of phase 4c)

```
crates/migration/src/m20260920_000001_add_helper_events_alerted_at.rs  NEW  ALTER TABLE helper_events ADD COLUMN alerted_at TEXT
crates/migration/src/lib.rs                                   MOD  register
crates/qf_core/src/lib.rs                                     MOD  pub mod housekeeping
crates/qf_core/src/housekeeping/mod.rs                        NEW  start, Gates, tick, TickReport, alert_backup_failed
crates/qf_core/src/housekeeping/backup.rs                     NEW  file_name, parse_date, to_delete, write, verify, prune, run
crates/qf_core/src/paths.rs                                   MOD  backup_dir field, backups_dir()
crates/qf_core/src/startup.rs                                 MOD  CoreConfig.backup_dir, Paths::new(.., backup_dir), housekeeping::start
crates/qf_core/src/trader/lifecycle.rs                        MOD  Checklist.auto_delete_off, ready_for
crates/qf_core/src/trader/controller.rs                       MOD  Platform::auto_delete, ready_for at start/idle/tick, tests
crates/qf_core/src/trader/platform.rs                         MOD  LivePlatform::auto_delete
crates/qf_core/src/trader/item_entry.rs                       MOD  accessors return Option
crates/qf_core/src/trader/item.rs                             MOD  three skips, regression test
crates/qf_core/src/helper_link/trades/events.rs               MOD  alerted_at, needing_alert, mark_alerted, set_reason
crates/qf_core/src/helper_link/trades/mod.rs                  MOD  APPLY_INTERRUPTED, STUCK_AFTER_S, TradeEnv::alert, sweep_alerts, tests
crates/qf_core/src/helper_link/trades/live.rs                 MOD  LiveEnv::alert, alert_variables
crates/qf_core/src/helper_link/trades/sets.rs                 MOD  atomic save
crates/qf_core/src/app/types/settings/notifications_setting.rs MOD  on_alert
crates/qf_core/src/collector/maintenance.rs                   MOD  drop helper_events retention + field
crates/qf_core/src/collector/runner.rs                        MOD  hourly log line
crates/qf-helper/src/queue.rs                                 MOD  sync_all in push
crates/qf-server/src/config.rs                                MOD  QF_BACKUP_DIR
crates/qf-server/tests/http.rs                                MOD  FakeTrades::alert
web/src/types/tauri.type.ts                                   MOD  TraderChecklist.auto_delete_off, HelperEvent.alerted_at, NotificationsSetting.on_alert
web/src/pages/live_scraper/TraderPanel.tsx                    MOD  auto_delete_off row
web/src/components/Forms/Settings/Tabs/Notifications/index.tsx MOD  on_alert tab
web/src/components/Forms/LiveScraperControl/                  DEL  dead upstream confirm modal
web/public/lang/en.json                                       MOD  strings
compose.yaml                                                  MOD  ./backups:/backups, QF_BACKUP_DIR
README.md                                                     MOD  backup and restore runbook
docs/PHASE-4C-ACCEPTANCE.md                                   NEW
```

---

### Task 1: Start gate on `auto_delete` (server)

**Files:**
- Modify: `crates/qf_core/src/trader/lifecycle.rs`
- Modify: `crates/qf_core/src/trader/controller.rs`
- Modify: `crates/qf_core/src/trader/platform.rs`

**Interfaces:**
- Consumes: `Checklist`, `Platform`, `TraderController::{checklist, idle_state, start, tick, finish}`, `states::try_app_state()`, `settings.live_scraper.general.auto_delete`.
- Produces: `Checklist { …, auto_delete_off: bool }` (serialized field name `auto_delete_off`); `Checklist::ready_for(&self, dry_run: bool) -> bool`; `Platform::auto_delete(&self) -> bool`; `TraderController::idle_state(&self, dry_run: bool, now) -> LifecycleState`.

- [ ] **Step 1: Failing lifecycle test**

In `crates/qf_core/src/trader/lifecycle.rs` tests, replace `ready_needs_every_checklist_item` with:

```rust
    fn all() -> Checklist {
        Checklist { token_valid: true, ws_connected: true, game_data_loaded: true, helper_connected: true, warframe_running: true, auto_delete_off: true }
    }

    #[test]
    fn ready_needs_every_checklist_item() {
        assert!(all().ready());
        for broken in [
            Checklist { token_valid: false, ..all() },
            Checklist { ws_connected: false, ..all() },
            Checklist { game_data_loaded: false, ..all() },
            Checklist { helper_connected: false, ..all() },
            Checklist { warframe_running: false, ..all() },
        ] {
            assert!(!broken.ready());
            assert!(!broken.ready_for(true));
            assert!(!broken.ready_for(false));
        }
    }

    #[test]
    fn auto_delete_only_blocks_a_live_start() {
        let auto_delete_on = Checklist { auto_delete_off: false, ..all() };
        assert!(auto_delete_on.ready(), "ready() ignores auto_delete (amendment H1)");
        assert!(auto_delete_on.ready_for(true), "dry-run start is allowed");
        assert!(!auto_delete_on.ready_for(false), "live start is refused");
        assert!(all().ready_for(false));
    }
```

- [ ] **Step 2: Run it, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::lifecycle`
Expected: FAIL — `no field auto_delete_off`, `no method ready_for`.

- [ ] **Step 3: Implement the rule**

In `lifecycle.rs`, extend the struct and impl:

```rust
pub struct Checklist {
    pub token_valid: bool,
    pub ws_connected: bool,
    pub game_data_loaded: bool,
    /// A qf-helper heartbeat arrived within `READY_WITHIN_S`.
    pub helper_connected: bool,
    /// The latest heartbeat reported Warframe running.
    pub warframe_running: bool,
    /// `live_scraper.general.auto_delete` is off. Only a live start needs it (amendment H1).
    pub auto_delete_off: bool,
}

impl Checklist {
    /// The five items every start needs. `auto_delete` is judged by `ready_for`.
    pub fn ready(&self) -> bool {
        self.token_valid && self.ws_connected && self.game_data_loaded && self.helper_connected && self.warframe_running
    }

    /// Ready for a start in the given mode: a live start also needs `auto_delete` off (amendment H1).
    pub fn ready_for(&self, dry_run: bool) -> bool {
        self.ready() && (dry_run || self.auto_delete_off)
    }
}
```

- [ ] **Step 4: Run the lifecycle tests, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::lifecycle`
Expected: PASS (3 tests). The crate still fails to compile elsewhere until Step 7.

- [ ] **Step 5: Failing controller test**

In `crates/qf_core/src/trader/controller.rs` tests, add `auto_delete: AtomicBool` to `Fake` (it derives `Default`, so `false` = off) and implement the new trait method on `Fake`:

```rust
        fn auto_delete(&self) -> bool {
            self.auto_delete.load(Ordering::SeqCst)
        }
```

Add after `without_a_heartbeat_the_trader_stays_offline`:

```rust
    #[tokio::test]
    async fn a_live_start_is_refused_while_auto_delete_is_on_but_a_dry_run_start_is_not() {
        let (_dir, fake, controller) = ready_controller().await;
        fake.auto_delete.store(true, Ordering::SeqCst);
        // Dry-run (the default from ready_controller): the item is informational.
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Ready);
        assert!(!controller.status(now()).await.checklist.auto_delete_off);
        let started = controller.start(now()).await.unwrap();
        assert_eq!(started.state, LifecycleState::Trading);
        controller.stop(StopReason::UserStop, now()).await.unwrap();

        // Live: the badge goes Offline and start() refuses.
        controller.set_options(Some(false), None).await.unwrap();
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Offline);
        let err = controller.start(now()).await.unwrap_err();
        assert_eq!(err.component, "Trader:Start");

        // Turning auto_delete off makes the same live start possible.
        fake.auto_delete.store(false, Ordering::SeqCst);
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Ready);
        assert_eq!(controller.start(now()).await.unwrap().state, LifecycleState::Trading);
        controller.stop(StopReason::UserStop, now()).await.unwrap();
    }
```

- [ ] **Step 6: Run it, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::controller`
Expected: FAIL — `Platform` has no `auto_delete`, `Checklist` literal missing a field.

- [ ] **Step 7: Wire the controller and the live platform**

In `controller.rs`:

1. Add to the `Platform` trait, after `game_data_loaded`:
   ```rust
    /// `live_scraper.general.auto_delete` (amendment H1).
    fn auto_delete(&self) -> bool;
   ```
2. In `checklist(..)` add the field: `auto_delete_off: !self.platform.auto_delete(),`.
3. Change `idle_state` to take the mode and use `ready_for`:
   ```rust
    fn idle_state(&self, dry_run: bool, now: DateTime<Utc>) -> LifecycleState {
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        if self.checklist(&session, &helper).ready_for(dry_run) { LifecycleState::Ready } else { LifecycleState::Offline }
    }
   ```
   and update its one caller in `finish`: `inner.state = self.idle_state(inner.options.dry_run, now);`.
4. In `start(..)`, replace the readiness check with:
   ```rust
        let dry_run = inner.options.dry_run;
        if !self.checklist(&session, &helper).ready_for(dry_run) {
            return Err(Error::new("Trader:Start", "The trader is not ready; see the start checklist", get_location!()));
        }
   ```
   and delete the later duplicate `let dry_run = inner.options.dry_run;`.
5. In `tick(..)`, the idle branch only: `let next = if checklist.ready_for(inner.options.dry_run) { LifecycleState::Ready } else { LifecycleState::Offline };`. The running branch is untouched.

In `platform.rs`, implement on `LivePlatform` (same shape as `LiveEnv::auto_trade`):

```rust
    fn auto_delete(&self) -> bool {
        states::try_app_state().is_some_and(|app| app.settings.live_scraper.general.auto_delete)
    }
```

- [ ] **Step 8: Run the trader tests, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::`
Expected: PASS, including the new test. Then `cargo test -p qf_core --lib` (whole lib) and `cargo test -p qf-server` (its tests build `TraderStatus` JSON) must pass with no new warnings.

- [ ] **Step 9: Commit and push**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): refuse a live start while auto_delete is on

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 2: Start gate on `auto_delete` (web)

**Files:**
- Modify: `web/src/types/tauri.type.ts` (`TraderChecklist`), `web/src/pages/live_scraper/TraderPanel.tsx`, `web/public/lang/en.json`
- Delete: `web/src/components/Forms/LiveScraperControl/` (whole directory)

**Interfaces:**
- Consumes: `TraderStatus.checklist.auto_delete_off` and `TraderStatus.options.dry_run` from Task 1.
- Produces: the checklist row keyed `auto_delete_off`.

- [ ] **Step 1: Type**

In `web/src/types/tauri.type.ts`, add to `TraderChecklist` after `warframe_running: boolean;`:

```ts
    auto_delete_off: boolean;
```

- [ ] **Step 2: Panel row, grey in dry-run**

In `TraderPanel.tsx`, replace the `checks` array and its rendering:

```tsx
  // [key, ok, blocking]: auto_delete only blocks a live start (spec §19 H1).
  const checks: Array<[string, boolean, boolean]> = [
    ["token_valid", status.checklist.token_valid, true],
    ["ws_connected", status.checklist.ws_connected, true],
    ["game_data_loaded", status.checklist.game_data_loaded, true],
    ["helper_connected", status.checklist.helper_connected, true],
    ["warframe_running", status.checklist.warframe_running, true],
    ["auto_delete_off", status.checklist.auto_delete_off, !status.options.dry_run],
  ];
```

```tsx
          {checks.map(([key, ok, blocking]) => (
            <List.Item
              key={key}
              icon={
                <ThemeIcon color={ok ? "green" : blocking ? "red" : "gray"} size={16} radius="xl">
                  {ok ? "✓" : "✕"}
                </ThemeIcon>
              }
            >
              {t(`checklist.${key}`)}
            </List.Item>
          ))}
```

- [ ] **Step 3: String**

In `web/public/lang/en.json`, inside `pages.live_scraper.trader.checklist` (currently ending with the `warframe_running` line at about line 2248), add a new last entry:

```json
          "auto_delete_off": "auto_delete is off (on, it deletes every non-blacklisted order on a live start)"
```

Keep the JSON valid (a comma after the previous value, none after the last). Check: `python3 -c "import json;json.load(open('web/public/lang/en.json'))"`.

- [ ] **Step 4: Delete the dead component**

First confirm nothing imports it: `grep -rn LiveScraperControl web/src` must print only files inside `web/src/components/Forms/LiveScraperControl/`. Then `git rm -r web/src/components/Forms/LiveScraperControl`. Its strings under `components.live_scraper_control` in en.json stay.

- [ ] **Step 5: Web checks**

Run: `python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: `0 missing`; tsc and vite clean.

- [ ] **Step 6: Commit and push**

```bash
git add web
git commit -m "feat(web): show the auto_delete start-gate row and drop the dead confirm modal

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: Trader tolerates rows removed mid-cycle

**Files:**
- Modify: `crates/qf_core/src/trader/item_entry.rs:151-191`
- Modify: `crates/qf_core/src/trader/item.rs:239`, `:365`, `:519`, tests

**Interfaces:**
- Consumes: `StockItemQuery::find_by_id`, `WishListQuery::get_by_id`, the `log` closure each `progress_*` function already defines.
- Produces: `ItemEntry::get_stock_item(&self, conn) -> Result<Option<StockItemModel>, Error>`, `get_wish_list_item -> Result<Option<WishListModel>, Error>`, `get_stock_item_or_error` / `get_wishlist_item_or_error` with the same `Option` payloads. A `None` id is still `Err`.

- [ ] **Step 1: Failing accessor test**

In `item_entry.rs`, add a tests module (or extend the existing one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use utils::Properties;

    fn entry(stock_id: Option<i64>, wish_list_id: Option<i64>) -> ItemEntry {
        ItemEntry::new(stock_id, wish_list_id, "item1_slug", "item1", None, 0, 1, 1, vec!["Sell".into()], "closed", Properties::default())
    }

    #[tokio::test]
    async fn a_missing_row_is_none_and_a_missing_id_is_an_error() {
        let (_dir, conn) = crate::trader::store::tests::db().await;
        assert!(entry(Some(999), None).get_stock_item(&conn).await.unwrap().is_none());
        assert!(entry(None, Some(999)).get_wish_list_item(&conn).await.unwrap().is_none());
        assert_eq!(entry(None, None).get_stock_item(&conn).await.unwrap_err().component, "ItemEntry:GetStockItem");
        assert_eq!(entry(None, None).get_wish_list_item(&conn).await.unwrap_err().component, "ItemEntry:GetWishListItem");
    }
}
```

- [ ] **Step 2: Run it, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::item_entry`
Expected: FAIL — `is_none` on `StockItemModel`.

- [ ] **Step 3: Change the four accessors**

```rust
    /// `Ok(None)` when the row no longer exists: a trade or the "sold" button removed it mid-cycle (amendment H2).
    pub async fn get_stock_item(&self, conn: &DatabaseConnection) -> Result<Option<StockItemModel>, Error> {
        let stock_id = self
            .stock_id
            .ok_or_else(|| Error::new("ItemEntry:GetStockItem", "Stock ID is None", get_location!()))?;
        StockItemQuery::find_by_id(conn, stock_id).await.map_err(|e| e.with_location(get_location!()))
    }

    pub async fn get_wish_list_item(&self, conn: &DatabaseConnection) -> Result<Option<WishListModel>, Error> {
        let wish_list_id = self
            .wish_list_id
            .ok_or_else(|| Error::new("ItemEntry:GetWishListItem", "Wish List ID is None", get_location!()))?;
        WishListQuery::get_by_id(conn, wish_list_id).await.map_err(|e| e.with_location(get_location!()))
    }

    pub async fn get_stock_item_or_error(&self, conn: &DatabaseConnection) -> Result<Option<StockItemModel>, Error> {
        self.get_stock_item(conn).await.map_err(|e| e.with_location(get_location!()).with_context(self.to_json()))
    }

    pub async fn get_wishlist_item_or_error(&self, conn: &DatabaseConnection) -> Result<Option<WishListModel>, Error> {
        self.get_wish_list_item(conn).await.map_err(|e| e.with_location(get_location!()).with_context(self.to_json()))
    }
```

- [ ] **Step 4: Skip at the three call sites**

Each `progress_*` function has a local `log` closure; use it. Replace:

`item.rs:239` (`progress_buying`):
```rust
        let Some(stock_item) = entry.get_stock_item_or_error(conn).await? else {
            log(&format!("Item {} stock row {:?} is gone (sold or removed mid-cycle). Skipping.", item_info.name, entry.stock_id));
            return Ok(());
        };
```
`item.rs:365` (`progress_selling`):
```rust
    let Some(mut stock_item) = entry.get_stock_item_or_error(conn).await? else {
        log(&format!("Item {} stock row {:?} is gone (sold or removed mid-cycle). Skipping.", item_info.name, entry.stock_id));
        return Ok(());
    };
```
`item.rs:519` (`progress_wish_list`):
```rust
    let Some(mut wishlist_item) = entry.get_wishlist_item_or_error(conn).await? else {
        log(&format!("Item {} wish-list row {:?} is gone (bought or removed mid-cycle). Skipping.", item_info.name, entry.wish_list_id));
        return Ok(());
    };
```

- [ ] **Step 5: Regression test in `item.rs`**

Add next to `selling_ignores_wtb_max_price_drop`:

```rust
    #[tokio::test]
    async fn a_stock_row_deleted_mid_cycle_is_skipped_and_the_rest_still_runs() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let route = route_for(true, true);
        let gone_id = stock(&ctx, 10, 1).await;
        let kept_id = stock(&ctx, 10, 1).await;
        StockItemMutation::delete(&ctx.conn, gone_id).await.unwrap();
        seed_order(&ctx, OrderType::Sell, 30, route).await;
        let live = book(&[20, 21], &[5]);
        let before = ctx.orders.dry_log().len();
        let mut gone = entry("Sell", Some(gone_id), None);
        gone.apply_market_info(&live);
        progress_selling(&ctx, &item_info(), &mut gone, &price(25.0, true), &live, route).await.unwrap();
        assert_eq!(ctx.orders.dry_log().len(), before, "a missing row writes nothing");
        let mut kept = entry("Sell", Some(kept_id), None);
        kept.apply_market_info(&live);
        progress_selling(&ctx, &item_info(), &mut kept, &price(25.0, true), &live, route).await.unwrap();
        assert!(ctx.orders.dry_log().len() > before, "the next entry is still processed");
    }
```

`StockItemMutation::delete(conn, id)` is the mutation the `stock_item_delete` command uses (`crates/qf_core/src/commands/stock_item.rs:78`); if its name differs there, use the one that command calls.

- [ ] **Step 6: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib trader::`
Expected: PASS. `grep -rn 'get_stock_item\|get_wishlist_item' crates/qf_core/src --include=*.rs | grep -v 'fn '` must show only the three call sites and the two wrappers.

- [ ] **Step 7: Commit and push**

```bash
git add crates/qf_core/src/trader
git commit -m "fix(trader): skip stock and wish-list rows removed mid-cycle instead of aborting the cycle

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: `alerted_at` column and the alert queries

**Files:**
- Create: `crates/migration/src/m20260920_000001_add_helper_events_alerted_at.rs`
- Modify: `crates/migration/src/lib.rs`, `crates/qf_core/src/helper_link/trades/events.rs`, `crates/qf_core/src/helper_link/trades/mod.rs:234` (struct literal), `crates/qf_core/src/helper_link/trades/live.rs` tests (`applied_sale`)

**Interfaces:**
- Consumes: `collector::{stmt, ts, db_err}`, `collector::store::{exec, count}`, `trader::store::tests::db()`.
- Produces: `HelperEvent.alerted_at: Option<String>`; `events::needing_alert(conn, stuck_before: DateTime<Utc>) -> Result<Vec<HelperEvent>, Error>` (oldest first); `events::mark_alerted(conn, event_id, at: DateTime<Utc>) -> Result<bool, Error>`; `events::set_reason(conn, event_id, reason: &str) -> Result<bool, Error>`.

- [ ] **Step 1: Migration**

`crates/migration/src/m20260920_000001_add_helper_events_alerted_at.rs`, same shape as `m20260918_000001_create_helper_events.rs`:

```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

// When the housekeeping sweep alerted on this row; NULL until then (spec §19 H3).
const UP: &[&str] = &["ALTER TABLE helper_events ADD COLUMN alerted_at TEXT"];
const DOWN: &[&str] = &["ALTER TABLE helper_events DROP COLUMN alerted_at"];

async fn run(manager: &SchemaManager<'_>, statements: &[&str]) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for sql in statements {
        db.execute(Statement::from_string(db.get_database_backend(), sql.to_string())).await?;
    }
    Ok(())
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        run(manager, UP).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        run(manager, DOWN).await
    }
}
```

Register it in `crates/migration/src/lib.rs`: add `mod m20260920_000001_add_helper_events_alerted_at;` after the `m20260918…` line and `Box::new(m20260920_000001_add_helper_events_alerted_at::Migration),` last in the vector.

- [ ] **Step 2: Failing store tests**

In `events.rs` tests add:

```rust
    #[tokio::test]
    async fn needing_alert_returns_failed_rows_and_stale_applying_rows_once() {
        let (_dir, conn) = db().await;
        let mut failed = event("failed", "2026-09-16T12:00:00Z", NEEDS_REVIEW);
        failed.reason = Some("apply_failed: HandleItem".into());
        let mut young = event("young", "2026-09-16T12:04:30Z", NEEDS_REVIEW);
        young.reason = Some("applying".into());
        let mut stale = event("stale", "2026-09-16T12:00:00Z", NEEDS_REVIEW);
        stale.reason = Some("applying".into());
        let mut ordinary = event("ordinary", "2026-09-16T11:00:00Z", NEEDS_REVIEW);
        ordinary.reason = Some("unresolved: Paryy".into());
        for e in [&failed, &young, &stale, &ordinary] {
            insert(&conn, e).await.unwrap();
        }
        // now = 12:05:00, stuck cutoff = now - 180 s = 12:02:00
        let cutoff = at("2026-09-16T12:02:00Z");
        let ids: Vec<String> = needing_alert(&conn, cutoff).await.unwrap().into_iter().map(|e| e.event_id).collect();
        assert_eq!(ids, vec!["failed".to_string(), "stale".to_string()], "oldest first; young and ordinary rows are left alone");

        assert!(mark_alerted(&conn, "failed", at("2026-09-16T12:05:00Z")).await.unwrap());
        assert!(set_reason(&conn, "stale", "apply_interrupted").await.unwrap());
        assert!(mark_alerted(&conn, "stale", at("2026-09-16T12:05:00Z")).await.unwrap());
        assert!(needing_alert(&conn, cutoff).await.unwrap().is_empty());
        let stale = get(&conn, "stale").await.unwrap().unwrap();
        assert_eq!((stale.reason.as_deref(), stale.alerted_at.as_deref()), (Some("apply_interrupted"), Some("2026-09-16T12:05:00Z")));
        assert!(!mark_alerted(&conn, "missing", at("2026-09-16T12:05:00Z")).await.unwrap());
    }
```

Also add `alerted_at: None,` to the `event(..)` fixture literal.

- [ ] **Step 3: Run, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib helper_link::trades::events`
Expected: FAIL — unknown field / functions.

- [ ] **Step 4: Store changes**

In `events.rs`:

1. `COLUMNS` gains `, alerted_at` at the end; `HelperEvent` gains `pub alerted_at: Option<String>,` after `reviewed_at`; `from_row` reads it like `reviewed_at`; `insert` binds it as the tenth `?` (`event.alerted_at.clone().into()`) and the `VALUES` list gets a tenth `?`.
2. New functions:

```rust
/// Rows the housekeeping sweep must alert on, oldest first (amendment H3): `apply_failed:` rows,
/// and rows still `applying` received at or before `stuck_before`.
pub async fn needing_alert(conn: &DatabaseConnection, stuck_before: DateTime<Utc>) -> Result<Vec<HelperEvent>, Error> {
    const C: &str = "HelperEvents:NeedingAlert";
    conn.query_all(stmt(
        &format!(
            "SELECT {COLUMNS} FROM helper_events \
             WHERE status = 'needs_review' AND alerted_at IS NULL \
               AND (reason LIKE 'apply_failed:%' OR (reason = 'applying' AND received_at <= ?)) \
             ORDER BY received_at ASC, rowid ASC"
        ),
        vec![ts(stuck_before).into()],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|row| from_row(C, row))
    .collect()
}

/// Returns false when no row has that id.
pub async fn mark_alerted(conn: &DatabaseConnection, event_id: &str, at: DateTime<Utc>) -> Result<bool, Error> {
    let changed = exec(conn, "HelperEvents:MarkAlerted", "UPDATE helper_events SET alerted_at = ? WHERE event_id = ?", vec![ts(at).into(), event_id.into()]).await?;
    Ok(changed > 0)
}

/// Rewrites only the reason (used for `applying` → `apply_interrupted`). Returns false when no row has that id.
pub async fn set_reason(conn: &DatabaseConnection, event_id: &str, reason: &str) -> Result<bool, Error> {
    let changed = exec(conn, "HelperEvents:SetReason", "UPDATE helper_events SET reason = ? WHERE event_id = ?", vec![reason.into(), event_id.into()]).await?;
    Ok(changed > 0)
}
```

3. Every `HelperEvent { .. }` literal gets `alerted_at: None`: `mod.rs:234` (`handle_incoming`), `live.rs` tests `applied_sale`, `events.rs` tests `event`. Find them with `grep -rn 'HelperEvent {' crates --include=*.rs`.

- [ ] **Step 5: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib helper_link` then `cargo test -p qf_core --lib` and `cargo test -p qf-server`.
Expected: all PASS. The migration applies on the test database (WAL, `ALTER TABLE … ADD COLUMN`).

- [ ] **Step 6: Commit and push**

```bash
git add crates/migration crates/qf_core
git commit -m "feat(core): track alerted_at on helper events and query rows needing an alert

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: `on_alert` notification, `TradeEnv::alert` and `sweep_alerts`

**Files:**
- Modify: `crates/qf_core/src/app/types/settings/notifications_setting.rs`, `crates/qf_core/src/helper_link/trades/mod.rs`, `crates/qf_core/src/helper_link/trades/live.rs`, `crates/qf-server/tests/http.rs` (`FakeTrades`)
- Modify: `web/src/types/tauri.type.ts` (`NotificationsSetting`, `HelperEvent`), `web/src/components/Forms/Settings/Tabs/Notifications/index.tsx`, `web/public/lang/en.json`

**Interfaces:**
- Consumes: Task 4 (`needing_alert`, `mark_alerted`, `set_reason`, `HelperEvent.alerted_at`), `notify_gui!`, `NotificationSetting::send`, `live::{trade_variables, toast_values}`.
- Produces: `NotificationsSetting.on_alert: NotificationSetting` (serde default); `trades::APPLY_INTERRUPTED = "apply_interrupted"`, `trades::STUCK_AFTER_S: i64 = 180`; `TradeEnv::alert(&self, event: &HelperEvent)`; `trades::sweep_alerts(conn, env: &dyn TradeEnv, now) -> Result<usize, Error>`; `live::alert_variables(event) -> HashMap<String, String>`; `trades::tests::{Fake, fake}` become `pub(crate)`; en.json `common.notifications.on_trade_event.alert.{title,message}` and `forms.settings.tabs.notifications.on_alert_title`.

- [ ] **Step 1: Setting with a default (server)**

In `notifications_setting.rs`, add the field and its default after `on_token_expiring`:

```rust
    #[serde(default = "default_on_alert")]
    pub on_alert: NotificationSetting,
```

```rust
fn default_on_alert() -> NotificationSetting {
    NotificationSetting::new(
        DiscordNotify::new("<MENTION>\n```ansi\n\x1B[1;31m🚨 Quantframe alert: <KIND>\x1B[0m\n\n\x1B[1;33m📝 Reason:\x1B[0m <REASON>\n\x1B[1;33m👤 Player:\x1B[0m <PLAYER_NAME>\n\x1B[1;33m🆔 Event:\x1B[0m  <EVENT_ID>\n\x1B[1;33m🕒 Time:\x1B[0m   <TIME>\n```", "", vec![]),
        SystemNotify::new("Quantframe alert: <KIND>", "<REASON>", "windows_xp_error.mp3", 1.0),
        WebHookNotify::new("<WEBHOOK_URL>"),
    )
}
```

and `on_alert: default_on_alert(),` in `Default::default()`.

- [ ] **Step 2: Failing pipeline tests**

In `trades/mod.rs` tests: make the module `pub(crate) mod tests`, the struct `pub(crate) struct Fake` and the constructor `pub(crate) fn fake(..)` (Task 7 reuses them); add `alerted: Mutex<Vec<(String, Option<String>)>>` to `Fake` and implement the new trait method:

```rust
        fn alert(&self, event: &HelperEvent) {
            self.alerted.lock().unwrap().push((event.event_id.clone(), event.reason.clone()));
        }
```

Add tests (the module already has `db()`, `fake(..)`, `incoming(..)`, `trade(..)`, `raw(..)`, `now()` helpers; use them as the neighbouring tests do):

```rust
    #[tokio::test]
    async fn a_failed_apply_is_alerted_exactly_once() {
        let (_dir, conn) = db().await;
        let mut env = fake(true);
        env.fail_on = Some("arcane_nullifier".into());
        let sale = trade(vec![raw("Arcane Nullifier", 1, Some(5))], vec![raw("Platinum", 70, None)]);
        let outcome = handle_incoming(&conn, &env, "gaming-pc", incoming('f', sale), now()).await.unwrap();
        assert_eq!(outcome.status, events::NEEDS_REVIEW);
        assert!(outcome.reason.as_deref().unwrap().starts_with("apply_failed:"));

        assert_eq!(sweep_alerts(&conn, &env, now()).await.unwrap(), 1);
        assert_eq!(sweep_alerts(&conn, &env, now()).await.unwrap(), 0, "alerted_at stops a second alert");
        let alerted = env.alerted.lock().unwrap().clone();
        assert_eq!(alerted.len(), 1);
        assert!(alerted[0].1.as_deref().unwrap().starts_with("apply_failed:"));
    }

    #[tokio::test]
    async fn a_stranded_applying_row_is_relabelled_and_alerted_after_180_s() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let mut event = events::tests::event("stuck", "2026-09-16T12:00:00Z", events::NEEDS_REVIEW);
        event.reason = Some(APPLYING.into());
        events::insert(&conn, &event).await.unwrap();

        let young = events::tests::at("2026-09-16T12:02:59Z");
        assert_eq!(sweep_alerts(&conn, &env, young).await.unwrap(), 0, "179 s: could still be a slow apply");
        let old = events::tests::at("2026-09-16T12:03:00Z");
        assert_eq!(sweep_alerts(&conn, &env, old).await.unwrap(), 1);
        let stored = events::get(&conn, "stuck").await.unwrap().unwrap();
        assert_eq!(stored.reason.as_deref(), Some(APPLY_INTERRUPTED));
        assert_eq!(stored.status, events::NEEDS_REVIEW);
        assert!(stored.alerted_at.is_some());
        assert_eq!(env.alerted.lock().unwrap()[0], ("stuck".to_string(), Some(APPLY_INTERRUPTED.to_string())));
        assert_eq!(sweep_alerts(&conn, &env, old).await.unwrap(), 0);
    }
```

(`incoming(..)` in this module takes a char id and a trade; keep its existing signature. `events::tests::{event, at}` are `pub(crate)` already.)

- [ ] **Step 3: Run, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib helper_link::trades`
Expected: FAIL — no `alert` in `TradeEnv`, no `sweep_alerts`.

- [ ] **Step 4: Trait, constants and the sweep**

In `trades/mod.rs`:

```rust
/// Reason written by the sweep over a row still `applying` after `STUCK_AFTER_S` (amendment H3).
pub const APPLY_INTERRUPTED: &str = "apply_interrupted";
/// Longer than the helper's 120 s request timeout, so a row this old is stranded, not slow.
pub const STUCK_AFTER_S: i64 = 180;
```

Add to `TradeEnv` after `notify`:

```rust
    /// One failure alert per row (amendment H3): a red toast and `notifications.on_alert`.
    fn alert(&self, event: &HelperEvent);
```

Add the sweep (public, after `ignore`):

```rust
/// Housekeeping sweep (amendment H3): alerts once on `apply_failed:` rows and on rows still
/// `applying` after `STUCK_AFTER_S`, relabelling the latter `apply_interrupted`. Returns how many rows were alerted.
pub async fn sweep_alerts(conn: &DatabaseConnection, env: &dyn TradeEnv, now: DateTime<Utc>) -> Result<usize, Error> {
    let stuck_before = now - chrono::Duration::seconds(STUCK_AFTER_S);
    let mut alerted = 0;
    for mut event in events::needing_alert(conn, stuck_before).await? {
        if event.reason.as_deref() == Some(APPLYING) {
            events::set_reason(conn, &event.event_id, APPLY_INTERRUPTED).await?;
            event.reason = Some(APPLY_INTERRUPTED.into());
        }
        env.alert(&event);
        events::mark_alerted(conn, &event.event_id, now).await?;
        alerted += 1;
    }
    Ok(alerted)
}
```

In `live.rs`, implement on `LiveEnv` and add the variables helper:

```rust
    fn alert(&self, event: &HelperEvent) {
        notify_gui!("on_trade_event", "red", "alert", toast_values(event), json!({"autoClose": false}));
        if let Some(app) = states::try_app_state() {
            app.settings.notifications.on_alert.send(
                &alert_variables(event),
                Some(json!({"event": "trade_alert", "event_id": event.event_id, "reason": event.reason})),
            );
        }
    }
```

```rust
/// `on_alert` variables for a trade row: the `on_new_trade` set plus `<KIND>`, `<REASON>` and `<EVENT_ID>`.
pub fn alert_variables(event: &HelperEvent) -> HashMap<String, String> {
    let mut variables = trade_variables(event);
    let reason = event.reason.clone().unwrap_or_default();
    let kind = if reason.starts_with("apply_failed:") { "apply_failed" } else { "apply_interrupted" };
    variables.insert("<KIND>".into(), kind.into());
    variables.insert("<REASON>".into(), reason);
    variables.insert("<EVENT_ID>".into(), event.event_id.clone());
    variables
}
```

Add a unit test next to the existing `live.rs` tests:

```rust
    #[test]
    fn alert_variables_name_the_kind_reason_and_event() {
        let mut event = applied_sale();
        event.reason = Some("apply_failed: HandleItem".into());
        let vars = alert_variables(&event);
        assert_eq!(vars["<KIND>"], "apply_failed");
        assert_eq!(vars["<REASON>"], "apply_failed: HandleItem");
        assert_eq!(vars["<EVENT_ID>"], event.event_id);
        event.reason = Some(super::super::APPLY_INTERRUPTED.into());
        assert_eq!(alert_variables(&event)["<KIND>"], "apply_interrupted");
    }
```

In `crates/qf-server/tests/http.rs`, `FakeTrades` gets `fn alert(&self, _event: &HelperEvent) {}`.

- [ ] **Step 5: Run the Rust tests, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib helper_link` then `cargo test -p qf_core --lib` and `cargo test -p qf-server`.
Expected: PASS, no new warnings.

- [ ] **Step 6: Web side**

1. `web/src/types/tauri.type.ts`: add `on_alert: NotificationSetting;` to `NotificationsSetting` after `on_token_expiring`; add `alerted_at?: string | null;` to the helper event interface after `reviewed_at?: string | null;` (line ~1170).
2. `web/src/components/Forms/Settings/Tabs/Notifications/index.tsx`: add `{ id: "on_alert", labelKey: "on_alert_title" },` after the `on_token_expiring` entry.
3. `web/public/lang/en.json`, targeted insertions:
   - inside `common.notifications.on_trade_event`, a new sibling after the `needs_review` object (about line 60):
     ```json
         "alert": {
           "title": "Trade with {{player_name}} needs attention",
           "message": "{{reason}}. Some items may already be applied; check Live Scraper → Trades before re-applying."
         }
     ```
   - inside `forms.settings.tabs.notifications` after `"on_token_expiring_title": "On Sign-in Expiring"` (about line 1616): `"on_alert_title": "On Alert (failed applies, backups)"`. Place commas so the objects stay valid.
4. Check: `python3 -c "import json;json.load(open('web/public/lang/en.json'))" && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`.

- [ ] **Step 7: Commit and push**

```bash
git add crates/qf_core crates/qf-server web
git commit -m "feat(core): alert once on failed and stranded trade applies through on_alert

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: Helper queue fsync and atomic set cache

**Files:**
- Modify: `crates/qf-helper/src/queue.rs:43-54`, `crates/qf_core/src/helper_link/trades/sets.rs:127-139` and its tests

**Interfaces:**
- Consumes: nothing new.
- Produces: no signature changes.

- [ ] **Step 1: Queue fsync**

In `Queue::push`, replace the final `writeln!` line with:

```rust
        writeln!(file, "{line}").map_err(|e| format!("cannot write {}: {e}", self.path.display()))?;
        // A detected trade must be on disk before the tail moves on (spec §19 H4).
        file.sync_all().map_err(|e| format!("cannot sync {}: {e}", self.path.display()))
```

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf-helper` — PASS (no new test; an fsync is not observable without a crash harness).

- [ ] **Step 2: Failing set-cache test**

In `sets.rs` tests, add (the module uses `tempfile::tempdir()` already):

```rust
    #[test]
    fn save_leaves_no_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let cache = SetCache::new(dir.path());
        cache.parts.lock().unwrap().insert("wolf_sledge_set".into(), vec!["wolf_sledge_handle".into()]);
        cache.save();
        cache.save();
        assert!(dir.path().join(SETS_FILE).is_file());
        assert!(!dir.path().join(format!("{SETS_FILE}.tmp")).exists());
        let reread = SetCache::new(dir.path());
        assert_eq!(reread.parts.lock().unwrap().get("wolf_sledge_set").map(Vec::len), Some(1));
    }
```

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib helper_link::trades::sets`. It passes before the change too (there is no temp file yet); it pins the property after Step 3.

- [ ] **Step 3: Atomic save**

Replace the `Ok(text)` arm of `save`:

```rust
            Ok(text) => {
                // Write beside the file and rename over it, like Queue::pop, so a crash never leaves a truncated sets.json.
                let mut temp = self.file.clone().into_os_string();
                temp.push(".tmp");
                let temp = std::path::PathBuf::from(temp);
                if let Err(e) = std::fs::write(&temp, text).and_then(|_| std::fs::rename(&temp, &self.file)) {
                    warning("HelperLink:Sets", format!("Could not save {}: {e}", self.file.display()), &LoggerOptions::default());
                }
            }
```

- [ ] **Step 4: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib helper_link::trades::sets`
Expected: PASS.

- [ ] **Step 5: Commit and push**

```bash
git add crates/qf-helper/src/queue.rs crates/qf_core/src/helper_link/trades/sets.rs
git commit -m "fix: fsync queued trades in the helper and write sets.json atomically

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 7: Housekeeping loop with alert sweep and retention

**Files:**
- Create: `crates/qf_core/src/housekeeping/mod.rs`, `crates/qf_core/src/housekeeping/backup.rs` (stub; Task 8 fills it)
- Modify: `crates/qf_core/src/lib.rs`, `crates/qf_core/src/paths.rs`, `crates/qf_core/src/startup.rs`, `crates/qf_core/src/collector/maintenance.rs`, `crates/qf_core/src/collector/runner.rs:260-267`

**Interfaces:**
- Consumes: `collector::runner::supervise`, `helper_link::trades::{sweep_alerts, TradeEnv, live::LiveEnv, events::apply_retention, tests::fake}`, `trader::store::tests::db()`.
- Produces: `housekeeping::TICK: Duration = 60 s`; `housekeeping::HOURLY: chrono::Duration = 1 h`; `housekeeping::Gates { last_hourly: Option<DateTime<Utc>>, backup_failed_on: Option<NaiveDate> }` (Default); `housekeeping::TickReport { alerts: usize, deleted_events: Option<u64>, backup: Option<PathBuf> }`; `housekeeping::tick(conn, env: &dyn TradeEnv, now, gates: &mut Gates, backup_dir: Option<&Path>) -> Result<TickReport, Error>`; `housekeeping::alert_backup_failed(reason: &str, now)`; `housekeeping::start(conn: DatabaseConnection, backup_dir: PathBuf)`; `backup::run(conn, dir, today: NaiveDate) -> Result<Option<PathBuf>, Error>` (stub returning `Ok(None)` until Task 8); `Paths::backups_dir() -> PathBuf`.

- [ ] **Step 1: Failing tick test**

Create `crates/qf_core/src/housekeeping/mod.rs`:

```rust
//! Always-on maintenance that must not depend on the collector (spec §19 H5): the alert sweep every
//! tick, `helper_events` retention hourly, and the daily backup (H6).

use std::path::{Path, PathBuf};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use service::sea_orm::DatabaseConnection;
use utils::{error, info, Error, LoggerOptions};

use crate::helper_link::trades::{self, events, TradeEnv};

pub mod backup;

pub const TICK: StdDuration = StdDuration::from_secs(60);
const RESTART_DELAY: StdDuration = StdDuration::from_secs(5);

fn hourly() -> Duration {
    Duration::hours(1)
}

#[derive(Debug, Default)]
pub struct Gates {
    pub last_hourly: Option<DateTime<Utc>>,
    /// A failed backup is not retried until the next UTC day (H6).
    pub backup_failed_on: Option<NaiveDate>,
}

#[derive(Debug, Default, PartialEq)]
pub struct TickReport {
    pub alerts: usize,
    pub deleted_events: Option<u64>,
    pub backup: Option<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::trades::events::tests::{at, event};
    use crate::helper_link::trades::events::{insert, list, NEEDS_REVIEW};
    use crate::trader::store::tests::db;

    #[tokio::test]
    async fn a_tick_sweeps_alerts_and_runs_retention_once_an_hour() {
        let (_dir, conn) = db().await;
        let env = crate::helper_link::trades::tests::fake(true);
        let mut old = event("old", "2026-06-01T00:00:00Z", NEEDS_REVIEW);
        old.reason = Some("unresolved: x".into());
        insert(&conn, &old).await.unwrap();
        let mut failed = event("failed", "2026-09-16T12:00:00Z", NEEDS_REVIEW);
        failed.reason = Some("apply_failed: HandleItem".into());
        insert(&conn, &failed).await.unwrap();

        let mut gates = Gates::default();
        let first = tick(&conn, &env, at("2026-09-16T12:01:00Z"), &mut gates, None).await.unwrap();
        assert_eq!(first, TickReport { alerts: 1, deleted_events: Some(1), backup: None }, "first tick runs the hourly job");
        assert_eq!(list(&conn, None, 1, 10).await.unwrap().total, 1, "the 107-day-old event is gone");

        let second = tick(&conn, &env, at("2026-09-16T12:02:00Z"), &mut gates, None).await.unwrap();
        assert_eq!(second, TickReport { alerts: 0, deleted_events: None, backup: None }, "one minute later: no hourly job, nothing new to alert");

        let third = tick(&conn, &env, at("2026-09-16T13:01:00Z"), &mut gates, None).await.unwrap();
        assert_eq!(third.deleted_events, Some(0), "an hour later the hourly job runs again");
    }
}
```

Create `crates/qf_core/src/housekeeping/backup.rs` as the stub with the final signature:

```rust
//! Daily database backup (spec §19 H6). Task 8 fills this in.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use service::sea_orm::DatabaseConnection;
use utils::Error;

/// Writes today's backup unless it already exists.
pub async fn run(_conn: &DatabaseConnection, _dir: &Path, _today: NaiveDate) -> Result<Option<PathBuf>, Error> {
    Ok(None)
}
```

Add `pub mod housekeeping;` to `crates/qf_core/src/lib.rs` after `pub mod helper_link;`.

- [ ] **Step 2: Run, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping`
Expected: FAIL — `tick` not found.

- [ ] **Step 3: Implement `tick`, the failure alert, the loop and `start`**

Add to `housekeeping/mod.rs` above the tests module:

```rust
/// One housekeeping pass. `backup_dir = None` skips the backup job (tests).
pub async fn tick(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    now: DateTime<Utc>,
    gates: &mut Gates,
    backup_dir: Option<&Path>,
) -> Result<TickReport, Error> {
    let mut report = TickReport { alerts: trades::sweep_alerts(conn, env, now).await?, ..Default::default() };
    if gates.last_hourly.is_none_or(|t| now - t >= hourly()) {
        report.deleted_events = Some(events::apply_retention(conn, now).await?);
        gates.last_hourly = Some(now);
    }
    if let Some(dir) = backup_dir {
        let today = now.date_naive();
        if gates.backup_failed_on != Some(today) {
            match backup::run(conn, dir, today).await {
                Ok(written) => report.backup = written,
                Err(e) => {
                    gates.backup_failed_on = Some(today);
                    alert_backup_failed(&e.message, now);
                }
            }
        }
    }
    Ok(report)
}

/// Critical log, red toast and `on_alert` with `<KIND> = backup_failed` (H6). No retry until the next UTC day.
pub fn alert_backup_failed(reason: &str, now: DateTime<Utc>) {
    use serde_json::json;
    use std::collections::HashMap;
    error("Housekeeping:Backup", format!("Backup failed: {reason}"), &LoggerOptions::default());
    crate::notify_gui!("on_backup", "red", "failed", json!({"reason": reason}), json!({"autoClose": false}));
    if let Some(app) = crate::utils::modules::states::try_app_state() {
        let variables = HashMap::from([
            ("<KIND>".to_string(), "backup_failed".to_string()),
            ("<REASON>".to_string(), reason.to_string()),
            ("<TIME>".to_string(), crate::collector::ts(now)),
            ("<PLAYER_NAME>".to_string(), String::new()),
            ("<EVENT_ID>".to_string(), String::new()),
        ]);
        app.settings.notifications.on_alert.send(&variables, Some(json!({"event": "backup_failed", "reason": reason, "at": crate::collector::ts(now)})));
    }
}

async fn run_loop(conn: DatabaseConnection, backup_dir: PathBuf) {
    let env = crate::helper_link::trades::live::LiveEnv;
    let mut gates = Gates::default();
    loop {
        match tick(&conn, &env, Utc::now(), &mut gates, Some(&backup_dir)).await {
            Ok(report) => {
                if report.alerts > 0 || report.deleted_events.is_some() || report.backup.is_some() {
                    info(
                        "Housekeeping",
                        format!(
                            "alerts {}, deleted events {}, backup {}",
                            report.alerts,
                            report.deleted_events.map_or("-".to_string(), |n| n.to_string()),
                            report.backup.as_ref().map_or("-".to_string(), |p| p.display().to_string())
                        ),
                        &LoggerOptions::default(),
                    );
                }
            }
            Err(e) => error("Housekeeping", format!("{} ({})", e.message, e.component), &LoggerOptions::default()),
        }
        tokio::time::sleep(TICK).await;
    }
}

/// Starts the supervised loop. Unconditional: it does not depend on `QF_COLLECTOR` (H5).
pub fn start(conn: DatabaseConnection, backup_dir: PathBuf) {
    crate::collector::runner::supervise("Housekeeping", RESTART_DELAY, move || run_loop(conn.clone(), backup_dir.clone()));
    info("Housekeeping", format!("Started: tick {} s, backups in {}", TICK.as_secs(), backup_dir.display()), &LoggerOptions::default());
}
```

The `notify_gui!` macro uses `json!` from the caller's scope; keep the `use serde_json::json;` inside `alert_backup_failed`.

- [ ] **Step 4: Move retention out of the collector**

In `collector/maintenance.rs`: remove the `deleted_events` field from `HourlyReport` (and its doc comment) and the `let deleted_events = …` line in `hourly`, returning `HourlyReport { hourly_rows, daily_rows, deleted_summaries, deleted_vanished }`. In `collector/runner.rs:260-267` drop `, deleted events {}` and `report.deleted_events` from the log line. Fix any `maintenance.rs` test that names the field.

- [ ] **Step 5: Start it from startup**

In `paths.rs` add:

```rust
    pub fn backups_dir(&self) -> PathBuf {
        self.subdir("backups")
    }
```

(Task 8 changes this to honour `QF_BACKUP_DIR`.) In `startup.rs`, after the collector start and before `crate::trader::start(conn.clone()).await?;`:

```rust
    crate::housekeeping::start(conn.clone(), paths::get().backups_dir());
```

- [ ] **Step 6: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping` then `cargo test -p qf_core --lib` and `cargo test -p qf-server`.
Expected: PASS; `grep -rn deleted_events crates` prints nothing.

- [ ] **Step 7: Commit and push**

```bash
git add crates/qf_core
git commit -m "feat(core): add the always-on housekeeping loop for alert sweeps and event retention

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 8: Nightly backup, integrity check and pruning

**Files:**
- Modify: `crates/qf_core/src/housekeeping/backup.rs` (replace the stub), `crates/qf_core/src/paths.rs`, `crates/qf_core/src/startup.rs` (`CoreConfig.backup_dir`, `Paths::new`), `crates/qf-server/src/config.rs`, `web/public/lang/en.json` (`on_backup.failed`)

**Interfaces:**
- Consumes: Task 7 (`tick`, `Gates`, `TickReport`), `collector::stmt`, `service::sea_orm::{Database, ConnectionTrait}`, `trader::store::tests::db()`.
- Produces: `backup::file_name(date: NaiveDate) -> String`; `backup::parse_date(name: &str) -> Option<NaiveDate>`; `backup::to_delete(dates: &[NaiveDate]) -> Vec<NaiveDate>`; `backup::write(conn, dir, date) -> Result<PathBuf, Error>`; `backup::verify(path) -> Result<(), Error>`; `backup::prune(dir) -> Result<Vec<PathBuf>, Error>`; `backup::run(conn, dir, today) -> Result<Option<PathBuf>, Error>`; `Paths { data_dir, resources_dir, backup_dir }`, `Paths::new(data_dir, resources_dir, backup_dir: Option<PathBuf>)`, `Paths::backups_dir()`; `CoreConfig.backup_dir: Option<PathBuf>`; `Config.backup_dir: Option<PathBuf>` from `QF_BACKUP_DIR`.

- [ ] **Step 1: Failing backup tests (write these first: `VACUUM INTO` through sea-orm is the one unproven assumption)**

Replace the stub's body of `backup.rs` with the tests below (keep the imports; the functions come in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::store::tests::db;

    fn d(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn file_names_round_trip_and_ignore_strangers() {
        assert_eq!(file_name(d("2026-09-16")), "quantframe-2026-09-16.sqlite");
        assert_eq!(parse_date("quantframe-2026-09-16.sqlite"), Some(d("2026-09-16")));
        assert_eq!(parse_date("quantframe.sqlite"), None);
        assert_eq!(parse_date("quantframe-2026-09-16.sqlite.tmp"), None);
    }

    #[test]
    fn retention_keeps_seven_dailies_and_four_sundays() {
        // 30 consecutive days ending Wednesday 2026-09-16.
        let dates: Vec<NaiveDate> = (0..30).map(|i| d("2026-09-16") - chrono::Duration::days(i)).collect();
        let delete = to_delete(&dates);
        let kept: Vec<NaiveDate> = dates.iter().copied().filter(|x| !delete.contains(x)).collect();
        assert_eq!(kept.len(), 11);
        for i in 0..7 {
            assert!(kept.contains(&(d("2026-09-16") - chrono::Duration::days(i))), "the 7 newest survive");
        }
        for sunday in ["2026-09-06", "2026-08-30", "2026-08-23"] {
            assert!(kept.contains(&d(sunday)), "{sunday} is a kept Sunday");
        }
        assert!(!kept.contains(&d("2026-09-08")), "a weekday older than 7 days goes");
        // No Sunday in the tail: exactly 7 survive.
        let weekdays: Vec<NaiveDate> =
            (0..12).map(|i| d("2026-09-16") - chrono::Duration::days(i)).filter(|x| x.weekday() != chrono::Weekday::Sun).collect();
        assert_eq!(weekdays.len() - to_delete(&weekdays).len(), 7);
    }

    #[tokio::test]
    async fn vacuum_into_writes_a_consistent_copy_once_per_day_and_prunes() {
        let (dir, conn) = db().await;
        let backups = dir.path().join("backups");
        let today = d("2026-09-16");
        let written = run(&conn, &backups, today).await.unwrap().expect("first run writes");
        assert_eq!(written, backups.join("quantframe-2026-09-16.sqlite"));
        assert!(written.is_file());
        assert!(!backups.join("quantframe-2026-09-16.sqlite-wal").exists());
        assert!(!backups.join("quantframe-2026-09-16.sqlite-shm").exists());
        verify(&written).await.unwrap();
        assert_eq!(run(&conn, &backups, today).await.unwrap(), None, "today's file exists: nothing to do");

        // Old files are pruned by the rule, whatever their content.
        for i in 1..40 {
            std::fs::write(backups.join(file_name(today - chrono::Duration::days(i))), b"x").unwrap();
        }
        std::fs::write(backups.join("unrelated.txt"), b"x").unwrap();
        run(&conn, &backups, today + chrono::Duration::days(1)).await.unwrap().expect("next day writes again");
        let names: Vec<String> =
            std::fs::read_dir(&backups).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
        let dated = names.iter().filter(|n| parse_date(n).is_some()).count();
        assert_eq!(dated, 11, "7 dailies + 4 Sundays");
        assert!(names.contains(&"unrelated.txt".to_string()), "only dated backup files are touched");
    }

    #[tokio::test]
    async fn a_corrupt_file_fails_verification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quantframe-2026-09-16.sqlite");
        std::fs::write(&path, b"not a database").unwrap();
        assert!(verify(&path).await.is_err());
    }
}
```

- [ ] **Step 2: Run, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping::backup`
Expected: FAIL — functions missing.

- [ ] **Step 3: Implement `backup.rs`**

```rust
//! Daily database backup (spec §19 H6): `VACUUM INTO` a dated file, verify it on a read-only
//! connection, keep 7 dailies and 4 Sunday weeklies.

use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate, Weekday};
use service::sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use utils::{get_location, info, Error, LoggerOptions};

use crate::collector::stmt;

const PREFIX: &str = "quantframe-";
const SUFFIX: &str = ".sqlite";
pub const KEEP_DAILY: usize = 7;
pub const KEEP_WEEKLY: usize = 4;

pub fn file_name(date: NaiveDate) -> String {
    format!("{PREFIX}{}{SUFFIX}", date.format("%Y-%m-%d"))
}

/// The date in a backup file name; `None` for anything else in the directory.
pub fn parse_date(name: &str) -> Option<NaiveDate> {
    let middle = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    NaiveDate::parse_from_str(middle, "%Y-%m-%d").ok()
}

/// Dates to delete: keep the `KEEP_DAILY` newest, then the `KEEP_WEEKLY` newest Sundays among the rest.
pub fn to_delete(dates: &[NaiveDate]) -> Vec<NaiveDate> {
    let mut sorted = dates.to_vec();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    sorted.dedup();
    let rest = &sorted[sorted.len().min(KEEP_DAILY)..];
    let mut sundays_kept = 0;
    rest.iter()
        .copied()
        .filter(|date| {
            if date.weekday() == Weekday::Sun && sundays_kept < KEEP_WEEKLY {
                sundays_kept += 1;
                false
            } else {
                true
            }
        })
        .collect()
}

/// `VACUUM INTO` a fresh file. SQLite refuses to overwrite, so the caller checks first.
pub async fn write(conn: &DatabaseConnection, dir: &Path, date: NaiveDate) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(dir)
        .map_err(|e| Error::new("Housekeeping:Backup", format!("cannot create {}: {e}", dir.display()), get_location!()))?;
    let path = dir.join(file_name(date));
    let target = path.to_string_lossy();
    if target.contains('\'') {
        return Err(Error::new("Housekeeping:Backup", format!("backup path must not contain a quote: {target}"), get_location!()));
    }
    conn.execute_unprepared(&format!("VACUUM INTO '{target}'"))
        .await
        .map_err(|e| Error::new("Housekeeping:Backup", format!("VACUUM INTO {} failed: {e}", path.display()), get_location!()))?;
    Ok(path)
}

/// `PRAGMA integrity_check` on a second, read-only connection to the copy.
pub async fn verify(path: &Path) -> Result<(), Error> {
    let url = format!("sqlite://{}?mode=ro", path.display());
    let conn = Database::connect(url).await.map_err(|e| Error::new("Housekeeping:Verify", e.to_string(), get_location!()))?;
    let row = conn
        .query_one(stmt("PRAGMA integrity_check", vec![]))
        .await
        .map_err(|e| Error::new("Housekeeping:Verify", e.to_string(), get_location!()))?
        .ok_or_else(|| Error::new("Housekeeping:Verify", "integrity_check returned no row", get_location!()))?;
    let verdict: String = row.try_get("", "integrity_check").map_err(|e| Error::new("Housekeeping:Verify", e.to_string(), get_location!()))?;
    if verdict == "ok" {
        Ok(())
    } else {
        Err(Error::new("Housekeeping:Verify", format!("integrity_check: {verdict}"), get_location!()))
    }
}

/// Deletes dated backup files the retention rule no longer keeps. Other files are left alone.
pub fn prune(dir: &Path) -> Result<Vec<PathBuf>, Error> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| Error::new("Housekeeping:Prune", format!("cannot list {}: {e}", dir.display()), get_location!()))?;
    let dated: Vec<(NaiveDate, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| parse_date(&entry.file_name().to_string_lossy()).map(|date| (date, entry.path())))
        .collect();
    let dates: Vec<NaiveDate> = dated.iter().map(|(d, _)| *d).collect();
    let doomed = to_delete(&dates);
    let mut removed = Vec::new();
    for (date, path) in dated {
        if doomed.contains(&date) {
            std::fs::remove_file(&path)
                .map_err(|e| Error::new("Housekeeping:Prune", format!("cannot delete {}: {e}", path.display()), get_location!()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// The daily job: skip if today's file exists; else write, verify (deleting a bad copy), prune.
pub async fn run(conn: &DatabaseConnection, dir: &Path, today: NaiveDate) -> Result<Option<PathBuf>, Error> {
    if dir.join(file_name(today)).exists() {
        return Ok(None);
    }
    let path = write(conn, dir, today).await?;
    if let Err(e) = verify(&path).await {
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    let removed = prune(dir)?;
    info("Housekeeping:Backup", format!("Wrote {} and pruned {} old backup(s)", path.display(), removed.len()), &LoggerOptions::default());
    Ok(Some(path))
}
```

- [ ] **Step 4: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping`
Expected: PASS. If `VACUUM INTO` fails with "cannot VACUUM from within a transaction" or a pool error, run it on a dedicated connection instead: read the live file from `conn.query_one(stmt("PRAGMA database_list", vec![]))` (column `file` of the `main` row), open `Database::connect(format!("sqlite://{file}?mode=ro"))`, run the `VACUUM INTO` there, and record the deviation in the report.

- [ ] **Step 5: `QF_BACKUP_DIR` plumbing**

1. `paths.rs`: add `pub backup_dir: PathBuf,`; change the constructor to `pub fn new(data_dir: impl Into<PathBuf>, resources_dir: impl Into<PathBuf>, backup_dir: Option<PathBuf>) -> Result<Self, Error>` with `let backup_dir = backup_dir.unwrap_or_else(|| data_dir.join("backups"));` and store it; replace the Task 7 `backups_dir` with:
   ```rust
    /// `QF_BACKUP_DIR`, default `<data_dir>/backups`; created on demand (spec §19 H6).
    pub fn backups_dir(&self) -> PathBuf {
        let _ = fs::create_dir_all(&self.backup_dir);
        self.backup_dir.clone()
    }
   ```
   Update the `paths.rs` test to `Paths::new(dir.path(), dir.path().join("res"), None)` and assert `paths.backups_dir() == dir.path().join("backups")`; add one assertion that `Paths::new(dir.path(), dir.path().join("res"), Some(dir.path().join("elsewhere"))).unwrap().backups_dir()` equals `dir.path().join("elsewhere")` and that it exists.
2. `startup.rs`: `pub backup_dir: Option<PathBuf>,` on `CoreConfig`; `paths::init(Paths::new(&cfg.data_dir, &cfg.resources_dir, cfg.backup_dir.clone())?);`.
3. `crates/qf-server/src/config.rs`: `pub backup_dir: Option<PathBuf>,` on `Config`, set with `backup_dir: get("QF_BACKUP_DIR").map(PathBuf::from),`, passed through in `core()`; add a test asserting `QF_BACKUP_DIR=/backups` yields `Some(PathBuf::from("/backups"))` and absence yields `None`.
4. `grep -rn 'Paths::new(' crates` must show only `startup.rs` and the `paths.rs` tests.

- [ ] **Step 6: Toast strings**

`web/public/lang/en.json`, targeted insertion inside `common.notifications` as a new sibling of `on_trade_event`:

```json
      "on_backup": {
        "failed": {
          "title": "Database backup failed",
          "message": "{{reason}}. No retry until tomorrow (UTC); check the server log."
        }
      },
```

Check the file parses and `(cd web && pnpm build)` is clean.

- [ ] **Step 7: Full gate**

Run (with `CARGO_TARGET_DIR` set): `cargo test -p qf_core --lib && cargo test -p qf-server && cargo test -p migration && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`.
Expected: all green, no new warnings.

- [ ] **Step 8: Commit and push**

```bash
git add crates web/public/lang/en.json
git commit -m "feat(core): take a nightly integrity-checked database backup with 7-daily and 4-weekly retention

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 9: Compose bind mount and the backup/restore runbook

**Files:**
- Modify: `compose.yaml`, `README.md:31`

**Interfaces:**
- Consumes: `QF_BACKUP_DIR` (Task 8), container uid 10001.
- Produces: `./backups:/backups` bind mount; README section on backups and restore.

- [ ] **Step 1: compose.yaml**

Under `environment:` add `QF_BACKUP_DIR: /backups`; under `volumes:` add `- ./backups:/backups`. Result:

```yaml
    environment:
      QF_PUBLIC_ORIGIN: ${QF_PUBLIC_ORIGIN:?set QF_PUBLIC_ORIGIN in .env}
      QF_COLLECTOR: "on"
      QF_BACKUP_DIR: /backups
    volumes:
      - qf-data:/data
      - ./backups:/backups
```

Validate: `python3 -c "import yaml; yaml.safe_load(open('compose.yaml'))"` if PyYAML is installed, else `docker compose -f compose.yaml config >/dev/null` if docker is; if neither exists locally, say so in the report (Task 10 validates on ockohome).

- [ ] **Step 2: README**

Replace line 31 (`- **Backups:** the qf-data volume holds …`) with:

```markdown
- **Backups:** every UTC day the server writes `backups/quantframe-<YYYY-MM-DD>.sqlite` (a `VACUUM INTO` copy, integrity-checked) into the host folder mounted at `/backups`, keeping the last 7 days and the last 4 Sundays. Create the folder once, owned by the container's uid: `mkdir -p backups && sudo chown 10001 backups` (or `chmod 1777 backups` without sudo). A failed backup logs a Critical line, shows a red toast and sends the `On Alert` notification; it is retried the next day. The start-time `quantframe.sqlite_backup` copy in the volume is only a migration safety net.
- **Restore:** `docker compose stop`, then copy the chosen file over the live database and drop the stale WAL:
  ```bash
  docker run --rm -v quantframe-server_qf-data:/data -v "$PWD/backups:/b:ro" busybox sh -c \
    'cp /b/quantframe-<date>.sqlite /data/quantframe.sqlite && rm -f /data/quantframe.sqlite-wal /data/quantframe.sqlite-shm && chown 10001 /data/quantframe.sqlite'
  docker compose up -d
  ```
  Then confirm `Database ready` in the log and that the Trades tab and transaction counts match the backup's date. (The volume name is `<stack folder>_qf-data`; on ockohome the stack folder is `quantframe-server`.)
```

- [ ] **Step 3: Commit and push**

```bash
git add compose.yaml README.md
git commit -m "docs: mount ./backups and document the backup and restore runbook

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 10: Deploy, acceptance (H8) and the record

**Files:**
- Create: `docs/PHASE-4C-ACCEPTANCE.md`

**Interfaces:**
- Consumes: everything above, deployed on ockohome.
- Produces: an acceptance record in the layout of `docs/PHASE-4B-ACCEPTANCE.md`. Merge only after the user's go-ahead.

This task runs against the live homelab. Steps marked **(ask)** need the user's explicit go before they run.

- [ ] **Step 1: Local gate**

```bash
cargo test -p qf_core --lib && cargo test -p qf-server && cargo test -p qf-helper && cargo test -p migration
python3 scripts/check-rpc-commands.py && (cd web && pnpm build)
```

Record the counts.

- [ ] **Step 2: Prepare the host folder and deploy**

```bash
ssh -o ClearAllForwardings=yes christopher@ockohome 'cd ~/stacks/quantframe-server && mkdir -p backups && (sudo -n chown 10001 backups 2>/dev/null || chmod 1777 backups) && ls -ld backups'
rsync -a --delete --dry-run --itemize-changes --exclude .git --exclude target --exclude web/node_modules --exclude web/dist --exclude secrets --exclude .env --exclude .superpowers --exclude backups ./ christopher@ockohome:~/stacks/quantframe-server/ | grep deleting
```

If only expected paths would be deleted (none, or `web/src/components/Forms/LiveScraperControl/*`), run the same `rsync` without `--dry-run --itemize-changes`, then:

```bash
ssh -o ClearAllForwardings=yes christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose up -d --build && docker compose ps'
ssh -o ClearAllForwardings=yes christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose logs --since 10m | sed "s/\x1b\[[0-9;]*m//g" | grep -E "Database ready|Housekeeping|Migrat|panic|CRITICAL|Trader started"'
```

Expected: healthy; `Database ready`; `Housekeeping Started: tick 60 s, backups in /backups`; no panic; no `Trader started`. Within about a minute the first backup appears: `ls -la backups/` on ockohome shows `quantframe-<today UTC>.sqlite` owned by uid 10001, and on the host `python3 -c "import sqlite3,sys; print(sqlite3.connect(sys.argv[1]).execute('PRAGMA integrity_check').fetchone()[0])" backups/quantframe-<date>.sqlite` prints `ok`.

- [ ] **Step 3: Configure the alert webhook (ask)**

Ask the user to open Settings → Notifications → **On Alert**, enable Discord, paste their webhook URL and Save. Without it, checks 3 and 4 prove the toast and the log line only.

- [ ] **Step 4: Acceptance checks**

RPC calls are made on ockohome with a session from the container's password secret and `Origin: http://quantframe.cgcorp.internal`, exactly as in the phase 4b record; never print the password. Planted rows use a throwaway `python:3-alpine` container on the same volume while the server is stopped, because the image has no SQLite tooling.

1. **Start gate, dry-run:** with `auto_delete` on (current setting) and dry-run on, `trader_status` shows `checklist.auto_delete_off = false` and `state = ready`; the row is grey in the browser.
2. **Start gate, live (ask):** `trader_set_options { dry_run: false }`; `trader_status` → `state = offline`; `trader_start` is refused with component `Trader:Start`. Set `live_scraper.general.auto_delete = false` through `app_update_settings`; `trader_status` → `state = ready`. **Do not start.** Restore `auto_delete = true` and `dry_run = true`; confirm both (`/data/settings.json` via `docker compose exec … cat`, and `trader_status.options.dry_run`).
3. **`apply_failed` alert (planted):** `docker compose stop`; insert a row with `event_id = sha256("phase-4c-acceptance-3")` (64 hex), `status = needs_review`, `reason = 'apply_failed: HandleItem'`, `received_at = detected_at = now`, `payload = {"player_name":"AcceptanceTest","ee_timestamp":"0","offered":[],"received":[]}` using `docker run --rm -v quantframe-server_qf-data:/data python:3-alpine python -c "<sqlite3 INSERT>"`; `docker compose up -d`. Within 60 s the log shows `Housekeeping alerts 1`, a red toast reaches an open browser, and one Discord message arrives if configured. The following minute logs no `Housekeeping alerts` line. Ignore the row from the Trades tab afterwards.
4. **`apply_interrupted` (planted):** same procedure with `event_id = sha256("phase-4c-acceptance-4")`, `reason = 'applying'`, `received_at = '2026-01-01T00:00:00Z'`. Within 60 s: `Housekeeping alerts 1`; the Trades tab shows reason `apply_interrupted`; one toast; one Discord message; the next minute is quiet. Ignore the row.
5. **Housekeeping without the collector:** run the built image with no volume so nothing touches live data:
   ```bash
   docker run -d --rm --name qf-hk-check -e QF_PUBLIC_ORIGIN=http://x -e QF_COLLECTOR=off \
     -e QF_DATA_DIR=/tmp/scratch -e QF_BACKUP_DIR=/tmp/scratch/backups \
     -e QF_WEB_PASSWORD_FILE=/tmp/scratch/pw -e QF_SECRET_KEY_FILE=/nonexistent \
     --entrypoint sh quantframe-server:local -c 'mkdir -p /tmp/scratch && echo x > /tmp/scratch/pw && exec qf-server'
   sleep 90; docker logs qf-hk-check 2>&1 | grep -E "QF_COLLECTOR=off|Housekeeping"; docker stop qf-hk-check
   ```
   Confirm the log shows `QF_COLLECTOR=off` **and** `Housekeeping Started`. If startup fails earlier for a reason unrelated to housekeeping (for example the empty data dir), record the failing line and treat this check as covered by the unit test plus the unconditional `start` call in `startup.rs`.
6. **Restore rehearsal (ask):** helper stopped and no trades in flight. Note `helper_trades` total and `get_transaction_pagination` total; `docker compose stop`; restore today's backup with the README command; `docker compose up -d`; confirm `Database ready`, the same two totals, and the rows from checks 3–4 present. Start the helper.
7. **Trader tolerance:** covered by the unit tests; during the next real sale with the trader running in dry-run the log must not show `ItemEntry:GetStockItem`. Record "to be observed" if no trade happens during acceptance.
8. **Web:** Settings still save; the Notifications tab lists On Alert; the Trades tab shows `apply_interrupted` for check 4's row; `alerted_at` is not displayed.

Evidence to record: log lines, event ids (hex only), backup file name and integrity result, restore totals before and after, the working command for check 5.

- [ ] **Step 5: Acceptance record and commit**

`docs/PHASE-4C-ACCEPTANCE.md` in the layout of `docs/PHASE-4B-ACCEPTANCE.md`: header (server, branch, commit, local gate counts, deploy notes, backups folder ownership), a table of checks 1–8 with Result and Notes, and `## Follow-ups` carrying forward phase 4b follow-ups 2–6, 8, 11, 13 and 15, closing 1, 10 and 14, marking 9 partial (`apply_failed` now alerts and the alert text states the re-apply caveat), and adding: the `on_alert` webhook is per-installation configuration; backups live on ockohome only (no off-site copy).

```bash
git add docs/PHASE-4C-ACCEPTANCE.md
git commit -m "docs: record phase 4c hardening acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

Ask the user whether to merge `phase-4c-hardening` into `main`, and update the project memory note.

---

## Self-review notes

- **§19 coverage:** H1 → Tasks 1–2; H2 → Task 3; H3 → Tasks 4–5 (column, queries, `on_alert`, `TradeEnv::alert`, `sweep_alerts`, toast keys, settings tab); H4 → Task 6; H5 → Task 7 (module, unconditional start, retention moved, 60 s tick); H6 → Tasks 8–9 (`VACUUM INTO`, verify, prune, `QF_BACKUP_DIR` plumbing through `Config` → `CoreConfig` → `Paths`, failure path with no same-day retry, compose mount, runbook); H7 → the tests inside Tasks 1, 3, 4, 5, 6, 7, 8; H8 → Task 10.
- **Type consistency:** `TradeEnv::alert(&self, &HelperEvent)` appears in Task 5's trait, `LiveEnv`, `trades::tests::Fake` and `FakeTrades`; `HelperEvent.alerted_at: Option<String>` is added in Task 4 and read in Task 5's tests; `housekeeping::tick(conn, env, now, &mut Gates, Option<&Path>)` is defined in Task 7 and unchanged by Task 8; `backup::run(conn, dir, today) -> Result<Option<PathBuf>, Error>` is stubbed in Task 7 with the final signature and implemented in Task 8; `backup::prune(dir)` takes only the directory; `Paths::new(.., Option<PathBuf>)` changes in Task 8 and its only production caller is `startup.rs`.
- **Ordering:** Task 7 compiles before Task 8 by stubbing `backup::run` and using `subdir("backups")`; Task 8 replaces both. Task 5 needs Task 4's column; Task 2 needs Task 1's field; Task 7 needs Task 5's `pub(crate)` fake.
- **Known limitations to carry into the record:** an `apply_interrupted` row re-applied from the modal may double-apply items the stranded apply had already written (the alert text says so); backups are on the same host as the database; check 5's exact docker invocation is recorded at acceptance time.
