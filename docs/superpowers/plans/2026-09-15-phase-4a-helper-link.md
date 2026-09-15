# Phase 4a: Helper Link — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task, **inline in the main session**. The user has ruled out subagent-driven development for this project: subagents may only explore the repo or write docs. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect `qf-helper` on the gaming PC to the server. It sends a heartbeat every 10 s saying whether Warframe is running, and the server uses that to decide Ready and to stop trading. The phase 3 dry-run-only helper override goes away.

**Architecture:**
- **Server (`qf_core::helper_link`):**
  - `keys` stores device keys as SHA-256 hashes in `helper_keys`.
  - `presence` keeps the latest heartbeat in memory.
  - The trader's `Platform` gains `helper(now)`. The checklist and stop triggers read it instead of `helper_override`.
- **Web server (`qf-server`):** adds `POST /helper/heartbeat` behind a Bearer-key middleware, outside the Origin check and the session middleware.
- **Helper (`crates/qf-helper`):** a native Linux binary with a TOML config, a `/proc` scan for `Warframe.x64.exe` and a heartbeat loop.
- **Browser:**
  - The trader checklist shows "qf-helper connected" and "Warframe running".
  - A new Helper devices tab on the Live Scraper page creates and revokes keys.

**Tech Stack:** Rust (tokio, axum 0.8, sea-orm 0.12 raw SQL on SQLite, reqwest 0.12, toml 0.8, sha2 0.10), React 19 with Mantine 9 and TanStack Query 5, pnpm 11.3.0, Docker Compose on the homelab, `systemd --user` on the gaming PC.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`. Read §5.7, §5.8, §7.1, §7.3, §8, §11, §16 and the new §17 (Task 1) first.

**Split:** phase 4 is split into **4a (this plan)** and **4b trade events**. 4b covers WFCD `warframe-items`, `overrides.toml`, `qf_log_parser`, `POST /helper/trade`, `helper_events`, trade resolution and the review modal, and gets its own plan.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-4a`, branch `phase-4a-helper-link`. All paths are relative to it. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target` to reuse the build cache.
- **Dry-run stays on** (`trader_state.dry_run = 1`). Nothing in this plan turns it off; that's phase 5.
- **Trading never resumes on its own.** Presence is in memory, so after a restart the helper checklist items stay ✕ until the next heartbeat.
- **Device keys:** `qfh_` followed by 64 lowercase hex characters (32 random bytes). Only the SHA-256 hex is stored. Keys are shown once.
- **Timing:** Ready needs a heartbeat within **30 s** with `warframe_running = true`. **No heartbeat for more than 60 s** stops trading, and so does `warframe_running = false`. The helper sends a heartbeat **every 10 s**.
- **The helper routes skip the Origin check and the session middleware.** They authenticate only with `Authorization: Bearer <device_key>`.
- **RPC names** must not contain the banned substrings in `allowlist_has_no_removed_features` (`riven, auction, chat, analytics, alert, syndicate, wfgdpr, wf_inventory, live_scraper, permission, exit, calculate_tax`).
- **Tests:** run `cargo test -p qf_core --lib`, `cargo test -p qf-server` and `cargo test -p qf-helper`; never bare `--workspace`. For the web, run `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`.
- **Docker** runs on ockohome only (`ssh christopher@ockohome`, `~/stacks/quantframe-server`). The Docker image still builds only `-p qf-server`. `qf-helper` is built and installed natively on the gaming PC (this desktop, Arch Linux, Warframe under Proton).
- **Commits:** conventional commits, ending with exactly this trailer (no `Claude-Session:` line):
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```
- **Pushes:** push `phase-4a-helper-link` after each task; never push `main` from this plan.

## File Structure (end of phase 4a)

```
Cargo.toml                                              MOD  workspace member crates/qf-helper
crates/migration/src/m20260917_000001_create_helper_keys.rs      NEW  helper_keys
crates/migration/src/m20260917_000002_drop_helper_override.rs    NEW  trader_state.helper_override removed
crates/migration/src/lib.rs                             MOD  register both
crates/qf_core/Cargo.toml                               MOD  sha2
crates/qf_core/src/lib.rs                               MOD  pub mod helper_link
crates/qf_core/src/db.rs                                MOD  re-export DatabaseConnection; table test
crates/qf_core/src/helper_link/mod.rs                   NEW  module list
crates/qf_core/src/helper_link/keys.rs                  NEW  create/list/revoke/authenticate
crates/qf_core/src/helper_link/presence.rs              NEW  Heartbeat, HelperSnapshot, Presence
crates/qf_core/src/trader/lifecycle.rs                  MOD  heartbeat checklist and stop triggers
crates/qf_core/src/trader/controller.rs                 MOD  Platform::helper, TraderStatus.helper, no override
crates/qf_core/src/trader/store.rs                      MOD  no helper_override
crates/qf_core/src/trader/platform.rs                   MOD  helper() from presence
crates/qf_core/src/commands/trader.rs                   MOD  trader_set_options without helper_override
crates/qf_core/src/commands/helper_link.rs              NEW  helper_devices / helper_device_create / helper_device_revoke
crates/qf_core/src/commands/{mod.rs,rpc.rs}             MOD  allowlist
crates/qf-server/Cargo.toml                             MOD  chrono
crates/qf-server/src/auth.rs                            MOD  bearer_from_headers
crates/qf-server/src/routes.rs                          MOD  ServerState.db, /helper/heartbeat, require_device_key
crates/qf-server/src/main.rs                            MOD  db from qf_core::DATABASE
crates/qf-server/tests/http.rs                          MOD  helper route tests
crates/qf-helper/Cargo.toml                             NEW
crates/qf-helper/src/{lib.rs,config.rs,process.rs,heartbeat.rs,main.rs}  NEW
contrib/qf-helper.service                               NEW  systemd --user unit
README.md                                               MOD  helper install section
web/src/types/tauri.type.ts                             MOD  checklist, helper snapshot, devices
web/src/api/live_scraper/index.ts                       MOD  setOptions without helperOverride
web/src/api/helper_link/index.ts                        NEW
web/src/api/index.ts                                    MOD  register helper_link
web/src/pages/live_scraper/TraderPanel.tsx              MOD  helper rows, no override switch
web/src/pages/live_scraper/Tabs/HelperDevices/index.tsx NEW
web/src/pages/live_scraper/Tabs/index.ts                MOD
web/src/pages/live_scraper/index.tsx                    MOD  Helper devices tab
web/public/lang/en.json                                 MOD  strings
docs/superpowers/specs/2026-09-14-quantframe-server-design.md    MOD  §17
docs/PHASE-4A-ACCEPTANCE.md                             NEW
```

---

### Task 1: Spec amendments for phase 4a

**Files:**
- Modify: `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`
- Add: `docs/superpowers/plans/2026-09-15-phase-4a-helper-link.md` (this plan)

**Interfaces:** none (documentation). Every later task follows §17.

- [ ] **Step 1: Update the status line**

Replace:

```markdown
- **Status:** Approved 2026-09-14. Amended by §14 (phase 1), §15 (phase 2) and §16 (phase 3) planning.
```

with:

```markdown
- **Status:** Approved 2026-09-14. Amended by §14 (phase 1), §15 (phase 2), §16 (phase 3) and §17 (phase 4a) planning.
```

- [ ] **Step 2: Append §17**

```markdown

## 17. Amendments from phase 4a planning (2026-09-15)

These amendments take precedence over the earlier sections.

- **D1 — Phase 4 is split.**
  - **4a, helper link:** device keys, the heartbeat, in-game detection, the lifecycle checklist and stop triggers, and a heartbeat-only `qf-helper`. The phase 3 helper override is removed here.
  - **4b, trade events:** WFCD `warframe-items` and `overrides.toml`, the `qf_log_parser` crate, `POST /helper/trade`, `helper_events`, trade resolution and the review modal.
  - The §11 phase 4 list is covered by 4a and 4b together.
- **D2 — Names.**
  - The server module is `qf_core::helper_link` with `keys` and `presence`, because `qf_core::helper` already holds upstream utilities.
  - The helper is the crate `crates/qf-helper`: library `qf_helper`, binary `qf-helper`.
- **D3 — Device keys.**
  - A key is `qfh_` followed by 64 lowercase hex characters (32 random bytes).
  - `helper_keys` is `id, name, key_hash UNIQUE, created_at, last_seen_at, revoked_at`. `key_hash` is the SHA-256 hex of the whole key.
  - Names are 1–64 characters after trimming.
  - Revoking sets `revoked_at`; a revoked key can't be restored, and a new key is created instead. Every successful authentication updates `last_seen_at`.
  - RPC commands:
    - `helper_devices` returns every device, active first.
    - `helper_device_create { name }` returns `{ device, key }`.
    - `helper_device_revoke { id }` returns whether a key was revoked.
- **D4 — Heartbeat endpoint.**
  - `POST /helper/heartbeat` with `Authorization: Bearer <key>` and a JSON body `{ warframe_running: bool, version: string }`. It returns `204`.
  - A missing, unknown or revoked key returns `401` with `{component, message}`.
  - The route is outside the Origin check and the session middleware.
  - Presence is kept in memory: the last heartbeat wins across devices, and a server restart clears it.
- **D5 — Lifecycle.**
  - Checklist items `helper_connected` (a heartbeat within 30 s) and `warframe_running` (from the latest heartbeat) replace `helper_ok` and `helper_override`.
  - Stop triggers, first match wins: signed out, 401, websocket down for more than 60 s, then no heartbeat for more than 60 s (`helper_silent`), then `warframe_running = false` (`warframe_closed`), plus engine exits and the Stop button.
  - A heartbeat between 30 and 60 s old doesn't stop trading, but Start stays disabled.
  - `TraderStatus` gains `helper: { connected, warframe_running, seconds_since_heartbeat, last_heartbeat_at, device_name, version }`.
- **D6 — The helper override is removed.**
  - Migration `m20260917_000002` drops `trader_state.helper_override`.
  - `trader_set_options` takes `{ dryRun?, deleteBuyOrdersOnStop? }`.
  - `StopReason::HelperLost` is replaced by `HelperSilent` and `WarframeClosed`.
- **D7 — `qf-helper` behaviour.**
  - **Config:** `$XDG_CONFIG_HOME/qf-helper/qf-helper.toml`, falling back to `~/.config/qf-helper/qf-helper.toml`; `--config <path>` overrides it.
    - Unknown keys are rejected.
    - `server_url` must start with `http://` or `https://` (a trailing `/` is trimmed).
    - `device_key` must start with `qfh_`.
    - `ee_log_path` is parsed now and used in 4b.
  - **Detection:** Warframe is running when some process other than the helper has a command-line argument whose file name, after the last `/` or `\`, equals `Warframe.x64.exe`, ignoring ASCII case. Matching a whole argument's file name avoids false positives from shell commands that merely mention the name.
  - **Heartbeat:** every 10 s, with a 5 s request timeout.
    - After a `401` it waits 60 s before retrying.
    - Other failures retry on the normal 10 s schedule.
    - It logs to stdout (journald) only when the state line changes.
  - **One-shot mode:** `--once` sends one heartbeat, prints the result and exits with 0 when accepted, 1 otherwise.
- **D8 — Install.**
  - `cargo build --release -p qf-helper`, installed as `~/.local/bin/qf-helper`, run by the `systemd --user` unit `contrib/qf-helper.service`.
  - The Docker image is unchanged: it still builds only `qf-server`.
- **D9 — Screens.**
  - Helper devices is a tab on the Live Scraper page.
  - A new key is shown once in a modal, together with a ready-to-paste `qf-helper.toml`. There's no copy button, because the clipboard API isn't available on a plain-HTTP LAN origin.
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers
git commit -m "docs: add phase 4a helper link plan and spec amendments

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-4a-helper-link
```

