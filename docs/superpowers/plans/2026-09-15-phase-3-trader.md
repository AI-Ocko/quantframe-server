# Phase 3: Trader — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task, **inline in the main session**. The user has ruled out subagent-driven development for this project: subagents may only explore the repo or write docs. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port quantframe's item trader onto the server, priced from phase 2's collected stats, and running in dry-run by default. Start it from a browser checklist, stop it automatically when the session or trader breaks, and alert through Discord.

**Architecture:**
- **Trader:** `qf_core::trader` holds a port of upstream `live_scraper`, items only (no rivens or syndicates). It reads prices through `PriceSource`, backed by `item_stats`.
- **Order writes:** every write goes through `TradeOrders`.
  - **Live:** calls wf-market.
  - **Dry-run:** writes to a simulated order book and to `dry_run_log`.
  - **Routing:** global dry-run sends everything to the simulated book. With it off, items that aren't `warm` still go there.
- **Loop and lifecycle:**
  - `engine::run_loop` runs `check()` cycles and exits on a critical error or 5 consecutive order failures.
  - `TraderController` owns the lifecycle (`Offline`, `Ready`, `Trading`, `Stopping`), the start and stop sequences, and a 5-second monitor tick.
  - The controller reaches the outside world through a `Platform` trait, so its tests use a fake.
  - `session` tracks the `/me` result, 401s, the websocket and token expiry.
- **Browser:** RPC commands `trader_*` feed a Trader panel and a Dry-run log tab on the Live Scraper page.

**Tech Stack:** Rust (tokio, sea-orm 0.12 raw SQL on SQLite, vendored wf-market), React 19 with Mantine 9 and TanStack Query 5, pnpm 11.3.0, Docker Compose on the homelab.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`. Read §5.5–5.7, §6, §8, §9, §14, §15 and the new §16 (Task 1) first.

**Upstream source:** `~/Projects/Personal/quantframe-react` at `3d59c4e7`, read-only. It's quoted as `upstream:src-tauri/src/live_scraper/...`. The ported code below is written out in full, so reading upstream is only for reference.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-3`, branch `phase-3-trader`. All paths are relative to it. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target` to reuse the build cache.
- **Dry-run is on by default** (`trader_state.dry_run = 1`). Nothing in this plan turns it off on the server; that's phase 5.
- **Trading never resumes on its own:** after any restart the lifecycle starts `Offline` or `Ready`, never `Trading`.
- **API calls:** warframe.market v1 only for `POST /v1/auth/signin`. Every wf-market call already goes through the Trader lane (phase 2 gate).
- **No riven, syndicate, auction, chat or analytics code.** The RPC allowlist ban test stays as it is, so new commands use a `trader_` prefix.
- **Upstream behaviour is kept** unless an amendment in §16 says otherwise.
- **Tests:** run `cargo test -p qf_core --lib` and `cargo test -p qf-server`; never bare `--workspace`. For the web, run `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`.
- **Docker** runs on ockohome only (`ssh christopher@ockohome`, `~/stacks/quantframe-server`).
- **Commits:** conventional commits, ending with the line below. No `Claude-Session:` trailer (user decision, 2026-09-14).
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```
- **Pushes:** push `phase-3-trader` after each task; never push `main` from this plan.

## File Structure (end of phase 3)

```
crates/migration/src/m20260916_000001_create_trader_tables.rs   NEW  trader_state, dry_run_log
crates/qf_core/
  src/lib.rs                               MOD  pub mod trader
  src/utils/modules/states.rs              MOD  try_settings()
  src/trader/mod.rs                        NEW  module list, TradeContext, start(), get()
  src/trader/store.rs                      NEW  trader_state + dry_run_log queries
  src/trader/price_source.rs               NEW  ItemPriceInfo, PriceSource, StatsPriceSource, get_interesting_items
  src/trader/session.rs                    NEW  /me, 401, websocket, token-expiry tracking
  src/trader/orders.rs                     NEW  Route, TradeOrders (live + simulated book + failure counter)
  src/trader/item_entry.rs                 NEW  port of upstream types/item_entry.rs
  src/trader/helpers.rs                    NEW  port of upstream modules/helpers.rs
  src/trader/item.rs                       NEW  port of upstream modules/item.rs (+ golden tests)
  src/trader/engine.rs                     NEW  run loop, error classification
  src/trader/lifecycle.rs                  NEW  pure state/checklist/stop-trigger rules
  src/trader/controller.rs                 NEW  TraderController, Platform trait, monitor tick
  src/trader/platform.rs                   NEW  LivePlatform (real side effects)
  src/collector/runner.rs                  MOD  hot set includes buy candidates
  src/app/modules/ws.rs                    MOD  report websocket state to session
  src/commands/auth.rs                     MOD  mark session signed in/out
  src/app/types/settings/notifications_setting.rs  MOD  on_trader_stopped, on_token_expiring
  src/types/ui_events.rs                   MOD  LifecycleState event
  src/commands/trader.rs                   NEW  trader_* RPC commands
  src/commands/{mod.rs,rpc.rs}             MOD  allowlist
  src/startup.rs                           MOD  mark session, start trader controller
web/
  src/types/tauri.type.ts                  MOD  trader types, notification fields, event
  src/api/live_scraper/index.ts            MOD  real trader calls
  src/pages/live_scraper/index.tsx         MOD  TraderPanel + Dry-run log tab
  src/pages/live_scraper/TraderPanel.tsx   NEW
  src/pages/live_scraper/Tabs/DryRunLog/index.tsx  NEW
  src/pages/live_scraper/Tabs/index.ts     MOD
  src/components/Forms/Settings/Tabs/Notifications/index.tsx  MOD  two new tabs
  public/lang/en.json                      MOD  strings
docs/superpowers/specs/2026-09-14-quantframe-server-design.md  MOD  §16
docs/PHASE-3-ACCEPTANCE.md                 NEW
```

---

### Task 1: Spec amendments for phase 3

**Files:**
- Modify: `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`

**Interfaces:** none (documentation). Every later task follows §16.

- [ ] **Step 1: Update the status line**

Replace:

```markdown
- **Status:** Approved 2026-09-14. Amended by §14 during phase 1 planning and §15 during phase 2 planning.
```

with:

```markdown
- **Status:** Approved 2026-09-14. Amended by §14 (phase 1), §15 (phase 2) and §16 (phase 3) planning.
```

- [ ] **Step 2: Append §16**

```markdown

## 16. Amendments from phase 3 planning (2026-09-15)

These amendments take precedence over the earlier sections.

- **C1 — Module and RPC names.**
  - The trader is `qf_core::trader`: a port of upstream `live_scraper` `client.rs`, `modules/item.rs`, `modules/helpers.rs` and `types/item_entry.rs`, with riven and syndicate code removed.
  - The RPC commands are `trader_status`, `trader_start`, `trader_stop`, `trader_set_options`, `trader_dry_run_log` and `trader_interesting_items`. The `live_scraper` ban in the RPC allowlist test stays.
  - The web `live_scraper` API module calls these commands.
- **C2 — `PriceSource`.**
  - `StatsPriceSource` is loaded from `item_stats` at the start of every trader cycle.
  - `ItemPriceInfo` keeps the upstream fields (except the `properties` bag) and adds `warm` and `history_days`.
  - `profit_margin`, `trading_tax` and `week_price_shift` are always 0, and their buy filters always pass.
  - Buy candidates pass the volume, profit and average-price filters. They are sorted by volume, highest first, and capped at 150.
- **C3 — Hot set.** When `Buy` is in `trade_modes`, the collector adds the current buy candidates to the hot set every 60 s, whether or not the trader is running, so candidates warm up before trading.
- **C4 — `OrderWriter` becomes `TradeOrders`.**
  - Routing: global dry-run gives `DryRun(Global)`. With it off, an item that isn't warm gives `DryRun(NotWarm)`, and anything else is `Live`.
  - Simulated orders live in an in-memory book. Under global dry-run it starts as a copy of the real cached orders, so the trader sees the same state it would live. Simulated order ids are `dry-<uuid>`.
  - `update` and `delete` go to the simulated book when the id is in it, or when global dry-run is on.
  - Every simulated write inserts a `dry_run_log` row. The table gains a `side` column; `action` is `create`, `update` or `delete`.
  - The trader monitor deletes `dry_run_log` rows older than 30 days every hour.
- **C5 — Upstream bug fixes.**
  - Sell-side repricing reads `settings.wts.max_price_drop` and `settings.wts.min_listings_below` (upstream read `wtb`, spec §5.6).
  - The buy-side max-stock-quantity delete now deletes the existing order. Upstream called `progress_order` with empty operations, so it did nothing.
- **C6 — Failures.**
  - `TradeOrders` counts consecutive failed live order calls (create, update, delete); a success resets the count.
  - The run loop stops at 5.
  - A `check()` error is classified as upstream does: WFM `ParsingError`, `BadRequest`, `Unknown`, `InternalServerError` or `InvalidType` is `Critical` and stops the trader; anything else is logged and the loop continues.
- **C7 — Lifecycle inputs.**
  - `token_valid`: signed in, no 401 since the last good `/me`, and `/me` succeeded within 20 min. `/me` is checked every 15 min.
  - `ws_connected`: taken from the websocket's internal connected, disconnected and reconnecting callbacks.
  - `game_data_loaded`: the tradable item list isn't empty.
  - `helper_ok` in phase 3: `helper_override && dry_run`.
  - The monitor ticks every 5 s. Stop triggers, first match wins: signed out, 401, websocket down for more than 60 s, `helper_ok` false, then the engine exiting (critical, failures or panic), plus the Stop button.
- **C8 — `trader_state` columns** are `dry_run`, `delete_buy_orders_on_stop`, `helper_override` (removed in phase 4), `last_stop_reason` and `last_stop_at`. Changing `dry_run` is refused while `Trading`.
- **C9 — Stop sequence.** Status is set to `invisible` even in dry-run. Buy orders are deleted on stop only when `delete_buy_orders_on_stop` is on and the run was live. Deleted orders are the real cached buy orders.
- **C10 — Alerts.**
  - New notification settings: `notifications.on_trader_stopped` (variables `<REASON>`, `<MODE>`, `<TIME>`) and `notifications.on_token_expiring` (variables `<EXPIRES_AT>`, `<DAYS_LEFT>`), each with Discord, system and webhook channels.
  - The token-expiry alert fires at most once per 24 h, and only while the token expires within 7 days.
- **C11 — Screens.** A Trader panel at the top of the Live Scraper page shows the state badge, the checklist, Start/Stop, option toggles and the last stop reason. A Dry-run log tab on the same page shows the newest entries first.
- **C12 — Session hooks.** Startup (with a websocket) and `auth_login` mark the session signed in, with the token expiry read from the JWT. `auth_logout` marks it signed out.
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers
git commit -m "docs: add phase 3 trader plan and spec amendments

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-3-trader
```

---

### Task 2: Trader tables and store

**Files:**
- Create: `crates/migration/src/m20260916_000001_create_trader_tables.rs`
- Modify: `crates/migration/src/lib.rs`
- Create: `crates/qf_core/src/trader/mod.rs`, `crates/qf_core/src/trader/store.rs`
- Modify: `crates/qf_core/src/lib.rs`

**Interfaces:**
- Consumes: `collector::{db_err, stmt, ts}` and `collector::store::{exec, count}` (phase 2, `pub(crate)`).
- Produces:
  - `store::TraderOptions { dry_run, delete_buy_orders_on_stop, helper_override: bool, last_stop_reason, last_stop_at: Option<String> }` (Serialize, Clone, PartialEq)
  - `store::DryRunEntry { id: i64, at, action, side, item_id, sub_type: String, price, quantity: Option<i64>, reason, forced_by: String }` (Serialize, Deserialize, Clone, PartialEq)
  - `store::DryRunPage { total: i64, page: i64, limit: i64, results: Vec<DryRunEntry> }` (Serialize)
  - Async functions, each returning `Result<_, utils::Error>`:
    - `load_options(conn) -> TraderOptions`
    - `save_flags(conn, dry_run: bool, delete_buy_orders_on_stop: bool, helper_override: bool) -> ()`
    - `record_stop(conn, reason: &str, at: DateTime<Utc>) -> ()`
    - `insert_dry_run(conn, &DryRunEntry) -> ()`
    - `dry_run_page(conn, page: i64, limit: i64) -> DryRunPage`
    - `prune_dry_run(conn, now: DateTime<Utc>) -> u64`
  - `trader::DRY_RUN_RETENTION_DAYS = 30`
  - `store::tests::db()` test helper (`pub(crate)`)

- [ ] **Step 1: Write the migration**

`crates/migration/src/m20260916_000001_create_trader_tables.rs`:

```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS trader_state (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        dry_run INTEGER NOT NULL DEFAULT 1,
        delete_buy_orders_on_stop INTEGER NOT NULL DEFAULT 0,
        helper_override INTEGER NOT NULL DEFAULT 0,
        last_stop_reason TEXT,
        last_stop_at TEXT
    )",
    "INSERT OR IGNORE INTO trader_state (id) VALUES (1)",
    "CREATE TABLE IF NOT EXISTS dry_run_log (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        at TEXT NOT NULL,
        action TEXT NOT NULL,
        side TEXT NOT NULL,
        item_id TEXT NOT NULL,
        sub_type TEXT NOT NULL,
        price INTEGER,
        quantity INTEGER,
        reason TEXT NOT NULL,
        forced_by TEXT NOT NULL
    )",
    "CREATE INDEX IF NOT EXISTS idx_dry_run_log_at ON dry_run_log (at)",
];

const DOWN: &[&str] = &["DROP TABLE IF EXISTS dry_run_log", "DROP TABLE IF EXISTS trader_state"];

async fn run(manager: &SchemaManager<'_>, statements: &[&str]) -> Result<(), DbErr> {
    let db = manager.get_connection();
    for sql in statements {
        db.execute(Statement::from_string(db.get_database_backend(), sql.to_string()))
            .await?;
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

In `crates/migration/src/lib.rs`, add `mod m20260916_000001_create_trader_tables;` after `mod m20260915_000001_create_collector_tables;`. Add `Box::new(m20260916_000001_create_trader_tables::Migration),` after `Box::new(m20260915_000001_create_collector_tables::Migration),`.

- [ ] **Step 2: Write the store with its tests**

`crates/qf_core/src/trader/mod.rs`:

```rust
//! Item trader (spec §5.6, §5.7 and amendments §16).

pub mod store;

pub const DRY_RUN_RETENTION_DAYS: i64 = 30;
```

In `crates/qf_core/src/lib.rs`, add `pub mod trader;` after `pub mod startup;`.

`crates/qf_core/src/trader/store.rs`:

```rust
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use crate::collector::store::{count, exec};
use crate::collector::{db_err, stmt, ts};