---

### Task 2: Device keys table and store

**Files:**
- Create: `crates/migration/src/m20260917_000001_create_helper_keys.rs`
- Modify: `crates/migration/src/lib.rs`
- Modify: `crates/qf_core/Cargo.toml` (add `sha2 = "0.10"`)
- Create: `crates/qf_core/src/helper_link/mod.rs`, `crates/qf_core/src/helper_link/keys.rs`
- Modify: `crates/qf_core/src/lib.rs`, `crates/qf_core/src/db.rs`

**Interfaces:**
- Consumes: `collector::{db_err, stmt, ts}`, `collector::store::exec` (phase 2), `trader::store::tests::db()` (phase 3).
- Produces:
  - `keys::KEY_PREFIX = "qfh_"`
  - `keys::HelperDevice { id: i64, name: String, created_at: String, last_seen_at: Option<String>, revoked_at: Option<String> }` (Serialize, Clone, PartialEq)
  - `keys::CreatedDevice { device: HelperDevice, key: String }` (Serialize)
  - `keys::DeviceIdentity { id: i64, name: String }` (Clone, PartialEq)
  - `keys::hash_key(&str) -> String`, `keys::generate_key() -> Result<String, Error>`
  - Async functions, each returning `Result<_, utils::Error>`:
    - `create(conn, name: &str, now: DateTime<Utc>) -> CreatedDevice`
    - `list(conn) -> Vec<HelperDevice>`
    - `revoke(conn, id: i64, now) -> bool`
    - `authenticate(conn, key: &str, now) -> Option<DeviceIdentity>`
  - `qf_core::db::DatabaseConnection` (re-export)

- [ ] **Step 1: Write the migration**

`crates/migration/src/m20260917_000001_create_helper_keys.rs`:

```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &["CREATE TABLE IF NOT EXISTS helper_keys (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL,
        key_hash TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL,
        last_seen_at TEXT,
        revoked_at TEXT
    )"];

const DOWN: &[&str] = &["DROP TABLE IF EXISTS helper_keys"];

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

In `crates/migration/src/lib.rs`, add `mod m20260917_000001_create_helper_keys;` after `mod m20260916_000001_create_trader_tables;`, and `Box::new(m20260917_000001_create_helper_keys::Migration),` after `Box::new(m20260916_000001_create_trader_tables::Migration),`.

- [ ] **Step 2: Dependency, module list and re-export**

In `crates/qf_core/Cargo.toml` `[dependencies]`, add `sha2 = "0.10"` after `hex = "0.4"`. It is already in `Cargo.lock`.

In `crates/qf_core/src/lib.rs`, add `pub mod helper_link;` after `pub mod helper;`.

In `crates/qf_core/src/db.rs`, replace:

```rust
use service::sea_orm::{ConnectionTrait, Database, DatabaseConnection};
```

with:

```rust
pub use service::sea_orm::DatabaseConnection;
use service::sea_orm::{ConnectionTrait, Database};
```

and in its test `migrations_create_collector_tables`, add `"helper_keys",` after `"item_stats_daily",`.

`crates/qf_core/src/helper_link/mod.rs`:

```rust
//! Link to `qf-helper` on the gaming PC: device keys and heartbeat presence (spec §5.8, amendments D1–D9).

pub mod keys;
```

- [ ] **Step 3: Write `keys.rs` with its tests**

`crates/qf_core/src/helper_link/keys.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, QueryResult};
use sha2::{Digest, Sha256};
use utils::{get_location, Error};

use crate::collector::store::exec;
use crate::collector::{db_err, stmt, ts};

pub const KEY_PREFIX: &str = "qfh_";
const MAX_NAME_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HelperDevice {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_seen_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CreatedDevice {
    pub device: HelperDevice,
    /// Shown once; only its SHA-256 is stored (amendment D3).
    pub key: String,
}

/// A device whose key is valid and not revoked.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceIdentity {
    pub id: i64,
    pub name: String,
}

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

pub fn generate_key() -> Result<String, Error> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| {
        Error::new("HelperLink:Key", format!("OS random number generator unavailable: {}", e), get_location!())
    })?;
    Ok(format!("{}{}", KEY_PREFIX, hex::encode(bytes)))
}

fn device_from_row(component: &str, row: &QueryResult) -> Result<HelperDevice, Error> {
    Ok(HelperDevice {
        id: row.try_get("", "id").map_err(|e| db_err(component, e))?,
        name: row.try_get("", "name").map_err(|e| db_err(component, e))?,
        created_at: row.try_get("", "created_at").map_err(|e| db_err(component, e))?,
        last_seen_at: row.try_get("", "last_seen_at").map_err(|e| db_err(component, e))?,
        revoked_at: row.try_get("", "revoked_at").map_err(|e| db_err(component, e))?,
    })
}