use super::DRY_RUN_RETENTION_DAYS;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraderOptions {
    pub dry_run: bool,
    pub delete_buy_orders_on_stop: bool,
    pub helper_override: bool,
    pub last_stop_reason: Option<String>,
    pub last_stop_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DryRunEntry {
    pub id: i64,
    pub at: String,
    pub action: String,
    pub side: String,
    pub item_id: String,
    pub sub_type: String,
    pub price: Option<i64>,
    pub quantity: Option<i64>,
    pub reason: String,
    pub forced_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DryRunPage {
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub results: Vec<DryRunEntry>,
}

pub async fn load_options(conn: &DatabaseConnection) -> Result<TraderOptions, Error> {
    const C: &str = "Trader:LoadOptions";
    let row = conn
        .query_one(stmt(
            "SELECT dry_run, delete_buy_orders_on_stop, helper_override, last_stop_reason, last_stop_at
             FROM trader_state WHERE id = 1",
            vec![],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .ok_or_else(|| db_err(C, "trader_state row is missing"))?;
    Ok(TraderOptions {
        dry_run: row.try_get::<i64>("", "dry_run").map_err(|e| db_err(C, e))? != 0,
        delete_buy_orders_on_stop: row.try_get::<i64>("", "delete_buy_orders_on_stop").map_err(|e| db_err(C, e))? != 0,
        helper_override: row.try_get::<i64>("", "helper_override").map_err(|e| db_err(C, e))? != 0,
        last_stop_reason: row.try_get("", "last_stop_reason").map_err(|e| db_err(C, e))?,
        last_stop_at: row.try_get("", "last_stop_at").map_err(|e| db_err(C, e))?,
    })
}

pub async fn save_flags(
    conn: &DatabaseConnection,
    dry_run: bool,
    delete_buy_orders_on_stop: bool,
    helper_override: bool,
) -> Result<(), Error> {
    exec(
        conn,
        "Trader:SaveFlags",
        "UPDATE trader_state SET dry_run = ?, delete_buy_orders_on_stop = ?, helper_override = ? WHERE id = 1",
        vec![(dry_run as i64).into(), (delete_buy_orders_on_stop as i64).into(), (helper_override as i64).into()],
    )
    .await
    .map(|_| ())
}

pub async fn record_stop(conn: &DatabaseConnection, reason: &str, at: DateTime<Utc>) -> Result<(), Error> {
    exec(
        conn,
        "Trader:RecordStop",
        "UPDATE trader_state SET last_stop_reason = ?, last_stop_at = ? WHERE id = 1",
        vec![reason.into(), ts(at).into()],
    )
    .await
    .map(|_| ())
}

pub async fn insert_dry_run(conn: &DatabaseConnection, entry: &DryRunEntry) -> Result<(), Error> {
    exec(
        conn,
        "Trader:InsertDryRun",
        "INSERT INTO dry_run_log (at, action, side, item_id, sub_type, price, quantity, reason, forced_by)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            entry.at.clone().into(),
            entry.action.clone().into(),
            entry.side.clone().into(),
            entry.item_id.clone().into(),
            entry.sub_type.clone().into(),
            entry.price.into(),
            entry.quantity.into(),
            entry.reason.clone().into(),
            entry.forced_by.clone().into(),
        ],
    )
    .await
    .map(|_| ())
}

/// Newest first. `page` starts at 1; `limit` is clamped to 1..=500.
pub async fn dry_run_page(conn: &DatabaseConnection, page: i64, limit: i64) -> Result<DryRunPage, Error> {
    const C: &str = "Trader:DryRunPage";
    let page = page.max(1);
    let limit = limit.clamp(1, 500);
    let total = count(conn, C, "SELECT COUNT(*) AS n FROM dry_run_log", vec![]).await?;
    let results = conn
        .query_all(stmt(
            "SELECT id, at, action, side, item_id, sub_type, price, quantity, reason, forced_by
             FROM dry_run_log ORDER BY id DESC LIMIT ? OFFSET ?",
            vec![limit.into(), ((page - 1) * limit).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| {
            Ok(DryRunEntry {
                id: r.try_get("", "id").map_err(|e| db_err(C, e))?,
                at: r.try_get("", "at").map_err(|e| db_err(C, e))?,
                action: r.try_get("", "action").map_err(|e| db_err(C, e))?,
                side: r.try_get("", "side").map_err(|e| db_err(C, e))?,
                item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
                sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
                price: r.try_get("", "price").map_err(|e| db_err(C, e))?,
                quantity: r.try_get("", "quantity").map_err(|e| db_err(C, e))?,
                reason: r.try_get("", "reason").map_err(|e| db_err(C, e))?,
                forced_by: r.try_get("", "forced_by").map_err(|e| db_err(C, e))?,
            })
        })
        .collect::<Result<_, Error>>()?;
    Ok(DryRunPage { total, page, limit, results })
}

pub async fn prune_dry_run(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    exec(
        conn,
        "Trader:PruneDryRun",
        "DELETE FROM dry_run_log WHERE at < ?",
        vec![ts(now - Duration::days(DRY_RUN_RETENTION_DAYS)).into()],
    )
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collector::parse_ts;

    pub(crate) async fn db() -> (tempfile::TempDir, DatabaseConnection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        (dir, conn)
    }

    fn entry(at: &str, action: &str) -> DryRunEntry {
        DryRunEntry {
            id: 0,
            at: at.into(),
            action: action.into(),
            side: "buy".into(),
            item_id: "item1".into(),
            sub_type: "rank=0".into(),
            price: Some(17),
            quantity: Some(1),
            reason: "Create".into(),
            forced_by: "global".into(),
        }
    }

    #[tokio::test]
    async fn options_default_to_dry_run_and_persist() {
        let (_dir, conn) = db().await;
        let options = load_options(&conn).await.unwrap();
        assert_eq!(
            options,
            TraderOptions { dry_run: true, delete_buy_orders_on_stop: false, helper_override: false, last_stop_reason: None, last_stop_at: None }
        );
        save_flags(&conn, false, true, true).await.unwrap();
        record_stop(&conn, "Stop button", parse_ts("2026-09-16T00:00:00Z").unwrap()).await.unwrap();
        let options = load_options(&conn).await.unwrap();
        assert!(!options.dry_run && options.delete_buy_orders_on_stop && options.helper_override);
        assert_eq!(options.last_stop_reason.as_deref(), Some("Stop button"));
        assert_eq!(options.last_stop_at.as_deref(), Some("2026-09-16T00:00:00Z"));
    }

    #[tokio::test]
    async fn dry_run_log_pages_newest_first_and_prunes_old_rows() {
        let (_dir, conn) = db().await;
        insert_dry_run(&conn, &entry("2026-08-01T00:00:00Z", "create")).await.unwrap();
        insert_dry_run(&conn, &entry("2026-09-15T00:00:00Z", "update")).await.unwrap();
        insert_dry_run(&conn, &entry("2026-09-15T00:01:00Z", "delete")).await.unwrap();
        let page = dry_run_page(&conn, 1, 2).await.unwrap();
        assert_eq!(page.total, 3);
        assert_eq!(page.results.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), vec!["delete", "update"]);
        assert_eq!(dry_run_page(&conn, 2, 2).await.unwrap().results[0].action, "create");
        let removed = prune_dry_run(&conn, parse_ts("2026-09-16T00:00:00Z").unwrap()).await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(dry_run_page(&conn, 1, 10).await.unwrap().total, 2);
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p qf_core --lib trader::store`
Expected: `test result: ok. 2 passed`. The migration runs through `crate::db::connect`.

- [ ] **Step 4: Commit**

```bash
git add crates/migration/src crates/qf_core/src/trader crates/qf_core/src/lib.rs
git commit -m "feat(trader): add trader_state and dry_run_log tables with store

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: PriceSource from item_stats

**Files:**
- Create: `crates/qf_core/src/trader/price_source.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod price_source;`)
- Modify: `crates/qf_core/src/utils/modules/states.rs` (add `try_app_state`)

**Interfaces:**
- Consumes: `collector::orders::sub_type_key`, `collector::stats::ItemStats`, `collector::{db_err, stmt}` (phase 2); `CacheState`, `ItemSettings`, `Settings`, `TradeMode` (upstream port).
- Produces:
  - `ItemPriceInfo`: the upstream fields (except `properties`) plus `warm: bool` and `history_days: i64`. Derives Default, Clone, PartialEq, Serialize, Deserialize.
  - `key_of(&Option<SubType>) -> String`, `sub_type_from_key(&str) -> Option<SubType>`
  - `trait PriceSource: Send + Sync { fn find_by(&self, wfm_id: &str, sub_type: &Option<SubType>) -> Option<ItemPriceInfo>; fn all(&self) -> Vec<ItemPriceInfo>; }`
  - `StatsPriceSource::from_stats(Vec<ItemStats>, url_of: impl Fn(&str) -> Option<String>) -> Self`
  - `async StatsPriceSource::load(conn, &CacheState) -> Result<Self, Error>`
  - `async all_item_stats(conn) -> Result<Vec<ItemStats>, Error>`
  - `is_disabled(i64) -> bool`, `get_interesting_items(&ItemSettings, &dyn PriceSource) -> Vec<ItemPriceInfo>`, `buy_candidate_ids(&Settings, &dyn PriceSource) -> HashSet<String>`
  - `MAX_BUY_CANDIDATES = 150`
  - `states::try_app_state() -> Option<AppState>`

- [ ] **Step 1: Add `try_app_state`**

In `crates/qf_core/src/utils/modules/states.rs`, after `pub fn app_state()`:

```rust
/// The app state if it has been initialised (it isn't in unit tests or before startup finishes).
pub fn try_app_state() -> Option<AppState> {
    APP_STATE.get().and_then(|m| m.lock().ok()).map(|app| app.clone())
}
```

- [ ] **Step 2: Write `price_source.rs` with its tests**

```rust
//! Trader prices from the collector's `item_stats` (spec §5.5 `PriceSource`, amendment C2).

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::{Error, SubType};

use crate::app::{ItemSettings, Settings};
use crate::cache::client::CacheState;
use crate::collector::orders::sub_type_key;
use crate::collector::stats::ItemStats;
use crate::collector::{db_err, stmt};
use crate::enums::TradeMode;

pub const MAX_BUY_CANDIDATES: usize = 150;

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Default)]
pub struct ItemPriceInfo {
    pub wfm_url: String,
    pub wfm_id: String,
    pub uuid: String,
    pub volume: f64,
    pub max_price: f64,
    pub min_price: f64,
    pub avg_price: f64,
    pub moving_avg: Option<f64>,
    pub median: f64,
    pub profit: f64,
    #[serde(default)]
    pub profit_margin: f64,
    #[serde(default)]
    pub trading_tax: i64,
    #[serde(default)]
    pub week_price_shift: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
    #[serde(default)]
    pub warm: bool,
    #[serde(default)]
    pub history_days: i64,
}

pub fn is_disabled(value: i64) -> bool {
    value <= -1
}

/// The collector `sub_type` column value for a stock or wish-list sub-type (amendment B4).
pub fn key_of(sub_type: &Option<SubType>) -> String {
    match sub_type {
        None => String::new(),
        Some(s) => sub_type_key(s.rank, s.charges, s.variant.as_deref(), s.amber_stars, s.cyan_stars),
    }
}

pub fn sub_type_from_key(key: &str) -> Option<SubType> {
    if key.is_empty() {
        return None;
    }
    let mut sub_type = SubType::default();
    for part in key.split(';') {
        let Some((name, value)) = part.split_once('=') else { continue };
        match name {
            "rank" => sub_type.rank = value.parse().ok(),
            "charges" => sub_type.charges = value.parse().ok(),
            "subtype" => sub_type.variant = Some(value.to_string()),
            "amber" => sub_type.amber_stars = value.parse().ok(),
            "cyan" => sub_type.cyan_stars = value.parse().ok(),
            _ => {}
        }
    }
    Some(sub_type)
}

pub trait PriceSource: Send + Sync {
    fn find_by(&self, wfm_id: &str, sub_type: &Option<SubType>) -> Option<ItemPriceInfo>;
    fn all(&self) -> Vec<ItemPriceInfo>;
}

#[derive(Default)]
pub struct StatsPriceSource {
    items: HashMap<(String, String), ItemPriceInfo>,
}

impl StatsPriceSource {
    /// Items whose id `url_of` can't resolve (no longer tradable) are skipped.
    pub fn from_stats(stats: Vec<ItemStats>, url_of: impl Fn(&str) -> Option<String>) -> Self {
        let items = stats
            .into_iter()
            .filter_map(|s| {
                let wfm_url = url_of(&s.item_id)?;
                let info = ItemPriceInfo {
                    uuid: format!("{}:{}", s.item_id, s.sub_type),
                    wfm_url,
                    wfm_id: s.item_id.clone(),
                    sub_type: sub_type_from_key(&s.sub_type),
                    volume: s.volume,
                    max_price: s.max_price.map(|v| v as f64).unwrap_or(0.0),
                    min_price: s.min_price.map(|v| v as f64).unwrap_or(0.0),
                    avg_price: s.avg_price.unwrap_or(0.0),
                    moving_avg: s.moving_avg,
                    median: s.median.unwrap_or(0.0),
                    profit: s.profit.unwrap_or(0.0),
                    profit_margin: 0.0,
                    trading_tax: 0,
                    week_price_shift: 0.0,
                    warm: s.warm,
                    history_days: s.history_days,
                };
                Some(((s.item_id, s.sub_type), info))
            })
            .collect();
        Self { items }
    }

    pub async fn load(conn: &DatabaseConnection, cache: &CacheState) -> Result<Self, Error> {
        let stats = all_item_stats(conn).await?;
        let tradable = cache.tradable_item();
        Ok(Self::from_stats(stats, |id| tradable.get_by(id).ok().map(|item| item.wfm_url)))
    }
}

impl PriceSource for StatsPriceSource {
    fn find_by(&self, wfm_id: &str, sub_type: &Option<SubType>) -> Option<ItemPriceInfo> {
        self.items.get(&(wfm_id.to_string(), key_of(sub_type))).cloned()
    }

    fn all(&self) -> Vec<ItemPriceInfo> {
        self.items.values().cloned().collect()
    }
}

pub async fn all_item_stats(conn: &DatabaseConnection) -> Result<Vec<ItemStats>, Error> {
    const C: &str = "Trader:ItemStats";
    conn.query_all(stmt(
        "SELECT item_id, sub_type, volume, avg_price, moving_avg, profit, min_price, max_price, median, history_days, warm, updated_at
         FROM item_stats",
        vec![],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|r| {
        Ok(ItemStats {
            item_id: r.try_get("", "item_id").map_err(|e| db_err(C, e))?,
            sub_type: r.try_get("", "sub_type").map_err(|e| db_err(C, e))?,
            volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
            avg_price: r.try_get("", "avg_price").map_err(|e| db_err(C, e))?,
            moving_avg: r.try_get("", "moving_avg").map_err(|e| db_err(C, e))?,
            profit: r.try_get("", "profit").map_err(|e| db_err(C, e))?,
            min_price: r.try_get("", "min_price").map_err(|e| db_err(C, e))?,
            max_price: r.try_get("", "max_price").map_err(|e| db_err(C, e))?,
            median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
            history_days: r.try_get("", "history_days").map_err(|e| db_err(C, e))?,
            warm: r.try_get::<i64>("", "warm").map_err(|e| db_err(C, e))? != 0,
            updated_at: r.try_get("", "updated_at").map_err(|e| db_err(C, e))?,
        })
    })
    .collect()
}

/// Port of upstream `helpers::get_interesting_items` (amendment C2): the volume, profit and
/// average-price filters apply; by volume descending, at most `MAX_BUY_CANDIDATES`.
pub fn get_interesting_items(settings: &ItemSettings, prices: &dyn PriceSource) -> Vec<ItemPriceInfo> {
    let wtb = &settings.wtb;
    let mut items: Vec<ItemPriceInfo> = prices
        .all()
        .into_iter()
        .filter(|i| is_disabled(wtb.volume_threshold) || i.volume > wtb.volume_threshold as f64)
        .filter(|i| is_disabled(wtb.profit_threshold) || i.profit > wtb.profit_threshold as f64)
        .filter(|i| is_disabled(wtb.avg_price_cap) || i.avg_price <= wtb.avg_price_cap as f64)
        .collect();
    items.sort_by(|a, b| {
        b.volume
            .partial_cmp(&a.volume)
            .unwrap_or(Ordering::Equal)
            .then_with(|| a.uuid.cmp(&b.uuid))
    });
    items.truncate(MAX_BUY_CANDIDATES);
    items
}

/// Buy-candidate item ids for the collector hot set (amendment C3).
pub fn buy_candidate_ids(settings: &Settings, prices: &dyn PriceSource) -> HashSet<String> {
    if !settings.live_scraper.has_trade_mode(TradeMode::Buy) {
        return HashSet::new();
    }
    get_interesting_items(&settings.live_scraper.items, prices)
        .into_iter()
        .map(|i| i.wfm_id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(item_id: &str, sub_type: &str, volume: f64, profit: f64, avg: f64) -> ItemStats {
        ItemStats {
            item_id: item_id.into(),
            sub_type: sub_type.into(),
            volume,
            avg_price: Some(avg),
            moving_avg: Some(avg),
            profit: Some(profit),
            min_price: Some(1),
            max_price: Some(999),
            median: Some(avg),
            history_days: 8,
            warm: true,
            updated_at: "2026-09-15T00:00:00Z".into(),
        }
    }

    fn source(rows: Vec<ItemStats>) -> StatsPriceSource {
        StatsPriceSource::from_stats(rows, |id| (id != "gone").then(|| format!("{id}_slug")))
    }

    #[test]
    fn sub_type_keys_round_trip() {
        assert_eq!(key_of(&None), "");
        assert_eq!(sub_type_from_key(""), None);
        for key in ["rank=5", "subtype=intact", "amber=0;cyan=1", "rank=0;charges=3"] {
            assert_eq!(key_of(&sub_type_from_key(key)), key);
        }
        assert_eq!(sub_type_from_key("rank=5").unwrap().rank, Some(5));
    }

    #[test]
    fn stats_map_to_price_info_by_item_and_sub_type() {
        let prices = source(vec![stats("a", "rank=0", 20.0, 15.0, 100.0), stats("a", "rank=5", 2.0, 40.0, 300.0), stats("gone", "", 99.0, 99.0, 1.0)]);
        assert_eq!(prices.all().len(), 2, "untradable items are skipped");
        let rank5 = prices.find_by("a", &sub_type_from_key("rank=5")).unwrap();
        assert_eq!(rank5.wfm_url, "a_slug");
        assert_eq!(rank5.avg_price, 300.0);
        assert_eq!(rank5.max_price, 999.0);
        assert!(rank5.warm);
        assert_eq!(rank5.history_days, 8);
        assert!(prices.find_by("a", &None).is_none());
    }

    #[test]
    fn interesting_items_filter_sort_and_respect_disabled_thresholds() {
        let prices = source(vec![
            stats("a", "", 20.0, 15.0, 100.0),
            stats("b", "", 30.0, 5.0, 100.0),
            stats("c", "", 16.0, 50.0, 700.0),
            stats("d", "", 40.0, 20.0, 50.0),
        ]);
        let mut settings = ItemSettings::default();
        settings.wtb.volume_threshold = 15;
        settings.wtb.profit_threshold = 10;
        settings.wtb.avg_price_cap = 600;
        let ids = |items: Vec<ItemPriceInfo>| items.into_iter().map(|i| i.wfm_id).collect::<Vec<_>>();
        assert_eq!(ids(get_interesting_items(&settings, &prices)), vec!["d", "a"]);
        settings.wtb.profit_threshold = -1;
        assert_eq!(ids(get_interesting_items(&settings, &prices)), vec!["d", "b", "a"]);
    }

    #[test]
    fn buy_candidates_are_capped_and_need_buy_mode() {
        let rows = (0..200).map(|n| stats(&format!("i{n:03}"), "", 100.0 + n as f64, 50.0, 10.0)).collect();
        let prices = source(rows);
        let mut settings = Settings::default();
        assert_eq!(buy_candidate_ids(&settings, &prices).len(), MAX_BUY_CANDIDATES);
        settings.live_scraper.general.trade_modes.retain(|m| *m != TradeMode::Buy);
        assert!(buy_candidate_ids(&settings, &prices).is_empty());
    }
}
```

Add `pub mod price_source;` to `crates/qf_core/src/trader/mod.rs` after `pub mod store;`, in alphabetical order (`price_source` before `store`).

- [ ] **Step 3: Run the tests**

Run: `cargo test -p qf_core --lib trader::price_source`
Expected: `test result: ok. 4 passed`.

If `ItemSettings::default()` sets non-default wtb values, the explicit assignments in the test already cover it. If `crate::app::ItemSettings` isn't re-exported at that path, use `crate::app::types::settings::ItemSettings`: check with `grep -rn "pub use" crates/qf_core/src/app/mod.rs`.

- [ ] **Step 4: Commit**

```bash
git add crates/qf_core/src
git commit -m "feat(trader): price trader decisions from collected item stats

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Session health tracking

**Files:**
- Create: `crates/qf_core/src/trader/session.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod session;`)
- Modify: `crates/qf_core/src/app/modules/ws.rs`, `crates/qf_core/src/commands/auth.rs`, `crates/qf_core/src/startup.rs`

**Interfaces:**
- Consumes: `states::try_app_state` (Task 3), `crypto::jwt_expiry`, `wf_market` `user().me()`.
- Produces:
  - `session::SessionSnapshot { signed_in, token_valid, unauthorized, ws_connected: bool, ws_down_for_s: Option<i64>, last_me_ok_at, token_expires_at: Option<String> }` (Serialize, Default, Clone, PartialEq)
  - `session::Session` (Default) with:
    - `mark_signed_in(at, token_expires_at: Option<DateTime<Utc>>)`, `mark_signed_out()`
    - `mark_me_ok(at)`, `mark_unauthorized()`, `set_ws_connected(connected: bool, at)`
    - `snapshot(now) -> SessionSnapshot`
    - `expiry_alert_due(now) -> Option<DateTime<Utc>>`
  - `session::get() -> &'static Session`, `async session::me_check_loop()`, `async session::check_me_once()`
  - Constants `ME_CHECK_EVERY`, `TOKEN_FRESH_MINUTES = 20`, `EXPIRY_WARNING_DAYS = 7`

- [ ] **Step 1: Write `session.rs` with its tests**

```rust
//! warframe.market session health for the trader lifecycle (amendments C7, C10, C12).

use std::sync::{Mutex, OnceLock};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use utils::{error, LoggerOptions};
use wf_market::errors::ApiError;

use crate::collector::ts;
use crate::utils::modules::states;

pub const ME_CHECK_EVERY: StdDuration = StdDuration::from_secs(15 * 60);
pub const TOKEN_FRESH_MINUTES: i64 = 20;
pub const EXPIRY_WARNING_DAYS: i64 = 7;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SessionSnapshot {
    pub signed_in: bool,
    pub token_valid: bool,
    pub unauthorized: bool,
    pub ws_connected: bool,
    pub ws_down_for_s: Option<i64>,
    pub last_me_ok_at: Option<String>,
    pub token_expires_at: Option<String>,
}

#[derive(Default)]
struct Inner {
    signed_in: bool,
    unauthorized: bool,
    last_me_ok: Option<DateTime<Utc>>,
    ws_connected: bool,
    ws_down_since: Option<DateTime<Utc>>,
    token_expires_at: Option<DateTime<Utc>>,
    last_expiry_alert: Option<DateTime<Utc>>,
}

#[derive(Default)]
pub struct Session {
    inner: Mutex<Inner>,
}

static SESSION: OnceLock<Session> = OnceLock::new();

pub fn get() -> &'static Session {
    SESSION.get_or_init(Session::default)
}

impl Session {
    pub fn mark_signed_in(&self, at: DateTime<Utc>, token_expires_at: Option<DateTime<Utc>>) {
        let mut inner = self.inner.lock().unwrap();
        inner.signed_in = true;
        inner.unauthorized = false;
        inner.last_me_ok = Some(at);
        inner.token_expires_at = token_expires_at;
        inner.last_expiry_alert = None;
    }

    pub fn mark_signed_out(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.signed_in = false;
        inner.unauthorized = false;
        inner.last_me_ok = None;
        inner.token_expires_at = None;
        inner.last_expiry_alert = None;
    }

    pub fn mark_me_ok(&self, at: DateTime<Utc>) {
        let mut inner = self.inner.lock().unwrap();
        if inner.signed_in {
            inner.last_me_ok = Some(at);
            inner.unauthorized = false;
        }
    }

    pub fn mark_unauthorized(&self) {
        self.inner.lock().unwrap().unauthorized = true;
    }

    pub fn set_ws_connected(&self, connected: bool, at: DateTime<Utc>) {
        let mut inner = self.inner.lock().unwrap();
        if connected {
            inner.ws_connected = true;
            inner.ws_down_since = None;
        } else {
            inner.ws_connected = false;
            if inner.ws_down_since.is_none() {
                inner.ws_down_since = Some(at);
            }
        }
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> SessionSnapshot {
        let inner = self.inner.lock().unwrap();
        SessionSnapshot {
            signed_in: inner.signed_in,
            token_valid: inner.signed_in
                && !inner.unauthorized
                && inner.last_me_ok.is_some_and(|t| now - t <= Duration::minutes(TOKEN_FRESH_MINUTES)),
            unauthorized: inner.unauthorized,
            ws_connected: inner.ws_connected,
            ws_down_for_s: if inner.ws_connected { None } else { inner.ws_down_since.map(|t| (now - t).num_seconds()) },
            last_me_ok_at: inner.last_me_ok.map(ts),
            token_expires_at: inner.token_expires_at.map(ts),
        }
    }

    /// The token expiry when an alert is due (expires within 7 days, none sent in 24 h). Records the alert.
    pub fn expiry_alert_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut inner = self.inner.lock().unwrap();
        let expires = inner.token_expires_at?;
        if !inner.signed_in || expires - now > Duration::days(EXPIRY_WARNING_DAYS) {
            return None;
        }
        if inner.last_expiry_alert.is_some_and(|t| now - t < Duration::hours(24)) {
            return None;
        }
        inner.last_expiry_alert = Some(now);
        Some(expires)
    }
}

/// Checks `/me` every 15 minutes while signed in (spec §5.1).
pub async fn me_check_loop() {
    loop {
        tokio::time::sleep(ME_CHECK_EVERY).await;
        check_me_once().await;
    }
}

pub async fn check_me_once() {
    let session = get();
    if !session.snapshot(Utc::now()).signed_in {
        return;
    }
    let Some(app) = states::try_app_state() else { return };
    match app.wfm_client.user().me().await {
        Ok(_) => session.mark_me_ok(Utc::now()),
        Err(ApiError::Unauthorized(_)) => session.mark_unauthorized(),
        Err(e) => error("Trader:Session", format!("/me check failed: {}", e), &LoggerOptions::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;

    fn at(minutes: i64) -> DateTime<Utc> {
        parse_ts("2026-09-16T00:00:00Z").unwrap() + Duration::minutes(minutes)
    }

    #[test]
    fn token_is_valid_only_while_signed_in_fresh_and_authorised() {
        let session = Session::default();
        assert!(!session.snapshot(at(0)).token_valid);
        session.mark_signed_in(at(0), None);
        assert!(session.snapshot(at(20)).token_valid);
        assert!(!session.snapshot(at(21)).token_valid, "stale /me");
        session.mark_me_ok(at(21));
        assert!(session.snapshot(at(30)).token_valid);
        session.mark_unauthorized();
        let snap = session.snapshot(at(30));
        assert!(snap.unauthorized && !snap.token_valid);
        session.mark_me_ok(at(31));
        assert!(session.snapshot(at(31)).token_valid, "a good /me clears the 401");
        session.mark_signed_out();
        assert!(!session.snapshot(at(31)).signed_in);
    }

    #[test]
    fn websocket_down_time_counts_from_the_first_disconnect() {
        let session = Session::default();
        assert_eq!(session.snapshot(at(0)).ws_down_for_s, None, "unknown before any callback");
        session.set_ws_connected(true, at(0));
        session.set_ws_connected(false, at(1));
        session.set_ws_connected(false, at(2));
        assert_eq!(session.snapshot(at(3)).ws_down_for_s, Some(120));
        session.set_ws_connected(true, at(4));
        let snap = session.snapshot(at(5));
        assert!(snap.ws_connected);
        assert_eq!(snap.ws_down_for_s, None);
    }

    #[test]
    fn expiry_alert_fires_within_seven_days_at_most_daily() {
        let session = Session::default();
        session.mark_signed_in(at(0), Some(at(0) + Duration::days(10)));
        assert_eq!(session.expiry_alert_due(at(0)), None, "10 days left");
        let four_days_later = at(0) + Duration::days(4);
        assert!(session.expiry_alert_due(four_days_later).is_some());
        assert_eq!(session.expiry_alert_due(four_days_later + Duration::hours(23)), None);
        assert!(session.expiry_alert_due(four_days_later + Duration::hours(24)).is_some());
    }
}
```

Add `pub mod session;` to `crates/qf_core/src/trader/mod.rs`, keeping the module list alphabetical.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib trader::session`
Expected: `test result: ok. 3 passed`.

- [ ] **Step 3: Hook the websocket, auth and startup**

In `crates/qf_core/src/app/modules/ws.rs`, inside `setup_socket`:
- In the `"internal/connected"` callback, before `send_ws_state("Main:Connected", msg);`, add:
  ```rust
              crate::trader::session::get().set_ws_connected(true, chrono::Utc::now());
  ```
- In both the `"internal/disconnected"` and `"internal/reconnecting"` callbacks, before `send_ws_state("Main:Disconnected", msg);`, add:
  ```rust
              crate::trader::session::get().set_ws_connected(false, chrono::Utc::now());
  ```

In `crates/qf_core/src/commands/auth.rs`:
- In `auth_login`, replace:
  ```rust
      send_event!(UIEvent::RefreshCache, "Cache refreshed successfully");
      Ok(updated_user)
  ```
  with:
  ```rust
      send_event!(UIEvent::RefreshCache, "Cache refreshed successfully");
      crate::trader::session::get()
          .mark_signed_in(chrono::Utc::now(), crate::crypto::jwt_expiry(&app.wfm_client.get_token()));
      Ok(updated_user)
  ```
- In `auth_logout`, replace:
  ```rust
      app.wfm_socket = None;
      Ok(new_user)
  ```
  with:
  ```rust
      app.wfm_socket = None;
      crate::trader::session::get().mark_signed_out();
      Ok(new_user)
  ```

In `crates/qf_core/src/startup.rs`, replace:

```rust
    if states::app_state()?.wfm_socket.is_some() {
```

with:

```rust
    if states::app_state()?.wfm_socket.is_some() {
        crate::trader::session::get().mark_signed_in(
            chrono::Utc::now(),
            crypto::jwt_expiry(&states::app_state()?.wfm_client.get_token()),
        );
```

- [ ] **Step 4: Run all qf_core tests**

Run: `cargo test -p qf_core --lib`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add crates/qf_core/src
git commit -m "feat(trader): track warframe.market session, websocket and token expiry

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: TradeOrders (live and dry-run order writes)

**Files:**
- Create: `crates/qf_core/src/trader/orders.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod orders;`)

**Interfaces:**
- Consumes: `store::{insert_dry_run, DryRunEntry}`, `store::tests::db` (Task 2); `session::get().mark_unauthorized()` (Task 4); `collector::ts`; `ErrorFromExt::from_wfm`.
- Produces:
  - `ForcedBy { Global, NotWarm }` with `as_str()`, and `Route { Live, DryRun(ForcedBy) }` (Copy)
  - `route_for(global_dry_run: bool, warm: bool) -> Route`
  - `WriteMeta { sub_type: String, reason: String }` (Default)
  - `MAX_CONSECUTIVE_FAILURES = 5`
  - `TradeOrders::new(live: Option<wf_market::Client<Authenticated>>, conn: Option<DatabaseConnection>, global_dry_run: bool)`
  - `TradeOrders` methods:
    - `global_dry_run() -> bool`, `cache_orders() -> OrderList<Order>`
    - `find_order(wfm_id: &str, sub_type: &WFSubType, order_type: OrderType, route: Route) -> Option<Order>`
    - `can_create_order(route) -> bool`
    - `async create(params: CreateOrderParams, route, meta: &WriteMeta) -> Result<Order, Error>`
    - `async update(order_id: &str, params: UpdateOrderParams, meta) -> Result<Order, Error>`
    - `async delete(order_id: &str, meta) -> Result<(), Error>`
    - `async refresh()`, `async get_orders_by_item(slug) -> Result<OrderList<OrderWithUser>, Error>`
    - `consecutive_failures() -> u32`, `dry_log() -> Vec<DryRunEntry>`
    - `live_buy_order_ids() -> Vec<String>`

- [ ] **Step 1: Write `orders.rs` with its tests**

```rust
//! Order writes for the trader: live wf-market calls or a simulated book (amendments C4, C6).

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use chrono::Utc;
use service::sea_orm::DatabaseConnection;
use serde::Serialize;
use utils::{error, get_location, Error, LogLevel, LoggerOptions};
use wf_market::client::Authenticated;
use wf_market::enums::OrderType;
use wf_market::errors::ApiError;
use wf_market::types::{
    CreateOrderParams, Order, OrderList, OrderWithUser, Properties as WFProperties, SubType as WFSubType,
    UpdateOrderParams,
};
use wf_market::Client;

use super::session;
use super::store::{self, DryRunEntry};
use crate::collector::ts;
use crate::utils::ErrorFromExt;

pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
const DRY_LOG_MEMORY: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ForcedBy {
    Global,
    NotWarm,
}

impl ForcedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            ForcedBy::Global => "global",
            ForcedBy::NotWarm => "not_warm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    Live,
    DryRun(ForcedBy),
}

pub fn route_for(global_dry_run: bool, warm: bool) -> Route {
    if global_dry_run {
        Route::DryRun(ForcedBy::Global)
    } else if !warm {
        Route::DryRun(ForcedBy::NotWarm)
    } else {
        Route::Live
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WriteMeta {
    pub sub_type: String,
    pub reason: String,
}

fn side(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Buy => "buy",
        OrderType::Sell => "sell",
    }
}

fn simulated_order(params: &CreateOrderParams) -> Order {
    let now = ts(Utc::now());
    Order {
        id: format!("dry-{}", uuid::Uuid::new_v4()),
        order_type: params.order_type,
        platinum: params.platinum,
        quantity: params.quantity,
        per_trade: params.per_trade.map(|p| p.min(u8::MAX as u32) as u8),
        subtype: params.subtype.clone().unwrap_or_default(),
        visible: params.visible,
        item_id: params.item_id.clone(),
        created_at: now.clone(),
        updated_at: now,
        properties: params.properties.clone().map(WFProperties::from).unwrap_or_default(),
    }
}

pub struct TradeOrders {
    live: Option<Client<Authenticated>>,
    conn: Option<DatabaseConnection>,
    global_dry_run: bool,
    book: Mutex<OrderList<Order>>,
    log: Mutex<Vec<DryRunEntry>>,
    failures: AtomicU32,
}

impl TradeOrders {
    /// Under global dry-run the simulated book starts as a copy of the real cached orders.
    pub fn new(live: Option<Client<Authenticated>>, conn: Option<DatabaseConnection>, global_dry_run: bool) -> Self {
        let book = match (&live, global_dry_run) {
            (Some(client), true) => client.order().cache_orders(),
            _ => OrderList::new(vec![]),
        };
        Self {
            live,
            conn,
            global_dry_run,
            book: Mutex::new(book),
            log: Mutex::new(Vec::new()),
            failures: AtomicU32::new(0),
        }
    }

    pub fn global_dry_run(&self) -> bool {
        self.global_dry_run
    }

    /// Every order the trader reasons about: the simulated book, plus real cached orders when not in global dry-run.
    pub fn cache_orders(&self) -> OrderList<Order> {
        let book = self.book.lock().unwrap().clone();
        match (&self.live, self.global_dry_run) {
            (Some(client), false) => {
                let mut all = client.order().cache_orders().to_vec();
                all.extend(book.to_vec());
                OrderList::new(all)
            }
            _ => book,
        }
    }

    /// Real cached buy orders, used by the stop sequence (amendment C9).
    pub fn live_buy_order_ids(&self) -> Vec<String> {
        self.live
            .as_ref()
            .map(|client| client.order().cache_orders().order_ids(OrderType::Buy))
            .unwrap_or_default()
    }

    pub fn find_order(&self, wfm_id: &str, sub_type: &WFSubType, order_type: OrderType, route: Route) -> Option<Order> {
        match route {
            Route::DryRun(_) => self.book.lock().unwrap().find_order(wfm_id, sub_type, order_type),
            Route::Live => self.live.as_ref()?.order().cache_orders().find_order(wfm_id, sub_type, order_type),
        }
    }

    pub fn can_create_order(&self, route: Route) -> bool {
        match route {
            Route::DryRun(_) => true,
            Route::Live => self.live.as_ref().is_some_and(|client| client.order().can_create_order()),
        }
    }

    pub fn consecutive_failures(&self) -> u32 {
        self.failures.load(Ordering::SeqCst)
    }

    pub fn dry_log(&self) -> Vec<DryRunEntry> {
        self.log.lock().unwrap().clone()
    }

    fn in_book(&self, order_id: &str) -> bool {
        self.global_dry_run || self.book.lock().unwrap().get_by_id(order_id).is_some()
    }

    fn book_forced_by(&self) -> ForcedBy {
        if self.global_dry_run { ForcedBy::Global } else { ForcedBy::NotWarm }
    }

    async fn record(&self, action: &str, order: &Order, price: Option<i64>, quantity: Option<i64>, meta: &WriteMeta, forced_by: ForcedBy) {
        let entry = DryRunEntry {
            id: 0,
            at: ts(Utc::now()),
            action: action.to_string(),
            side: side(order.order_type).to_string(),
            item_id: order.item_id.clone(),
            sub_type: meta.sub_type.clone(),
            price,
            quantity,
            reason: meta.reason.clone(),
            forced_by: forced_by.as_str().to_string(),
        };
        if let Some(conn) = &self.conn {
            if let Err(e) = store::insert_dry_run(conn, &entry).await {
                let _ = e.log("trader_dry_run.log");
            }
        }
        let mut log = self.log.lock().unwrap();
        log.push(entry);
        if log.len() > DRY_LOG_MEMORY {
            let excess = log.len() - DRY_LOG_MEMORY;
            log.drain(0..excess);
        }
    }

    fn live_client(&self) -> Result<&Client<Authenticated>, Error> {
        self.live.as_ref().ok_or_else(|| {
            self.failures.fetch_add(1, Ordering::SeqCst);
            Error::new("Trader:Orders", "No live warframe.market client", get_location!())
        })
    }

    async fn finish_live<T>(&self, action: &str, result: Result<T, ApiError>) -> Result<T, Error> {
        match result {
            Ok(value) => {
                self.failures.store(0, Ordering::SeqCst);
                Ok(value)
            }
            Err(e) => {
                self.failures.fetch_add(1, Ordering::SeqCst);
                let level = match &e {
                    ApiError::OrderLimitExceededSamePrice(_) | ApiError::NotFound(_) | ApiError::OrderLimitExceeded(_) => {
                        self.refresh().await;
                        LogLevel::Warning
                    }
                    ApiError::Unauthorized(_) => {
                        session::get().mark_unauthorized();
                        LogLevel::Error
                    }
                    _ => LogLevel::Error,
                };
                Err(Error::from_wfm(
                    format!("Trader:Orders:{}", action),
                    format!("Failed to {} order", action.to_lowercase()),
                    e,
                    get_location!(),
                )
                .set_log_level(level))
            }
        }
    }

    pub async fn create(&self, params: CreateOrderParams, route: Route, meta: &WriteMeta) -> Result<Order, Error> {
        match route {
            Route::DryRun(forced_by) => {
                let order = simulated_order(&params);
                self.book.lock().unwrap().add(order.clone());
                self.record("create", &order, Some(order.platinum as i64), Some(order.quantity as i64), meta, forced_by)
                    .await;
                Ok(order)
            }
            Route::Live => {
                let client = self.live_client()?;
                let result = client.order().create(params).await;
                self.finish_live("Create", result).await
            }
        }
    }

    pub async fn update(&self, order_id: &str, params: UpdateOrderParams, meta: &WriteMeta) -> Result<Order, Error> {
        if self.in_book(order_id) {
            let price = params.platinum.map(i64::from);
            let quantity = params.quantity.map(i64::from);
            let order = {
                let mut book = self.book.lock().unwrap();
                book.update(order_id, params);
                book.get_by_id(order_id)
            }
            .ok_or_else(|| {
                Error::new("Trader:Orders:Update", format!("Simulated order {} not found", order_id), get_location!())
            })?;
            self.record("update", &order, price, quantity, meta, self.book_forced_by()).await;
            return Ok(order);
        }
        let client = self.live_client()?;
        let result = client.order().update(order_id, params).await;
        self.finish_live("Update", result).await
    }

    pub async fn delete(&self, order_id: &str, meta: &WriteMeta) -> Result<(), Error> {
        if self.in_book(order_id) {
            let order = {
                let mut book = self.book.lock().unwrap();
                let order = book.get_by_id(order_id);
                book.remove_by_id(order_id);
                order
            };
            if let Some(order) = order {
                self.record("delete", &order, None, None, meta, self.book_forced_by()).await;
            }
            return Ok(());
        }
        let client = self.live_client()?;
        let result = client.order().delete(order_id).await;
        self.finish_live("Delete", result).await.map(|_| ())
    }

    pub async fn refresh(&self) {
        if let Some(client) = &self.live {
            if let Err(e) = client.order().my_orders().await {
                error("Trader:Orders:Refresh", format!("Failed to refresh orders: {}", e), &LoggerOptions::default());
            }
        }
    }

    pub async fn get_orders_by_item(&self, slug: &str) -> Result<OrderList<OrderWithUser>, Error> {
        let client = self.live.as_ref().ok_or_else(|| {
            Error::new("Trader:Orders:Book", "No warframe.market client to read the order book", get_location!())
        })?;
        client.order().get_orders_by_item(slug).await.map_err(|e| {
            Error::from_wfm("Trader:Orders:Book", format!("Failed to get live orders for item {}", slug), e, get_location!())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(item: &str, order_type: OrderType, platinum: u32) -> CreateOrderParams {
        CreateOrderParams::new_with_subtype(item, order_type, platinum, 1, true, None, WFSubType::default())
    }

    fn meta(reason: &str) -> WriteMeta {
        WriteMeta { sub_type: String::new(), reason: reason.into() }
    }

    #[test]
    fn routing_follows_global_dry_run_then_warm() {
        assert_eq!(route_for(true, true), Route::DryRun(ForcedBy::Global));
        assert_eq!(route_for(true, false), Route::DryRun(ForcedBy::Global));
        assert_eq!(route_for(false, false), Route::DryRun(ForcedBy::NotWarm));
        assert_eq!(route_for(false, true), Route::Live);
    }

    #[tokio::test]
    async fn global_dry_run_mirrors_writes_in_the_book_and_logs_them() {
        let orders = TradeOrders::new(None, None, true);
        let route = route_for(true, true);
        let created = orders.create(params("item1", OrderType::Buy, 17), route, &meta("Create")).await.unwrap();
        assert!(created.id.starts_with("dry-"));
        assert_eq!(orders.cache_orders().buy_orders.len(), 1);
        assert!(orders.find_order("item1", &WFSubType::default(), OrderType::Buy, route).is_some());

        let updated = orders.update(&created.id, UpdateOrderParams::new().with_platinum(20), &meta("Update")).await.unwrap();
        assert_eq!(updated.platinum, 20);
        orders.delete(&created.id, &meta("Update,Delete")).await.unwrap();
        assert!(orders.cache_orders().buy_orders.is_empty());

        let log = orders.dry_log();
        assert_eq!(log.iter().map(|e| e.action.as_str()).collect::<Vec<_>>(), vec!["create", "update", "delete"]);
        assert_eq!(log.iter().map(|e| e.price).collect::<Vec<_>>(), vec![Some(17), Some(20), None]);
        assert!(log.iter().all(|e| e.forced_by == "global" && e.side == "buy" && e.item_id == "item1"));
    }

    #[tokio::test]
    async fn not_warm_items_are_simulated_when_global_dry_run_is_off() {
        let orders = TradeOrders::new(None, None, false);
        let created = orders
            .create(params("item2", OrderType::Sell, 40), route_for(false, false), &meta("Create"))
            .await
            .unwrap();
        orders.update(&created.id, UpdateOrderParams::new().with_platinum(35), &meta("Update")).await.unwrap();
        let log = orders.dry_log();
        assert_eq!(log.len(), 2);
        assert!(log.iter().all(|e| e.forced_by == "not_warm" && e.side == "sell"));
    }

    #[tokio::test]
    async fn live_failures_are_counted() {
        let orders = TradeOrders::new(None, None, false);
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            assert!(orders.create(params("item3", OrderType::Buy, 10), Route::Live, &meta("Create")).await.is_err());
        }
        assert_eq!(orders.consecutive_failures(), MAX_CONSECUTIVE_FAILURES);
    }

    #[tokio::test]
    async fn simulated_writes_are_persisted_when_a_database_is_given() {
        let (_dir, conn) = crate::trader::store::tests::db().await;
        let orders = TradeOrders::new(None, Some(conn.clone()), true);
        orders.create(params("item1", OrderType::Buy, 17), route_for(true, true), &meta("Create")).await.unwrap();
        let page = store::dry_run_page(&conn, 1, 10).await.unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.results[0].price, Some(17));
    }
}
```

Add `pub mod orders;` to `crates/qf_core/src/trader/mod.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib trader::orders`
Expected: `test result: ok. 5 passed`.

If `CreateOrderParams::new_with_subtype` takes `impl Into<String>` rather than `&str`, the calls compile unchanged. If `UpdateOrderParams`'s `platinum` or `quantity` fields are private, read them before moving `params` using the builder's getters. Check with `grep -n "pub platinum" crates/wf-market/src/types/update_order.rs`.

- [ ] **Step 3: Commit**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): route order writes to warframe.market or a logged dry-run book

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: Port item entries and trader helpers

**Files:**
- Create: `crates/qf_core/src/trader/item_entry.rs` (port of `upstream:src-tauri/src/live_scraper/types/item_entry.rs`)
- Create: `crates/qf_core/src/trader/helpers.rs` (port of `upstream:src-tauri/src/live_scraper/modules/helpers.rs`)
- Modify: `crates/qf_core/src/trader/mod.rs` (modules and `TradeContext`)

**Interfaces:**
- Consumes: `TradeOrders`, `Route`, `WriteMeta` (Task 5); `PriceSource`, `ItemPriceInfo`, `get_interesting_items`, `is_disabled`, `key_of`, `StatsPriceSource` (Task 3).
- Produces:
  - `TradeContext { conn: DatabaseConnection, cache: CacheState, settings: Settings, orders: Arc<TradeOrders>, prices: Arc<dyn PriceSource>, username: String, banned: bool }`, with `async TradeContext::load(conn, orders) -> Result<Self, Error>`
  - `item_entry::{ItemMarketInfo, ItemEntry}`: same API as upstream, minus the syndicate conversion
  - `helpers` functions:
    - `knapsack`
    - `async collect_interesting_items(ctx, component: &str) -> Result<Vec<ItemEntry>, Error>`
    - `get_order_info(entry, order_type, orders: &TradeOrders, route) -> (String, i64, WFProperties, OperationSet)`
    - `populate_order_properties`, `set_order_market_metrics`, `push_price_history`
    - `orders_to_delete(settings, just_started: bool, my_orders) -> Vec<String>`
    - `async load_orders(component, orders, item_url, fake_path: Option<&Path>)`
    - `async progress_order(component, entry, orders, route, order_type, post_price: u32, per_trade, log_options, properties, trade_operations)`
    - `async delete_order(component, entry, order_type, orders, route)`
    - `log_summary`, `get_per_trade`, `is_blacklisted`, `should_apply_max_price_drop`

- [ ] **Step 1: Add `TradeContext` and the module list**

`crates/qf_core/src/trader/mod.rs` becomes:

```rust
//! Item trader (spec §5.6, §5.7 and amendments §16).

pub mod helpers;
pub mod item_entry;
pub mod orders;
pub mod price_source;
pub mod session;
pub mod store;

use std::sync::Arc;

use service::sea_orm::DatabaseConnection;
use utils::Error;

use crate::app::Settings;
use crate::cache::client::CacheState;
use crate::utils::modules::states;
use orders::TradeOrders;
use price_source::{PriceSource, StatsPriceSource};

pub const DRY_RUN_RETENTION_DAYS: i64 = 30;

/// Everything one trader cycle reads. Rebuilt every cycle so settings and prices are current.
pub struct TradeContext {
    pub conn: DatabaseConnection,
    pub cache: CacheState,
    pub settings: Settings,
    pub orders: Arc<TradeOrders>,
    pub prices: Arc<dyn PriceSource>,
    pub username: String,
    pub banned: bool,
}

impl TradeContext {
    pub async fn load(conn: &DatabaseConnection, orders: Arc<TradeOrders>) -> Result<Self, Error> {
        let app = states::app_state()?;
        let cache = states::cache_client()?;
        let prices = StatsPriceSource::load(conn, &cache).await?;
        Ok(Self {
            conn: conn.clone(),
            cache,
            settings: app.settings.clone(),
            orders,
            prices: Arc::new(prices),
            username: app.user.wfm_username.clone(),
            banned: app.user.is_banned(),
        })
    }
}
```

- [ ] **Step 2: Write `item_entry.rs`**

```rust
use std::{
    fmt::Display,
    hash::{Hash, Hasher},
};

use entity::stock_item::Model as StockItemModel;
use entity::wish_list::Model as WishListModel;
use serde::{Deserialize, Serialize};
use serde_json::json;
use service::{sea_orm::DatabaseConnection, StockItemMutation, StockItemQuery, WishListMutation, WishListQuery};
use utils::{get_location, info, Error, LoggerOptions, OperationSet, Properties, SubType};
use wf_market::{
    enums::OrderType,
    types::{OrderList, OrderWithUser},
};

use super::price_source::ItemPriceInfo;
use crate::{send_event, types::UIEvent};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ItemMarketInfo {
    pub lowest_price: i64,
    pub highest_price: i64,
    pub price_range: i64,
    pub volume: usize,
}

impl ItemMarketInfo {
    pub fn new(live_orders: &OrderList<OrderWithUser>, order_type: OrderType) -> Self {
        Self {
            lowest_price: live_orders.lowest_price(order_type),
            highest_price: live_orders.highest_price(order_type),
            price_range: live_orders.price_range(order_type),
            volume: if order_type == OrderType::Buy {
                live_orders.buy_orders.len()
            } else {
                live_orders.sell_orders.len()
            },
        }
    }
}

impl Display for ItemMarketInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Lowest: {} | Highest: {} | Range: {} | Volume: {}",
            self.lowest_price, self.highest_price, self.price_range, self.volume
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stock_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wish_list_id: Option<i64>,
    #[serde(rename = "wfm_url")]
    pub wfm_url: String,
    #[serde(rename = "wfm_id")]
    pub wfm_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub buy_quantity: i64,
    pub sell_quantity: i64,
    #[serde(default, flatten)]
    pub operations: OperationSet,
    #[serde(default)]
    pub order_type: String,
    #[serde(default)]
    pub buy_market_info: ItemMarketInfo,
    #[serde(default)]
    pub sell_market_info: ItemMarketInfo,
    #[serde(default, flatten)]
    pub properties: Properties,
}

impl Hash for ItemEntry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.wfm_url.hash(state);
        self.sub_type.hash(state);
    }
}

impl ItemEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stock_id: Option<i64>,
        wish_list_id: Option<i64>,
        wfm_url: impl Into<String>,
        wfm_id: impl Into<String>,
        sub_type: Option<SubType>,
        priority: i64,
        buy_quantity: i64,
        sell_quantity: i64,
        operations: Vec<String>,
        order_type: &str,
        properties: Properties,
    ) -> Self {
        Self {
            stock_id,
            wish_list_id,
            wfm_url: wfm_url.into(),
            wfm_id: wfm_id.into(),
            sub_type,
            priority,
            buy_quantity,
            sell_quantity,
            operations: OperationSet::from(operations),
            order_type: order_type.to_owned(),
            buy_market_info: ItemMarketInfo::default(),
            sell_market_info: ItemMarketInfo::default(),
            properties,
        }
    }

    pub fn apply_market_info(&mut self, live_orders: &OrderList<OrderWithUser>) {
        self.buy_market_info = ItemMarketInfo::new(live_orders, OrderType::Buy);
        self.sell_market_info = ItemMarketInfo::new(live_orders, OrderType::Sell);
    }

    pub fn uuid(&self) -> String {
        match &self.sub_type {
            Some(sub_type) => format!("{}-{}", self.wfm_url, sub_type.shot_display()),
            None => self.wfm_url.clone(),
        }
    }

    pub fn get_quantity(&self, order_type: OrderType) -> i64 {
        match order_type {
            OrderType::Buy => self.buy_quantity,
            OrderType::Sell => self.sell_quantity,
        }
    }

    pub fn set_quantity(&mut self, order_type: OrderType, quantity: i64) -> Self {
        match order_type {
            OrderType::Buy => self.buy_quantity = quantity,
            OrderType::Sell => self.sell_quantity = quantity,
        }
        self.clone()
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    pub async fn get_stock_item(&self, conn: &DatabaseConnection) -> Result<StockItemModel, Error> {
        let stock_id = self
            .stock_id
            .ok_or_else(|| Error::new("ItemEntry:GetStockItem", "Stock ID is None", get_location!()))?;
        let item = StockItemQuery::find_by_id(conn, stock_id)
            .await
            .map_err(|e| e.with_location(get_location!()))?;
        item.ok_or_else(|| {
            Error::new("ItemEntry:GetStockItem", format!("Stock item not found for ID: {}", stock_id), get_location!())
                .set_log_level(utils::LogLevel::Warning)
        })
    }

    pub async fn get_wish_list_item(&self, conn: &DatabaseConnection) -> Result<WishListModel, Error> {
        let wish_list_id = self
            .wish_list_id
            .ok_or_else(|| Error::new("ItemEntry:GetWishListItem", "Wish List ID is None", get_location!()))?;
        let item = WishListQuery::get_by_id(conn, wish_list_id)
            .await
            .map_err(|e| e.with_location(get_location!()))?;
        item.ok_or_else(|| {
            Error::new(
                "ItemEntry:GetWishListItem",
                format!("Wish List item not found for ID: {}", wish_list_id),
                get_location!(),
            )
            .set_log_level(utils::LogLevel::Warning)
        })
    }

    pub async fn get_stock_item_or_error(&self, conn: &DatabaseConnection) -> Result<StockItemModel, Error> {
        self.get_stock_item(conn)
            .await
            .map_err(|e| e.with_location(get_location!()).with_context(self.to_json()))
    }

    pub async fn get_wishlist_item_or_error(&self, conn: &DatabaseConnection) -> Result<WishListModel, Error> {
        self.get_wish_list_item(conn)
            .await
            .map_err(|e| e.with_location(get_location!()).with_context(self.to_json()))
    }

    pub async fn finalize_stock_item(
        &self,
        conn: &DatabaseConnection,
        component: &str,
        stock_item: &mut StockItemModel,
        log_options: &LoggerOptions,
    ) -> Result<(), Error> {
        if stock_item.is_dirty {
            StockItemMutation::update_by_id(conn, stock_item.to_update())
                .await
                .map_err(|e| e.with_location(get_location!()))?;
            info(
                format!("{}StockItemUpdate", component),
                &format!("Updated stock item: {:?}", self.stock_id),
                log_options,
            );
            send_event!(UIEvent::RefreshStockItems, json!({"id": self.stock_id, "source": component}));
        }
        Ok(())
    }

    pub async fn finalize_wishlist_item(
        &self,
        conn: &DatabaseConnection,
        component: &str,
        wishlist_item: &mut WishListModel,
        log_options: &LoggerOptions,
    ) -> Result<(), Error> {
        if wishlist_item.is_dirty {
            WishListMutation::update_by_id(conn, wishlist_item.to_update())
                .await
                .map_err(|e| e.with_location(get_location!()))?;
            info(
                format!("{}WishListUpdate", component),
                &format!("Updated wishlist item: {:?}", self.wish_list_id),
                log_options,
            );
            send_event!(UIEvent::RefreshWishListItems, json!({"id": self.wish_list_id, "source": component}));
        }
        Ok(())
    }
}

impl From<&ItemPriceInfo> for ItemEntry {
    fn from(item: &ItemPriceInfo) -> Self {
        Self::new(
            None,
            None,
            item.wfm_url.clone(),
            item.wfm_id.clone(),
            item.sub_type.clone(),
            0,
            1,
            0,
            vec!["Buy".into()],
            "closed",
            Properties::default(),
        )
    }
}

impl From<&StockItemModel> for ItemEntry {
    fn from(item: &StockItemModel) -> Self {
        Self::new(
            Some(item.id),
            None,
            item.wfm_url.clone(),
            item.wfm_id.clone(),
            item.sub_type.clone(),
            1,
            0,
            item.owned,
            vec!["Sell".into()],
            "closed",
            Properties::default(),
        )
    }
}

impl From<&WishListModel> for ItemEntry {
    fn from(item: &WishListModel) -> Self {
        Self::new(
            None,
            Some(item.id),
            item.wfm_url.clone(),
            item.wfm_id.clone(),
            item.sub_type.clone(),
            2,
            item.quantity,
            0,
            vec!["WishList".into()],
            "buy",
            Properties::default(),
        )
    }
}
```

Upstream sent `RefreshStockItems` only when `stock_item.update_gui()`; the target entity has no `update_gui`, so the event is sent after every write. `WishListQuery::get_by_id` and the `update_by_id` mutations exist in the target `service` crate. If a name differs, find it with `grep -n "pub async fn" crates/service/src/query/wish_list_query.rs crates/service/src/mutation/*_mutation.rs`.

- [ ] **Step 3: Write `helpers.rs` with its tests**

```rust
use std::{collections::HashMap, path::Path};

use entity::dto::{add_price_history, PriceHistory};
use entity::stock_item::StockItemPaginationQueryDto;
use entity::wish_list::WishListPaginationQueryDto;
use serde_json::json;
use service::{StockItemQuery, WishListQuery};
use utils::{debug, get_location, info, warning, Error, LoggerOptions, OperationSet};
use wf_market::{
    enums::OrderType,
    types::{CreateOrderParams, Order, OrderList, OrderWithUser, UpdateOrderParams},
};

use super::item_entry::ItemEntry;
use super::orders::{Route, TradeOrders, WriteMeta};
use super::price_source::{get_interesting_items, is_disabled, key_of, ItemPriceInfo};
use super::TradeContext;
use crate::{
    app::{ItemSettings, Settings},
    cache::types::CacheTradableItem,
    enums::TradeMode,
    send_event,
    types::UIEvent,
    utils::SubTypeExt,
};

pub fn knapsack(
    items: Vec<(i64, f64, String, String)>,
    max_weight: i64,
) -> (Vec<(i64, f64, String, String)>, Vec<(i64, f64, String, String)>) {
    let n = items.len();
    let w_max = max_weight.max(0) as usize;
    let mut dp = vec![0.0; w_max + 1];
    let mut choice = vec![vec![false; w_max + 1]; n];
    for (i, item) in items.iter().enumerate() {
        let weight = item.0.max(0) as usize;
        let value = item.1;
        if weight > w_max {
            continue;
        }
        for w in (weight..=w_max).rev() {
            let new_val = dp[w - weight] + value;
            if new_val > dp[w] {
                dp[w] = new_val;
                choice[i][w] = true;
            }
        }
    }
    let mut selected_items = Vec::new();
    let mut unselected_items = Vec::new();
    let mut w = w_max;
    for i in (0..n).rev() {
        let weight = items[i].0.max(0) as usize;
        if w >= weight && choice[i][w] {
            selected_items.push(items[i].clone());
            w -= weight;
        } else {
            unselected_items.push(items[i].clone());
        }
    }
    selected_items.reverse();
    unselected_items.reverse();
    (selected_items, unselected_items)
}

pub async fn collect_interesting_items(ctx: &TradeContext, component: &str) -> Result<Vec<ItemEntry>, Error> {
    let settings = &ctx.settings;
    let conn = &ctx.conn;
    let stock_item_settings = &settings.live_scraper.items;
    let mut interesting_items: HashMap<String, ItemEntry> = HashMap::new();

    if !settings.debugging.live_scraper.entries.is_empty() {
        debug(
            format!("{}Debug", component),
            "Debugging enabled for the trader, using predefined entries",
            &LoggerOptions::default(),
        );
        return serde_json::from_value(serde_json::Value::Array(settings.debugging.live_scraper.entries.clone()))
            .map_err(|e| Error::new(format!("{}Debug", component), format!("Invalid debugging entries: {}", e), get_location!()));
    }

    if settings.live_scraper.has_trade_mode(TradeMode::Buy) {
        for item in get_interesting_items(stock_item_settings, ctx.prices.as_ref()) {
            let item_entry = ItemEntry::from(&item).set_quantity(OrderType::Buy, stock_item_settings.wtb.buy_quantity);
            if !stock_item_settings.general.is_item_blacklisted(&item.wfm_id, &item.sub_type, &TradeMode::Buy) {
                interesting_items.insert(item_entry.uuid(), item_entry);
            }
        }
    }

    if settings.live_scraper.has_trade_mode(TradeMode::Sell) {
        let stock_items = StockItemQuery::get_all(conn, StockItemPaginationQueryDto::new(1, -1))
            .await
            .map_err(|e| e.with_location(get_location!()))?;
        for item in stock_items.results {
            if !stock_item_settings.general.is_item_blacklisted(&item.wfm_id, &item.sub_type, &TradeMode::Sell) {
                interesting_items
                    .entry(item.uuid())
                    .and_modify(|entry| {
                        entry.priority = 1;
                        entry.sell_quantity = item.owned;
                        entry.stock_id = Some(item.id);
                        entry.operations.add("Sell".to_string());
                    })
                    .or_insert_with(|| ItemEntry::from(&item).set_quantity(OrderType::Sell, item.owned));
            }
        }
    }

    if settings.live_scraper.has_trade_mode(TradeMode::WishList) {
        let wish_items = WishListQuery::get_all(conn, WishListPaginationQueryDto::new(1, -1))
            .await
            .map_err(|e| e.with_location(get_location!()))?;
        for item in wish_items.results {
            if !stock_item_settings.general.is_item_blacklisted(&item.wfm_id, &item.sub_type, &TradeMode::WishList) {
                interesting_items
                    .entry(item.uuid())
                    .and_modify(|entry| {
                        entry.priority = 2;
                        entry.buy_quantity = item.quantity;
                        entry.wish_list_id = Some(item.id);
                        entry.operations.add("WishList".to_string());
                    })
                    .or_insert_with(|| ItemEntry::from(&item));
            }
        }
    }
    Ok(interesting_items.into_values().collect())
}

pub fn get_order_info(
    entry: &ItemEntry,
    order_type: OrderType,
    orders: &TradeOrders,
    route: Route,
) -> (String, i64, wf_market::types::Properties, OperationSet) {
    match orders.find_order(&entry.wfm_id, &SubTypeExt::from_entity(entry.sub_type.clone()), order_type, route) {
        None => (String::new(), 0, wf_market::types::Properties::default(), OperationSet::from(vec!["Create"])),
        Some(order) => {
            let mut properties = order.properties;
            properties.set_property_value("id", order.id.clone());
            properties.set_property_value("original_update_string", format!("p:{}", order.platinum));
            (order.id.clone(), i64::from(order.platinum), properties, OperationSet::from(vec!["Update"]))
        }
    }
}

pub fn populate_order_properties(
    properties: &mut wf_market::types::Properties,
    item: &CacheTradableItem,
    entry: &ItemEntry,
    trade_operations: &OperationSet,
) {
    properties.set_property_value("wfm_id", item.wfm_id.clone());
    properties.set_property_value("wfm_url", item.wfm_url.clone());
    properties.set_property_value("name", item.name.clone());
    properties.set_property_value("sub_type", entry.sub_type.clone());
    properties.set_property_value("image", item.icon.clone());
    properties.set_property_value("t_type", item.sub_type.clone());
    let mut operations = entry.operations.clone();
    operations.merge(trade_operations);
    properties.set_property_value("operations", operations);
}

pub fn set_order_market_metrics(
    properties: &mut wf_market::types::Properties,
    post_price: i64,
    profit: i64,
    item_price_info: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    order_type: OrderType,
) {
    let sell_highest = live_orders.highest_price(OrderType::Sell);
    let sell_lowest = live_orders.lowest_price(OrderType::Sell);
    let buy_highest = live_orders.highest_price(OrderType::Buy);
    let buy_lowest = live_orders.lowest_price(OrderType::Buy);
    properties.set_property_value("update_string", format!("p:{}", post_price));
    properties.set_property_value("closed_avg", item_price_info.avg_price);
    properties.set_property_value("potential_profit", profit);
    properties.set_property_value("sell_highest_price", sell_highest);
    properties.set_property_value("sell_lowest_price", sell_lowest);
    properties.set_property_value("buy_highest_price", buy_highest);
    properties.set_property_value("buy_lowest_price", buy_lowest);
    properties.set_property_value("supply", live_orders.sell_orders.len());
    properties.set_property_value("demand", live_orders.buy_orders.len());
    let spread = sell_lowest - buy_highest;
    properties.set_property_value("spread", spread);
    let spread_pct = if sell_lowest > 0 { spread as f64 / sell_lowest as f64 * 100.0 } else { 0.0 };
    properties.set_property_value("spread_percent", spread_pct);
    properties.set_property_value("orders", live_orders.take_top(5, order_type));
    push_price_history(properties, post_price);
}

pub fn push_price_history(properties: &mut wf_market::types::Properties, price: i64) {
    let mut history = properties.get_property_value::<Vec<PriceHistory>>("price_history", vec![]);
    add_price_history(&mut history, PriceHistory::new(chrono::Local::now().naive_local().to_string(), price));
    properties.set_property_value("price_history", history);
}

pub fn orders_to_delete(settings: &Settings, just_started: bool, my_orders: &OrderList<Order>) -> Vec<String> {
    if settings.live_scraper.general.auto_delete && just_started {
        return my_orders
            .to_vec()
            .into_iter()
            .filter(|order| {
                let mode = match order.order_type {
                    OrderType::Buy => TradeMode::Buy,
                    OrderType::Sell => TradeMode::Sell,
                };
                !settings
                    .live_scraper
                    .items
                    .general
                    .is_item_blacklisted(&order.item_id, &SubTypeExt::to_entity(&order.subtype), &mode)
            })
            .map(|order| order.id)
            .collect();
    }
    match (
        settings.live_scraper.has_trade_mode(TradeMode::Buy),
        settings.live_scraper.has_trade_mode(TradeMode::Sell),
        settings.live_scraper.has_trade_mode(TradeMode::WishList),
    ) {
        (true, false, true) => my_orders.order_ids(OrderType::Sell),
        (false, true, false) => my_orders.order_ids(OrderType::Buy),
        _ => vec![],
    }
}

pub async fn load_orders(
    component: &str,
    orders: &TradeOrders,
    item_url: &str,
    fake_path: Option<&Path>,
) -> Result<OrderList<OrderWithUser>, Error> {
    if let Some(path) = fake_path {
        if path.exists() {
            if let Ok(cached) = utils::read_json_file(&path.to_path_buf()) {
                return Ok(cached);
            }
        }
    }
    let live = orders.get_orders_by_item(item_url).await.map_err(|e| e.set_component(component))?;
    if let Some(path) = fake_path {
        utils::write_json_file(path, &live)?;
    }
    Ok(live)
}

#[allow(clippy::too_many_arguments)]
pub async fn progress_order(
    component: &str,
    entry: &ItemEntry,
    orders: &TradeOrders,
    route: Route,
    order_type: OrderType,
    post_price: u32,
    per_trade: Option<i64>,
    log_options: &LoggerOptions,
    properties: &mut wf_market::types::Properties,
    trade_operations: &OperationSet,
) -> Result<OperationSet, Error> {
    let can_create_order = orders.can_create_order(route);
    let quantity = entry.get_quantity(order_type);
    let order_id = properties.get_property_value("id", String::new());
    let name = properties.get_property_value("name", String::new());
    let update_string = properties.get_property_value("update_string", String::new());
    let original_update_string = properties.get_property_value("original_update_string", String::new());
    let meta = WriteMeta { sub_type: key_of(&entry.sub_type), reason: trade_operations.operations.join(",") };

    if trade_operations.has("Create") && !trade_operations.has("Delete") && can_create_order {
        let params = CreateOrderParams::new_with_subtype(
            &entry.wfm_id,
            order_type,
            post_price,
            quantity as u32,
            true,
            per_trade.map(|pt| pt as u32),
            SubTypeExt::from_entity(entry.sub_type.clone()),
        )
        .with_properties(json!(properties.properties));
        let order = orders.create(params, route, &meta).await.map_err(|e| e.with_location(get_location!()))?;
        info(format!("{}CreateSuccess", component), &format!("Created order for item {}: {}", name, order.id), log_options);
        send_event!(UIEvent::RefreshWfmOrders, json!({"source": component}));
    } else if trade_operations.has("Update") && !trade_operations.has("Delete") {
        let params = UpdateOrderParams::new()
            .with_platinum(post_price)
            .with_quantity(quantity as u32)
            .with_per_trade(per_trade.map(|pt| pt as u32))
            .with_properties(json!(properties.properties));
        let order = orders.update(&order_id, params, &meta).await.map_err(|e| e.with_location(get_location!()))?;
        info(format!("{}UpdateSuccess", component), &format!("Updated order for item {}: {}", name, order.id), log_options);
        if original_update_string != update_string {
            send_event!(UIEvent::RefreshWfmOrders, json!({"source": component}));
        }
    } else if trade_operations.has("Update") && trade_operations.has("Delete") {
        orders.delete(&order_id, &meta).await.map_err(|e| e.with_location(get_location!()))?;
        info(format!("{}DeleteSuccess", component), &format!("Deleted order for item {}: {}", name, order_id), log_options);
        send_event!(UIEvent::RefreshWfmOrders, json!({"source": component}));
    } else if !can_create_order {
        warning(format!("{}Skip", component), &format!("Item {} has reached the order limit. Skipping.", name), log_options);
    } else {
        warning(format!("{}Skip", component), &format!("Item {} is not optimal for buying. Skipping.", name), log_options);
    }
    Ok(OperationSet::default())
}

/// Deletes this item's existing order, if there is one (amendment C5: upstream never deleted).
pub async fn delete_order(
    component: &str,
    entry: &ItemEntry,
    order_type: OrderType,
    orders: &TradeOrders,
    route: Route,
) -> Result<OperationSet, Error> {
    let (order_id, _, mut properties, _) = get_order_info(entry, order_type, orders, route);
    if order_id.is_empty() {
        return Ok(OperationSet::default());
    }
    let operations = OperationSet::from(vec!["Update", "Delete", "MaxStock"]);
    progress_order(component, entry, orders, route, order_type, 1, None, &LoggerOptions::default(), &mut properties, &operations).await
}

pub fn log_summary(component: &str, message: impl AsRef<str>, options: &LoggerOptions) {
    info(format!("{}Summary", component), message.as_ref(), options);
}

pub fn get_per_trade(item_info: &CacheTradableItem) -> Option<i64> {
    if item_info.bulk_tradable { Some(1) } else { None }
}

pub fn is_blacklisted(settings: &ItemSettings, item_info: &CacheTradableItem, entry: &ItemEntry, mode: &TradeMode) -> bool {
    settings.general.is_item_blacklisted(&item_info.wfm_id, &entry.sub_type, mode)
}

pub fn should_apply_max_price_drop(
    max_price_drop: i64,
    min_listings_below: i64,
    current_order_price: i64,
    post_price: i64,
    prices: Vec<i64>,
    order_type: OrderType,
) -> Option<String> {
    if is_disabled(max_price_drop) && is_disabled(min_listings_below) {
        return None;
    }
    let (is_price_invalid, price_change, listing_count) = match order_type {
        OrderType::Buy => (
            current_order_price > post_price,
            post_price - current_order_price,
            prices.iter().filter(|&&p| p > current_order_price).count() as i64,
        ),
        OrderType::Sell => (
            current_order_price < post_price,
            current_order_price - post_price,
            prices.iter().filter(|&&p| p < current_order_price).count() as i64,
        ),
    };
    if is_price_invalid {
        return None;
    }
    let should_skip = !is_disabled(max_price_drop)
        && price_change > max_price_drop
        && (is_disabled(min_listings_below) || listing_count <= min_listings_below);
    should_skip.then(|| "MaxPriceDrop".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wf_market::types::SubType as WFSubType;

    fn order(id: &str, order_type: OrderType, item: &str) -> Order {
        Order {
            id: id.into(),
            order_type,
            platinum: 10,
            quantity: 1,
            per_trade: None,
            subtype: WFSubType::default(),
            visible: true,
            item_id: item.into(),
            created_at: String::new(),
            updated_at: String::new(),
            properties: Default::default(),
        }
    }

    #[test]
    fn knapsack_keeps_the_most_profitable_orders_within_the_cap() {
        let items = vec![
            (60, 10.0, "a".to_string(), "oa".to_string()),
            (50, 30.0, "b".to_string(), "ob".to_string()),
            (50, 25.0, "c".to_string(), "oc".to_string()),
        ];
        let (selected, unselected) = knapsack(items, 100);
        assert_eq!(selected.iter().map(|i| i.2.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
        assert_eq!(unselected.iter().map(|i| i.2.as_str()).collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn max_price_drop_holds_the_price_when_it_would_fall_too_far() {
        assert_eq!(should_apply_max_price_drop(-1, -1, 30, 20, vec![20, 21], OrderType::Sell), None);
        assert_eq!(should_apply_max_price_drop(5, -1, 30, 20, vec![20, 21], OrderType::Sell), Some("MaxPriceDrop".into()));
        assert_eq!(should_apply_max_price_drop(15, -1, 30, 20, vec![20, 21], OrderType::Sell), None);
        assert_eq!(should_apply_max_price_drop(5, 1, 30, 20, vec![20, 21], OrderType::Sell), None, "two listings below");
        assert_eq!(should_apply_max_price_drop(5, -1, 10, 20, vec![20], OrderType::Buy), Some("MaxPriceDrop".into()));
    }

    #[test]
    fn orders_to_delete_follows_auto_delete_and_trade_modes() {
        let book = OrderList::new(vec![order("b1", OrderType::Buy, "i1"), order("s1", OrderType::Sell, "i2")]);
        let mut settings = Settings::default();
        settings.live_scraper.general.auto_delete = true;
        let mut all = orders_to_delete(&settings, true, &book);
        all.sort();
        assert_eq!(all, vec!["b1".to_string(), "s1".to_string()]);

        settings.live_scraper.general.trade_modes = vec![TradeMode::Buy, TradeMode::WishList];
        assert_eq!(orders_to_delete(&settings, false, &book), vec!["s1".to_string()]);
        settings.live_scraper.general.trade_modes = vec![TradeMode::Sell];
        assert_eq!(orders_to_delete(&settings, false, &book), vec!["b1".to_string()]);
        settings.live_scraper.general.trade_modes = vec![TradeMode::Buy, TradeMode::Sell, TradeMode::WishList];
        assert!(orders_to_delete(&settings, false, &book).is_empty());
    }
}
```

`knapsack` guards against negative weights and against items heavier than the cap; upstream would index out of range there. Everything else is the upstream logic with `TradeOrders` in place of `wfm_client`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p qf_core --lib trader::helpers`
Expected: `test result: ok. 3 passed`.

If a `service`/`entity` import path differs, find the right one with `grep -rn "pub struct StockItemPaginationQueryDto\|pub struct WishListPaginationQueryDto\|pub struct StockItemQuery\|pub struct WishListQuery" crates/entity crates/service`.

- [ ] **Step 5: Commit**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): port item entries and trading helpers onto TradeOrders

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 7: Port the item trader with golden tests

**Files:**
- Create: `crates/qf_core/src/trader/item.rs` (port of `upstream:src-tauri/src/live_scraper/modules/item.rs`, without `progress_syndicate`)
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod item;`)

**Interfaces:**
- Consumes: everything from Task 6; `route_for` (Task 5); `StockItemMutation` / `WishListMutation` (tests only).
- Produces:
  - `item::ItemTrader::new(running: Arc<AtomicBool>, just_started: Arc<AtomicBool>)` with `async check(&self, ctx: &TradeContext) -> Result<(), Error>`
  - `item::progress_buying(ctx, item_info, entry, price, live_orders, route)`
  - `item::progress_selling(...)`, same parameters
  - `item::progress_wish_list(...)`, same parameters

  Each returns `Result<(), Error>`.

- [ ] **Step 1: Write `item.rs`**

```rust
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use entity::{dto::PriceHistory, enums::stock_status::StockStatus};
use serde_json::json;
use utils::{error, get_location, info, warning, Error, LoggerOptions};
use wf_market::{
    enums::{OrderType, StatusType},
    types::{Order, OrderList, OrderWithUser},
};

use super::helpers::*;
use super::item_entry::ItemEntry;
use super::orders::{route_for, Route, WriteMeta};
use super::price_source::{is_disabled, ItemPriceInfo};
use super::TradeContext;
use crate::{cache::types::CacheTradableItem, enums::TradeMode, send_event, types::UIEvent, utils::{OrderListExt, SubTypeExt}};

static COMPONENT: &str = "Trader:Item:";
static LOG_FILE: &str = "trader_item.log";

fn comp(suffix: &str) -> String {
    format!("{}{}", COMPONENT, suffix)
}

pub struct ItemTrader {
    running: Arc<AtomicBool>,
    just_started: Arc<AtomicBool>,
}

impl ItemTrader {
    pub fn new(running: Arc<AtomicBool>, just_started: Arc<AtomicBool>) -> Self {
        Self { running, just_started }
    }

    fn send_event(&self, key: &str, values: Option<serde_json::Value>) {
        send_event!(
            UIEvent::SendLiveScraperMessage,
            json!({"i18nKey": format!("item.{}", key), "values": values})
        );
    }

    fn should_stop(&self, ctx: &TradeContext) -> bool {
        !self.running.load(Ordering::SeqCst) || ctx.banned
    }

    async fn delete_unwanted_orders(&self, ctx: &TradeContext, my_orders: &OrderList<Order>) -> Result<(), Error> {
        let general = &ctx.settings.live_scraper.general;
        if !general.delete_conflicting_orders && !general.auto_delete {
            return Ok(());
        }
        let order_ids = orders_to_delete(&ctx.settings, self.just_started.load(Ordering::SeqCst), my_orders);
        let total = order_ids.len();
        let mut current_index = total;
        let meta = WriteMeta { sub_type: String::new(), reason: "AutoDelete".into() };
        for id in order_ids.iter() {
            if self.should_stop(ctx) {
                warning(comp("Delete"), "Trader is not running or user is banned, stopping deletion.", &LoggerOptions::default());
                break;
            }
            match ctx.orders.delete(id, &meta).await {
                Ok(_) => {
                    info(comp("Delete"), &format!("Deleted order with ID: {} {}/{}", id, current_index, total), &LoggerOptions::default());
                    self.send_event("deleted", Some(json!({"current": current_index, "total": total, "id": id})));
                }
                Err(e) => error(
                    comp("Delete"),
                    &format!("Failed to delete order with ID {}: {}", id, e.message),
                    &LoggerOptions::default().set_file(LOG_FILE),
                ),
            }
            current_index -= 1;
        }
        Ok(())
    }

    pub async fn check(&self, ctx: &TradeContext) -> Result<(), Error> {
        info(comp("Check"), "Checking items...", &LoggerOptions::default());
        let my_orders = ctx.orders.cache_orders();
        self.delete_unwanted_orders(ctx, &my_orders).await?;
        let interesting_items = collect_interesting_items(ctx, COMPONENT).await?;
        self.process_items(interesting_items, ctx).await
    }

    async fn process_items(&self, mut interesting_items: Vec<ItemEntry>, ctx: &TradeContext) -> Result<(), Error> {
        let use_fake = ctx.settings.debugging.live_scraper.fake_orders;
        let mut current_index = 1;
        let existing_buy_order_ids: HashSet<String> =
            ctx.orders.cache_orders().buy_orders.iter().map(|o| o.id.clone()).collect();

        interesting_items.sort_by(|a, b| b.priority.cmp(&a.priority));
        let total = interesting_items.len();

        for item_entry in interesting_items.iter_mut() {
            if self.should_stop(ctx) {
                warning(comp("ProcessItem"), "Trader is not running or user is banned, stopping processing.", &LoggerOptions::default());
                break;
            }
            let item_info = match ctx.cache.tradable_item().get_by(&item_entry.wfm_url) {
                Ok(item) => item,
                Err(e) => {
                    let _ = e.set_component(comp("ProcessItem")).log(LOG_FILE);
                    continue;
                }
            };
            let item_price = ctx.prices.find_by(&item_info.wfm_id, &item_entry.sub_type).unwrap_or_default();
            let route = route_for(ctx.orders.global_dry_run(), item_price.warm);

            self.send_event(
                "checking",
                Some(json!({
                    "current": current_index,
                    "total": total,
                    "name": item_info.name,
                    "sub_type": item_entry.sub_type,
                    "price": item_price
                })),
            );

            let order_path = PathBuf::from(utils::get_base_path())
                .join("fake_orders")
                .join(format!("order_{}.json", item_info.wfm_url));
            let mut orders = load_orders(
                &comp("ProcessItem:LoadOrders:"),
                &ctx.orders,
                &item_entry.wfm_url,
                use_fake.then_some(order_path.as_path()),
            )
            .await?;

            orders.filter_by_sub_type(wf_market::types::SubType::from_entity(item_entry.sub_type.clone()), false);
            orders.filter_username(&ctx.username, true);
            orders.filter_user_status(StatusType::InGame, false);
            orders.sort_by_platinum();
            item_entry.apply_market_info(&orders);

            info(
                &comp("ProcessItem"),
                &format!(
                    "Processing Item: {} | Buy Orders: {} | Sell Orders: {} | Operations: {:?} | Route: {:?} | Progress: {}/{}",
                    item_info.name,
                    orders.buy_orders.len(),
                    orders.sell_orders.len(),
                    item_entry.operations.operations,
                    route,
                    current_index,
                    total
                ),
                &LoggerOptions::default(),
            );

            if item_entry.operations.has("Buy") && !item_entry.operations.has("WishList") {
                progress_buying(ctx, &item_info, item_entry, &item_price, &orders, route)
                    .await
                    .map_err(|e| e.with_location(get_location!()))?;
            }
            if item_entry.operations.has("WishList") {
                progress_wish_list(ctx, &item_info, item_entry, &item_price, &orders, route)
                    .await
                    .map_err(|e| e.with_location(get_location!()))?;
            }
            if item_entry.operations.has("Sell") && item_entry.stock_id.is_some() {
                progress_selling(ctx, &item_info, item_entry, &item_price, &orders, route)
                    .await
                    .map_err(|e| e.with_location(get_location!()))?;
            }
            current_index += 1;
        }

        let all_buy_orders = ctx.orders.cache_orders().extract_order_summary(OrderType::Buy);
        let max_total_price_cap = ctx.settings.live_scraper.items.wtb.max_total_price_cap;
        if all_buy_orders.len() > 1 && !is_disabled(max_total_price_cap) {
            info(
                &comp("GlobalKnapsack"),
                &format!("Running global knapsack check: {} buy orders | Cap: {}", all_buy_orders.len(), max_total_price_cap),
                &LoggerOptions::default(),
            );
            let (_, unselected) = knapsack(all_buy_orders, max_total_price_cap);
            let meta = WriteMeta { sub_type: String::new(), reason: "Knapsack".into() };
            for order in &unselected {
                if order.3.is_empty() || !existing_buy_order_ids.contains(&order.3) {
                    continue;
                }
                if let Err(err) = ctx.orders.delete(&order.3, &meta).await {
                    error(
                        &comp("GlobalKnapsack"),
                        &format!("Failed to delete {}: {}", order.3, err.message),
                        &LoggerOptions::default().set_file(LOG_FILE),
                    );
                }
            }
        }
        Ok(())
    }
}

/// WTB workflow for one item (upstream `progress_buying`).
pub async fn progress_buying(
    ctx: &TradeContext,
    item_info: &CacheTradableItem,
    entry: &mut ItemEntry,
    price: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    route: Route,
) -> Result<(), Error> {
    let conn = &ctx.conn;
    let log_options = &LoggerOptions::default().set_enable(true);
    let component = comp("Buying");
    let settings = &ctx.settings.live_scraper.items;
    let log = |msg: &str| info(&component, msg, log_options);

    if is_blacklisted(settings, item_info, entry, &TradeMode::Buy) {
        log(&format!("Item {} is blacklisted for buying. Skipping.", item_info.name));
        return Ok(());
    }
    let per_trade = get_per_trade(item_info);
    let closed_avg = price.moving_avg.unwrap_or(0.0);
    let max_stock_quantity = settings.wtb.max_stock_quantity;
    let avg_price_cap = settings.wtb.avg_price_cap;
    let max_total_price_cap = settings.wtb.max_total_price_cap;
    let profit_threshold = settings.wtb.profit_threshold;
    let market_info = entry.buy_market_info.clone();
    let mut post_price = market_info.highest_price;
    let (order_id, current_order_price, mut properties, mut trade_operations) =
        get_order_info(entry, OrderType::Buy, &ctx.orders, route);

    if entry.buy_market_info.volume == 0 || entry.sell_market_info.volume == 0 {
        log(&format!("Item {} has no market volume. Skipping WTB order creation.", item_info.name));
        return Ok(());
    }

    if !is_disabled(max_stock_quantity) && entry.stock_id.is_some() {
        let stock_item = entry.get_stock_item_or_error(conn).await?;
        if stock_item.owned >= max_stock_quantity {
            log(&format!(
                "Item {} already has {} units in stock (max: {}). Deleting its WTB order.",
                item_info.name, stock_item.owned, max_stock_quantity
            ));
            delete_order(&component, entry, OrderType::Buy, &ctx.orders, route)
                .await
                .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;
            return Ok(());
        }
    }

    if let Some(reason) = should_apply_max_price_drop(
        settings.wtb.max_price_drop,
        settings.wtb.min_listings_below,
        current_order_price,
        post_price,
        live_orders.get_price_list(OrderType::Buy, None),
        OrderType::Buy,
    ) {
        post_price = current_order_price;
        trade_operations.add(reason);
    }

    let closed_avg_metric = closed_avg as i64 - post_price;
    let potential_profit = closed_avg_metric - 1;

    let item_max_price = settings.general.get_item_max_price(&item_info.wfm_id);
    if item_max_price > 0 && post_price > item_max_price {
        trade_operations.add("AboveMaxBuyPrice");
        post_price = item_max_price;
    }

    if !is_disabled(avg_price_cap) && post_price > avg_price_cap {
        trade_operations.add("AboveAvgPrice");
        trade_operations.add("Delete");
        log(&format!("Item {} is above the average price cap.", item_info.name));
    }

    if !is_disabled(max_total_price_cap) {
        let mut all_orders = ctx.orders.cache_orders().extract_order_summary(OrderType::Buy);
        if !all_orders.iter().any(|i| i.2 == item_info.wfm_id) {
            all_orders.push((post_price, potential_profit as f64, item_info.wfm_id.clone(), String::new()));
        }
        let (selected, _) = knapsack(all_orders, max_total_price_cap);
        if !selected.iter().any(|o| o.2 == item_info.wfm_id) {
            log(&format!("{} was not selected by the knapsack.", item_info.name));
            return Ok(());
        }
    }

    if closed_avg_metric < 0 {
        trade_operations.add("Delete");
        trade_operations.add("Overpriced");
    }
    if market_info.price_range < profit_threshold {
        trade_operations.add("Delete");
        trade_operations.add("Underpriced");
    }
    post_price = post_price.max(1);

    log_summary(
        &component,
        format!(
            "Item {} | Post: {} | CurOrder: {} ({}) | Market: {} | Price: MovingAvg: {} | Avg: {} | Min: {} | Max: {} | Warm: {} \
             | Profit: Metric: {} | Potential: {} | Threshold: {} | Route: {:?} | Ops: {:?}",
            item_info.name,
            post_price,
            current_order_price,
            order_id,
            market_info,
            closed_avg,
            price.avg_price,
            price.min_price,
            price.max_price,
            price.warm,
            closed_avg_metric,
            potential_profit,
            profit_threshold,
            route,
            trade_operations.operations
        ),
        log_options,
    );

    populate_order_properties(&mut properties, item_info, entry, &trade_operations);
    set_order_market_metrics(&mut properties, post_price, potential_profit, price, live_orders, OrderType::Buy);
    progress_order(
        &component,
        entry,
        &ctx.orders,
        route,
        OrderType::Buy,
        post_price as u32,
        per_trade,
        log_options,
        &mut properties,
        &trade_operations,
    )
    .await
    .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;
    Ok(())
}

/// WTS workflow for one stock item (upstream `progress_selling`, with the `wts` fix from amendment C5).
pub async fn progress_selling(
    ctx: &TradeContext,
    item_info: &CacheTradableItem,
    entry: &mut ItemEntry,
    price: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    route: Route,
) -> Result<(), Error> {
    let conn = &ctx.conn;
    let log_options = &LoggerOptions::default();
    let component = comp("Selling");
    let settings = &ctx.settings.live_scraper.items;
    let log = |msg: &str| info(&component, msg, log_options);

    if is_blacklisted(settings, item_info, entry, &TradeMode::Sell) {
        log(&format!("Item {} is blacklisted for selling. Skipping.", item_info.name));
        return Ok(());
    }
    let per_trade = get_per_trade(item_info);
    let closed_avg = price.moving_avg.unwrap_or(0.0) as i64;
    let mut stock_item = entry.get_stock_item_or_error(conn).await?;
    let bought_price = stock_item.bought;
    let market_info = entry.sell_market_info.clone();
    let (_, current_order_price, mut properties, mut trade_operations) =
        get_order_info(entry, OrderType::Sell, &ctx.orders, route);

    let (min_price, min_profit, min_sma) = (
        stock_item.properties.get_property_value("min_price", None::<i64>),
        stock_item.properties.get_property_value("min_profit", None::<i64>),
        stock_item.properties.get_property_value("min_sma", None::<i64>),
    );

    if stock_item.is_hidden && stock_item.status == StockStatus::InActive {
        log(&format!("Item {} is marked as hidden and inactive. Skipping.", item_info.name));
        return Ok(());
    } else if stock_item.is_hidden && stock_item.status != StockStatus::InActive {
        stock_item.set_status(StockStatus::InActive);
        stock_item.set_list_price(None);
        stock_item.locked = true;
        trade_operations.add("Delete");
    }

    let lowest_price = if market_info.volume >= 2 {
        market_info.lowest_price
    } else if min_price.is_none() {
        trade_operations.add("Delete");
        trade_operations.add("NoSellers");
        stock_item.set_status(StockStatus::NoSellers);
        stock_item.set_list_price(None);
        stock_item.locked = true;
        0
    } else {
        0
    };

    let mut post_price = lowest_price;
    if let Some(min_price) = min_price {
        let capped_price = post_price.max(min_price);
        if capped_price != post_price {
            post_price = capped_price;
            trade_operations.add("MinimumPrice");
        }
    }

    if let Some(reason) = should_apply_max_price_drop(
        settings.wts.max_price_drop,
        settings.wts.min_listings_below,
        current_order_price,
        post_price,
        live_orders.get_price_list(OrderType::Sell, None),
        OrderType::Sell,
    ) {
        log(&format!("Item {} max price drop applied ({}).", item_info.name, reason));
        post_price = current_order_price;
        trade_operations.add(reason);
    }

    let minimum_sma = min_sma.unwrap_or(settings.wts.min_sma);
    if !is_disabled(minimum_sma) && post_price < (closed_avg - minimum_sma) && lowest_price > bought_price {
        post_price = closed_avg;
        trade_operations.add("SMALimit");
        stock_item.set_list_price(Some(post_price));
        stock_item.set_status(StockStatus::SMALimit);
        stock_item.locked = true;
    }

    let mut profit = post_price - bought_price;
    let minimum_profit = min_profit.unwrap_or(settings.wts.min_profit);
    if !is_disabled(minimum_profit) && profit < minimum_profit {
        post_price += minimum_profit - profit;
        stock_item.set_status(StockStatus::ToLowProfit);
        stock_item.set_list_price(Some(post_price));
        stock_item.locked = true;
        trade_operations.add("LowProfit");
        profit = post_price - bought_price;
    }

    stock_item.set_list_price(Some(post_price));
    stock_item.set_status(StockStatus::Live);
    stock_item.add_price_history(PriceHistory::new(chrono::Local::now().naive_local().to_string(), post_price));
    post_price = post_price.max(1);

    log_summary(
        &component,
        format!(
            "Item {} | Post: {} | CurOrder: {} | Lowest: {} | ClosedAvg: {} | Bought: {} | Profit: {} | Market: {} \
             | Price: AVG: {} | Min: {} | Max: {} | Median: {} | Warm: {} | MinSMA: {} | MinProfit: {} | MinPrice: {:?} \
             | Hidden: {} | Status: {:?} | Route: {:?} | Ops: {:?}",
            item_info.name,
            post_price,
            current_order_price,
            lowest_price,
            closed_avg,
            bought_price,
            profit,
            market_info,
            price.avg_price,
            price.min_price,
            price.max_price,
            price.median,
            price.warm,
            minimum_sma,
            minimum_profit,
            min_price,
            stock_item.is_hidden,
            stock_item.status,
            route,
            trade_operations.operations,
        ),
        log_options,
    );

    populate_order_properties(&mut properties, item_info, entry, &trade_operations);
    set_order_market_metrics(&mut properties, post_price, profit, price, live_orders, OrderType::Sell);
    progress_order(
        &component,
        entry,
        &ctx.orders,
        route,
        OrderType::Sell,
        post_price as u32,
        per_trade,
        log_options,
        &mut properties,
        &trade_operations,
    )
    .await
    .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;

    entry.finalize_stock_item(conn, &component, &mut stock_item, log_options).await?;
    Ok(())
}

/// Wish-list workflow for one item (upstream `progress_wish_list`).
pub async fn progress_wish_list(
    ctx: &TradeContext,
    item_info: &CacheTradableItem,
    entry: &mut ItemEntry,
    price: &ItemPriceInfo,
    live_orders: &OrderList<OrderWithUser>,
    route: Route,
) -> Result<(), Error> {
    let conn = &ctx.conn;
    let component = comp("WishList:");
    let log_options = LoggerOptions::default();
    let settings = &ctx.settings.live_scraper.items;
    let log = |msg: &str| info(&component, msg, &log_options);

    if is_blacklisted(settings, item_info, entry, &TradeMode::WishList) {
        log(&format!("Item {} is blacklisted for wishlist buying. Skipping.", item_info.name));
        return Ok(());
    }
    let market = entry.buy_market_info.clone();
    let per_trade = get_per_trade(item_info);
    let mut wishlist_item = entry.get_wishlist_item_or_error(conn).await?;
    let (_, _, mut properties, mut trade_operations) = get_order_info(entry, OrderType::Buy, &ctx.orders, route);
    let min_price = wishlist_item.properties.get_property_value("min_price", 0i64);
    let max_price = wishlist_item.properties.get_property_value("max_price", 0i64);

    if wishlist_item.is_hidden {
        if wishlist_item.status == StockStatus::InActive {
            log(&format!("Item {} is marked as hidden and inactive. Skipping.", item_info.name));
            return Ok(());
        }
        wishlist_item.set_status(StockStatus::InActive);
        wishlist_item.set_list_price(None);
        wishlist_item.locked = true;
        trade_operations.add("Delete");
    }

    let mut post_price = if market.volume == 0 {
        trade_operations.add("NoBuyers");
        wishlist_item.set_status(StockStatus::NoBuyers);
        price.avg_price as i64
    } else {
        market.highest_price
    };
    if max_price > 0 && post_price > max_price {
        post_price = max_price;
        trade_operations.add("MaxPrice");
    }
    if min_price > 0 && post_price < min_price {
        post_price = min_price;
        trade_operations.add("MinPrice");
    }
    post_price = post_price.max(1);

    wishlist_item.set_list_price(Some(post_price));
    wishlist_item.set_status(StockStatus::Live);
    wishlist_item.add_price_history(PriceHistory::new(chrono::Local::now().naive_local().to_string(), post_price));

    log_summary(
        &component,
        format!(
            "Item {} | Post: {} | Market: {} | Price: Avg: {} | MovingAvg: {} | Warm: {} | MinPrice: {} | MaxPrice: {} | Route: {:?} | Ops: {:?}",
            item_info.name,
            post_price,
            market,
            price.avg_price,
            price.moving_avg.unwrap_or(0.0),
            price.warm,
            min_price,
            max_price,
            route,
            trade_operations.operations,
        ),
        &log_options,
    );

    populate_order_properties(&mut properties, item_info, entry, &trade_operations);
    set_order_market_metrics(&mut properties, post_price, 0, price, live_orders, OrderType::Buy);
    progress_order(
        &component,
        entry,
        &ctx.orders,
        route,
        OrderType::Buy,
        post_price as u32,
        per_trade,
        &log_options,
        &mut properties,
        &trade_operations,
    )
    .await
    .map_err(|e| e.with_location(get_location!()).with_context(entry.to_json()))?;

    entry.finalize_wishlist_item(conn, &component, &mut wishlist_item, &log_options).await?;
    Ok(())
}
```

The only behaviour change from upstream in the knapsack step: an item the knapsack doesn't select now returns without adding `Skip`/`Delete` operations. Upstream added them but also returned immediately, so they had no effect.

- [ ] **Step 2: Write the golden tests**

Append to `crates/qf_core/src/trader/item.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Settings;
    use crate::cache::client::CacheState;
    use crate::trader::orders::TradeOrders;
    use crate::trader::price_source::StatsPriceSource;
    use entity::{stock_item, wish_list};
    use service::{StockItemMutation, WishListMutation};
    use utils::Properties;
    use wf_market::types::{CreateOrderParams, SubType as WFSubType};

    async fn ctx_with(global_dry_run: bool, edit: impl FnOnce(&mut Settings)) -> (tempfile::TempDir, TradeContext) {
        let (dir, conn) = crate::trader::store::tests::db().await;
        let mut settings = Settings::default();
        {
            let items = &mut settings.live_scraper.items;
            items.wtb.max_total_price_cap = -1;
            items.wtb.profit_threshold = -1;
            items.wtb.max_stock_quantity = -1;
            items.wtb.max_price_drop = -1;
            items.wtb.min_listings_below = -1;
            items.wtb.avg_price_cap = -1;
            items.wts.min_sma = -1;
            items.wts.min_profit = -1;
            items.wts.max_price_drop = -1;
            items.wts.min_listings_below = -1;
        }
        edit(&mut settings);
        let ctx = TradeContext {
            conn: conn.clone(),
            cache: CacheState::new(dir.path().to_path_buf()),
            settings,
            orders: Arc::new(TradeOrders::new(None, Some(conn), global_dry_run)),
            prices: Arc::new(StatsPriceSource::default()),
            username: "me".into(),
            banned: false,
        };
        (dir, ctx)
    }

    fn item_info() -> CacheTradableItem {
        CacheTradableItem {
            name: "Test Item".into(),
            unique_name: String::new(),
            wfm_id: "item1".into(),
            wfm_url: "item1_slug".into(),
            trade_tax: 0,
            mr_requirement: 0,
            tags: vec![],
            icon: String::new(),
            bulk_tradable: false,
            sub_type: None,
            variant_to_unique_name: Default::default(),
        }
    }

    fn live_order(side: &str, n: usize, platinum: i64) -> OrderWithUser {
        serde_json::from_value(json!({
            "id": format!("{side}{n}"), "type": side, "platinum": platinum, "quantity": 1, "visible": true,
            "itemId": "item1", "createdAt": "2026-09-01T00:00:00Z", "updatedAt": "2026-09-01T00:00:00Z",
            "user": {"id": format!("u-{side}{n}"), "ingameName": format!("Player{side}{n}"), "reputation": 1, "status": "ingame"}
        }))
        .unwrap()
    }

    fn book(sells: &[i64], buys: &[i64]) -> OrderList<OrderWithUser> {
        let mut orders: Vec<OrderWithUser> = sells.iter().enumerate().map(|(n, p)| live_order("sell", n, *p)).collect();
        orders.extend(buys.iter().enumerate().map(|(n, p)| live_order("buy", n, *p)));
        let mut list = OrderList::new(orders);
        list.sort_by_platinum();
        list
    }

    fn price(moving_avg: f64, warm: bool) -> ItemPriceInfo {
        ItemPriceInfo {
            wfm_id: "item1".into(),
            wfm_url: "item1_slug".into(),
            avg_price: moving_avg,
            moving_avg: Some(moving_avg),
            warm,
            ..Default::default()
        }
    }

    fn entry(ops: &str, stock_id: Option<i64>, wish_list_id: Option<i64>) -> ItemEntry {
        ItemEntry::new(stock_id, wish_list_id, "item1_slug", "item1", None, 0, 1, 1, vec![ops.into()], "closed", Properties::default())
    }

    async fn seed_order(ctx: &TradeContext, order_type: OrderType, platinum: u32, route: Route) {
        let params = CreateOrderParams::new_with_subtype("item1", order_type, platinum, 1, true, None, WFSubType::default());
        ctx.orders.create(params, route, &WriteMeta::default()).await.unwrap();
    }

    async fn stock(ctx: &TradeContext, bought: i64, owned: i64) -> i64 {
        StockItemMutation::create(
            &ctx.conn,
            stock_item::Model::new("item1".into(), "item1_slug".into(), "Test Item".into(), "".into(), None, bought, owned, false, Default::default()),
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn buying_posts_at_the_highest_buy_price_in_global_dry_run() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route_for(true, true)).await.unwrap();
        let log = ctx.orders.dry_log();
        assert_eq!(log.len(), 1);
        assert_eq!((log[0].action.as_str(), log[0].side.as_str(), log[0].price, log[0].forced_by.as_str()), ("create", "buy", Some(17), "global"));
    }

    #[tokio::test]
    async fn overpriced_buy_order_is_deleted() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let route = route_for(true, true);
        seed_order(&ctx, OrderType::Buy, 17, route).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(10.0, true), &live, route).await.unwrap();
        let log = ctx.orders.dry_log();
        assert_eq!(log.last().unwrap().action, "delete");
        assert!(log.last().unwrap().reason.contains("Overpriced"));
    }

    #[tokio::test]
    async fn not_warm_items_are_simulated_even_with_global_dry_run_off() {
        let (_dir, ctx) = ctx_with(false, |_| {}).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", None, None);
        e.apply_market_info(&live);
        let price = price(30.0, false);
        progress_buying(&ctx, &item_info(), &mut e, &price, &live, route_for(false, price.warm)).await.unwrap();
        assert_eq!(ctx.orders.dry_log()[0].forced_by, "not_warm");
    }

    #[tokio::test]
    async fn max_stock_quantity_deletes_the_existing_buy_order() {
        let (_dir, ctx) = ctx_with(true, |s| s.live_scraper.items.wtb.max_stock_quantity = 3).await;
        let route = route_for(true, true);
        let stock_id = stock(&ctx, 10, 5).await;
        seed_order(&ctx, OrderType::Buy, 17, route).await;
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("Buy", Some(stock_id), None);
        e.apply_market_info(&live);
        progress_buying(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route).await.unwrap();
        assert_eq!(ctx.orders.dry_log().last().unwrap().action, "delete");
        assert!(ctx.orders.cache_orders().buy_orders.is_empty());
    }

    #[tokio::test]
    async fn selling_max_price_drop_reads_wts_settings() {
        let (_dir, ctx) = ctx_with(true, |s| s.live_scraper.items.wts.max_price_drop = 5).await;
        let route = route_for(true, true);
        let stock_id = stock(&ctx, 10, 1).await;
        seed_order(&ctx, OrderType::Sell, 30, route).await;
        let live = book(&[20, 21], &[5]);
        let mut e = entry("Sell", Some(stock_id), None);
        e.apply_market_info(&live);
        progress_selling(&ctx, &item_info(), &mut e, &price(25.0, true), &live, route).await.unwrap();
        let last = ctx.orders.dry_log().last().unwrap().clone();
        assert_eq!((last.action.as_str(), last.price), ("update", Some(30)));
        assert!(last.reason.contains("MaxPriceDrop"));
    }

    #[tokio::test]
    async fn selling_ignores_wtb_max_price_drop() {
        let (_dir, ctx) = ctx_with(true, |s| s.live_scraper.items.wtb.max_price_drop = 5).await;
        let route = route_for(true, true);
        let stock_id = stock(&ctx, 10, 1).await;
        seed_order(&ctx, OrderType::Sell, 30, route).await;
        let live = book(&[20, 21], &[5]);
        let mut e = entry("Sell", Some(stock_id), None);
        e.apply_market_info(&live);
        progress_selling(&ctx, &item_info(), &mut e, &price(25.0, true), &live, route).await.unwrap();
        let last = ctx.orders.dry_log().last().unwrap().clone();
        assert_eq!((last.action.as_str(), last.price), ("update", Some(20)));
    }

    #[tokio::test]
    async fn wish_list_buy_price_is_capped_by_max_price() {
        let (_dir, ctx) = ctx_with(true, |_| {}).await;
        let wish = WishListMutation::create(
            &ctx.conn,
            &wish_list::Model::new(
                "item1".into(),
                "item1_slug".into(),
                "Test Item".into(),
                "".into(),
                None,
                1,
                Properties::from(json!({"max_price": 16})),
            ),
        )
        .await
        .unwrap();
        let live = book(&[20, 25], &[15, 17]);
        let mut e = entry("WishList", None, Some(wish.id));
        e.apply_market_info(&live);
        progress_wish_list(&ctx, &item_info(), &mut e, &price(30.0, true), &live, route_for(true, true)).await.unwrap();
        let last = ctx.orders.dry_log().last().unwrap().clone();
        assert_eq!((last.action.as_str(), last.price), ("create", Some(16)));
    }
}
```

Add `pub mod item;` to `crates/qf_core/src/trader/mod.rs`.

- [ ] **Step 3: Run the golden tests**

Run: `cargo test -p qf_core --lib trader::item`
Expected: `test result: ok. 7 passed`.

Before changing any trading logic because a test fails, check the fixtures:
- **Order book won't parse:** add the missing required field to `live_order`. Find it with `sed -n '/pub struct UserShort/,/^}/p' crates/wf-market/src/types/user_short.rs`.
- **Wrong `CacheTradableItem` fields:** compare with `crates/qf_core/src/cache/types/cache_tradable_item.rs`.
- **Different wish-list property:** if `wish_list::Model::new` stores properties under another name, look at the `properties` column in `crates/entity/src/wish_list/wish_list.rs`.

- [ ] **Step 4: Commit**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): port item buy, sell and wish-list decisions with golden tests

Fixes sell repricing reading wtb max price drop settings, and the no-op
max-stock delete.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 8: Run loop and error classification

**Files:**
- Create: `crates/qf_core/src/trader/engine.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod engine;`)

**Interfaces:**
- Consumes: `TradeOrders::consecutive_failures`, `MAX_CONSECUTIVE_FAILURES` (Task 5).
- Produces:
  - `EngineExit { Stopped, Critical(String), OrderFailures(u32) }` (Clone, PartialEq, Debug)
  - `classify(&Error) -> LogLevel`
  - `CYCLE_PAUSE: Duration` (1 s)
  - `async run_loop<F, Fut>(running: Arc<AtomicBool>, just_started: Arc<AtomicBool>, orders: Arc<TradeOrders>, check: F, pause: Duration) -> EngineExit`, where `F: FnMut() -> Fut` and `Fut: Future<Output = Result<(), Error>>`

- [ ] **Step 1: Write `engine.rs` with its tests**

```rust
//! Trader run loop and error classification (upstream `client.rs`, amendment C6).

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use utils::{Error, LogLevel};

use super::orders::{TradeOrders, MAX_CONSECUTIVE_FAILURES};

pub const CYCLE_PAUSE: Duration = Duration::from_secs(1);
static LOG_FILE: &str = "trader_item.log";

#[derive(Debug, Clone, PartialEq)]
pub enum EngineExit {
    Stopped,
    Critical(String),
    OrderFailures(u32),
}

/// Upstream classification: these wf-market error types stop the trader; everything else is a warning.
pub fn classify(error: &Error) -> LogLevel {
    match error.properties.get_property_value("type", String::new()).as_str() {
        "ParsingError" | "BadRequest" | "Unknown" | "InternalServerError" | "InvalidType" => LogLevel::Critical,
        _ => LogLevel::Warning,
    }
}

/// Runs `check` cycles until `running` is cleared, a critical error occurs, or order calls keep failing.
pub async fn run_loop<F, Fut>(
    running: Arc<AtomicBool>,
    just_started: Arc<AtomicBool>,
    orders: Arc<TradeOrders>,
    mut check: F,
    pause: Duration,
) -> EngineExit
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), Error>>,
{
    just_started.store(true, Ordering::SeqCst);
    while running.load(Ordering::SeqCst) {
        if let Err(mut e) = check().await {
            e.log_level = classify(&e);
            let _ = e.log(LOG_FILE);
            if matches!(e.log_level, LogLevel::Critical) {
                running.store(false, Ordering::SeqCst);
                return EngineExit::Critical(format!("{}: {}", e.component, e.message));
            }
        }
        let failures = orders.consecutive_failures();
        if failures >= MAX_CONSECUTIVE_FAILURES {
            running.store(false, Ordering::SeqCst);
            return EngineExit::OrderFailures(failures);
        }
        tokio::time::sleep(pause).await;
        just_started.store(false, Ordering::SeqCst);
    }
    EngineExit::Stopped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trader::orders::{Route, WriteMeta};
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex;
    use utils::Properties;
    use wf_market::enums::OrderType;
    use wf_market::types::{CreateOrderParams, SubType as WFSubType};

    fn wfm_error(kind: &str) -> Error {
        let mut e = Error::new("Test", "boom", "here");
        e.properties = Properties::from(json!({"type": kind}));
        e
    }

    #[test]
    fn critical_types_match_upstream() {
        for kind in ["ParsingError", "BadRequest", "Unknown", "InternalServerError", "InvalidType"] {
            assert!(matches!(classify(&wfm_error(kind)), LogLevel::Critical), "{kind}");
        }
        for kind in ["TooManyRequests", "NotFound", "OrderLimitExceeded", ""] {
            assert!(matches!(classify(&wfm_error(kind)), LogLevel::Warning), "{kind}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn warnings_continue_and_just_started_is_only_true_first() {
        let running = Arc::new(AtomicBool::new(true));
        let just_started = Arc::new(AtomicBool::new(false));
        let orders = Arc::new(TradeOrders::new(None, None, true));
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let check = {
            let (running, just_started, calls, seen) = (running.clone(), just_started.clone(), calls.clone(), seen.clone());
            move || {
                let (running, just_started, calls, seen) = (running.clone(), just_started.clone(), calls.clone(), seen.clone());
                async move {
                    seen.lock().unwrap().push(just_started.load(Ordering::SeqCst));
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    if n == 2 {
                        running.store(false, Ordering::SeqCst);
                    }
                    if n == 0 { Err(wfm_error("NotFound")) } else { Ok(()) }
                }
            }
        };
        let exit = run_loop(running, just_started, orders, check, CYCLE_PAUSE).await;
        assert_eq!(exit, EngineExit::Stopped);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(*seen.lock().unwrap(), vec![true, false, false]);
    }

    #[tokio::test(start_paused = true)]
    async fn critical_error_stops_the_loop() {
        let running = Arc::new(AtomicBool::new(true));
        let orders = Arc::new(TradeOrders::new(None, None, true));
        let exit = run_loop(running.clone(), Arc::new(AtomicBool::new(false)), orders, || async { Err(wfm_error("BadRequest")) }, CYCLE_PAUSE).await;
        assert_eq!(exit, EngineExit::Critical("Test: boom".into()));
        assert!(!running.load(Ordering::SeqCst));
    }

    #[tokio::test(start_paused = true)]
    async fn five_consecutive_order_failures_stop_the_loop() {
        let orders = Arc::new(TradeOrders::new(None, None, false));
        for _ in 0..MAX_CONSECUTIVE_FAILURES {
            let params = CreateOrderParams::new_with_subtype("item1", OrderType::Buy, 10, 1, true, None, WFSubType::default());
            let _ = orders.create(params, Route::Live, &WriteMeta::default()).await;
        }
        let exit = run_loop(Arc::new(AtomicBool::new(true)), Arc::new(AtomicBool::new(false)), orders, || async { Ok(()) }, CYCLE_PAUSE).await;
        assert_eq!(exit, EngineExit::OrderFailures(MAX_CONSECUTIVE_FAILURES));
    }
}
```

Add `pub mod engine;` to `crates/qf_core/src/trader/mod.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib trader::engine`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 3: Commit**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): add run loop that stops on critical errors and repeated order failures

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 9: Lifecycle rules (pure)

**Files:**
- Create: `crates/qf_core/src/trader/lifecycle.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod lifecycle;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `LifecycleState { Offline, Ready, Trading, Stopping }` (Copy, PartialEq, Serialize as snake_case)
  - `Checklist { token_valid, ws_connected, game_data_loaded, helper_ok, helper_override: bool }` with `ready() -> bool` (Default, Clone, PartialEq, Serialize)
  - `helper_ok(helper_override: bool, dry_run: bool) -> bool`
  - `StopReason { UserStop, SignedOut, Unauthorized, WebsocketDown, HelperLost, TraderCritical(String), OrderFailures(u32), TraderPanic(String) }` with `describe() -> String` (Clone, PartialEq, Debug, Serialize with tag `kind` and content `detail`)
  - `TriggerInput { signed_in, unauthorized: bool, ws_down_for_s: Option<i64>, helper_ok: bool }`
  - `stop_trigger(&TriggerInput) -> Option<StopReason>`
  - `WS_DOWN_LIMIT_S = 60`

- [ ] **Step 1: Write `lifecycle.rs` with its tests**

```rust
//! Pure lifecycle rules: readiness checklist and stop triggers (spec §5.7, amendment C7).

use serde::Serialize;

pub const WS_DOWN_LIMIT_S: i64 = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Offline,
    Ready,
    Trading,
    Stopping,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Checklist {
    pub token_valid: bool,
    pub ws_connected: bool,
    pub game_data_loaded: bool,
    pub helper_ok: bool,
    pub helper_override: bool,
}

impl Checklist {
    pub fn ready(&self) -> bool {
        self.token_valid && self.ws_connected && self.game_data_loaded && self.helper_ok
    }
}

/// Phase 3 has no helper: the dry-run-only override stands in for it (spec §11 phase 3).
pub fn helper_ok(helper_override: bool, dry_run: bool) -> bool {
    helper_override && dry_run
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum StopReason {
    UserStop,
    SignedOut,
    Unauthorized,
    WebsocketDown,
    HelperLost,
    TraderCritical(String),
    OrderFailures(u32),
    TraderPanic(String),
}

impl StopReason {
    pub fn describe(&self) -> String {
        match self {
            StopReason::UserStop => "Stop button".into(),
            StopReason::SignedOut => "Signed out of warframe.market".into(),
            StopReason::Unauthorized => "warframe.market returned 401 Unauthorized".into(),
            StopReason::WebsocketDown => format!("warframe.market websocket down for more than {} s", WS_DOWN_LIMIT_S),
            StopReason::HelperLost => "Helper unavailable (the helper override needs dry-run)".into(),
            StopReason::TraderCritical(message) => format!("Trader error: {}", message),
            StopReason::OrderFailures(count) => format!("{} consecutive order failures", count),
            StopReason::TraderPanic(message) => format!("Trader panicked: {}", message),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriggerInput {
    pub signed_in: bool,
    pub unauthorized: bool,
    pub ws_down_for_s: Option<i64>,
    pub helper_ok: bool,
}

/// First matching stop trigger while trading. Engine exits are handled by the controller.
pub fn stop_trigger(input: &TriggerInput) -> Option<StopReason> {
    if !input.signed_in {
        Some(StopReason::SignedOut)
    } else if input.unauthorized {
        Some(StopReason::Unauthorized)
    } else if input.ws_down_for_s.is_some_and(|s| s > WS_DOWN_LIMIT_S) {
        Some(StopReason::WebsocketDown)
    } else if !input.helper_ok {
        Some(StopReason::HelperLost)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> TriggerInput {
        TriggerInput { signed_in: true, unauthorized: false, ws_down_for_s: None, helper_ok: true }
    }

    #[test]
    fn ready_needs_every_checklist_item() {
        let all = Checklist { token_valid: true, ws_connected: true, game_data_loaded: true, helper_ok: true, helper_override: true };
        assert!(all.ready());
        for broken in [
            Checklist { token_valid: false, ..all.clone() },
            Checklist { ws_connected: false, ..all.clone() },
            Checklist { game_data_loaded: false, ..all.clone() },
            Checklist { helper_ok: false, ..all.clone() },
        ] {
            assert!(!broken.ready());
        }
    }

    #[test]
    fn helper_override_only_counts_in_dry_run() {
        assert!(helper_ok(true, true));
        assert!(!helper_ok(true, false));
        assert!(!helper_ok(false, true));
    }

    #[test]
    fn stop_triggers_in_priority_order() {
        assert_eq!(stop_trigger(&healthy()), None);
        assert_eq!(stop_trigger(&TriggerInput { signed_in: false, unauthorized: true, ..healthy() }), Some(StopReason::SignedOut));
        assert_eq!(stop_trigger(&TriggerInput { unauthorized: true, ws_down_for_s: Some(99), ..healthy() }), Some(StopReason::Unauthorized));
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(60), ..healthy() }), None, "60 s is allowed");
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(61), helper_ok: false, ..healthy() }), Some(StopReason::WebsocketDown));
        assert_eq!(stop_trigger(&TriggerInput { helper_ok: false, ..healthy() }), Some(StopReason::HelperLost));
    }

    #[test]
    fn stop_reasons_serialize_with_kind_and_detail() {
        assert_eq!(serde_json::to_value(StopReason::OrderFailures(5)).unwrap(), serde_json::json!({"kind": "order_failures", "detail": 5}));
        assert_eq!(serde_json::to_value(StopReason::UserStop).unwrap(), serde_json::json!({"kind": "user_stop"}));
        assert_eq!(StopReason::OrderFailures(5).describe(), "5 consecutive order failures");
    }
}
```

Add `pub mod lifecycle;` to `crates/qf_core/src/trader/mod.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib trader::lifecycle`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 3: Commit**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): add lifecycle checklist and stop-trigger rules

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 10: TraderController (start, stop, monitor tick)

**Files:**
- Create: `crates/qf_core/src/trader/controller.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (add `pub mod controller;`)

**Interfaces:**
- Consumes: `EngineExit` (Task 8); `LifecycleState`, `Checklist`, `helper_ok`, `StopReason`, `TriggerInput`, `stop_trigger` (Task 9); `SessionSnapshot` (Task 4); `store::{load_options, save_flags, record_stop, prune_dry_run, TraderOptions}` (Task 2).
- Produces:
  - `BoxFuture<'a, T>`
  - `trait Platform: Send + Sync` with these methods:
    - `session(now) -> SessionSnapshot`
    - `game_data_loaded() -> bool`
    - `spawn_engine(dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit>`
    - `set_status(&'static str) -> BoxFuture<Result<(), Error>>`
    - `delete_live_buy_orders() -> BoxFuture<Result<usize, Error>>`
    - `notify_stopped(&StopReason, dry_run: bool, at)`
    - `token_expiry_alert_due(now) -> Option<DateTime<Utc>>`
    - `notify_token_expiring(expires_at, now)`
    - `broadcast(&TraderStatus)`
  - `TraderStatus { state, checklist, options, session, running_since: Option<String>, running_dry_run: Option<bool> }` (Serialize, Clone)
  - `TraderController` with:
    - `async new(conn, Arc<dyn Platform>) -> Result<Self, Error>`
    - `async status(now) -> TraderStatus`
    - `async start(now) -> Result<TraderStatus, Error>`
    - `async stop(StopReason, now) -> Result<TraderStatus, Error>`
    - `async tick(now) -> Result<Option<StopReason>, Error>`
    - `async set_options(dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool>, helper_override: Option<bool>) -> Result<TraderOptions, Error>`
    - `conn() -> &DatabaseConnection`
  - `async monitor_loop(Arc<TraderController>)`
  - `MONITOR_EVERY` (5 s)

- [ ] **Step 1: Write `controller.rs`**

```rust
//! Trader lifecycle: checklist, start and stop sequences, and the monitor tick (spec §5.7, amendments C7–C10).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use service::sea_orm::DatabaseConnection;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use utils::{error, get_location, info, Error, LoggerOptions};

use super::engine::EngineExit;
use super::lifecycle::{helper_ok, stop_trigger, Checklist, LifecycleState, StopReason, TriggerInput};
use super::session::SessionSnapshot;
use super::store::{self, TraderOptions};
use crate::collector::ts;

pub const MONITOR_EVERY: StdDuration = StdDuration::from_secs(5);
const PRUNE_EVERY: StdDuration = StdDuration::from_secs(60 * 60);

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Everything the controller does outside itself. Faked in tests, `LivePlatform` in production.
pub trait Platform: Send + Sync {
    fn session(&self, now: DateTime<Utc>) -> SessionSnapshot;
    fn game_data_loaded(&self) -> bool;
    fn spawn_engine(&self, dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit>;
    fn set_status(&self, status: &'static str) -> BoxFuture<'_, Result<(), Error>>;
    fn delete_live_buy_orders(&self) -> BoxFuture<'_, Result<usize, Error>>;
    fn notify_stopped(&self, reason: &StopReason, dry_run: bool, at: DateTime<Utc>);
    fn token_expiry_alert_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>>;
    fn notify_token_expiring(&self, expires_at: DateTime<Utc>, now: DateTime<Utc>);
    fn broadcast(&self, status: &TraderStatus);
}

#[derive(Debug, Clone, Serialize)]
pub struct TraderStatus {
    pub state: LifecycleState,
    pub checklist: Checklist,
    pub options: TraderOptions,
    pub session: SessionSnapshot,
    pub running_since: Option<String>,
    pub running_dry_run: Option<bool>,
}

struct Running {
    flag: Arc<AtomicBool>,
    handle: JoinHandle<EngineExit>,
    started_at: DateTime<Utc>,
    dry_run: bool,
}

struct Inner {
    state: LifecycleState,
    options: TraderOptions,
    running: Option<Running>,
}

pub struct TraderController {
    conn: DatabaseConnection,
    platform: Arc<dyn Platform>,
    inner: Mutex<Inner>,
}

impl TraderController {
    /// Loads the saved options and starts `Offline`. It never resumes trading (spec §5.7).
    pub async fn new(conn: DatabaseConnection, platform: Arc<dyn Platform>) -> Result<Self, Error> {
        let options = store::load_options(&conn).await?;
        Ok(Self { conn, platform, inner: Mutex::new(Inner { state: LifecycleState::Offline, options, running: None }) })
    }

    pub fn conn(&self) -> &DatabaseConnection {
        &self.conn
    }

    fn checklist(&self, session: &SessionSnapshot, options: &TraderOptions) -> Checklist {
        Checklist {
            token_valid: session.token_valid,
            ws_connected: session.ws_connected,
            game_data_loaded: self.platform.game_data_loaded(),
            helper_ok: helper_ok(options.helper_override, options.dry_run),
            helper_override: options.helper_override,
        }
    }

    fn status_of(&self, inner: &Inner, now: DateTime<Utc>) -> TraderStatus {
        let session = self.platform.session(now);
        TraderStatus {
            state: inner.state,
            checklist: self.checklist(&session, &inner.options),
            options: inner.options.clone(),
            session,
            running_since: inner.running.as_ref().map(|r| ts(r.started_at)),
            running_dry_run: inner.running.as_ref().map(|r| r.dry_run),
        }
    }

    fn idle_state(&self, inner: &Inner, now: DateTime<Utc>) -> LifecycleState {
        let session = self.platform.session(now);
        if self.checklist(&session, &inner.options).ready() { LifecycleState::Ready } else { LifecycleState::Offline }
    }

    pub async fn status(&self, now: DateTime<Utc>) -> TraderStatus {
        let inner = self.inner.lock().await;
        self.status_of(&inner, now)
    }

    /// `Ready → Trading` (spec §5.7): status `ingame` unless dry-run, start the loop, broadcast.
    pub async fn start(&self, now: DateTime<Utc>) -> Result<TraderStatus, Error> {
        let mut inner = self.inner.lock().await;
        if inner.running.is_some() {
            return Ok(self.status_of(&inner, now));
        }
        let session = self.platform.session(now);
        if !self.checklist(&session, &inner.options).ready() {
            return Err(Error::new("Trader:Start", "The trader is not ready; see the start checklist", get_location!()));
        }
        let dry_run = inner.options.dry_run;
        if !dry_run {
            self.platform.set_status("ingame").await?;
        }
        let flag = Arc::new(AtomicBool::new(true));
        let handle = self.platform.spawn_engine(dry_run, flag.clone());
        inner.running = Some(Running { flag, handle, started_at: now, dry_run });
        inner.state = LifecycleState::Trading;
        info("Trader:Start", format!("Trader started ({})", if dry_run { "dry-run" } else { "live" }), &LoggerOptions::default());
        let status = self.status_of(&inner, now);
        self.platform.broadcast(&status);
        Ok(status)
    }

    /// `Trading → Stopping → Ready/Offline`. When `reason` is `None`, it comes from how the engine exited.
    async fn finish(&self, inner: &mut Inner, reason: Option<StopReason>, now: DateTime<Utc>) -> Result<Option<StopReason>, Error> {
        let Some(running) = inner.running.take() else { return Ok(None) };
        inner.state = LifecycleState::Stopping;
        self.platform.broadcast(&self.status_of(inner, now));

        running.flag.store(false, Ordering::SeqCst);
        let exit = running.handle.await;
        let reason = reason.unwrap_or_else(|| match exit {
            Ok(EngineExit::Critical(message)) => StopReason::TraderCritical(message),
            Ok(EngineExit::OrderFailures(count)) => StopReason::OrderFailures(count),
            Ok(EngineExit::Stopped) => StopReason::UserStop,
            Err(join_error) => StopReason::TraderPanic(join_error.to_string()),
        });

        if let Err(e) = self.platform.set_status("invisible").await {
            error("Trader:Stop", format!("Could not set status invisible: {}", e.message), &LoggerOptions::default());
        }
        if inner.options.delete_buy_orders_on_stop && !running.dry_run {
            match self.platform.delete_live_buy_orders().await {
                Ok(count) => info("Trader:Stop", format!("Deleted {} buy orders", count), &LoggerOptions::default()),
                Err(e) => error("Trader:Stop", format!("Could not delete buy orders: {}", e.message), &LoggerOptions::default()),
            }
        }
        let description = reason.describe();
        if let Err(e) = store::record_stop(&self.conn, &description, now).await {
            let _ = e.log("trader.log");
        }
        inner.options.last_stop_reason = Some(description.clone());
        inner.options.last_stop_at = Some(ts(now));
        self.platform.notify_stopped(&reason, running.dry_run, now);
        inner.state = self.idle_state(inner, now);
        info("Trader:Stop", format!("Trader stopped: {}", description), &LoggerOptions::default());
        self.platform.broadcast(&self.status_of(inner, now));
        Ok(Some(reason))
    }

    pub async fn stop(&self, reason: StopReason, now: DateTime<Utc>) -> Result<TraderStatus, Error> {
        let mut inner = self.inner.lock().await;
        self.finish(&mut inner, Some(reason), now).await?;
        Ok(self.status_of(&inner, now))
    }

    /// Runs every `MONITOR_EVERY`: expiry alerts, idle state, engine exits and stop triggers.
    pub async fn tick(&self, now: DateTime<Utc>) -> Result<Option<StopReason>, Error> {
        if let Some(expires_at) = self.platform.token_expiry_alert_due(now) {
            self.platform.notify_token_expiring(expires_at, now);
        }
        let mut inner = self.inner.lock().await;
        let session = self.platform.session(now);
        let checklist = self.checklist(&session, &inner.options);
        let Some(running) = inner.running.as_ref() else {
            let next = if checklist.ready() { LifecycleState::Ready } else { LifecycleState::Offline };
            if next != inner.state {
                inner.state = next;
                self.platform.broadcast(&self.status_of(&inner, now));
            }
            return Ok(None);
        };
        if running.handle.is_finished() {
            return self.finish(&mut inner, None, now).await;
        }
        let trigger = stop_trigger(&TriggerInput {
            signed_in: session.signed_in,
            unauthorized: session.unauthorized,
            ws_down_for_s: session.ws_down_for_s,
            helper_ok: checklist.helper_ok,
        });
        match trigger {
            Some(reason) => self.finish(&mut inner, Some(reason), now).await,
            None => Ok(None),
        }
    }

    pub async fn set_options(
        &self,
        dry_run: Option<bool>,
        delete_buy_orders_on_stop: Option<bool>,
        helper_override: Option<bool>,
    ) -> Result<TraderOptions, Error> {
        let mut inner = self.inner.lock().await;
        let mut next = inner.options.clone();
        if let Some(value) = dry_run {
            if inner.running.is_some() && value != next.dry_run {
                return Err(Error::new("Trader:Options", "Stop the trader before changing dry-run", get_location!()));
            }
            next.dry_run = value;
        }
        if let Some(value) = delete_buy_orders_on_stop {
            next.delete_buy_orders_on_stop = value;
        }
        if let Some(value) = helper_override {
            next.helper_override = value;
        }
        store::save_flags(&self.conn, next.dry_run, next.delete_buy_orders_on_stop, next.helper_override).await?;
        inner.options = next.clone();
        self.platform.broadcast(&self.status_of(&inner, Utc::now()));
        Ok(next)
    }
}

pub async fn monitor_loop(controller: Arc<TraderController>) {
    let mut last_prune: Option<Instant> = None;
    loop {
        let now = Utc::now();
        if let Err(e) = controller.tick(now).await {
            let _ = e.log("trader.log");
        }
        if last_prune.is_none_or(|t| t.elapsed() >= PRUNE_EVERY) {
            if let Err(e) = store::prune_dry_run(controller.conn(), now).await {
                let _ = e.log("trader.log");
            }
            last_prune = Some(Instant::now());
        }
        tokio::time::sleep(MONITOR_EVERY).await;
    }
}
```

- [ ] **Step 2: Write the controller tests**

Append to `controller.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Fake {
        session: StdMutex<SessionSnapshot>,
        loaded: AtomicBool,
        exit: StdMutex<Option<EngineExit>>,
        statuses: StdMutex<Vec<&'static str>>,
        deletes: AtomicUsize,
        stopped: StdMutex<Vec<(StopReason, bool)>>,
        expiry: StdMutex<Option<DateTime<Utc>>>,
        expiry_alerts: AtomicUsize,
        broadcasts: AtomicUsize,
    }

    impl Platform for Fake {
        fn session(&self, _now: DateTime<Utc>) -> SessionSnapshot {
            self.session.lock().unwrap().clone()
        }
        fn game_data_loaded(&self) -> bool {
            self.loaded.load(Ordering::SeqCst)
        }
        fn spawn_engine(&self, _dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit> {
            let exit = self.exit.lock().unwrap().clone();
            tokio::spawn(async move {
                if let Some(exit) = exit {
                    return exit;
                }
                while running.load(Ordering::SeqCst) {
                    tokio::time::sleep(StdDuration::from_millis(5)).await;
                }
                EngineExit::Stopped
            })
        }
        fn set_status(&self, status: &'static str) -> BoxFuture<'_, Result<(), Error>> {
            self.statuses.lock().unwrap().push(status);
            Box::pin(async { Ok(()) })
        }
        fn delete_live_buy_orders(&self) -> BoxFuture<'_, Result<usize, Error>> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(2) })
        }
        fn notify_stopped(&self, reason: &StopReason, dry_run: bool, _at: DateTime<Utc>) {
            self.stopped.lock().unwrap().push((reason.clone(), dry_run));
        }
        fn token_expiry_alert_due(&self, _now: DateTime<Utc>) -> Option<DateTime<Utc>> {
            self.expiry.lock().unwrap().take()
        }
        fn notify_token_expiring(&self, _expires_at: DateTime<Utc>, _now: DateTime<Utc>) {
            self.expiry_alerts.fetch_add(1, Ordering::SeqCst);
        }
        fn broadcast(&self, _status: &TraderStatus) {
            self.broadcasts.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-16T12:00:00Z").unwrap()
    }

    fn healthy_session() -> SessionSnapshot {
        SessionSnapshot { signed_in: true, token_valid: true, ws_connected: true, ..Default::default() }
    }

    /// Ready in dry-run with the helper override on.
    async fn ready_controller() -> (tempfile::TempDir, Arc<Fake>, TraderController) {
        let (dir, conn) = crate::trader::store::tests::db().await;
        store::save_flags(&conn, true, true, true).await.unwrap();
        let fake = Arc::new(Fake::default());
        *fake.session.lock().unwrap() = healthy_session();
        fake.loaded.store(true, Ordering::SeqCst);
        let controller = TraderController::new(conn, fake.clone()).await.unwrap();
        (dir, fake, controller)
    }

    #[tokio::test]
    async fn starts_offline_then_ticks_to_ready_and_never_trades_by_itself() {
        let (_dir, _fake, controller) = ready_controller().await;
        assert_eq!(controller.status(now()).await.state, LifecycleState::Offline);
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Ready);
    }

    #[tokio::test]
    async fn start_is_refused_until_the_checklist_passes() {
        let (_dir, fake, controller) = ready_controller().await;
        fake.session.lock().unwrap().ws_connected = false;
        assert!(controller.start(now()).await.is_err());
        controller.set_options(Some(false), None, None).await.unwrap();
        fake.session.lock().unwrap().ws_connected = true;
        let status = controller.status(now()).await;
        assert!(!status.checklist.helper_ok, "the helper override does not apply live");
        assert!(controller.start(now()).await.is_err());
    }

    #[tokio::test]
    async fn dry_run_start_and_stop_follow_the_sequence() {
        let (_dir, fake, controller) = ready_controller().await;
        let status = controller.start(now()).await.unwrap();
        assert_eq!(status.state, LifecycleState::Trading);
        assert_eq!(status.running_dry_run, Some(true));
        assert!(fake.statuses.lock().unwrap().is_empty(), "no ingame status in dry-run");

        let status = controller.stop(StopReason::UserStop, now()).await.unwrap();
        assert_eq!(status.state, LifecycleState::Ready);
        assert_eq!(*fake.statuses.lock().unwrap(), vec!["invisible"]);
        assert_eq!(fake.deletes.load(Ordering::SeqCst), 0, "dry-run never deletes real buy orders");
        assert_eq!(*fake.stopped.lock().unwrap(), vec![(StopReason::UserStop, true)]);
        assert_eq!(status.options.last_stop_reason.as_deref(), Some("Stop button"));
        let saved = store::load_options(controller.conn()).await.unwrap();
        assert_eq!(saved.last_stop_reason.as_deref(), Some("Stop button"));
    }

    #[tokio::test]
    async fn session_problems_stop_trading() {
        for (edit, expected) in [
            (Box::new(|s: &mut SessionSnapshot| s.unauthorized = true) as Box<dyn Fn(&mut SessionSnapshot)>, StopReason::Unauthorized),
            (Box::new(|s: &mut SessionSnapshot| { s.ws_connected = false; s.ws_down_for_s = Some(61) }), StopReason::WebsocketDown),
            (Box::new(|s: &mut SessionSnapshot| s.signed_in = false), StopReason::SignedOut),
        ] {
            let (_dir, fake, controller) = ready_controller().await;
            controller.start(now()).await.unwrap();
            edit(&mut fake.session.lock().unwrap());
            assert_eq!(controller.tick(now()).await.unwrap(), Some(expected.clone()));
            assert_ne!(controller.status(now()).await.state, LifecycleState::Trading);
        }
    }

    #[tokio::test]
    async fn turning_off_the_helper_override_stops_trading() {
        let (_dir, _fake, controller) = ready_controller().await;
        controller.start(now()).await.unwrap();
        controller.set_options(None, None, Some(false)).await.unwrap();
        assert_eq!(controller.tick(now()).await.unwrap(), Some(StopReason::HelperLost));
    }

    #[tokio::test]
    async fn engine_exit_becomes_the_stop_reason() {
        let (_dir, fake, controller) = ready_controller().await;
        *fake.exit.lock().unwrap() = Some(EngineExit::Critical("Trader:Item: bad request".into()));
        controller.start(now()).await.unwrap();
        tokio::time::sleep(StdDuration::from_millis(20)).await;
        assert_eq!(
            controller.tick(now()).await.unwrap(),
            Some(StopReason::TraderCritical("Trader:Item: bad request".into()))
        );
    }

    #[tokio::test]
    async fn dry_run_cannot_change_while_trading() {
        let (_dir, _fake, controller) = ready_controller().await;
        controller.start(now()).await.unwrap();
        assert!(controller.set_options(Some(false), None, None).await.is_err());
        assert!(controller.set_options(None, Some(false), None).await.is_ok());
    }

    #[tokio::test]
    async fn tick_sends_token_expiry_alerts() {
        let (_dir, fake, controller) = ready_controller().await;
        *fake.expiry.lock().unwrap() = Some(now());
        controller.tick(now()).await.unwrap();
        controller.tick(now()).await.unwrap();
        assert_eq!(fake.expiry_alerts.load(Ordering::SeqCst), 1);
    }
}
```

Add `pub mod controller;` to `crates/qf_core/src/trader/mod.rs`.

- [ ] **Step 3: Run the tests**

Run: `cargo test -p qf_core --lib trader::controller`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 4: Commit**

```bash
git add crates/qf_core/src/trader
git commit -m "feat(trader): add lifecycle controller with start, stop and monitor tick

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 11: Live platform, alerts, hot-set candidates and startup

**Files:**
- Create: `crates/qf_core/src/trader/platform.rs`
- Modify: `crates/qf_core/src/trader/mod.rs` (platform module, global controller, `start`)
- Modify: `crates/qf_core/src/types/ui_events.rs` (`LifecycleState`)
- Modify: `crates/qf_core/src/app/types/settings/notifications_setting.rs` (two new settings)
- Modify: `crates/qf_core/src/collector/runner.rs` (`refresh_hot_set`)
- Modify: `crates/qf_core/src/startup.rs`

**Interfaces:**
- Consumes: Tasks 2–10; `collector::runner::supervise`; `commands::user::user_set_status`; `NotificationSetting::send`.
- Produces:
  - `platform::LivePlatform::new(conn)`, which implements `Platform`
  - `trader::start(conn) -> Result<(), Error>` (async), `trader::get() -> Option<Arc<TraderController>>`
  - `UIEvent::LifecycleState`, which serialises as `"Lifecycle:State"`
  - `NotificationsSetting.on_trader_stopped` and `NotificationsSetting.on_token_expiring`

- [ ] **Step 1: Add the event and notification settings**

In `crates/qf_core/src/types/ui_events.rs`:
- Add `LifecycleState,` after `OnWfmChatMessage,` in the enum.
- Add `UIEvent::LifecycleState => "Lifecycle:State",` in `as_str`.

In `crates/qf_core/src/app/types/settings/notifications_setting.rs`, add two fields after `on_new_trade`:

```rust
    #[serde(default = "default_on_trader_stopped")]
    pub on_trader_stopped: NotificationSetting,
    #[serde(default = "default_on_token_expiring")]
    pub on_token_expiring: NotificationSetting,
```

add these functions below the struct:

```rust
fn default_on_trader_stopped() -> NotificationSetting {
    NotificationSetting::new(
        DiscordNotify::new("<MENTION>\n```ansi\n\x1B[1;31m⛔ Trader stopped\x1B[0m\n\n\x1B[1;33m📝 Reason:\x1B[0m <REASON>\n\x1B[1;33m🧪 Mode:\x1B[0m   <MODE>\n\x1B[1;33m🕒 Time:\x1B[0m   <TIME>\n```", "", vec![]),
        SystemNotify::new("Trader stopped", "<REASON>", "windows_xp_error.mp3", 1.0),
        WebHookNotify::new("<WEBHOOK_URL>"),
    )
}

fn default_on_token_expiring() -> NotificationSetting {
    NotificationSetting::new(
        DiscordNotify::new("<MENTION>\n```ansi\n\x1B[1;33m⚠️ warframe.market sign-in expires soon\x1B[0m\n\n\x1B[1;33m📅 Expires:\x1B[0m <EXPIRES_AT> (<DAYS_LEFT> days)\nSign in again from the web UI.\n```", "", vec![]),
        SystemNotify::new("warframe.market sign-in expires soon", "Expires <EXPIRES_AT>", "cat_meow.mp3", 1.0),
        WebHookNotify::new("<WEBHOOK_URL>"),
    )
}
```

and in `impl Default for NotificationsSetting`, add after the `on_new_trade: ...` entry:

```rust
            on_trader_stopped: default_on_trader_stopped(),
            on_token_expiring: default_on_token_expiring(),
```

- [ ] **Step 2: Write `platform.rs`**

```rust
//! Production side effects for the trader controller.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::json;
use service::sea_orm::DatabaseConnection;
use tokio::task::JoinHandle;
use utils::{error, Error, LoggerOptions};
use wf_market::enums::OrderType;

use super::controller::{BoxFuture, Platform, TraderStatus};
use super::engine::{self, EngineExit};
use super::item::ItemTrader;
use super::lifecycle::{LifecycleState, StopReason};
use super::orders::TradeOrders;
use super::session::{self, SessionSnapshot};
use super::TradeContext;
use crate::collector::ts;
use crate::types::UIEvent;
use crate::utils::modules::states;
use crate::send_event;

pub struct LivePlatform {
    conn: DatabaseConnection,
}

impl LivePlatform {
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { conn }
    }
}

impl Platform for LivePlatform {
    fn session(&self, now: DateTime<Utc>) -> SessionSnapshot {
        session::get().snapshot(now)
    }

    fn game_data_loaded(&self) -> bool {
        states::cache_client()
            .ok()
            .and_then(|cache| cache.tradable_item().get_items().ok())
            .is_some_and(|items| !items.is_empty())
    }

    fn spawn_engine(&self, dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit> {
        let conn = self.conn.clone();
        tokio::spawn(async move {
            let live = states::try_app_state().map(|app| app.wfm_client);
            let orders = Arc::new(TradeOrders::new(live, Some(conn.clone()), dry_run));
            let just_started = Arc::new(AtomicBool::new(true));
            let trader = Arc::new(ItemTrader::new(running.clone(), just_started.clone()));
            let check = {
                let (conn, orders, trader) = (conn.clone(), orders.clone(), trader.clone());
                move || {
                    let (conn, orders, trader) = (conn.clone(), orders.clone(), trader.clone());
                    async move {
                        let ctx = TradeContext::load(&conn, orders).await?;
                        trader.check(&ctx).await
                    }
                }
            };
            engine::run_loop(running, just_started, orders, check, engine::CYCLE_PAUSE).await
        })
    }

    fn set_status(&self, status: &'static str) -> BoxFuture<'_, Result<(), Error>> {
        Box::pin(crate::commands::user::user_set_status(status.to_string()))
    }

    fn delete_live_buy_orders(&self) -> BoxFuture<'_, Result<usize, Error>> {
        Box::pin(async move {
            let app = states::app_state()?;
            let mut deleted = 0;
            for id in app.wfm_client.order().cache_orders().order_ids(OrderType::Buy) {
                match app.wfm_client.order().delete(&id).await {
                    Ok(_) => deleted += 1,
                    Err(e) => error("Trader:Stop", format!("Failed to delete buy order {}: {}", id, e), &LoggerOptions::default()),
                }
            }
            Ok(deleted)
        })
    }

    fn notify_stopped(&self, reason: &StopReason, dry_run: bool, at: DateTime<Utc>) {
        let Some(app) = states::try_app_state() else { return };
        let mode = if dry_run { "dry-run" } else { "live" };
        let variables = HashMap::from([
            ("<REASON>".to_string(), reason.describe()),
            ("<MODE>".to_string(), mode.to_string()),
            ("<TIME>".to_string(), ts(at)),
        ]);
        app.settings.notifications.on_trader_stopped.send(
            &variables,
            Some(json!({"event": "trader_stopped", "reason": reason, "mode": mode, "at": ts(at)})),
        );
    }

    fn token_expiry_alert_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        session::get().expiry_alert_due(now)
    }

    fn notify_token_expiring(&self, expires_at: DateTime<Utc>, now: DateTime<Utc>) {
        let days_left = (expires_at - now).num_days().max(0);
        send_event!(
            UIEvent::OnNotify,
            json!({
                "i18n_key": "token_expiring",
                "color": "yellow",
                "type": "warning",
                "values": {"expires_at": ts(expires_at), "days_left": days_left},
                "settings": {"autoClose": false}
            })
        );
        let Some(app) = states::try_app_state() else { return };
        let variables = HashMap::from([
            ("<EXPIRES_AT>".to_string(), ts(expires_at)),
            ("<DAYS_LEFT>".to_string(), days_left.to_string()),
        ]);
        app.settings.notifications.on_token_expiring.send(
            &variables,
            Some(json!({"event": "token_expiring", "expires_at": ts(expires_at), "days_left": days_left})),
        );
    }

    fn broadcast(&self, status: &TraderStatus) {
        send_event!(UIEvent::LifecycleState, json!(status));
        send_event!(UIEvent::UpdateLiveScraperRunningState, json!(status.state == LifecycleState::Trading));
    }
}
```

- [ ] **Step 3: Register the controller**

In `crates/qf_core/src/trader/mod.rs`, add `pub mod platform;` to the module list, and append:

```rust
static CONTROLLER: std::sync::OnceLock<Arc<controller::TraderController>> = std::sync::OnceLock::new();

pub fn get() -> Option<Arc<controller::TraderController>> {
    CONTROLLER.get().cloned()
}

/// Creates the controller (never trading), then supervises its monitor loop and the `/me` checks.
pub async fn start(conn: DatabaseConnection) -> Result<(), Error> {
    let platform = Arc::new(platform::LivePlatform::new(conn.clone()));
    let controller = Arc::new(controller::TraderController::new(conn, platform).await?);
    let _ = CONTROLLER.set(controller.clone());
    let delay = std::time::Duration::from_secs(5);
    crate::collector::runner::supervise("Trader:Monitor", delay, move || controller::monitor_loop(controller.clone()));
    crate::collector::runner::supervise("Trader:Session", delay, session::me_check_loop);
    Ok(())
}
```

- [ ] **Step 4: Add buy candidates to the hot set (amendment C3)**

In `crates/qf_core/src/collector/runner.rs`, replace `refresh_hot_set`:

```rust
    pub async fn refresh_hot_set(&self) -> Result<(), Error> {
        let ids = store::hot_item_ids(&self.conn).await?;
        self.set_hot(ids);
        Ok(())
    }
```

with:

```rust
    /// Stock, wish list and, when Buy mode is on, the trader's buy candidates (amendments B3, C3).
    pub async fn refresh_hot_set(&self) -> Result<(), Error> {
        let mut ids = store::hot_item_ids(&self.conn).await?;
        if let Some(app) = states::try_app_state() {
            let cache = states::cache_client()?;
            let prices = crate::trader::price_source::StatsPriceSource::load(&self.conn, &cache).await?;
            ids.extend(crate::trader::price_source::buy_candidate_ids(&app.settings, &prices));
        }
        self.set_hot(ids);
        Ok(())
    }
```

- [ ] **Step 5: Start the trader at boot**

In `crates/qf_core/src/startup.rs`, replace:

```rust
    let _ = HAS_STARTED.set(true);
```

with:

```rust
    crate::trader::start(conn.clone()).await?;

    let _ = HAS_STARTED.set(true);
```

- [ ] **Step 6: Build and run all Rust tests**

Run: `cargo test -p qf_core --lib && cargo test -p qf-server`
Expected: all pass (about 120 qf_core tests).

If `Box::pin(crate::commands::user::user_set_status(...))` fails with "future cannot be sent between threads", `user_set_status` is holding a `MutexGuard` across an `.await`. In that case, write a `set_status` that clones `app_mutex().lock()?.wfm_socket` in its own statement before awaiting, and send the same request `user_set_status` sends.

- [ ] **Step 7: Commit**

```bash
git add crates/qf_core/src
git commit -m "feat(trader): wire live platform, stop and expiry alerts, and buy candidates into the hot set

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 12: Trader RPC commands

**Files:**
- Create: `crates/qf_core/src/commands/trader.rs`
- Modify: `crates/qf_core/src/commands/mod.rs`, `crates/qf_core/src/commands/rpc.rs`

**Interfaces:**
- Consumes: `trader::get`, `TraderController` (Tasks 10–11), `store::dry_run_page` (Task 2), `StatsPriceSource`, `get_interesting_items` (Task 3).
- Produces RPC commands; response keys are snake_case:
  - `trader_status {}` → `TraderStatus`
  - `trader_start {}` → `TraderStatus`
  - `trader_stop {}` → `TraderStatus`
  - `trader_set_options { dryRun?, deleteBuyOrdersOnStop?, helperOverride? }` → `TraderOptions`
  - `trader_dry_run_log { page, limit }` → `DryRunPage`
  - `trader_interesting_items { settings: ItemSettings }` → `ItemPriceInfo[]`, each with an added `name`

- [ ] **Step 1: Write `commands/trader.rs`**

```rust
use std::sync::Arc;

use chrono::Utc;
use serde_json::{json, Value};
use utils::{get_location, Error};

use crate::app::ItemSettings;
use crate::trader::controller::{TraderController, TraderStatus};
use crate::trader::lifecycle::StopReason;
use crate::trader::price_source::{get_interesting_items, StatsPriceSource};
use crate::trader::store::{self, DryRunPage, TraderOptions};
use crate::utils::modules::states;
use crate::DATABASE;

fn controller() -> Result<Arc<TraderController>, Error> {
    crate::trader::get().ok_or_else(|| Error::new("Trader:Rpc", "The trader is not initialised yet", get_location!()))
}

pub async fn trader_status() -> Result<TraderStatus, Error> {
    Ok(controller()?.status(Utc::now()).await)
}

pub async fn trader_start() -> Result<TraderStatus, Error> {
    controller()?.start(Utc::now()).await
}

pub async fn trader_stop() -> Result<TraderStatus, Error> {
    controller()?.stop(StopReason::UserStop, Utc::now()).await
}

pub async fn trader_set_options(
    dry_run: Option<bool>,
    delete_buy_orders_on_stop: Option<bool>,
    helper_override: Option<bool>,
) -> Result<TraderOptions, Error> {
    controller()?.set_options(dry_run, delete_buy_orders_on_stop, helper_override).await
}

pub async fn trader_dry_run_log(page: i64, limit: i64) -> Result<DryRunPage, Error> {
    let conn = DATABASE.get().ok_or_else(|| Error::new("Trader:Rpc", "Database is not ready", get_location!()))?;
    store::dry_run_page(conn, page, limit).await
}

pub async fn trader_interesting_items(settings: ItemSettings) -> Result<Vec<Value>, Error> {
    let conn = DATABASE.get().ok_or_else(|| Error::new("Trader:Rpc", "Database is not ready", get_location!()))?;
    let cache = states::cache_client()?;
    let prices = StatsPriceSource::load(conn, &cache).await?;
    Ok(get_interesting_items(&settings, &prices)
        .into_iter()
        .map(|item| {
            let name = cache.tradable_item().get_by(&item.wfm_id).map(|i| i.name).unwrap_or_default();
            let mut value = serde_json::to_value(&item).unwrap_or_default();
            if let Some(object) = value.as_object_mut() {
                object.insert("name".into(), json!(name));
            }
            value
        })
        .collect())
}
```

In `crates/qf_core/src/commands/mod.rs`, add `pub mod trader;` before `pub mod transaction;`.

In `crates/qf_core/src/commands/rpc.rs`:
- Change `use crate::app::Settings;` to `use crate::app::{ItemSettings, Settings};`.
- Add these lines after `market_item_history => ...`:

```rust
    trader_status => trader::trader_status {},
    trader_start => trader::trader_start {},
    trader_stop => trader::trader_stop {},
    trader_set_options => trader::trader_set_options { dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool>, helper_override: Option<bool> },
    trader_dry_run_log => trader::trader_dry_run_log { page: i64, limit: i64 },
    trader_interesting_items => trader::trader_interesting_items { settings: ItemSettings },
```

Add to the rpc test module:

```rust
    #[tokio::test]
    async fn trader_commands_are_routable_and_validate_args() {
        for name in ["trader_status", "trader_start", "trader_stop", "trader_set_options", "trader_dry_run_log", "trader_interesting_items"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("trader_dry_run_log", json!({"page": 1})).await.unwrap().is_err(), "limit is required");
        assert!(dispatch("trader_set_options", json!({})).await.is_some(), "all options are optional");
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib commands::rpc && cargo test -p qf-server`
Expected: all pass, and `allowlist_has_no_removed_features` still passes.

- [ ] **Step 3: Commit**

```bash
git add crates/qf_core/src/commands
git commit -m "feat(core): expose trader status, start, stop, options and dry-run log over rpc

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 13: Trader panel, dry-run log and notification settings in the web UI

**Files:**
- Modify: `web/src/types/tauri.type.ts`
- Modify: `web/src/api/live_scraper/index.ts`
- Create: `web/src/pages/live_scraper/TraderPanel.tsx`
- Create: `web/src/pages/live_scraper/Tabs/DryRunLog/index.tsx`
- Modify: `web/src/pages/live_scraper/Tabs/index.ts`, `web/src/pages/live_scraper/index.tsx`
- Modify: `web/src/components/Forms/Settings/Tabs/Notifications/index.tsx`
- Modify: `web/public/lang/en.json`

**Interfaces:**
- Consumes: the Task 12 RPC commands; existing `api.cache.getTradableItems()`.
- Produces:
  - `TauriTypes`: `LifecycleState`, `TraderChecklist`, `TraderOptions`, `TraderSessionSnapshot`, `TraderStatus`, `DryRunEntry` and `DryRunPage`; `Events.LifecycleState`; the new `NotificationsSetting` fields; `ItemPriceInfo.warm` / `history_days` (optional)
  - `api.live_scraper`: `status()`, `start()`, `stop()`, `setOptions()`, `dryRunLog()`, plus `toggle()`, `get_state()` and `get_interesting_wtb_items()` for the existing callers

- [ ] **Step 1: Types**

In `web/src/types/tauri.type.ts`:
- In `export enum Events {`, add after `UpdateLiveScraperRunningState = "LiveScraper:UpdateRunningState",`:
  ```ts
    LifecycleState = "Lifecycle:State",
  ```
- In `export interface NotificationsSetting {`, add after `on_new_trade: NotificationSetting;`:
  ```ts
    on_trader_stopped: NotificationSetting;
    on_token_expiring: NotificationSetting;
  ```
- In `export interface ItemPriceInfo {`, add after `uuid: string;`:
  ```ts
    warm?: boolean;
    history_days?: number;
    name?: string;
  ```
- Directly before the final closing `}` of `export namespace TauriTypes`, add:

```ts
  export type LifecycleState = "offline" | "ready" | "trading" | "stopping";
  export interface TraderChecklist {
    token_valid: boolean;
    ws_connected: boolean;
    game_data_loaded: boolean;
    helper_ok: boolean;
    helper_override: boolean;
  }
  export interface TraderOptions {
    dry_run: boolean;
    delete_buy_orders_on_stop: boolean;
    helper_override: boolean;
    last_stop_reason?: string | null;
    last_stop_at?: string | null;
  }
  export interface TraderSessionSnapshot {
    signed_in: boolean;
    token_valid: boolean;
    unauthorized: boolean;
    ws_connected: boolean;
    ws_down_for_s?: number | null;
    last_me_ok_at?: string | null;
    token_expires_at?: string | null;
  }
  export interface TraderStatus {
    state: LifecycleState;
    checklist: TraderChecklist;
    options: TraderOptions;
    session: TraderSessionSnapshot;
    running_since?: string | null;
    running_dry_run?: boolean | null;
  }
  export interface DryRunEntry {
    id: number;
    at: string;
    action: "create" | "update" | "delete";
    side: "buy" | "sell";
    item_id: string;
    sub_type: string;
    price?: number | null;
    quantity?: number | null;
    reason: string;
    forced_by: "global" | "not_warm";
  }
  export interface DryRunPage {
    total: number;
    page: number;
    limit: number;
    results: DryRunEntry[];
  }
```

- [ ] **Step 2: API module**

Replace the whole of `web/src/api/live_scraper/index.ts` with:

```ts
import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class LiveScraperModule {
  constructor(private readonly client: TauriClient) {}

  status() {
    return this.client.sendInvoke<TauriTypes.TraderStatus>("trader_status");
  }
  start() {
    return this.client.sendInvoke<TauriTypes.TraderStatus>("trader_start");
  }
  stop() {
    return this.client.sendInvoke<TauriTypes.TraderStatus>("trader_stop");
  }
  setOptions(options: { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean; helperOverride?: boolean }) {
    return this.client.sendInvoke<TauriTypes.TraderOptions>("trader_set_options", options);
  }
  dryRunLog(page: number, limit: number) {
    return this.client.sendInvoke<TauriTypes.DryRunPage>("trader_dry_run_log", { page, limit });
  }

  async toggle(): Promise<TauriTypes.TraderStatus> {
    const status = await this.status();
    return status.state === "trading" ? this.stop() : this.start();
  }
  async get_state(): Promise<{ is_running: boolean }> {
    const status = await this.status();
    return { is_running: status.state === "trading" };
  }
  get_interesting_wtb_items(settings: TauriTypes.ItemSettings): Promise<TauriTypes.ItemPriceInfo[]> {
    return this.client.sendInvoke<TauriTypes.ItemPriceInfo[]>("trader_interesting_items", { settings });
  }
}
```

- [ ] **Step 3: Trader panel**

`web/src/pages/live_scraper/TraderPanel.tsx`:

```tsx
import api from "@api/index";
import { TauriTypes } from "$types";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Group, List, Paper, Stack, Switch, Text, ThemeIcon } from "@mantine/core";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

const STATE_COLOR: Record<TauriTypes.LifecycleState, string> = {
  offline: "gray",
  ready: "blue",
  trading: "green",
  stopping: "orange",
};

type OptionsInput = { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean; helperOverride?: boolean };

export function TraderPanel() {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trader.${key}`, context);
  const queryClient = useQueryClient();
  const { data: status, error } = useQuery({
    queryKey: ["trader_status"],
    queryFn: () => api.live_scraper.status(),
    refetchInterval: 5000,
    retry: false,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["trader_status"] });
  const start = useMutation({ mutationFn: () => api.live_scraper.start(), onSettled: refresh });
  const stop = useMutation({ mutationFn: () => api.live_scraper.stop(), onSettled: refresh });
  const options = useMutation({ mutationFn: (input: OptionsInput) => api.live_scraper.setOptions(input), onSettled: refresh });

  if (error) return <Alert color="red">{String((error as any)?.message ?? error)}</Alert>;
  if (!status) return null;

  const active = status.state === "trading" || status.state === "stopping";
  const checks: Array<[string, boolean]> = [
    ["token_valid", status.checklist.token_valid],
    ["ws_connected", status.checklist.ws_connected],
    ["game_data_loaded", status.checklist.game_data_loaded],
    ["helper_ok", status.checklist.helper_ok],
  ];
  const failure = start.error ?? stop.error ?? options.error;

  return (
    <Paper withBorder p="md" my="md">
      <Stack gap="sm">
        <Group justify="space-between">
          <Group>
            <Badge color={STATE_COLOR[status.state]} size="lg">
              {t(`states.${status.state}`)}
            </Badge>
            <Badge color={status.options.dry_run ? "yellow" : "red"} variant="outline">
              {status.options.dry_run ? t("dry_run") : t("live")}
            </Badge>
            {status.running_since && (
              <Text size="sm" c="dimmed">
                {t("running_since", { at: status.running_since })}
              </Text>
            )}
          </Group>
          {active ? (
            <Button color="red" loading={stop.isPending || status.state === "stopping"} onClick={() => stop.mutate()}>
              {t("stop")}
            </Button>
          ) : (
            <Button disabled={status.state !== "ready"} loading={start.isPending} onClick={() => start.mutate()}>
              {t("start")}
            </Button>
          )}
        </Group>
        <List spacing={4} size="sm">
          {checks.map(([key, ok]) => (
            <List.Item
              key={key}
              icon={
                <ThemeIcon color={ok ? "green" : "red"} size={16} radius="xl">
                  {ok ? "✓" : "✕"}
                </ThemeIcon>
              }
            >
              {t(`checklist.${key}`)}
            </List.Item>
          ))}
        </List>
        <Group>
          <Switch
            label={t("options.dry_run")}
            checked={status.options.dry_run}
            disabled={active}
            onChange={(e) => options.mutate({ dryRun: e.currentTarget.checked })}
          />
          <Switch
            label={t("options.helper_override")}
            checked={status.options.helper_override}
            onChange={(e) => options.mutate({ helperOverride: e.currentTarget.checked })}
          />
          <Switch
            label={t("options.delete_buy_orders_on_stop")}
            checked={status.options.delete_buy_orders_on_stop}
            onChange={(e) => options.mutate({ deleteBuyOrdersOnStop: e.currentTarget.checked })}
          />
        </Group>
        {status.options.last_stop_reason && (
          <Text size="sm" c="dimmed">
            {t("last_stop", { reason: status.options.last_stop_reason, at: status.options.last_stop_at ?? "" })}
          </Text>
        )}
        {failure && <Alert color="red">{String((failure as any)?.message ?? failure)}</Alert>}
      </Stack>
    </Paper>
  );
}
```

- [ ] **Step 4: Dry-run log tab**

`web/src/pages/live_scraper/Tabs/DryRunLog/index.tsx`:

```tsx
import api from "@api/index";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Badge, Pagination, Stack, Table, Text } from "@mantine/core";
import { useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

const LIMIT = 50;

export function DryRunLogPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trader.dry_run_log.${key}`, context);
  const [page, setPage] = useState(1);
  const { data } = useQuery({
    queryKey: ["trader_dry_run_log", page],
    queryFn: () => api.live_scraper.dryRunLog(page, LIMIT),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const { data: items } = useQuery({ queryKey: ["cache_items"], queryFn: () => api.cache.getTradableItems() });
  const names = useMemo(() => new Map((items ?? []).map((item) => [item.wfmId, item.name])), [items]);
  const pages = Math.max(1, Math.ceil((data?.total ?? 0) / LIMIT));

  return (
    <Stack mt="md">
      <Text size="sm" c="dimmed">
        {t("total", { total: data?.total ?? 0 })}
      </Text>
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("at")}</Table.Th>
            <Table.Th>{t("action")}</Table.Th>
            <Table.Th>{t("side")}</Table.Th>
            <Table.Th>{t("item")}</Table.Th>
            <Table.Th>{t("sub_type")}</Table.Th>
            <Table.Th>{t("price")}</Table.Th>
            <Table.Th>{t("quantity")}</Table.Th>
            <Table.Th>{t("forced_by")}</Table.Th>
            <Table.Th>{t("reason")}</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(data?.results ?? []).map((entry) => (
            <Table.Tr key={entry.id}>
              <Table.Td>{entry.at}</Table.Td>
              <Table.Td>
                <Badge color={entry.action === "delete" ? "red" : entry.action === "create" ? "green" : "blue"}>{entry.action}</Badge>
              </Table.Td>
              <Table.Td>{entry.side}</Table.Td>
              <Table.Td>{names.get(entry.item_id) ?? entry.item_id}</Table.Td>
              <Table.Td>{entry.sub_type || "—"}</Table.Td>
              <Table.Td>{entry.price ?? "—"}</Table.Td>
              <Table.Td>{entry.quantity ?? "—"}</Table.Td>
              <Table.Td>{entry.forced_by}</Table.Td>
              <Table.Td>{entry.reason}</Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Pagination total={pages} value={page} onChange={setPage} />
    </Stack>
  );
}
```

In `web/src/pages/live_scraper/Tabs/index.ts`, add `export * from "./DryRunLog";`.

In `web/src/pages/live_scraper/index.tsx`:
- Replace `import { ItemPanel, WishListPanel } from "./Tabs";` with:
  ```tsx
  import { DryRunLogPanel, ItemPanel, WishListPanel } from "./Tabs";
  import { TraderPanel } from "./TraderPanel";
  ```
- Add this entry after the `wish_list` entry in `tabs`:
  ```tsx
      {
        label: useTranslateForm("trader.dry_run_log.title"),
        component: (isActive: boolean) => <DryRunLogPanel isActive={isActive} />,
        id: "dry_run_log",
      },
  ```
- Replace `<Container size={"100%"}>` with:
  ```tsx
      <Container size={"100%"}>
        <TraderPanel />
  ```

In `web/src/components/Forms/Settings/Tabs/Notifications/index.tsx`, add these entries after `{ id: "on_new_trade", labelKey: "on_new_trade_title" },`:

```tsx
    { id: "on_trader_stopped", labelKey: "on_trader_stopped_title" },
    { id: "on_token_expiring", labelKey: "on_token_expiring_title" },
```

- [ ] **Step 5: English strings**

In `web/public/lang/en.json`:
- After `"on_new_trade_title": "On New Trade",`, add:

```json
            "on_trader_stopped_title": "On Trader Stopped",
            "on_token_expiring_title": "On Sign-in Expiring",
```

- Directly after the line `    "live_scraper": {` (four spaces of indent, under `"pages"`), add:

```json
      "trader": {
        "states": { "offline": "Offline", "ready": "Ready", "trading": "Trading", "stopping": "Stopping" },
        "dry_run": "Dry-run",
        "live": "Live",
        "running_since": "Running since {{at}}",
        "start": "Start",
        "stop": "Stop",
        "checklist": {
          "token_valid": "warframe.market sign-in valid",
          "ws_connected": "warframe.market websocket connected",
          "game_data_loaded": "Item list loaded",
          "helper_ok": "Helper connected (override available in dry-run until phase 4)"
        },
        "options": {
          "dry_run": "Dry-run",
          "helper_override": "Helper override",
          "delete_buy_orders_on_stop": "Delete buy orders on stop"
        },
        "last_stop": "Last stop: {{reason}} ({{at}})",
        "dry_run_log": {
          "title": "Dry-run log",
          "total": "{{total}} simulated order writes",
          "at": "Time",
          "action": "Action",
          "side": "Side",
          "item": "Item",
          "sub_type": "Rank / variant",
          "price": "Price",
          "quantity": "Qty",
          "forced_by": "Forced by",
          "reason": "Reason"
        }
      },
```

Verify:

```bash
python3 -c "import json;d=json.load(open('web/public/lang/en.json'));print(d['pages']['live_scraper']['trader']['dry_run_log']['title'], d['components'] if False else 'ok')"
git diff --stat web/public/lang/en.json
```

Expected: `Dry-run log ok` is printed and the diff shows only insertions. If the `on_new_trade_title` anchor sits under a different parent, confirm the notifications path with `grep -n on_new_trade_title web/public/lang/en.json` before editing.

- [ ] **Step 6: Check the RPC allowlist and build the web**

```bash
python3 scripts/check-rpc-commands.py
cd web && pnpm build
```

Expected: `0 missing`; `pnpm build` passes with no TypeScript errors.

- [ ] **Step 7: Commit**

```bash
git add web/src web/public/lang/en.json
git commit -m "feat(web): add trader panel, dry-run log and trader notification settings

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 14: Deploy in dry-run and accept

**Files:**
- Create: `docs/PHASE-3-ACCEPTANCE.md`

**Interfaces:**
- Consumes: everything above, deployed.
- Produces: an acceptance record. Merge only after the user's go-ahead.

- [ ] **Step 1: Run the local gate**

```bash
cargo test -p wf-market --lib && cargo test -p qf_core --lib && cargo test -p qf-server
python3 scripts/check-rpc-commands.py && (cd web && pnpm build)
```

Expected: all green.

- [ ] **Step 2: Sync and rebuild on the server**

```bash
rsync -a --delete --dry-run --itemize-changes \
  --exclude .git --exclude target --exclude web/node_modules --exclude web/dist --exclude secrets --exclude .env \
  ./ christopher@ockohome:~/stacks/quantframe-server/ | grep deleting
```

If only expected paths would be deleted, run the same `rsync` command without `--dry-run --itemize-changes`, then:

```bash
ssh christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose up -d --build && docker compose ps'
ssh christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose logs --since 10m | sed "s/\x1b\[[0-9;]*m//g" | grep -E "Trader|Collector|panic|CRITICAL" | grep -v WarframeMarket:API'
```

Expected: the container is healthy, the collector starts, there's no panic and no `Trader started` (it must not auto-start).

- [ ] **Step 3: Acceptance checks (in the browser, with the user)**

On `http://ockohome:8080/live_scraper`:
1. The Trader panel shows **Offline** or **Ready**, never Trading, right after deploy. The **Dry-run** badge is on.
2. With the helper override off, the "Helper connected" item is ✕ and Start is disabled.
3. Turn on the helper override. Every checklist item turns ✓, the state becomes **Ready**, and Start is enabled.
4. **Start**: the state goes to **Trading**, and the user's warframe.market status stays **invisible** (dry-run).
5. After 1–2 minutes, the Dry-run log tab shows rows for stock and wish-list items with `forced_by = global`. The user's real warframe.market orders are unchanged; check on the website.
6. Turn off the helper override. Within 5 s the trader stops with "Helper unavailable", and the last-stop line shows it.
7. Start again, then **Stop**. The last stop reads "Stop button", and a Discord message arrives if a webhook is set in Settings → Notifications → On Trader Stopped.
8. `docker compose restart`: afterwards the panel is Offline or Ready, not Trading.
9. Settings → Notifications shows the two new tabs and saves.

- [ ] **Step 4: Write the acceptance record and commit**

`docs/PHASE-3-ACCEPTANCE.md`: a table of checks 1–9 with Result and Notes, using the observed values, plus a `## Follow-ups` section. The follow-ups must include importing the user's existing desktop trading data (user request, 2026-09-14). The source is `~/.local/share/dev.kenya.quantframe/quantframeV2.sqlite`, which held `transaction` 280 rows and `stock_item` 16 rows on 2026-09-14. Rivens are out of scope. This isn't scheduled into a phase yet.

```bash
git add docs/PHASE-3-ACCEPTANCE.md
git commit -m "docs: record phase 3 dry-run acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

Ask the user whether to merge `phase-3-trader` into `main`, and update the project memory note.

---

## Self-review notes

- **Spec coverage (§11 phase 3):**
  - `PriceSource` wiring: Tasks 3, 6 and 7.
  - `OrderWriter` with `DryRunOrders` and the dry-run log: Tasks 2 and 5.
  - Lifecycle and the Start checklist with the dry-run-only helper override: Tasks 4 and 8–10, with the UI in Task 13.
  - Discord stop and expiry alerts: Task 11.
- **§5.6 coverage:**
  - Items only: riven and syndicate code is not ported.
  - `wts` fix with regression tests: Task 7.
  - Five failures, or a `Critical` error, stop the trader: Tasks 5 and 8.
- **§5.7 coverage:**
  - Every Ready condition except the real helper, which phase 4 adds: Task 10.
  - Start sequence, stop triggers and stop sequence: Task 10.
  - No auto-resume: Task 10 test, plus Task 14 check 8.
- **§9 tests:**
  - Trader golden tests for buy, sell, wish list, not-warm routing and the `wts` regression.
  - `DryRunOrders` routing, global and not-warm.
  - Lifecycle transitions and each stop trigger.
- **Deferred to phase 4:** heartbeat stop triggers (`warframe_running = false`, no heartbeat for 60 s) and removing `helper_override`.
- **Known limitation:** under global dry-run, `potential_profit` on the user's existing real orders is 0 in the knapsack. Upstream `apply_trade_info` used Quantframe API prices, which no longer exist. Orders the trader creates carry `potential_profit` in their properties.