pub async fn create(conn: &DatabaseConnection, name: &str, now: DateTime<Utc>) -> Result<CreatedDevice, Error> {
    const C: &str = "HelperLink:Create";
    let name = name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(Error::new(C, "Device name must be 1-64 characters", get_location!()));
    }
    let key = generate_key()?;
    let key_hash = hash_key(&key);
    exec(
        conn,
        C,
        "INSERT INTO helper_keys (name, key_hash, created_at) VALUES (?, ?, ?)",
        vec![name.into(), key_hash.clone().into(), ts(now).into()],
    )
    .await?;
    let row = conn
        .query_one(stmt(
            "SELECT id, name, created_at, last_seen_at, revoked_at FROM helper_keys WHERE key_hash = ?",
            vec![key_hash.into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .ok_or_else(|| db_err(C, "created device row is missing"))?;
    Ok(CreatedDevice { device: device_from_row(C, &row)?, key })
}

/// Active devices first, newest first within each group.
pub async fn list(conn: &DatabaseConnection) -> Result<Vec<HelperDevice>, Error> {
    const C: &str = "HelperLink:List";
    conn.query_all(stmt(
        "SELECT id, name, created_at, last_seen_at, revoked_at FROM helper_keys
         ORDER BY revoked_at IS NOT NULL, id DESC",
        vec![],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|row| device_from_row(C, row))
    .collect()
}

/// Returns false when the device doesn't exist or was already revoked.
pub async fn revoke(conn: &DatabaseConnection, id: i64, now: DateTime<Utc>) -> Result<bool, Error> {
    let changed = exec(
        conn,
        "HelperLink:Revoke",
        "UPDATE helper_keys SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL",
        vec![ts(now).into(), id.into()],
    )
    .await?;
    Ok(changed > 0)
}

/// The device for a valid, non-revoked key; updates `last_seen_at`.
pub async fn authenticate(
    conn: &DatabaseConnection,
    key: &str,
    now: DateTime<Utc>,
) -> Result<Option<DeviceIdentity>, Error> {
    const C: &str = "HelperLink:Authenticate";
    if !key.starts_with(KEY_PREFIX) {
        return Ok(None);
    }
    let Some(row) = conn
        .query_one(stmt(
            "SELECT id, name FROM helper_keys WHERE key_hash = ? AND revoked_at IS NULL",
            vec![hash_key(key).into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?
    else {
        return Ok(None);
    };
    let identity = DeviceIdentity {
        id: row.try_get("", "id").map_err(|e| db_err(C, e))?,
        name: row.try_get("", "name").map_err(|e| db_err(C, e))?,
    };
    exec(
        conn,
        C,
        "UPDATE helper_keys SET last_seen_at = ? WHERE id = ?",
        vec![ts(now).into(), identity.id.into()],
    )
    .await?;
    Ok(Some(identity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::trader::store::tests::db;

    fn at(text: &str) -> DateTime<Utc> {
        parse_ts(text).unwrap()
    }

    #[tokio::test]
    async fn create_authenticate_and_revoke() {
        let (_dir, conn) = db().await;
        let created = create(&conn, "  gaming-pc  ", at("2026-09-17T10:00:00Z")).await.unwrap();
        assert!(created.key.starts_with(KEY_PREFIX));
        assert_eq!(created.key.len(), KEY_PREFIX.len() + 64);
        assert_eq!(created.device.name, "gaming-pc");
        assert_eq!(created.device.last_seen_at, None);

        let identity = authenticate(&conn, &created.key, at("2026-09-17T10:05:00Z")).await.unwrap();
        assert_eq!(identity, Some(DeviceIdentity { id: created.device.id, name: "gaming-pc".into() }));
        assert_eq!(list(&conn).await.unwrap()[0].last_seen_at.as_deref(), Some("2026-09-17T10:05:00Z"));

        assert_eq!(authenticate(&conn, "qfh_wrong", at("2026-09-17T10:06:00Z")).await.unwrap(), None);
        assert_eq!(authenticate(&conn, "not-a-key", at("2026-09-17T10:06:00Z")).await.unwrap(), None);

        assert!(revoke(&conn, created.device.id, at("2026-09-17T11:00:00Z")).await.unwrap());
        assert!(!revoke(&conn, created.device.id, at("2026-09-17T11:01:00Z")).await.unwrap(), "already revoked");
        assert_eq!(authenticate(&conn, &created.key, at("2026-09-17T11:02:00Z")).await.unwrap(), None);
        assert_eq!(list(&conn).await.unwrap()[0].revoked_at.as_deref(), Some("2026-09-17T11:00:00Z"));
    }

    #[tokio::test]
    async fn only_the_hash_is_stored_and_active_devices_list_first() {
        let (_dir, conn) = db().await;
        let first = create(&conn, "old-pc", at("2026-09-17T10:00:00Z")).await.unwrap();
        let second = create(&conn, "gaming-pc", at("2026-09-17T10:01:00Z")).await.unwrap();
        revoke(&conn, second.device.id, at("2026-09-17T10:02:00Z")).await.unwrap();

        let stored = conn
            .query_all(stmt("SELECT key_hash FROM helper_keys", vec![]))
            .await
            .unwrap()
            .iter()
            .map(|r| r.try_get::<String>("", "key_hash").unwrap())
            .collect::<Vec<_>>();
        assert!(stored.contains(&hash_key(&first.key)));
        assert!(stored.iter().all(|h| !h.contains(&first.key) && !h.contains(&second.key)));

        let names = list(&conn).await.unwrap().into_iter().map(|d| d.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["old-pc", "gaming-pc"]);
    }

    #[tokio::test]
    async fn device_names_must_be_1_to_64_characters() {
        let (_dir, conn) = db().await;
        let now = at("2026-09-17T10:00:00Z");
        assert!(create(&conn, "   ", now).await.is_err());
        assert!(create(&conn, &"x".repeat(65), now).await.is_err());
        assert!(create(&conn, &"x".repeat(64), now).await.is_ok());
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p qf_core --lib helper_link::keys && cargo test -p qf_core --lib db::tests`
Expected: `test result: ok. 3 passed` and `test result: ok. 1 passed`.

If `QueryResult` isn't exported from `service::sea_orm`, the type is `sea_orm::QueryResult`; `service` re-exports all of `sea_orm`, so `service::sea_orm::QueryResult` should resolve.

- [ ] **Step 5: Commit**

```bash
git add crates/migration/src crates/qf_core/Cargo.toml Cargo.lock crates/qf_core/src/helper_link crates/qf_core/src/lib.rs crates/qf_core/src/db.rs
git commit -m "feat(helper): add device keys stored as sha-256 hashes

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: Heartbeat presence

**Files:**
- Create: `crates/qf_core/src/helper_link/presence.rs`
- Modify: `crates/qf_core/src/helper_link/mod.rs` (add `pub mod presence;`)

**Interfaces:**
- Consumes: `collector::{ts, parse_ts}`.
- Produces:
  - `presence::READY_WITHIN_S = 30`, `presence::SILENT_AFTER_S = 60`
  - `presence::Heartbeat { warframe_running: bool, version: String }` (Deserialize, Clone, PartialEq; `version` defaults to `""`)
  - `presence::HelperSnapshot { connected: bool, warframe_running: bool, seconds_since_heartbeat: Option<i64>, last_heartbeat_at: Option<String>, device_name: Option<String>, version: Option<String> }` (Serialize, Default, Clone, PartialEq)
  - `presence::Presence` (Default) with `record(&self, device_name: &str, heartbeat: Heartbeat, at: DateTime<Utc>)` and `snapshot(&self, now) -> HelperSnapshot`
  - `presence::get() -> &'static Presence`

- [ ] **Step 1: Write `presence.rs` with its tests**

```rust
//! Latest qf-helper heartbeat, kept in memory (spec §5.7, amendment D4). A restart clears it.

use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::collector::ts;

/// Ready needs a heartbeat at most this old.
pub const READY_WITHIN_S: i64 = 30;
/// Trading stops when the last heartbeat is older than this.
pub const SILENT_AFTER_S: i64 = 60;

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Heartbeat {
    pub warframe_running: bool,
    #[serde(default)]
    pub version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct HelperSnapshot {
    /// A heartbeat arrived within `READY_WITHIN_S`.
    pub connected: bool,
    /// As reported by the latest heartbeat; false when there has been none.
    pub warframe_running: bool,
    pub seconds_since_heartbeat: Option<i64>,
    pub last_heartbeat_at: Option<String>,
    pub device_name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
struct Last {
    at: DateTime<Utc>,
    device_name: String,
    heartbeat: Heartbeat,
}

#[derive(Default)]
pub struct Presence {
    last: Mutex<Option<Last>>,
}

static PRESENCE: OnceLock<Presence> = OnceLock::new();

pub fn get() -> &'static Presence {
    PRESENCE.get_or_init(Presence::default)
}

impl Presence {
    pub fn record(&self, device_name: &str, heartbeat: Heartbeat, at: DateTime<Utc>) {
        *self.last.lock().unwrap() = Some(Last { at, device_name: device_name.to_string(), heartbeat });
    }

    pub fn snapshot(&self, now: DateTime<Utc>) -> HelperSnapshot {
        let last = self.last.lock().unwrap();
        let Some(last) = last.as_ref() else { return HelperSnapshot::default() };
        let since = (now - last.at).num_seconds().max(0);
        HelperSnapshot {
            connected: since <= READY_WITHIN_S,
            warframe_running: last.heartbeat.warframe_running,
            seconds_since_heartbeat: Some(since),
            last_heartbeat_at: Some(ts(last.at)),
            device_name: Some(last.device_name.clone()),
            version: Some(last.heartbeat.version.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use chrono::Duration;

    fn at(seconds: i64) -> DateTime<Utc> {
        parse_ts("2026-09-17T10:00:00Z").unwrap() + Duration::seconds(seconds)
    }

    fn beat(running: bool) -> Heartbeat {
        Heartbeat { warframe_running: running, version: "0.1.0".into() }
    }

    #[test]
    fn no_heartbeat_means_not_connected() {
        assert_eq!(Presence::default().snapshot(at(0)), HelperSnapshot::default());
    }

    #[test]
    fn connected_for_thirty_seconds_after_a_heartbeat() {
        let presence = Presence::default();
        presence.record("gaming-pc", beat(true), at(0));
        let fresh = presence.snapshot(at(30));
        assert!(fresh.connected && fresh.warframe_running);
        assert_eq!(fresh.seconds_since_heartbeat, Some(30));
        assert_eq!(fresh.device_name.as_deref(), Some("gaming-pc"));
        assert_eq!(fresh.version.as_deref(), Some("0.1.0"));
        assert_eq!(fresh.last_heartbeat_at.as_deref(), Some("2026-09-17T10:00:00Z"));

        let stale = presence.snapshot(at(31));
        assert!(!stale.connected && stale.warframe_running, "stale keeps the last reported game state");
        assert_eq!(stale.seconds_since_heartbeat, Some(31));
    }

    #[test]
    fn the_latest_heartbeat_wins() {
        let presence = Presence::default();
        presence.record("gaming-pc", beat(true), at(0));
        presence.record("laptop", beat(false), at(10));
        let snap = presence.snapshot(at(12));
        assert!(snap.connected && !snap.warframe_running);
        assert_eq!(snap.device_name.as_deref(), Some("laptop"));
    }

    #[test]
    fn heartbeat_json_needs_warframe_running() {
        let ok: Heartbeat = serde_json::from_str(r#"{"warframe_running": true}"#).unwrap();
        assert_eq!(ok, Heartbeat { warframe_running: true, version: String::new() });
        assert!(serde_json::from_str::<Heartbeat>(r#"{"version": "0.1.0"}"#).is_err());
    }
}
```

In `crates/qf_core/src/helper_link/mod.rs`, add `pub mod presence;` after `pub mod keys;`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib helper_link::presence`
Expected: `test result: ok. 4 passed`.

- [ ] **Step 3: Commit**

```bash
git add crates/qf_core/src/helper_link
git commit -m "feat(helper): track the latest heartbeat in memory

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Heartbeat-driven lifecycle, override removed

**Files:**
- Create: `crates/migration/src/m20260917_000002_drop_helper_override.rs`
- Modify: `crates/migration/src/lib.rs`
- Modify: `crates/qf_core/src/trader/lifecycle.rs` (full replacement)
- Modify: `crates/qf_core/src/trader/controller.rs`, `crates/qf_core/src/trader/store.rs`, `crates/qf_core/src/trader/platform.rs`
- Modify: `crates/qf_core/src/commands/trader.rs`, `crates/qf_core/src/commands/rpc.rs`

**Interfaces:**
- Consumes: `presence::{HelperSnapshot, SILENT_AFTER_S}`, `presence::get()` (Task 3).
- Produces:
  - `Checklist { token_valid, ws_connected, game_data_loaded, helper_connected, warframe_running: bool }`
  - `StopReason::{HelperSilent, WarframeClosed}`, replacing `HelperLost`
  - `TriggerInput { signed_in, unauthorized: bool, ws_down_for_s: Option<i64>, helper_seconds_since: Option<i64>, warframe_running: bool }`
  - `Platform::helper(&self, now: DateTime<Utc>) -> HelperSnapshot`
  - `TraderStatus.helper: HelperSnapshot`
  - `TraderOptions { dry_run, delete_buy_orders_on_stop: bool, last_stop_reason, last_stop_at: Option<String> }`
  - `store::save_flags(conn, dry_run: bool, delete_buy_orders_on_stop: bool)`
  - `TraderController::set_options(dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool>)`
  - `commands::trader::trader_set_options(dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool>)`

- [ ] **Step 1: Migration that drops the column**

`crates/migration/src/m20260917_000002_drop_helper_override.rs`:

```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &["ALTER TABLE trader_state DROP COLUMN helper_override"];

const DOWN: &[&str] = &["ALTER TABLE trader_state ADD COLUMN helper_override INTEGER NOT NULL DEFAULT 0"];

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

Register it in `crates/migration/src/lib.rs` after the `m20260917_000001_create_helper_keys` lines (both `mod` and `Box::new`). SQLite in the image is 3.44, which supports `DROP COLUMN` (3.35+).

- [ ] **Step 2: Replace `lifecycle.rs`**

`crates/qf_core/src/trader/lifecycle.rs` becomes:

```rust
//! Pure lifecycle rules: readiness checklist and stop triggers (spec §5.7, amendments C7, D5).

use serde::Serialize;

use crate::helper_link::presence::SILENT_AFTER_S;

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
    /// A qf-helper heartbeat arrived within `READY_WITHIN_S`.
    pub helper_connected: bool,
    /// The latest heartbeat reported Warframe running.
    pub warframe_running: bool,
}

impl Checklist {
    pub fn ready(&self) -> bool {
        self.token_valid && self.ws_connected && self.game_data_loaded && self.helper_connected && self.warframe_running
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
pub enum StopReason {
    UserStop,
    SignedOut,
    Unauthorized,
    WebsocketDown,
    HelperSilent,
    WarframeClosed,
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
            StopReason::HelperSilent => format!("No qf-helper heartbeat for more than {} s", SILENT_AFTER_S),
            StopReason::WarframeClosed => "Warframe closed on the gaming PC".into(),
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
    /// `None` when no heartbeat has arrived since the server started.
    pub helper_seconds_since: Option<i64>,
    pub warframe_running: bool,
}

/// First matching stop trigger while trading. Engine exits are handled by the controller.
pub fn stop_trigger(input: &TriggerInput) -> Option<StopReason> {
    if !input.signed_in {
        Some(StopReason::SignedOut)
    } else if input.unauthorized {
        Some(StopReason::Unauthorized)
    } else if input.ws_down_for_s.is_some_and(|s| s > WS_DOWN_LIMIT_S) {
        Some(StopReason::WebsocketDown)
    } else if input.helper_seconds_since.is_none_or(|s| s > SILENT_AFTER_S) {
        Some(StopReason::HelperSilent)
    } else if !input.warframe_running {
        Some(StopReason::WarframeClosed)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> TriggerInput {
        TriggerInput {
            signed_in: true,
            unauthorized: false,
            ws_down_for_s: None,
            helper_seconds_since: Some(5),
            warframe_running: true,
        }
    }

    #[test]
    fn ready_needs_every_checklist_item() {
        let all = Checklist { token_valid: true, ws_connected: true, game_data_loaded: true, helper_connected: true, warframe_running: true };
        assert!(all.ready());
        for broken in [
            Checklist { token_valid: false, ..all.clone() },
            Checklist { ws_connected: false, ..all.clone() },
            Checklist { game_data_loaded: false, ..all.clone() },
            Checklist { helper_connected: false, ..all.clone() },
            Checklist { warframe_running: false, ..all.clone() },
        ] {
            assert!(!broken.ready());
        }
    }

    #[test]
    fn stop_triggers_in_priority_order() {
        assert_eq!(stop_trigger(&healthy()), None);
        assert_eq!(stop_trigger(&TriggerInput { signed_in: false, unauthorized: true, ..healthy() }), Some(StopReason::SignedOut));
        assert_eq!(stop_trigger(&TriggerInput { unauthorized: true, ws_down_for_s: Some(99), ..healthy() }), Some(StopReason::Unauthorized));
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(60), ..healthy() }), None, "60 s is allowed");
        assert_eq!(stop_trigger(&TriggerInput { ws_down_for_s: Some(61), helper_seconds_since: None, ..healthy() }), Some(StopReason::WebsocketDown));
        assert_eq!(stop_trigger(&TriggerInput { helper_seconds_since: None, ..healthy() }), Some(StopReason::HelperSilent));
        assert_eq!(stop_trigger(&TriggerInput { helper_seconds_since: Some(60), ..healthy() }), None, "a heartbeat 60 s old is allowed");
        assert_eq!(stop_trigger(&TriggerInput { helper_seconds_since: Some(61), warframe_running: false, ..healthy() }), Some(StopReason::HelperSilent));
        assert_eq!(stop_trigger(&TriggerInput { warframe_running: false, ..healthy() }), Some(StopReason::WarframeClosed));
    }

    #[test]
    fn stop_reasons_serialize_with_kind_and_detail() {
        assert_eq!(serde_json::to_value(StopReason::OrderFailures(5)).unwrap(), serde_json::json!({"kind": "order_failures", "detail": 5}));
        assert_eq!(serde_json::to_value(StopReason::UserStop).unwrap(), serde_json::json!({"kind": "user_stop"}));
        assert_eq!(serde_json::to_value(StopReason::WarframeClosed).unwrap(), serde_json::json!({"kind": "warframe_closed"}));
        assert_eq!(StopReason::HelperSilent.describe(), "No qf-helper heartbeat for more than 60 s");
    }
}
```

- [ ] **Step 3: Store without the override**

In `crates/qf_core/src/trader/store.rs`:

- In `TraderOptions`, delete the line `    pub helper_override: bool,`.
- In `load_options`, replace `"SELECT dry_run, delete_buy_orders_on_stop, helper_override, last_stop_reason, last_stop_at` with `"SELECT dry_run, delete_buy_orders_on_stop, last_stop_reason, last_stop_at`, and delete the line `        helper_override: row.try_get::<i64>("", "helper_override").map_err(|e| db_err(C, e))? != 0,`.
- Replace the whole `save_flags` function with:

```rust
pub async fn save_flags(conn: &DatabaseConnection, dry_run: bool, delete_buy_orders_on_stop: bool) -> Result<(), Error> {
    exec(
        conn,
        "Trader:SaveFlags",
        "UPDATE trader_state SET dry_run = ?, delete_buy_orders_on_stop = ? WHERE id = 1",
        vec![(dry_run as i64).into(), (delete_buy_orders_on_stop as i64).into()],
    )
    .await
    .map(|_| ())
}
```

- In the test `options_default_to_dry_run_and_persist`, replace:

```rust
            TraderOptions { dry_run: true, delete_buy_orders_on_stop: false, helper_override: false, last_stop_reason: None, last_stop_at: None }
        );
        save_flags(&conn, false, true, true).await.unwrap();
```

with:

```rust
            TraderOptions { dry_run: true, delete_buy_orders_on_stop: false, last_stop_reason: None, last_stop_at: None }
        );
        save_flags(&conn, false, true).await.unwrap();
```

and replace `assert!(!options.dry_run && options.delete_buy_orders_on_stop && options.helper_override);` with `assert!(!options.dry_run && options.delete_buy_orders_on_stop);`.

- [ ] **Step 4: Controller reads the helper**

In `crates/qf_core/src/trader/controller.rs`:

Replace:

```rust
use super::lifecycle::{helper_ok, stop_trigger, Checklist, LifecycleState, StopReason, TriggerInput};
use super::session::SessionSnapshot;
use super::store::{self, TraderOptions};
use crate::collector::ts;
```

with:

```rust
use super::lifecycle::{stop_trigger, Checklist, LifecycleState, StopReason, TriggerInput};
use super::session::SessionSnapshot;
use super::store::{self, TraderOptions};
use crate::collector::ts;
use crate::helper_link::presence::HelperSnapshot;
```

Replace:

```rust
    fn session(&self, now: DateTime<Utc>) -> SessionSnapshot;
    fn game_data_loaded(&self) -> bool;
```

with:

```rust
    fn session(&self, now: DateTime<Utc>) -> SessionSnapshot;
    fn helper(&self, now: DateTime<Utc>) -> HelperSnapshot;
    fn game_data_loaded(&self) -> bool;
```

In `TraderStatus`, replace:

```rust
    pub session: SessionSnapshot,
    pub running_since: Option<String>,
```

with:

```rust
    pub session: SessionSnapshot,
    pub helper: HelperSnapshot,
    pub running_since: Option<String>,
```

Replace the three functions `checklist`, `status_of` and `idle_state` (from `    fn checklist(&self, session: &SessionSnapshot, options: &TraderOptions) -> Checklist {` through the end of `idle_state`) with:

```rust
    fn checklist(&self, session: &SessionSnapshot, helper: &HelperSnapshot) -> Checklist {
        Checklist {
            token_valid: session.token_valid,
            ws_connected: session.ws_connected,
            game_data_loaded: self.platform.game_data_loaded(),
            helper_connected: helper.connected,
            warframe_running: helper.warframe_running,
        }
    }

    fn status_of(&self, inner: &Inner, now: DateTime<Utc>) -> TraderStatus {
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        TraderStatus {
            state: inner.state,
            checklist: self.checklist(&session, &helper),
            options: inner.options.clone(),
            session,
            helper,
            running_since: inner.running.as_ref().map(|r| ts(r.started_at)),
            running_dry_run: inner.running.as_ref().map(|r| r.dry_run),
        }
    }

    fn idle_state(&self, now: DateTime<Utc>) -> LifecycleState {
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        if self.checklist(&session, &helper).ready() { LifecycleState::Ready } else { LifecycleState::Offline }
    }
```

In `start`, replace:

```rust
        let session = self.platform.session(now);
        if !self.checklist(&session, &inner.options).ready() {
```

with:

```rust
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        if !self.checklist(&session, &helper).ready() {
```

In `finish`, replace `        inner.state = self.idle_state(inner, now);` with `        inner.state = self.idle_state(now);`.

In `tick`, replace:

```rust
        let session = self.platform.session(now);
        let checklist = self.checklist(&session, &inner.options);
```

with:

```rust
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        let checklist = self.checklist(&session, &helper);
```

and replace:

```rust
            ws_down_for_s: session.ws_down_for_s,
            helper_ok: checklist.helper_ok,
        });
```

with:

```rust
            ws_down_for_s: session.ws_down_for_s,
            helper_seconds_since: helper.seconds_since_heartbeat,
            warframe_running: helper.warframe_running,
        });
```

Replace the whole `set_options` function with:

```rust
    pub async fn set_options(
        &self,
        dry_run: Option<bool>,
        delete_buy_orders_on_stop: Option<bool>,
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
        store::save_flags(&self.conn, next.dry_run, next.delete_buy_orders_on_stop).await?;
        inner.options = next.clone();
        self.platform.broadcast(&self.status_of(&inner, Utc::now()));
        Ok(next)
    }
```

Replace the whole test module (from `#[cfg(test)]` to the end of the file) with:

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
        helper: StdMutex<HelperSnapshot>,
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
        fn helper(&self, _now: DateTime<Utc>) -> HelperSnapshot {
            self.helper.lock().unwrap().clone()
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

    fn healthy_helper() -> HelperSnapshot {
        HelperSnapshot { connected: true, warframe_running: true, seconds_since_heartbeat: Some(3), ..Default::default() }
    }

    /// Ready in dry-run with a fresh heartbeat from a running game.
    async fn ready_controller() -> (tempfile::TempDir, Arc<Fake>, TraderController) {
        let (dir, conn) = crate::trader::store::tests::db().await;
        store::save_flags(&conn, true, true).await.unwrap();
        let fake = Arc::new(Fake::default());
        *fake.session.lock().unwrap() = healthy_session();
        *fake.helper.lock().unwrap() = healthy_helper();
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
    async fn without_a_heartbeat_the_trader_stays_offline() {
        let (_dir, fake, controller) = ready_controller().await;
        *fake.helper.lock().unwrap() = HelperSnapshot::default();
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        let status = controller.status(now()).await;
        assert_eq!(status.state, LifecycleState::Offline);
        assert!(!status.checklist.helper_connected && !status.checklist.warframe_running);
        assert!(controller.start(now()).await.is_err());
    }

    #[tokio::test]
    async fn start_is_refused_until_the_checklist_passes() {
        let (_dir, fake, controller) = ready_controller().await;
        fake.session.lock().unwrap().ws_connected = false;
        assert!(controller.start(now()).await.is_err());
        fake.session.lock().unwrap().ws_connected = true;
        fake.helper.lock().unwrap().warframe_running = false;
        let status = controller.status(now()).await;
        assert!(status.checklist.helper_connected && !status.checklist.warframe_running);
        assert!(controller.start(now()).await.is_err());
        fake.helper.lock().unwrap().warframe_running = true;
        assert_eq!(controller.start(now()).await.unwrap().state, LifecycleState::Trading);
    }

    #[tokio::test]
    async fn dry_run_start_and_stop_follow_the_sequence() {
        let (_dir, fake, controller) = ready_controller().await;
        let status = controller.start(now()).await.unwrap();
        assert_eq!(status.state, LifecycleState::Trading);
        assert_eq!(status.running_dry_run, Some(true));
        assert_eq!(status.helper, healthy_helper());
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
    async fn helper_problems_stop_trading() {
        for (edit, expected) in [
            (Box::new(|h: &mut HelperSnapshot| h.warframe_running = false) as Box<dyn Fn(&mut HelperSnapshot)>, StopReason::WarframeClosed),
            (Box::new(|h: &mut HelperSnapshot| { h.connected = false; h.seconds_since_heartbeat = Some(61) }), StopReason::HelperSilent),
        ] {
            let (_dir, fake, controller) = ready_controller().await;
            controller.start(now()).await.unwrap();
            edit(&mut fake.helper.lock().unwrap());
            assert_eq!(controller.tick(now()).await.unwrap(), Some(expected.clone()));
            assert_ne!(controller.status(now()).await.state, LifecycleState::Trading);
        }
    }

    #[tokio::test]
    async fn a_heartbeat_under_a_minute_old_keeps_trading() {
        let (_dir, fake, controller) = ready_controller().await;
        controller.start(now()).await.unwrap();
        {
            let mut helper = fake.helper.lock().unwrap();
            helper.connected = false;
            helper.seconds_since_heartbeat = Some(45);
        }
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Trading);
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
        assert!(controller.set_options(Some(false), None).await.is_err());
        assert!(controller.set_options(None, Some(false)).await.is_ok());
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

- [ ] **Step 5: Live platform and RPC**

In `crates/qf_core/src/trader/platform.rs`, add `use crate::helper_link::presence::{self, HelperSnapshot};` after `use crate::collector::ts;`, and after the `session` method add:

```rust
    fn helper(&self, now: DateTime<Utc>) -> HelperSnapshot {
        presence::get().snapshot(now)
    }
```

In `crates/qf_core/src/commands/trader.rs`, replace the whole `trader_set_options` function with:

```rust
pub async fn trader_set_options(
    dry_run: Option<bool>,
    delete_buy_orders_on_stop: Option<bool>,
) -> Result<TraderOptions, Error> {
    controller()?.set_options(dry_run, delete_buy_orders_on_stop).await
}
```

In `crates/qf_core/src/commands/rpc.rs`, replace:

```rust
    trader_set_options => trader::trader_set_options { dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool>, helper_override: Option<bool> },
```

with:

```rust
    trader_set_options => trader::trader_set_options { dry_run: Option<bool>, delete_buy_orders_on_stop: Option<bool> },
```

- [ ] **Step 6: Run all qf_core tests**

Run: `cargo test -p qf_core --lib`
Expected: all pass. Also check that `grep -rn helper_override crates/qf_core/src` finds nothing, and that `grep -rn "helper_ok\|HelperLost" crates/` finds nothing.

- [ ] **Step 7: Commit**

```bash
git add crates/migration/src crates/qf_core/src
git commit -m "feat(trader): gate ready and stop trading on qf-helper heartbeats

Removes the phase 3 dry-run-only helper override.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: Device key RPC commands

**Files:**
- Create: `crates/qf_core/src/commands/helper_link.rs`
- Modify: `crates/qf_core/src/commands/mod.rs`, `crates/qf_core/src/commands/rpc.rs`

**Interfaces:**
- Consumes: `keys::{create, list, revoke, HelperDevice, CreatedDevice}` (Task 2).
- Produces RPC commands:
  - `helper_devices {}` → `HelperDevice[]`
  - `helper_device_create { name }` → `CreatedDevice`
  - `helper_device_revoke { id }` → `bool`

- [ ] **Step 1: Write the commands**

`crates/qf_core/src/commands/helper_link.rs`:

```rust
use chrono::Utc;
use utils::{get_location, Error};

use crate::db::DatabaseConnection;
use crate::helper_link::keys::{self, CreatedDevice, HelperDevice};
use crate::DATABASE;

fn conn() -> Result<&'static DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("HelperLink:Rpc", "Database is not ready", get_location!()))
}

pub async fn helper_devices() -> Result<Vec<HelperDevice>, Error> {
    keys::list(conn()?).await
}

pub async fn helper_device_create(name: String) -> Result<CreatedDevice, Error> {
    keys::create(conn()?, &name, Utc::now()).await
}

pub async fn helper_device_revoke(id: i64) -> Result<bool, Error> {
    keys::revoke(conn()?, id, Utc::now()).await
}
```

In `crates/qf_core/src/commands/mod.rs`, add `pub mod helper_link;` after `pub mod handlers;`.

In `crates/qf_core/src/commands/rpc.rs`, add after the `trader_interesting_items => ...` line:

```rust
    helper_devices => helper_link::helper_devices {},
    helper_device_create => helper_link::helper_device_create { name: String },
    helper_device_revoke => helper_link::helper_device_revoke { id: i64 },
```

and add to its test module:

```rust
    #[tokio::test]
    async fn helper_device_commands_are_routable_and_validate_args() {
        for name in ["helper_devices", "helper_device_create", "helper_device_revoke"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("helper_device_create", json!({})).await.unwrap().is_err(), "name is required");
        assert!(dispatch("helper_device_revoke", json!({"id": "one"})).await.unwrap().is_err(), "id must be a number");
    }
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p qf_core --lib commands::rpc`
Expected: all pass, including `allowlist_has_no_removed_features`.

- [ ] **Step 3: Commit**

```bash
git add crates/qf_core/src/commands
git commit -m "feat(core): expose helper device keys over rpc

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: `POST /helper/heartbeat`

**Files:**
- Modify: `crates/qf-server/Cargo.toml` (add `chrono = "0.4"`)
- Modify: `crates/qf-server/src/auth.rs`, `crates/qf-server/src/routes.rs`, `crates/qf-server/src/main.rs`
- Modify: `crates/qf-server/tests/http.rs`

**Interfaces:**
- Consumes: `keys::authenticate`, `keys::DeviceIdentity` (Task 2); `presence::{get, Heartbeat}` (Task 3); `qf_core::db::DatabaseConnection`.
- Produces:
  - `auth::bearer_from_headers(&HeaderMap) -> Option<String>`
  - `ServerState.db: Option<qf_core::db::DatabaseConnection>`
  - Route `POST /helper/heartbeat` → 204 / 401 / 422 / 503

- [ ] **Step 1: Bearer parser**

Append to `crates/qf-server/src/auth.rs`:

```rust
/// The token from `Authorization: Bearer <token>` (scheme is case-insensitive).
pub fn bearer_from_headers(headers: &HeaderMap) -> Option<String> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then(|| token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_tokens_are_read_from_the_authorization_header() {
        let mut headers = HeaderMap::new();
        assert_eq!(bearer_from_headers(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer qfh_abc".parse().unwrap());
        assert_eq!(bearer_from_headers(&headers).as_deref(), Some("qfh_abc"));
        headers.insert(header::AUTHORIZATION, "bearer  qfh_abc ".parse().unwrap());
        assert_eq!(bearer_from_headers(&headers).as_deref(), Some("qfh_abc"));
        headers.insert(header::AUTHORIZATION, "Basic qfh_abc".parse().unwrap());
        assert_eq!(bearer_from_headers(&headers), None);
        headers.insert(header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert_eq!(bearer_from_headers(&headers), None);
    }
}
```

- [ ] **Step 2: Route and middleware**

In `crates/qf-server/Cargo.toml` `[dependencies]`, add `chrono = "0.4"` after `hex = "0.4"`.

In `crates/qf-server/src/routes.rs`:

Replace:

```rust
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Request, State,
    },
```

with:

```rust
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Extension, Path, Request, State,
    },
```

Replace:

```rust
use crate::auth::{cleared_cookie, session_cookie, token_from_headers, LoginLimiter, Sessions};
```

with:

```rust
use chrono::Utc;
use qf_core::helper_link::{
    keys::{self, DeviceIdentity},
    presence::{self, Heartbeat},
};

use crate::auth::{bearer_from_headers, cleared_cookie, session_cookie, token_from_headers, LoginLimiter, Sessions};
```

In `ServerState`, add after `    pub data_dir: PathBuf,`:

```rust
    /// `None` only in tests that don't exercise the helper routes.
    pub db: Option<qf_core::db::DatabaseConnection>,
```

Replace the body of `router` from `    Router::new()` to the end of the function with:

```rust
    // The helper authenticates with a device key, so it sits outside the Origin check and sessions (amendment D4).
    let helper = Router::new()
        .route("/helper/heartbeat", post(helper_heartbeat))
        .layer(middleware::from_fn_with_state(state.clone(), require_device_key));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
        .merge(protected)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        .merge(helper)
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state)
}

async fn require_device_key(State(state): State<ServerState>, mut req: Request, next: Next) -> Response {
    let Some(db) = state.db.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Database not ready").into_response();
    };
    let unauthorized = || {
        (StatusCode::UNAUTHORIZED, Json(json!({"component": "Helper", "message": "Invalid or revoked device key"})))
            .into_response()
    };
    let Some(key) = bearer_from_headers(req.headers()) else { return unauthorized() };
    match keys::authenticate(db, &key, Utc::now()).await {
        Ok(Some(device)) => {
            req.extensions_mut().insert(device);
            next.run(req).await
        }
        Ok(None) => unauthorized(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response(),
    }
}

async fn helper_heartbeat(Extension(device): Extension<DeviceIdentity>, Json(heartbeat): Json<Heartbeat>) -> StatusCode {
    presence::get().record(&device.name, heartbeat, Utc::now());
    StatusCode::NO_CONTENT
}
```

In `crates/qf-server/src/main.rs`, add after `        data_dir: cfg.data_dir.clone(),`:

```rust
        db: qf_core::DATABASE.get().cloned(),
```

- [ ] **Step 3: Integration tests**

In `crates/qf-server/tests/http.rs`:

Replace:

```rust
use http_body_util::BodyExt;
use qf_server::{
```

with:

```rust
use chrono::Utc;
use http_body_util::BodyExt;
use qf_core::helper_link::{keys, presence};
use qf_server::{
```

In `app()`, add after `        data_dir: dir.path().to_path_buf(),`:

```rust
        db: None,
```

Append:

```rust
async fn helper_app() -> (Router, ServerState, tempfile::TempDir) {
    let (_, mut state, dir) = app();
    state.db = Some(qf_core::db::connect(dir.path()).await.unwrap());
    (router(state.clone()), state, dir)
}

fn heartbeat(key: Option<&str>, body: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/helper/heartbeat")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        req = req.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    req.body(Body::from(body.to_string())).unwrap()
}

const BEAT: &str = r#"{"warframe_running": true, "version": "0.1.0"}"#;

#[tokio::test]
async fn heartbeat_requires_a_valid_device_key() {
    let (app, _, _dir) = helper_app().await;
    let missing = app.clone().oneshot(heartbeat(None, BEAT)).await.unwrap();
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);
    let wrong = app.oneshot(heartbeat(Some("qfh_0000"), BEAT)).await.unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn heartbeat_needs_no_origin_or_session_and_records_presence() {
    let (app, state, _dir) = helper_app().await;
    let db = state.db.as_ref().unwrap();
    let created = keys::create(db, "http-test-pc", Utc::now()).await.unwrap();

    let res = app.oneshot(heartbeat(Some(&created.key), BEAT)).await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let snap = presence::get().snapshot(Utc::now());
    assert!(snap.connected && snap.warframe_running);
    assert_eq!(snap.device_name.as_deref(), Some("http-test-pc"));
    assert!(keys::list(db).await.unwrap()[0].last_seen_at.is_some());
}

#[tokio::test]
async fn revoked_keys_are_rejected() {
    let (app, state, _dir) = helper_app().await;
    let db = state.db.as_ref().unwrap();
    let created = keys::create(db, "revoked-pc", Utc::now()).await.unwrap();
    keys::revoke(db, created.device.id, Utc::now()).await.unwrap();
    let res = app.oneshot(heartbeat(Some(&created.key), BEAT)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn heartbeat_body_must_say_whether_warframe_is_running() {
    let (app, state, _dir) = helper_app().await;
    let created = keys::create(state.db.as_ref().unwrap(), "bad-body-pc", Utc::now()).await.unwrap();
    let res = app.oneshot(heartbeat(Some(&created.key), r#"{"version": "0.1.0"}"#)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
```

Only one test records presence successfully, so the shared `presence::get()` global doesn't make the tests race.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p qf-server`
Expected: all pass (3 unit, 11 integration).

- [ ] **Step 5: Commit**

```bash
git add crates/qf-server Cargo.lock
git commit -m "feat(server): accept qf-helper heartbeats authenticated by device key

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 7: `qf-helper` crate: config and Warframe detection

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/qf-helper/Cargo.toml`, `crates/qf-helper/src/lib.rs`, `crates/qf-helper/src/config.rs`, `crates/qf-helper/src/process.rs`
- Create: `crates/qf-helper/src/main.rs` (placeholder that Task 8 replaces)

**Interfaces:**
- Produces:
  - `config::DEFAULT_EE_LOG` (relative to `$HOME`)
  - `config::Config { server_url: String, device_key: String, ee_log_path: PathBuf }` with:
    - `Config::parse(text: &str, home: &Path) -> Result<Config, String>`
    - `Config::load(path: &Path, home: &Path) -> Result<Config, String>`
  - `config::default_config_path(xdg_config_home: Option<&str>, home: &Path) -> PathBuf`
  - `process::WARFRAME_EXE = "Warframe.x64.exe"`
  - `process::is_warframe_cmdline(&[u8]) -> bool`
  - `process::warframe_running(proc_root: &Path, own_pid: u32) -> bool`

- [ ] **Step 1: Crate skeleton**

In the root `Cargo.toml`, add `"crates/qf-helper"` to the end of `members`.

`crates/qf-helper/Cargo.toml`:

```toml
[package]
name = "qf-helper"
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"

[lib]
name = "qf_helper"
path = "src/lib.rs"

[[bin]]
name = "qf-helper"
path = "src/main.rs"

[dependencies]
reqwest = { version = "0.12.19", features = ["json"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.45.1", features = ["macros", "rt-multi-thread", "time", "signal"] }
toml = "0.8"

[dev-dependencies]
axum = "0.8"
tempfile = "3"
tokio = { version = "1.45.1", features = ["full"] }
```

`crates/qf-helper/src/lib.rs`:

```rust
//! qf-helper: tells quantframe-server whether Warframe is running (spec §5.8, amendment D7).

pub mod config;
pub mod process;
```

`crates/qf-helper/src/main.rs` (placeholder until Task 8):

```rust
fn main() {
    println!("qf-helper {}", env!("CARGO_PKG_VERSION"));
}
```

- [ ] **Step 2: Write `config.rs` with its tests**

```rust
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Warframe's EE.log under Proton (Steam app 230410), relative to `$HOME`.
pub const DEFAULT_EE_LOG: &str =
    ".local/share/Steam/steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    server_url: String,
    device_key: String,
    ee_log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub server_url: String,
    pub device_key: String,
    /// Read in phase 4b; parsed now so the config file doesn't change.
    pub ee_log_path: PathBuf,
}

impl Config {
    pub fn parse(text: &str, home: &Path) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| format!("Invalid qf-helper.toml: {e}"))?;
        let server_url = raw.server_url.trim().trim_end_matches('/').to_string();
        if !(server_url.starts_with("http://") || server_url.starts_with("https://")) {
            return Err("server_url must start with http:// or https://".into());
        }
        let device_key = raw.device_key.trim().to_string();
        if !device_key.starts_with("qfh_") {
            return Err("device_key must be a key created in the web UI (it starts with qfh_)".into());
        }
        Ok(Self { server_url, device_key, ee_log_path: raw.ee_log_path.unwrap_or_else(|| home.join(DEFAULT_EE_LOG)) })
    }

    pub fn load(path: &Path, home: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        Self::parse(&text, home)
    }
}

/// `$XDG_CONFIG_HOME/qf-helper/qf-helper.toml`, or `~/.config/qf-helper/qf-helper.toml`.
pub fn default_config_path(xdg_config_home: Option<&str>, home: &Path) -> PathBuf {
    let base = match xdg_config_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".config"),
    };
    base.join("qf-helper").join("qf-helper.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/player";

    #[test]
    fn minimal_config_uses_the_proton_log_path() {
        let config = Config::parse("server_url = \"http://ockohome:8080/\"\ndevice_key = \"qfh_abc\"\n", Path::new(HOME)).unwrap();
        assert_eq!(config.server_url, "http://ockohome:8080");
        assert_eq!(config.device_key, "qfh_abc");
        assert_eq!(config.ee_log_path, Path::new(HOME).join(DEFAULT_EE_LOG));
    }

    #[test]
    fn explicit_log_path_is_kept() {
        let text = "server_url = \"https://qf.lan\"\ndevice_key = \"qfh_abc\"\nee_log_path = \"/games/EE.log\"\n";
        assert_eq!(Config::parse(text, Path::new(HOME)).unwrap().ee_log_path, PathBuf::from("/games/EE.log"));
    }

    #[test]
    fn invalid_configs_are_rejected_with_a_reason() {
        let home = Path::new(HOME);
        assert!(Config::parse("server_url = \"ockohome:8080\"\ndevice_key = \"qfh_abc\"\n", home).unwrap_err().contains("server_url"));
        assert!(Config::parse("server_url = \"http://ockohome:8080\"\ndevice_key = \"abc\"\n", home).unwrap_err().contains("device_key"));
        assert!(Config::parse("server_url = \"http://ockohome:8080\"\n", home).is_err(), "device_key is required");
        assert!(
            Config::parse("server_url = \"http://ockohome:8080\"\ndevice_key = \"qfh_abc\"\ndevice_kye = \"x\"\n", home).is_err(),
            "typos in key names are rejected"
        );
    }

    #[test]
    fn config_path_prefers_xdg_config_home() {
        let home = Path::new(HOME);
        assert_eq!(default_config_path(Some("/cfg"), home), PathBuf::from("/cfg/qf-helper/qf-helper.toml"));
        assert_eq!(default_config_path(Some(""), home), PathBuf::from("/home/player/.config/qf-helper/qf-helper.toml"));
        assert_eq!(default_config_path(None, home), PathBuf::from("/home/player/.config/qf-helper/qf-helper.toml"));
    }
}
```

- [ ] **Step 3: Write `process.rs` with its tests**

```rust
use std::path::Path;

pub const WARFRAME_EXE: &str = "Warframe.x64.exe";

/// True when one NUL-separated argument's file name (after the last `/` or `\`) is `Warframe.x64.exe`,
/// ignoring ASCII case. Shell commands that merely mention the name don't match (amendment D7).
pub fn is_warframe_cmdline(cmdline: &[u8]) -> bool {
    cmdline.split(|b| *b == 0).filter(|arg| !arg.is_empty()).any(|arg| {
        let arg = String::from_utf8_lossy(arg);
        arg.rsplit(|c| c == '/' || c == '\\').next().is_some_and(|name| name.eq_ignore_ascii_case(WARFRAME_EXE))
    })
}

/// Scans `<proc_root>/<pid>/cmdline` for Warframe, skipping `own_pid`.
pub fn warframe_running(proc_root: &Path, own_pid: u32) -> bool {
    let Ok(entries) = std::fs::read_dir(proc_root) else { return false };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_str().and_then(|name| name.parse::<u32>().ok()).is_some_and(|pid| pid != own_pid))
        .any(|entry| std::fs::read(entry.path().join("cmdline")).is_ok_and(|cmdline| is_warframe_cmdline(&cmdline)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<u8> {
        parts.join("\0").into_bytes()
    }

    #[test]
    fn matches_the_game_executable_under_proton_or_unix_paths() {
        assert!(is_warframe_cmdline(&args(&[r"Z:\home\player\.local\share\Steam\steamapps\common\Warframe\Downloaded\Public\Warframe.x64.exe", "-cluster:public"])));
        assert!(is_warframe_cmdline(&args(&["/usr/bin/wine64-preloader", "C:/Program Files/Warframe/warframe.x64.exe"])));
        assert!(is_warframe_cmdline(&args(&["Warframe.x64.exe"])));
    }

    #[test]
    fn mentions_and_similar_names_do_not_match() {
        assert!(!is_warframe_cmdline(&args(&["/usr/bin/bash", "-c", "pgrep -af 'Warframe.x64.exe' | head"])));
        assert!(!is_warframe_cmdline(&args(&["tail", "/tmp/Warframe.x64.exe.log"])));
        assert!(!is_warframe_cmdline(&args(&[r"Z:\Warframe\Tools\Launcher.exe"])));
        assert!(!is_warframe_cmdline(b""));
    }

    #[test]
    fn scans_proc_and_skips_its_own_process() {
        let root = tempfile::tempdir().unwrap();
        for (pid, cmdline) in [
            ("100", args(&["/usr/bin/bash", "-c", "pgrep Warframe.x64.exe"])),
            ("200", args(&[r"Z:\Warframe\Warframe.x64.exe"])),
            ("self", args(&[r"Z:\Warframe\Warframe.x64.exe"])),
        ] {
            std::fs::create_dir_all(root.path().join(pid)).unwrap();
            std::fs::write(root.path().join(pid).join("cmdline"), cmdline).unwrap();
        }
        assert!(warframe_running(root.path(), 1));
        assert!(!warframe_running(root.path(), 200), "own pid is skipped; `self` is not a pid");
        assert!(!warframe_running(&root.path().join("missing"), 1));
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p qf-helper`
Expected: `test result: ok. 7 passed`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock crates/qf-helper
git commit -m "feat(helper): add qf-helper crate with config and warframe detection

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 8: `qf-helper` heartbeat loop, service unit and install docs

**Files:**
- Create: `crates/qf-helper/src/heartbeat.rs`
- Modify: `crates/qf-helper/src/lib.rs` (add `pub mod heartbeat;`), `crates/qf-helper/src/main.rs` (replace)
- Create: `contrib/qf-helper.service`
- Modify: `README.md`

**Interfaces:**
- Consumes: `config::{Config, default_config_path}`, `process::warframe_running` (Task 7).
- Produces:
  - `heartbeat::HEARTBEAT_EVERY` (10 s), `heartbeat::REJECTED_BACKOFF` (60 s)
  - `heartbeat::Heartbeat { warframe_running: bool, version: String }` (Serialize)
  - `heartbeat::Outcome { Accepted, Rejected, Failed(String) }` with `next_delay() -> Duration`
  - `heartbeat::describe(warframe_running: bool, &Outcome) -> String`
  - `heartbeat::Client::new(server_url, device_key)`, `async Client::send(&Heartbeat) -> Outcome`

- [ ] **Step 1: Write `heartbeat.rs` with its tests**

```rust
use std::time::Duration;

use serde::Serialize;

pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
pub const REJECTED_BACKOFF: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Heartbeat {
    pub warframe_running: bool,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Accepted,
    /// 401: the device key is wrong or revoked.
    Rejected,
    Failed(String),
}

impl Outcome {
    pub fn next_delay(&self) -> Duration {
        match self {
            Outcome::Rejected => REJECTED_BACKOFF,
            _ => HEARTBEAT_EVERY,
        }
    }
}

/// One log line for the current state; the loop prints it only when it changes.
pub fn describe(warframe_running: bool, outcome: &Outcome) -> String {
    let game = if warframe_running { "yes" } else { "no" };
    match outcome {
        Outcome::Accepted => format!("Warframe running: {game}; heartbeat accepted"),
        Outcome::Rejected => format!(
            "Warframe running: {game}; device key rejected (401), retrying every {} s. Create a new key in the web UI",
            REJECTED_BACKOFF.as_secs()
        ),
        Outcome::Failed(reason) => format!("Warframe running: {game}; heartbeat failed: {reason}"),
    }
}

pub struct Client {
    http: reqwest::Client,
    url: String,
    key: String,
}

impl Client {
    pub fn new(server_url: &str, device_key: &str) -> Self {
        let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().expect("HTTP client");
        Self { http, url: format!("{server_url}/helper/heartbeat"), key: device_key.to_string() }
    }

    pub async fn send(&self, heartbeat: &Heartbeat) -> Outcome {
        match self.http.post(&self.url).bearer_auth(&self.key).json(heartbeat).send().await {
            Ok(res) if res.status().is_success() => Outcome::Accepted,
            Ok(res) if res.status() == reqwest::StatusCode::UNAUTHORIZED => Outcome::Rejected,
            Ok(res) => Outcome::Failed(format!("server returned {}", res.status())),
            Err(e) => Outcome::Failed(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{http::{HeaderMap, StatusCode}, routing::post, Json, Router};
    use std::sync::{Arc, Mutex};

    async fn mock_server() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let app = Router::new().route(
            "/helper/heartbeat",
            post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                let recorder = recorder.clone();
                async move {
                    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer qfh_good") {
                        return StatusCode::UNAUTHORIZED;
                    }
                    recorder.lock().unwrap().push(body);
                    StatusCode::NO_CONTENT
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    fn beat() -> Heartbeat {
        Heartbeat { warframe_running: true, version: "0.1.0".into() }
    }

    #[tokio::test]
    async fn accepted_heartbeats_send_the_expected_json() {
        let (url, seen) = mock_server().await;
        assert_eq!(Client::new(&url, "qfh_good").send(&beat()).await, Outcome::Accepted);
        assert_eq!(*seen.lock().unwrap(), vec![serde_json::json!({"warframe_running": true, "version": "0.1.0"})]);
    }

    #[tokio::test]
    async fn a_wrong_key_is_rejected_and_backs_off() {
        let (url, _) = mock_server().await;
        let outcome = Client::new(&url, "qfh_bad").send(&beat()).await;
        assert_eq!(outcome, Outcome::Rejected);
        assert_eq!(outcome.next_delay(), REJECTED_BACKOFF);
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_failure_on_the_normal_schedule() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let outcome = Client::new(&url, "qfh_good").send(&beat()).await;
        assert!(matches!(outcome, Outcome::Failed(_)));
        assert_eq!(outcome.next_delay(), HEARTBEAT_EVERY);
    }

    #[test]
    fn state_lines_name_the_game_and_the_outcome() {
        assert_eq!(describe(true, &Outcome::Accepted), "Warframe running: yes; heartbeat accepted");
        assert!(describe(false, &Outcome::Rejected).starts_with("Warframe running: no; device key rejected (401)"));
        assert_eq!(describe(false, &Outcome::Failed("timeout".into())), "Warframe running: no; heartbeat failed: timeout");
    }
}
```

In `crates/qf-helper/src/lib.rs`, add `pub mod heartbeat;` after `pub mod config;`.

- [ ] **Step 2: Replace `main.rs`**

```rust
use std::path::{Path, PathBuf};

use qf_helper::config::{default_config_path, Config};
use qf_helper::heartbeat::{describe, Client, Heartbeat, Outcome};
use qf_helper::process;

const USAGE: &str = "Usage: qf-helper [--config <path>] [--once]";

#[tokio::main]
async fn main() {
    let mut config_path: Option<PathBuf> = None;
    let mut once = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => match args.next() {
                Some(path) => config_path = Some(PathBuf::from(path)),
                None => exit_with(2, &format!("--config needs a path\n{USAGE}")),
            },
            "--once" => once = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => exit_with(2, &format!("Unknown argument {other}\n{USAGE}")),
        }
    }

    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let path = config_path.unwrap_or_else(|| default_config_path(std::env::var("XDG_CONFIG_HOME").ok().as_deref(), &home));
    let config = Config::load(&path, &home).unwrap_or_else(|e| exit_with(2, &e));
    println!(
        "qf-helper {} sending heartbeats to {} (config {}, EE.log {})",
        env!("CARGO_PKG_VERSION"),
        config.server_url,
        path.display(),
        config.ee_log_path.display()
    );

    let client = Client::new(&config.server_url, &config.device_key);
    let own_pid = std::process::id();
    let mut last_line = String::new();
    loop {
        let warframe_running = process::warframe_running(Path::new("/proc"), own_pid);
        let outcome = client
            .send(&Heartbeat { warframe_running, version: env!("CARGO_PKG_VERSION").to_string() })
            .await;
        let line = describe(warframe_running, &outcome);
        if once {
            println!("{line}");
            std::process::exit(if outcome == Outcome::Accepted { 0 } else { 1 });
        }
        if line != last_line {
            println!("{line}");
            last_line = line;
        }
        tokio::select! {
            _ = tokio::time::sleep(outcome.next_delay()) => {}
            _ = tokio::signal::ctrl_c() => {
                println!("qf-helper stopping");
                return;
            }
        }
    }
}

fn exit_with(code: i32, message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(code)
}
```

- [ ] **Step 3: Service unit and README**

`contrib/qf-helper.service`:

```ini
[Unit]
Description=Quantframe helper: heartbeats to quantframe-server
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=%h/.local/bin/qf-helper
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
```

Append to `README.md`:

````markdown
## qf-helper (gaming PC)

`qf-helper` runs on the PC that plays Warframe. Every 10 s it tells the server whether Warframe is running. The trader is only Ready while those heartbeats arrive, and it stops when Warframe closes or the heartbeats stop for more than 60 s.

1. In the web UI, open **Live Scraper → Helper devices**, create a device and copy the `qf-helper.toml` it shows. The key is shown only once.
2. Build and install it natively (not in Docker):
   ```bash
   cargo build --release -p qf-helper
   install -Dm755 target/release/qf-helper ~/.local/bin/qf-helper
   install -Dm600 /dev/stdin ~/.config/qf-helper/qf-helper.toml   # paste the config, then Ctrl-D
   ~/.local/bin/qf-helper --once                                   # prints the state and exits 0 when accepted
   ```
3. Run it as a user service:
   ```bash
   install -Dm644 contrib/qf-helper.service ~/.config/systemd/user/qf-helper.service
   systemctl --user daemon-reload
   systemctl --user enable --now qf-helper
   journalctl --user -u qf-helper -f
   ```

`qf-helper.toml`:

```toml
server_url = "http://ockohome:8080"
device_key = "qfh_…"
# ee_log_path = "/path/to/EE.log"   # optional; defaults to the Proton path under ~/.local/share/Steam
```
````

- [ ] **Step 4: Run the tests and a smoke run**

Run: `cargo test -p qf-helper`
Expected: `test result: ok. 11 passed`.

Run: `cargo run -p qf-helper -- --config /nonexistent.toml; echo "exit=$?"`
Expected: `Cannot read /nonexistent.toml: …` and `exit=2`.

- [ ] **Step 5: Commit**

```bash
git add crates/qf-helper contrib/qf-helper.service README.md Cargo.lock
git commit -m "feat(helper): send heartbeats from qf-helper with a systemd user unit

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 9: Helper checklist and Helper devices tab in the web UI

**Files:**
- Modify: `web/src/types/tauri.type.ts`, `web/src/api/live_scraper/index.ts`, `web/src/api/index.ts`
- Create: `web/src/api/helper_link/index.ts`
- Modify: `web/src/pages/live_scraper/TraderPanel.tsx`
- Create: `web/src/pages/live_scraper/Tabs/HelperDevices/index.tsx`
- Modify: `web/src/pages/live_scraper/Tabs/index.ts`, `web/src/pages/live_scraper/index.tsx`
- Modify: `web/public/lang/en.json`

**Interfaces:**
- Consumes: the Task 4 `TraderStatus` shape and the Task 5 RPC commands.
- Produces:
  - `TauriTypes.TraderHelperSnapshot`, `TauriTypes.HelperDevice`, `TauriTypes.HelperDeviceCreated`
  - `api.helper_link.devices()`, `create(name)`, `revoke(id)`

- [ ] **Step 1: Types**

In `web/src/types/tauri.type.ts`, replace:

```ts
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
```

with:

```ts
  export interface TraderChecklist {
    token_valid: boolean;
    ws_connected: boolean;
    game_data_loaded: boolean;
    helper_connected: boolean;
    warframe_running: boolean;
  }
  export interface TraderHelperSnapshot {
    connected: boolean;
    warframe_running: boolean;
    seconds_since_heartbeat?: number | null;
    last_heartbeat_at?: string | null;
    device_name?: string | null;
    version?: string | null;
  }
  export interface HelperDevice {
    id: number;
    name: string;
    created_at: string;
    last_seen_at?: string | null;
    revoked_at?: string | null;
  }
  export interface HelperDeviceCreated {
    device: HelperDevice;
    key: string;
  }
  export interface TraderOptions {
    dry_run: boolean;
    delete_buy_orders_on_stop: boolean;
    last_stop_reason?: string | null;
```

and replace:

```ts
    session: TraderSessionSnapshot;
    running_since?: string | null;
```

with:

```ts
    session: TraderSessionSnapshot;
    helper: TraderHelperSnapshot;
    running_since?: string | null;
```

- [ ] **Step 2: API modules**

In `web/src/api/live_scraper/index.ts`, replace:

```ts
  setOptions(options: { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean; helperOverride?: boolean }) {
```

with:

```ts
  setOptions(options: { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean }) {
```

`web/src/api/helper_link/index.ts`:

```ts
import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class HelperLinkModule {
  constructor(private readonly client: TauriClient) {}

  devices() {
    return this.client.sendInvoke<TauriTypes.HelperDevice[]>("helper_devices");
  }
  create(name: string) {
    return this.client.sendInvoke<TauriTypes.HelperDeviceCreated>("helper_device_create", { name });
  }
  revoke(id: number) {
    return this.client.sendInvoke<boolean>("helper_device_revoke", { id });
  }
}
```

In `web/src/api/index.ts`:
- Add `import { HelperLinkModule } from "./helper_link";` after `import { HandlesModule } from "./handles";`.
- Add `    this.helper_link = new HelperLinkModule(this);` after `    this.live_scraper = new LiveScraperModule(this);`.
- Add `  helper_link: HelperLinkModule;` after `  live_scraper: LiveScraperModule;`.

- [ ] **Step 3: Trader panel**

In `web/src/pages/live_scraper/TraderPanel.tsx`:

Replace:

```tsx
type OptionsInput = { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean; helperOverride?: boolean };
```

with:

```tsx
type OptionsInput = { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean };
```

Replace:

```tsx
    ["helper_ok", status.checklist.helper_ok],
  ];
```

with:

```tsx
    ["helper_connected", status.checklist.helper_connected],
    ["warframe_running", status.checklist.warframe_running],
  ];
```

Delete this block:

```tsx
          <Switch
            label={t("options.helper_override")}
            checked={status.options.helper_override}
            onChange={(e) => options.mutate({ helperOverride: e.currentTarget.checked })}
          />
```

Replace:

```tsx
        </List>
        <Group>
```

with:

```tsx
        </List>
        <Text size="sm" c="dimmed">
          {status.helper.last_heartbeat_at
            ? t("helper_line", {
                device: status.helper.device_name ?? "",
                version: status.helper.version ?? "",
                seconds: status.helper.seconds_since_heartbeat ?? 0,
              })
            : t("helper_none")}
        </Text>
        <Group>
```

- [ ] **Step 4: Helper devices tab**

`web/src/pages/live_scraper/Tabs/HelperDevices/index.tsx`:

```tsx
import api from "@api/index";
import { TauriTypes } from "$types";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Code, Group, Modal, Stack, Table, Text, TextInput } from "@mantine/core";
import { modals } from "@mantine/modals";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";

export function HelperDevicesPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.helper_devices.${key}`, context);
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [created, setCreated] = useState<TauriTypes.HelperDeviceCreated | null>(null);
  const { data: devices } = useQuery({
    queryKey: ["helper_devices"],
    queryFn: () => api.helper_link.devices(),
    refetchInterval: 15_000,
    enabled: !!isActive,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["helper_devices"] });
  const create = useMutation({
    mutationFn: (deviceName: string) => api.helper_link.create(deviceName),
    onSuccess: (result) => {
      setCreated(result);
      setName("");
    },
    onSettled: refresh,
  });
  const revoke = useMutation({ mutationFn: (id: number) => api.helper_link.revoke(id), onSettled: refresh });
  const failure = create.error ?? revoke.error;

  const confirmRevoke = (device: TauriTypes.HelperDevice) =>
    modals.openConfirmModal({
      title: t("revoke_title"),
      children: <Text size="sm">{t("revoke_message", { name: device.name })}</Text>,
      labels: { confirm: t("revoke"), cancel: t("cancel") },
      confirmProps: { color: "red" },
      onConfirm: () => revoke.mutate(device.id),
    });

  return (
    <Stack mt="md">
      <Text size="sm" c="dimmed">
        {t("description")}
      </Text>
      <Group align="flex-end">
        <TextInput label={t("name")} placeholder={t("name_placeholder")} value={name} maxLength={64} onChange={(e) => setName(e.currentTarget.value)} />
        <Button disabled={!name.trim()} loading={create.isPending} onClick={() => create.mutate(name.trim())}>
          {t("create")}
        </Button>
      </Group>
      {failure && <Alert color="red">{String((failure as any)?.message ?? failure)}</Alert>}
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("columns.name")}</Table.Th>
            <Table.Th>{t("columns.created_at")}</Table.Th>
            <Table.Th>{t("columns.last_seen_at")}</Table.Th>
            <Table.Th>{t("columns.status")}</Table.Th>
            <Table.Th />
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(devices ?? []).map((device) => (
            <Table.Tr key={device.id}>
              <Table.Td>{device.name}</Table.Td>
              <Table.Td>{device.created_at}</Table.Td>
              <Table.Td>{device.last_seen_at ?? "—"}</Table.Td>
              <Table.Td>
                {device.revoked_at ? <Badge color="gray">{t("revoked")}</Badge> : <Badge color="green">{t("active")}</Badge>}
              </Table.Td>
              <Table.Td>
                {!device.revoked_at && (
                  <Button size="xs" color="red" variant="light" onClick={() => confirmRevoke(device)}>
                    {t("revoke")}
                  </Button>
                )}
              </Table.Td>
            </Table.Tr>
          ))}
        </Table.Tbody>
      </Table>
      <Modal opened={!!created} onClose={() => setCreated(null)} title={t("created_title", { name: created?.device.name ?? "" })} size="lg">
        <Stack>
          <Alert color="yellow">{t("created_warning")}</Alert>
          <Text size="sm">{t("created_config")}</Text>
          <Code block>{`server_url = "${window.location.origin}"\ndevice_key = "${created?.key ?? ""}"`}</Code>
          <Group justify="flex-end">
            <Button onClick={() => setCreated(null)}>{t("done")}</Button>
          </Group>
        </Stack>
      </Modal>
    </Stack>
  );
}
```

In `web/src/pages/live_scraper/Tabs/index.ts`, add `export * from "./HelperDevices";` after `export * from "./DryRunLog";`.

In `web/src/pages/live_scraper/index.tsx`:

Replace:

```tsx
import { DryRunLogPanel, ItemPanel, WishListPanel } from "./Tabs";
```

with:

```tsx
import { DryRunLogPanel, HelperDevicesPanel, ItemPanel, WishListPanel } from "./Tabs";
```

Replace:

```tsx
      id: "dry_run_log",
    },
  ];
```

with:

```tsx
      id: "dry_run_log",
    },
    {
      label: useTranslateForm("helper_devices.title"),
      component: (isActive: boolean) => <HelperDevicesPanel isActive={isActive} />,
      id: "helper_devices",
    },
  ];
```

- [ ] **Step 5: English strings**

In `web/public/lang/en.json`, under `pages.live_scraper.trader`:

Replace:

```json
          "helper_ok": "Helper connected (override available in dry-run until phase 4)"
```

with:

```json
          "helper_connected": "qf-helper connected (heartbeat in the last 30 s)",
          "warframe_running": "Warframe running on the gaming PC"
```

Replace:

```json
          "dry_run": "Dry-run",
          "helper_override": "Helper override",
          "delete_buy_orders_on_stop": "Delete buy orders on stop"
```

with:

```json
          "dry_run": "Dry-run",
          "delete_buy_orders_on_stop": "Delete buy orders on stop"
```

Replace:

```json
        "last_stop": "Last stop: {{reason}} ({{at}})",
```

with:

```json
        "last_stop": "Last stop: {{reason}} ({{at}})",
        "helper_line": "Last heartbeat from {{device}} (qf-helper {{version}}) {{seconds}} s ago",
        "helper_none": "No qf-helper heartbeat since the server started",
```

Replace:

```json
          "reason": "Reason"
        }
      },
```

with:

```json
          "reason": "Reason"
        }
      },
      "helper_devices": {
        "title": "Helper devices",
        "description": "Each gaming PC running qf-helper needs its own device key. Keys are shown once; revoke a key you no longer use.",
        "name": "Device name",
        "name_placeholder": "gaming-pc",
        "create": "Create key",
        "columns": { "name": "Name", "created_at": "Created", "last_seen_at": "Last heartbeat", "status": "Status" },
        "active": "Active",
        "revoked": "Revoked",
        "revoke": "Revoke",
        "cancel": "Cancel",
        "revoke_title": "Revoke device key",
        "revoke_message": "{{name}} will be rejected on its next heartbeat. This can't be undone; create a new key instead.",
        "created_title": "Key for {{name}}",
        "created_warning": "This key is shown only once. Save it now.",
        "created_config": "Put this in ~/.config/qf-helper/qf-helper.toml on the gaming PC:",
        "done": "Done"
      },
```

Verify:

```bash
python3 -c "import json;d=json.load(open('web/public/lang/en.json'));t=d['pages']['live_scraper'];print(t['helper_devices']['title'], t['trader']['checklist']['warframe_running'], 'helper_override' in json.dumps(t))"
git diff --stat web/public/lang/en.json
```

Expected: `Helper devices Warframe running on the gaming PC False`.

- [ ] **Step 6: Check the RPC allowlist and build the web**

```bash
python3 scripts/check-rpc-commands.py
(cd web && pnpm build)
grep -rn "helper_override\|helperOverride\|helper_ok" web/src web/public/lang/en.json
```

Expected: `0 missing`; `pnpm build` (tsc + vite) passes; the grep finds nothing.

- [ ] **Step 7: Commit**

```bash
git add web/src web/public/lang/en.json
git commit -m "feat(web): show helper heartbeat checklist and manage helper device keys

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 10: Deploy, install the helper and accept

**Files:**
- Create: `docs/PHASE-4A-ACCEPTANCE.md`

**Interfaces:**
- Consumes: everything above, deployed on ockohome and installed on the gaming PC.
- Produces: an acceptance record. Merge only after the user's go-ahead.

- [ ] **Step 1: Run the local gate**

```bash
cargo test -p wf-market --lib && cargo test -p qf_core --lib && cargo test -p qf-server && cargo test -p qf-helper
python3 scripts/check-rpc-commands.py && (cd web && pnpm build)
```

Expected: all green.

- [ ] **Step 2: Sync and rebuild on the server**

```bash
rsync -a --delete --dry-run --itemize-changes \
  --exclude .git --exclude target --exclude web/node_modules --exclude web/dist --exclude secrets --exclude .env \
  ./ christopher@ockohome:~/stacks/quantframe-server/ | grep deleting
```

If only expected paths would be deleted, run the same `rsync` without `--dry-run --itemize-changes`, then:

```bash
ssh christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose up -d --build && docker compose ps'
ssh christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose logs --since 10m | sed "s/\x1b\[[0-9;]*m//g" | grep -E "Trader|Collector|panic|CRITICAL|Migrat|Db:" | grep -v WarframeMarket:API'
```

Expected: the container is healthy, the migrations apply (`Database ready`), there's no panic and no `Trader started`.

- [ ] **Step 3: Create a key and install the helper (gaming PC, with the user)**

1. The user opens `http://ockohome:8080/live_scraper` → **Helper devices**, creates `gaming-pc` and pastes the shown config into `~/.config/qf-helper/qf-helper.toml` (mode 600). The key stays out of the conversation and the repo.
2. Build and check:
   ```bash
   cargo build --release -p qf-helper
   install -Dm755 "$CARGO_TARGET_DIR/release/qf-helper" ~/.local/bin/qf-helper
   ~/.local/bin/qf-helper --once; echo "exit=$?"
   ```
   Expected with Warframe closed: `Warframe running: no; heartbeat accepted`, `exit=0`.
3. Service:
   ```bash
   install -Dm644 contrib/qf-helper.service ~/.config/systemd/user/qf-helper.service
   systemctl --user daemon-reload && systemctl --user enable --now qf-helper
   journalctl --user -u qf-helper --since "5 min ago" --no-pager
   ```

- [ ] **Step 4: Acceptance checks (in the browser, with the user)**

On `http://ockohome:8080/live_scraper`, with global dry-run on:
1. **Before any heartbeat:** "qf-helper connected" and "Warframe running" are ✕, the state is Offline, there's no Helper override switch, and the helper line says no heartbeat.
2. **After creating the device:** the key is shown once with a config snippet, and the device list shows `gaming-pc` Active.
3. **After `qf-helper --once` and the service:** "qf-helper connected" ✓, "Warframe running" ✕, the device's Last heartbeat fills in, and the helper line names `gaming-pc`.
4. **Launch Warframe:** within about 10 s "Warframe running" is ✓ and the state is **Ready**, provided sign-in, websocket and item list are ✓. Record `tr '\0' ' ' < /proc/$(pgrep -f 'Warframe.x64.exe' | head -1)/cmdline | grep -o 'Warframe.x64.exe'` as evidence of the matched argument.
5. **Start in dry-run, then quit Warframe:** within about 15 s the trader stops with "Warframe closed on the gaming PC".
6. **Relaunch, Start, then `systemctl --user stop qf-helper`:** within about 65 s the trader stops with "No qf-helper heartbeat for more than 60 s". Start the service again afterwards.
7. **Revoke `gaming-pc`:** the helper journal shows `device key rejected (401)`, and the checklist drops to ✕ within about 30 s. Then create a new key, update the config, `systemctl --user restart qf-helper`, and it's accepted again.
8. **`docker compose restart`:** the state is Offline until the next heartbeat (≤ 10 s), then Ready; never Trading.
9. **Settings still save,** and the Dry-run log tab still loads.

- [ ] **Step 5: Write the acceptance record and commit**

`docs/PHASE-4A-ACCEPTANCE.md`: a table of checks 1–9 with Result and Notes using the observed values (never the key), plus a `## Follow-ups` section that carries forward:
- the phase 4b plan
- the desktop trading-data import
- `auto_delete`
- the idle cycle cost
- the phase 3 limited check 5
- the open phase 2 time-based checks

```bash
git add docs/PHASE-4A-ACCEPTANCE.md
git commit -m "docs: record phase 4a helper link acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

Ask the user whether to merge `phase-4a-helper-link` into `main`, and update the project memory note.

---

## Self-review notes

- **Spec coverage (§11 phase 4, the 4a part):**
  - Device keys: Tasks 2, 5 and 9.
  - The `qf-helper` binary: Tasks 7 and 8.
  - Heartbeat and in-game detection: Tasks 3, 6, 7 and 8.
  - Removing the dry-run-only helper override: Tasks 4 and 9.
  - Deferred to 4b by D1: WFCD and `overrides.toml`, trade events, the review modal, `helper_events`.
- **§5.7 coverage:**
  - Ready needs a heartbeat within 30 s with `warframe_running`: Tasks 3 and 4.
  - Stop triggers `warframe_running = false` and no heartbeat for 60 s: Task 4.
  - No auto-resume, since presence is in memory: Task 4 test `without_a_heartbeat_the_trader_stays_offline`, plus Task 10 check 8.
- **§5.8 coverage:**
  - `systemd --user`: Task 8.
  - Config `server_url`, `device_key`, `ee_log_path`: Task 7.
  - `POST /helper/heartbeat` every 10 s with `{warframe_running, version}`: Tasks 6 and 8.
  - `/proc/*/cmdline` scan: Task 7.
  - Bearer auth with SHA-256 hashes, keys shown once: Tasks 2, 6 and 9.
- **§7.1 and §7.3:** the helper routes use device-key auth outside the Origin check (Task 6). `SENSITIVE_FIELDS` already masks `device_key`.
- **§8:** an invalid or revoked helper key gets 401 and the helper backs off with an error line (Tasks 6 and 8).
- **§9:** web integration tests for helper auth (Task 6).
- **Type consistency:**
  - `HelperSnapshot` field names match between Rust (Task 3), `TraderStatus.helper` (Task 4) and `TraderHelperSnapshot` (Task 9).
  - The checklist keys `helper_connected` and `warframe_running` match across Tasks 4 and 9 and the en.json keys.
  - `save_flags` and `set_options` lose the third argument everywhere (Task 4 Steps 3–5).
- **Known limitations:**
  - The last heartbeat wins across devices.
  - Detection depends on the Proton process keeping `Warframe.x64.exe` as an argument; Task 10 check 4 records the evidence.
