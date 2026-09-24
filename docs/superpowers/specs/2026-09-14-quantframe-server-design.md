# quantframe-server — Design Spec

- **Date:** 2026-09-14
- **Status:** Approved 2026-09-14. Amended by §14 (phase 1), §15 (phase 2), §16 (phase 3), §17 (phase 4a) and §18 (phase 4b) planning.
- **Source:** fork of [quantframe-react](https://github.com/Kenya-DK/quantframe-react) by Kenya-DK at commit `3d59c4e7` (v1.6.28)
- **License:** GPLv3, inherited from quantframe-react. Keep the upstream `LICENSE` and credit Kenya-DK in the README.

## 1. Goal

A server version of Quantframe for one user on a home network:

- A **collector runs 24/7** on a homelab server and gathers warframe.market listing data for every tradable item.
- **Auto-trading starts manually** from a web client served by that server. The user presses Start after logging in to Warframe on their gaming PC.
- A small **helper on the gaming PC** reports whether Warframe is running and forwards trades detected in `EE.log`.

## 2. Decisions

| # | Decision | Choice |
|---|---|---|
| D1 | What the server does while the user is offline | Collects market data only; never touches the user's orders |
| D2 | Collection scope | Hot set (stock, wish list, buy candidates) every ~5 min, plus a continuous cold pass over all other tradable items |
| D3 | Web access | Home network only; single-password login |
| D4 | Trade detection | `qf-helper` on the gaming PC tails `EE.log` and pushes events |
| D5 | Quantframe API (`api.quantframe.app`) | **Removed entirely.** warframe.market and the WFCD dataset only |
| D6 | Web client | Port quantframe's React UI; cut screens named in §10 |
| D7 | Price stats | Derived from listings (v2 only), with a per-item warm-up period and dry-run |
| D8 | Rivens | Riven trading is out of scope. Riven stock stays as a record only |
| D9 | Non-market game data | WFCD `warframe-items`, plus an `overrides.toml` for mismatches |
| D10 | Code structure | Fork in place and replace Tauri at three seams (approach 1) |

Defaults carried over from the user's earlier wfm-ledger decisions:

- **warframe.market sign-in** uses `POST /v1/auth/signin`. The only other permitted v1 call is `GET /v1/items/{slug}/statistics`, used by the one-off backfill (§24) and the daily closed-statistics refresh (§25). Everything else uses v2. Only the token is kept (valid ~60 days), encrypted with AES-256-GCM. The password is never stored.
- **Deployment** is Docker Compose on the homelab, LAN only. Images are built and run on the server; there is no Docker on the desktop.
- **Rate limiting** is one shared 3 req/s budget with priority lanes, in order: trader, hot, cold.
- **Dry-run is on by default.**

## 3. Non-goals (this spec)

- Riven auction trading. Every `wf-market` auction call is v1, so a v2 probe comes later.
- Syndicate price mode. It is already disabled upstream and needs data warframe.market doesn't have.
- Exposing the server to the internet, multi-user support, or 2FA.
- AlecaFrame inventory, the WF GDPR import, the updater, Patreon gating, analytics and bans.
- Storing full order books.

## 4. Architecture

A Rust workspace with two binaries. `qf-server` runs in a single Docker container.

```
gaming PC                               homelab (Docker Compose, LAN)
┌──────────────┐      LAN HTTP          ┌────────────────────────────────────────────┐
│  qf-helper   │ ── heartbeat/trades ──▶│ qf-server                                  │
│  (EE.log)    │                        │  axum: static UI · /rpc · /ws · /helper    │
└──────────────┘                        │  Services registry · EventSink · Paths     │
                                        │  WfmSession ── v1 signin / v2 REST + WS ──▶ warframe.market
browser ◀──── /ws events, /rpc ───────▶ │  Limiter (trader > hot > cold) ────────────▶
                                        │  Collector ─▶ Stats ─▶ PriceSource ─▶ Trader
                                        │  GameData (WFCD + WFM /items + overrides)  │
                                        │  SQLite (WAL)                              │
                                        └────────────────────────────────────────────┘
```

### 4.1 Workspace layout

These crates are kept from upstream. All of them are already free of Tauri.

- `entity`, `migration`, `service`: SeaORM entities, migrations and DB services. New tables are added here.
- `utils`: the logger, error type, file watcher and helpers.

Removed: `qf_api`.

The main crate changes as follows:

- `src-tauri/src` becomes the `qf_core` library, with Tauri removed.
- A new binary crate `qf-server` holds axum, config and startup.
- A new binary crate `qf-helper` runs on the gaming PC. It depends on the log-parser code, extracted from `qf_core` into its own crate `qf_log_parser` so the helper doesn't pull in the server.

The React app moves to `web/`.

### 4.2 Tauri seams (replace, don't rewrite)

Upstream reaches Tauri through only three things. Everything behind them stays as it is.

1. **The global `APP: OnceLock<tauri::AppHandle>` and `app.manage`/`app.state`** become `SERVICES: OnceLock<Services>`.
   - `Services` holds the same states: `AppState`, `CacheState` (renamed `GameDataState`), `LiveScraperState` and `LogParserState`. The server holds `LogParserState` only for event handling.
   - Change `utils/modules/states.rs` and the direct users at `app/modules/ws.rs:62` and `cache/client.rs:177`.
2. **`emit_event!`** (`macros.rs`) becomes an `EventSink` trait. The server implementation is a `tokio::sync::broadcast` channel fanned out to `/ws` clients.
   - The `{event, data}` envelopes for `message` and `message_update` stay unchanged, so the frontend listener only changes transport.
   - `play_sound` becomes an event the browser handles.
   - `send_system_notification!` is removed; Discord and webhooks cover notifications.
3. **`#[tauri::command]` wrappers** (120 of them) become plain `async fn`s that take their state from `Services`. `POST /rpc/:name` passes JSON arguments to them.
   - Only the commands on an explicit allowlist are routable (§7.3).
   - Commands for removed features are deleted.

Also:

- **Paths:** `helper::get_*_path` (`app.path()`) becomes a `Paths` struct built from config (`QF_DATA_DIR`). `get_device_id` becomes a stable ID generated once and saved to the data dir.
- **Spawning:** `tauri::async_runtime::spawn` becomes `tokio::spawn`.
- **Removed:** `tauri_plugin_os`, `package_info()` (use `env!("CARGO_PKG_VERSION")`) and all remaining plugins.
- **File dialogs** used by exports and imports (10 files) become HTTP download responses and multipart uploads.
- **Also removed:** `get_or_create_window`, the "processing-trades" webview window, the `explorer` call and `app_exit`.

## 5. Components

### 5.1 WfmSession

Replaces the Quantframe-account login in `app/modules/auth.rs`.

**Sign-in**

- The web UI submits the email and password to `auth_login`.
- The server calls `wf_market` `login` (v1 `/auth/signin`) and gets the JWT back.
- Only the token is stored, in table `wfm_account`: ciphertext, nonce, created-at, expiry decoded from the JWT, username and user ID.
- The encryption key comes from a Docker secret file (`QF_SECRET_KEY_FILE`, 32 random bytes).
- **The `qf_token` and `check_code` flow is deleted.**

**On boot**

- The server decrypts the token and logs in with it.
- It checks `/me` on v2, then opens the v2 websocket.
- It sends `@wfm|cmd/status/set invisible`, which forces invisible on every boot.

**Periodic check**

- `/me` is checked every 15 minutes.
- An expiry warning goes out 7 days before the token expires.

**Removed**

- `auth.json` is no longer written. The user record comes from `wfm_account` and `/me`.
- The v1 chat socket (`/im`) is dropped, along with the chat UI.

### 5.2 GameData

Replaces the Quantframe `cache` zip and `TradableItems.json`.

**Sources**

- **WFM `/v2/items`** (3,840 items, verified 2026-09-14): `id`, `slug`, `gameRef`, `tags`, `i18n{name, icon, thumb}`, plus `maxRank`, `bulkTradable`, `subtypes`, `vaulted`, `ducats` and amber/cyan stars where they apply. This is the list of tradable items the collector sweeps. `gameRef` is the in-game unique name and is non-empty on 3,805 items.
- **WFCD `warframe-items`** (category files such as `data/json/Arcanes.json`): `uniqueName`, `name`, `category`, `type`, `tradable` and `rarity`. The log parser uses it to resolve in-game display names from EE.log to a `uniqueName`, looked up per category like upstream `cache.arcane().get_by(name)`.
- **`overrides.toml`:** a manual slug↔uniqueName mapping for WFM items with an empty `gameRef`, or for display names that don't resolve.

**How it's used**

- **Matching:** an exact join of WFM `gameRef` to WFCD `uniqueName`. Names are not compared. Unmatched items and empty `gameRef`s fall back to `overrides.toml`.
- **Trade tax:** neither source has a trade-tax field, so it is dropped. Upstream used it only for the credits display on stock and wish-list entries; those fields become `0` and are hidden in the UI.
- **Refresh:** on startup and then daily. The last good copy stays on disk, and a failed refresh keeps using it.
- **Unmatched items:** exposed in the UI (`game_data_unmatched`) so they can be added to the overrides.
- **Upstream users:** `TradableItem` lookups (`get_by(slug)`, `bulk_tradable`, name and icon for order properties) are backed by this data. The riven weapon and attribute caches load from WFM `/riven/weapons` and `/riven/attributes`, which riven stock records need.

Fields verified 2026-09-14 against live WFM `/v2/items` and WFCD `Arcanes.json`.

### 5.3 Limiter

- **Model:** a single token bucket at 3 req/s with three priority lanes (`Trader`, `Hot`, `Cold`), strict priority. A lane only receives a token when no higher lane is waiting.
- **Scope:** every warframe.market REST call goes through it. Sign-in and `/me` go in the `Trader` lane.
- **On a 429:** the whole bucket pauses for 5 s, doubling to at most 60 s. The pause resets after 60 s with no 429.

### 5.4 Collector

- **Hot set:** the items in stock, the wish list and the trader's current buy candidates.
  - Refreshed every 60 s from the DB and the trader.
  - Each hot item is swept every 5 min in the `Hot` lane.
- **Cold pass:** loops continuously over every other tradable item in the `Cold` lane, least recently swept first.
- **Sweep:**
  1. `GET /v2/orders/item/{slug}`.
  2. Group the orders by `(item_id, sub_type)`, where sub_type is the rank/variant key normalised to a string.
  3. Write one `sweep_summary` row per group.
  4. Diff against `last_seen_orders` to produce `vanished_orders`.
  5. Update `last_seen_orders`.
  6. Recompute `item_stats` for the item.
- **v2 response shape** (verified 2026-09-14):
  - `data` is an array of every order for the item, including offline users. Arcane Energize had 1,494 orders.
  - Each order has `id`, `type` (`sell`/`buy`), `platinum`, `quantity`, `perTrade`, `visible`, `createdAt`, `updatedAt`, `itemId` and `user{id, ingameName, slug, reputation, platform, crossplay, status (offline/online/ingame), lastSeen, activity}`.
  - Ranked items add `rank`; relics and similar items add `subtype`; items like sets have neither.
  - A price edit keeps the same `id` and changes `updatedAt`, so edits are not vanishes.
- **Keeping `last_seen_orders` small:** a sweep writes only the difference. It inserts new IDs, deletes vanished IDs, and updates rows whose price or quantity changed. The per-order `last_seen` column is dropped; `sweep_state.last_swept_at` covers it.
- **Supervision:** the collector runs as a supervised task and is restarted after a panic.

### 5.5 Stats

These are pure functions over DB rows, so they can be unit-tested without I/O.

**Probable trade.** A `vanished_orders` row counts as a probable trade when all of these hold:

1. The same user has not posted a new order on the same `(item, sub_type, side)` within **2 h** of the vanish.
   - The row stays `pending` until the 2 h window closes.
   - It is then marked `trade` or `relist`.
2. That user did not lose **≥ 3 orders** (across all items) in the same sweep. This check is configurable, as `bulk_pull_threshold`.
3. `gap_seconds` (time since the previous sweep of that item) is **≤ 3×** the item's expected interval.
4. A drop in quantity on an order that is still listed counts as a probable trade for the difference (a partial fill). Rules 1–3 don't apply to it.

**Both sides count.** A vanished sell is a buy by someone else, and a vanished buy is a sale.

`item_stats` has one row per `(item_id, sub_type)`:

| Field | Definition |
|---|---|
| `volume` | Probable trades per day, averaged over the last 7 days |
| `avg_price` | Mean probable-trade price over the last 48 h |
| `moving_avg` | Mean of the daily median probable-trade price over the last 7 days |
| `profit` | Median vanished-sell price minus median vanished-buy price, over 7 days. If either side has fewer than 3 trades, the current live spread is used instead: lowest in-game sell minus highest in-game buy |
| `min_price`, `max_price`, `median` | Over probable trades in the last 7 days; used for logs and UI only |
| `history_days` | Days since this item was first swept |
| `warm` | `history_days ≥ 7 AND probable trades in the last 7 days ≥ 10` |
| `updated_at` | Timestamp of the last recompute |

The `profit_margin`, `week_price_shift` and `trading_tax` filters are not computed in phase 3. They are off by default upstream, and the UI hides them.

**`PriceSource` trait.**

- `find_by(wfm_id, sub_type) -> Option<ItemPriceInfo>` returns the upstream `ItemPriceInfo` shape filled from `item_stats`, plus a `warm` flag.
- `all() -> Vec<ItemPriceInfo>` feeds `get_interesting_items`.

**Known limitation.** Cold items are swept roughly every 25 min, so an order that is posted and filled between two passes is never seen. Cold-item volume is therefore undercounted. An item's stats improve once it joins the hot set.

### 5.6 Trader

This is upstream `live_scraper` with the following changes:

- **Items only.** Riven and syndicate code paths are deleted.
- **Price data** comes from `PriceSource`, not `item_price()`.
- **Syndicate branches** in `collect_interesting_items` are removed.
- **Order writes** go through an `OrderWriter` trait:
  - `LiveOrders` calls wf-market.
  - `DryRunOrders` writes to `dry_run_log`: timestamp, action, item, sub_type, price, quantity, reason and `forced_by` (`global` or `not_warm`).
  - Global dry-run on routes everything to `DryRunOrders`. With it off, an item that isn't `warm` still goes to `DryRunOrders`.
- **Bug fix:** sell-side repricing reads `settings.wts.max_price_drop` and `settings.wts.min_listings_below`. Upstream reads them from `wtb` (`item.rs:714-716`). A regression test covers the fix.
- **Error handling:** five consecutive order-call failures trigger `Stopping`, and a `Critical` result from `check()` also triggers `Stopping`.

### 5.7 Lifecycle

States: `Offline`, `Ready`, `Trading` and `Stopping`.

**Conditions for `Ready`.** The server is `Ready` only when all of these hold; otherwise it is `Offline`.

- The WFM token is valid, meaning the last `/me` check succeeded.
- The v2 websocket is connected.
- A helper heartbeat has arrived within the last 30 s with `warframe_running = true`.
- Game data is loaded.

The UI shows each condition as a checklist. **Start** is enabled only in `Ready`.

**Start sequence** (`Ready → Trading`):

1. Set the WFM status to `ingame`. Skipped under global dry-run.
2. Start the trader loop.
3. Broadcast `Lifecycle:State`.

**Stop triggers** (`Trading → Stopping`):

- The Stop button.
- The helper reports `warframe_running = false`.
- No heartbeat for 60 s.
- WFM returns 401.
- The websocket has been down for more than 60 s.
- A trader `Critical` result, five consecutive order failures, or a trader panic.

**`Stopping → Ready`/`Offline` sequence:**

1. Stop the loop and let the in-flight request finish.
2. Set the status to `invisible`.
3. If `delete_buy_orders_on_stop` is on (it is off by default), delete the bot's buy orders.
4. Record the reason and broadcast it, including to Discord.

**Restart behaviour.** Trading never resumes on its own after a server restart.

### 5.8 Helper (`qf-helper`)

**Build and install**

- A native Linux x86_64 binary; the gaming PC runs Arch Linux with Warframe under Proton. There is no cross-compile.
- It runs as a `systemd --user` service.

**Config**

- The helper reads `server_url`, `device_key` and an optional `ee_log_path` from `qf-helper.toml`.
- The default log path is `~/.local/share/Steam/steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log`, verified on the gaming PC (Steam app ID 230410).

**Heartbeat**

- `POST /helper/heartbeat` every 10 s with `{warframe_running, version}`.
- `warframe_running` means a process whose command line contains `Warframe.x64.exe`, found by scanning `/proc/*/cmdline`.

**Trade reporting**

- Upstream log-parser trade detection runs locally.
- Each detected trade becomes `POST /helper/trade` with `{event_id, detected_at, payload}`.
  - `event_id` is the SHA-256 of the raw log lines plus the detection timestamp.
  - `payload` holds the parsed trade: items, platinum and the other player.
  - The helper sends raw in-game names; the server resolves them through GameData.
- On the server, the existing `on_trading` handler logic runs, minus the webview window.
  - Stock and transactions are updated, and WFM orders are closed or adjusted.
  - If `auto_trade` is off, the trade is stored as `needs_review` and shown as a modal in open browsers.

**Queue and idempotency**

- Unsent events go to a local queue file and are replayed in order.
- The server keeps `helper_events(event_id PRIMARY KEY)` and ignores duplicates.

**Auth**

- `Authorization: Bearer <device_key>`.
- Keys are created and revoked in the web UI and shown once. The server stores only a SHA-256 hash in `helper_keys`.

## 6. Data model (new tables)

Every timestamp is stored as UTC ISO-8601 text, the same as the existing SeaORM usage.

| Table | Columns (key ones) | Retention |
|---|---|---|
| `wfm_account` | id=1, token_ciphertext, nonce, token_expires_at, wfm_user_id, username, created_at | — |
| `helper_keys` | id, name, key_hash, created_at, last_seen_at, revoked_at | — |
| `helper_events` | event_id PK, received_at, status (`applied`/`needs_review`/`ignored`), payload | 90 d |
| `sweep_summary` | item_id, sub_type, swept_at, lane, min_sell, max_buy, sell_count, buy_count, sell_ingame, buy_ingame, top_sells (json), top_buys (json) | raw 30 d |
| `sweep_summary_hourly` | item_id, sub_type, hour, min/avg/max of min_sell and max_buy, avg counts | indefinite |
| `last_seen_orders` | order_id PK, item_id, sub_type, side, platinum, quantity, user_id, first_seen, updated_at | live set |
| `vanished_orders` | order_id, item_id, sub_type, side, platinum, quantity, user_id, first_seen, vanished_at, gap_seconds, kind (`full`/`partial`), status (`pending`/`trade`/`relist`/`bulk`/`gap`) | 90 d |
| `item_stats` | (item_id, sub_type) PK, fields per §5.5 | current |
| `item_stats_daily` | item_id, sub_type, day, volume, median, min, max | indefinite |
| `sweep_state` | item_id PK, last_swept_at, expected_interval_s, consecutive_errors, active | current |
| `dry_run_log` | id, at, action, item_id, sub_type, price, quantity, reason, forced_by | 30 d |
| `trader_state` | id=1, dry_run, delete_buy_orders_on_stop, last_stop_reason, last_stop_at | — |
| `web_auth` | id=1, password_hash (argon2id), updated_at | — |

**Retention job:** runs hourly. It rolls raw data up into the hourly and daily tables, then deletes rows past their retention.

**Upstream tables** (stock items, wish list, transactions, trade entries and so on) are kept as they are.

**Upstream stock-riven tables** stay for record-keeping; nothing trades from them.

## 7. Web layer

### 7.1 Endpoints

| Route | Auth | Purpose |
|---|---|---|
| `GET /` and static assets | none (login page) / session | Built React app |
| `POST /auth/login`, `POST /auth/logout` | — | Password login: session cookie |
| `POST /rpc/:name` | session | Allowlisted commands, JSON args → JSON result |
| `GET /ws` | session | Event stream (`message`, `message_update`, `Lifecycle:State`, `play_sound`) |
| `GET /download/:kind` | session | Exports (stock, transactions, wish list, logs) |
| `POST /upload/:kind` | session | Imports |
| `POST /helper/heartbeat`, `POST /helper/trade` | device key | Helper API |
| `GET /healthz` | none | Container healthcheck |

### 7.2 First-run setup

On first start with no `web_auth` row, the server reads `QF_WEB_PASSWORD_FILE` (a Docker secret), hashes it and stores the hash. If that file is missing, the server logs an error and refuses to serve anything except `/healthz`.

### 7.3 Security

- **Session cookie:** `HttpOnly`, `SameSite=Strict`, 30-day expiry, and sessions are stored server-side in memory. A restart logs the user out, which is acceptable.
- **Login rate limit:** 5 attempts per minute per IP.
- **CSRF:** every non-GET request must carry an `Origin` header that matches the configured `QF_PUBLIC_ORIGIN`. No CORS headers are sent.
- **RPC allowlist:** `/rpc` dispatches only through a static `match` over allowlisted names; unknown names return 404.
- **Log masking:** the upstream `SENSITIVE_FIELDS` masking stays and gains `device_key` and `token_ciphertext`.
- **Encryption key:** if the key is missing or invalid, the WFM session stays signed out and an error is shown. The token is never stored unencrypted.

### 7.4 Frontend port

- **Transport:**
  - `TauriClient.sendInvoke` (`src/api/index.ts`) calls `fetch('/rpc/'+name)`.
  - `src/api/events` reads from a single `/ws` connection.
  - The direct `invoke`/`listen` calls go through the same wrappers: `initialized`, `wfgdpr_load`, `log`, `log_send` (upstream-broken), `app:ready`, `play_sound`, `add_trade` and drag-drop.
- **Replacements:**
  - `plugin-shell` open: `window.open`
  - clipboard: `navigator.clipboard`
  - `plugin-dialog`: `<input type=file>` and a download link
  - `plugin-fs`, `plugin-path`: removed
- **New screens:**
  - Start checklist and lifecycle panel
  - Dry-run log
  - Collector health (last pass, error rate, items behind, lane throughput)
  - Price history charts per item from `sweep_summary_hourly` and `item_stats_daily`
  - Helper devices (create, revoke, last seen)
  - WFM account (sign in, token expiry)
  - Game data unmatched list

## 8. Error handling

| Condition | Behaviour |
|---|---|
| WFM 429 | Global limiter pause 5 s doubling to 60 s |
| WFM 5xx / timeout (collector) | 2 retries with jitter, then skip item this pass; `consecutive_errors++` |
| WFM 5xx / timeout (trader) | Log and continue; 5 consecutive → `Stopping` |
| WFM 404 on item | `sweep_state.active = false` until next GameData refresh |
| WFM 401 | Session marked invalid; `Stopping` if trading; UI prompts re-sign-in |
| Token expires in ≤ 7 d | UI banner + Discord message (once per day) |
| v2 WS disconnect | Reconnect with backoff; > 60 s while trading → `Stopping` |
| Collection gap > 3× expected interval | Vanishes marked `gap`, excluded from stats |
| WFCD refresh fails | Keep last good copy; UI warning |
| Secret key missing/invalid | WFM session disabled with explicit error |
| Helper key invalid/revoked | 401; helper backs off, shows error |
| Server unreachable from helper | Events queued to disk, replayed with idempotent `event_id` |
| Collector panic | Supervisor restarts task; logged |
| Trader panic | `Stopping`; no auto-restart |
| DB migration | Backup copy first (upstream behaviour); daily backup, keep 7 daily and 4 weekly (§19 H6) |

**Logs:** written to stdout for Docker, and to the upstream file logs under `QF_DATA_DIR/logs`.

## 9. Testing

- **Stats (unit tests on pure functions).** Fixtures cover:
  - a normal trade
  - a partial fill
  - a relist within 2 h
  - a bulk pull of 3 or more orders
  - a gap longer than 3× the expected interval
  - the rank/variant split
  - the boundaries of `warm`
  - the fallback to the live spread when data is thin
- **Sweep diff.** Recorded real v2 `/orders/item` responses are stored as JSON fixtures; tests never touch the network.
- **Limiter.** Strict priority and the shared back-off after a 429, tested with `tokio::time::pause`.
- **Trader golden tests.** Given an order book, `PriceSource` and settings, assert the exact `OrderWriter` calls. Also cover `DryRunOrders` routing (global and not-warm) and the `wts` regression.
- **Lifecycle.** Every transition and every stop trigger, plus the check that trading never resumes on its own after a restart.
- **GameData matcher.** Runs against pinned snapshots of WFCD and WFM `/items`, asserts a minimum match rate and prints the unmatched items. The exact threshold is set once the first real match run is done.
- **Helper.** Sample EE.log trade blocks produce the expected payload. Queue replay must not create duplicates.
- **Web (axum integration tests).** Login, rate limit, the Origin check, the `/rpc` allowlist, the download endpoints and helper auth.
- **Frontend.** `tsc` typecheck and a Vite build in CI.
- **Acceptance.** At least 7 days of dry-run on the server, with the dry-run log reviewed, before dry-run is turned off.

## 10. Screens cut in phase 1

Removed from the UI:

- Quantframe login and the Patreon modal
- The updater modal
- Chat
- Riven trading controls in the live scraper; riven stock pages stay read-only
- Syndicate prices
- The AlecaFrame inventory and WF GDPR import tabs
- The theme folder opener
- Alerts, user-activity and analytics pages, which were backed by the QF API

## 11. Build phases

Each phase ends with something that runs on the server.

1. **Headless port** (plan: `docs/superpowers/plans/2026-09-14-phase-1-headless-port.md`). Includes:
   - Workspace fork and the three Tauri seams (§4.2)
   - The WFM `/v2/items` item list replacing the Quantframe item cache (§14 A1)
   - `qf_api` removed
   - WfmSession with the encrypted token
   - Web login, `/rpc`, `/ws`, downloads and uploads
   - React UI ported, with the §10 screens cut
   - Dockerfile and Compose file

   *Result:* stock, wish list and transactions can be managed in the browser.
2. **Data.** Includes:
   - Daily refresh of the WFM item list
   - Limiter lanes
   - Collector (hot and cold)
   - Stats and the retention job
   - Collector health and price-history screens

   *Result:* the warm-up clock starts.
3. **Trader.** Includes:
   - `PriceSource` wiring
   - `OrderWriter` with `DryRunOrders` and the dry-run log
   - Lifecycle and the Start checklist (with a manual "helper override" flag available only in dry-run, for testing before phase 4)
   - Discord stop and expiry alerts
4. **Helper.** Includes:
   - WFCD `warframe-items` and `overrides.toml` for EE.log name resolution (§14 A1)
   - The `qf-helper` binary
   - Device keys
   - Heartbeat and in-game detection
   - Trade events
   - The review modal

   The dry-run-only helper override from phase 3 is removed.
4c. **Hardening** (§19). The Start gate on `auto_delete`, trader tolerance of stock rows removed by trades, failure alerts, helper and cache durability, collector-independent retention, and nightly backups with a rehearsed restore.
4d. **Live log tab** (§20). The server log streamed into a Log tab on the Live Scraper page.
5. **Go-live** (§21). Prep first: simulated-delete wording, idle pause, dry-run summary, runbook. Then review the dry-run output and turn global dry-run off, on or after 2026-09-22.
6a. **Trading analytics** (§22). Items P&L, stock performance, trading partners and a profit timeline on the Trading Analytics page, from the transaction and stock tables.
6b. **Market data** (§23). Market overview, top movers, warm-up progress, and recent trades plus the order-book snapshot on Price History, from the collector tables.
6c. **Market history backfill** (§24). A one-off, repeatable import of warframe.market's 90-day closed-trade statistics into `item_stats_daily`, started from a button on the Collector tab.

## 12. Later (outside this spec)

- Probe warframe.market v2 for auction endpoints, then revisit riven trading.
- Syndicate mode backed by WFCD syndicate offerings.
- Remote access over VPN.

## 13. Verification status

1. ✅ WFCD fields and WFM `gameRef` join (§5.2). Trade tax is unavailable and dropped.
2. ✅ v2 `/orders/item/{slug}` shape (§5.4).
3. ⏳ The v2 websocket status command with a v1 sign-in token. It passed in the wfm-ledger spike on 2026-09-14. It needs the user's credentials, so it is re-checked as a manual step in phase 1.
4. ✅ Gaming PC is Linux with Proton: native helper binary and a verified `EE.log` path (§5.8).

## 14. Amendments from phase 1 planning (2026-09-14)

These amendments take precedence over the earlier sections.

- **A1 — Game data split across phases.** Every stock, wish-list and trade-entry handler validates items through the tradable item cache, so phase 1 needs item data.
  - Phase 1 builds `CacheTradableItem` from WFM `/v2/items`: `gameRef` becomes `uniqueName`, and `trade_tax` is `0`.
  - The last good copy is kept on disk.
  - WFCD `warframe-items` and `overrides.toml` move to phase 4, because only the EE.log parser needs them.
  - All other upstream cache modules (weapons, mods, relics and so on) are dropped. The phase 3 and 4 plans re-add them only if the trader or log parser needs them.
- **A2 — No riven stock UI in phase 1.** The only riven stock screen was the live-scraper Riven tab. It is removed, and the `stock_riven` table and data remain. Riven trade entries are rejected with an error.
- **A3 — No `/download` or `/upload` routes.**
  - Exports are RPC commands that return rows; the browser saves them as a JSON Blob.
  - Custom sounds upload as base64 through `sound_add_custom_sound`.
  - Log export is not exposed. `export_transaction_json` is exposed the same way as the other exports.
  - `transaction_calculate_tax` is removed because trade tax is unavailable.
- **A4 — `auth.json` still exists** for non-secret profile fields (username, id, avatar, status). `wfm_token` is `#[serde(skip)]` on `User`, so it is never written there or sent to the browser. The token lives only encrypted in `wfm_account`.
- **A5 — Web login routes.** The login page is a server-rendered page at `GET|POST /login`, and logout is `POST /logout`. These replace `/auth/login` and `/auth/logout` in §7.1.
- **A6 — Login rate limit is global.** It is 5 attempts per minute across all clients, which is stricter than per-IP and needs no connection info.
- **A7 — The web password file is the source of truth.** On start, if `QF_WEB_PASSWORD_FILE` exists and doesn't match the stored hash, it is re-hashed. The stored hash is used only when the file is absent. The password must be at least 12 characters.
- **A8 — Paths and database.**
  - The data directory is `QF_DATA_DIR`, holding `quantframe.sqlite`, `sounds/`, `cache/`, `logs/` and `device_id`.
  - Built-in sounds are served from `QF_RESOURCES_DIR/sounds` at `/sounds/builtin/*`, and custom sounds at `/sounds/custom/*`.
  - `QF_SECRET_KEY_FILE` holds 64 hex characters (`openssl rand -hex 32`).

## 15. Amendments from phase 2 planning (2026-09-15)

These amendments take precedence over the earlier sections.

- **B1 — One gate for wf-market.** The vendored `wf-market` crate gets a process-wide `RequestGate` that every `call_api` request passes before it is sent and that sees every response status. The server installs a gate that takes a `Trader`-lane token and reports 429s to the limiter. The `/v2/items` refresh takes a `Hot`-lane token.
- **B2 — Collector HTTP client.** The collector does not use `wf-market`. It calls the public `GET /v2/orders/item/{slug}` with its own unauthenticated `reqwest` client (headers `Platform: pc`, `Language: en`, `Crossplay: true`), through the limiter.
- **B3 — Hot set in phase 2** is every `wfm_id` in `stock_item` and `wish_list`. Phase 3 adds the trader's buy candidates.
- **B4 — `sub_type` key.** `""` when an order has no rank or variant fields. Otherwise it is `key=value` parts in the fixed order `rank`, `charges`, `subtype`, `amber`, `cyan`, joined by `;`. Examples: `rank=5`, `subtype=intact`, `amber=0;cyan=1`.
- **B5 — Orders with `visible = false`** are ignored everywhere.
- **B6 — `sweep_summary` columns.**
  - `min_sell` and `max_buy` are taken over orders from users whose status is `ingame`.
  - `sell_count` and `buy_count` count all visible orders; `sell_ingame` and `buy_ingame` count the in-game ones.
  - `top_sells` and `top_buys` are JSON arrays of up to 5 in-game orders as `[platinum, quantity]`, best price first.
  - The live spread used by `profit` (§5.5) is `min_sell − max_buy` from the item's latest sweep.
- **B7 — `sweep_state` columns** add `slug`, `first_swept_at` (for `history_days`) and `last_attempt_at` (cold ordering, so failing items don't block the pass).
  - `expected_interval_s` is 300 for hot sweeps.
  - For cold sweeps it is the duration of the last complete cold pass. Before the first pass completes, it is the number of active items in seconds, with a minimum of 60.
  - The gap rule (§5.5 rule 3) compares `gap_seconds` with `3 × max(previous expected_interval_s, current expected_interval_s)`, so an item that moves from hot to cold isn't wrongly marked `gap`.
- **B8 — Vanish statuses.**
  - A full vanish is stored as `gap` straight away when rule 3 fails, and as `pending` otherwise.
  - A partial fill is stored as `trade` straight away, with `quantity` set to the drop.
  - Once the 2 h window has closed, a `pending` row becomes one of:
    - `bulk`: the same user has at least `bulk_pull_threshold` (3) **full** vanishes, across all items, within ±15 min of this one. Each sweep sees only one item, so the "same sweep" in rule 2 becomes a 30-minute window.
    - `relist`: an order from the same user on the same `(item, sub_type, side)` was first seen after the vanish and no more than 2 h later, in either `last_seen_orders` or `vanished_orders`.
    - `trade`: neither of the above.
- **B9 — Trade counting.** Each `trade` row counts as one probable trade at its `platinum` (per unit). `volume` is trades in the last 7 days divided by 7.
- **B10 — Retries.** A 5xx, timeout, other unexpected status or 429 is retried at most twice. Each retry waits 0.5–1.5 s of jitter and takes a new limiter token. A 429 also triggers the limiter pause. A 404 is not retried.
- **B11 — When stats are recomputed.** `item_stats` is recomputed after every successful sweep, and for every item whose pending rows are resolved. Resolution runs every 5 min. Rollups (`sweep_summary_hourly` for the last 48 h, `item_stats_daily` for the last 3 days) and retention run hourly and are idempotent (`INSERT OR REPLACE`).
- **B12 — `QF_COLLECTOR`** (`on` default, or `off`) turns off all collector tasks, for desktop development. Any other value is a configuration error.
- **B13 — Screens.** Collector health and price history are the two tabs of one **Market Data** page. They are fed by the RPC commands `collector_health` and `market_item_history { wfmUrl, subType?, days }`. Price history shows hourly rollups, so the current hour appears after the next hourly run.

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

## 18. Amendments from phase 4b planning (2026-09-15)

These amendments take precedence over the earlier sections. They were decided against 167 real trade bundles that the desktop Quantframe left under `~/.local/share/dev.kenya.quantframe/logs` (2026-09-07 to 2026-09-14), each holding the raw EE.log lines and the upstream parser's log.

- **E1 — Names resolve through WFM English names; WFCD is dropped.**
  - Of the 114 distinct traded item names in the bundles, an exact match on the `/v2/items` English name resolves 95 before rank handling and line joining, and about 112 after. The WFCD name-to-`uniqueName`-to-`gameRef` join resolves 83 and misses every prime warframe blueprint part, because WFCD calls them "Chassis" rather than "Chassis Blueprint".
  - Normalisation before matching: trim, remove trailing characters in U+E000–U+F8FF, collapse runs of whitespace, compare case-insensitively with `i18n.en.name`.
  - `QF_DATA_DIR/overrides.toml` is optional and holds a `[names]` table mapping an in-game display name to a WFM slug. It is read on every resolution; a missing file is empty, and an invalid file is logged and treated as empty.
  - Superseded: the WFCD bullets in §5.2, decision D9 in §2, the "WFCD refresh fails" row in §8 and the "GameData matcher" test in §9. `game_data_unmatched` is not built.
- **E2 — The trade dialog is parsed from a byte stream, not line by line.** The crate is `crates/qf_log_parser` (library `qf_log_parser`, dependencies `serde`, `serde_json` and `sha2` only). Warframe writes the dialog in chunks that can split a word, a rank suffix or the end marker across two lines, so the scanner accumulates text instead of matching lines.
  - Start marker: `Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:`. End marker: `, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)`. The text between them is split on `\r` and `\n`, trimmed, and empty pieces are dropped.
  - The piece `and will receive from <name> the following:` separates the offered lines from the received lines. The player name loses its trailing U+E000–U+F8FF characters.
  - Result markers after a dialog: `description=The trade was successful!` reports the trade; `description=The trade failed.`, `description=The trade was cancelled` and `OnTradeAccepted failed` discard it. A new start marker before any result discards the pending dialog. Only successes leave the helper.
  - Each remaining piece is a raw item `{ name, quantity, rank }`: `Platinum x N` is platinum with quantity N; `<name> (<WORD> RANK <n>)` gives the name and rank n (the word is a rarity or a mod family such as GALVANIZED and is ignored); trailing U+E000–U+F8FF glyphs on arcanes are removed and their count is the rank (the bundles confirm five glyphs on rank-5 arcanes and three on a rank-3 one); pieces with the same name and rank fold into one item with the summed quantity.
  - `event_id` is the SHA-256 hex of the start line's EE.log timestamp, a newline, and the dialog text between the markers. `detected_at` is the helper's UTC wall clock when the success marker was seen. This replaces the §5.8 definition.
  - Only the English strings are supported. The Russian set from upstream is not ported.
- **E3 — Helper tailing and queue.**
  - The helper polls `ee_log_path` (D7) every 1 s and reads the bytes added since its last offset. On start it begins at the end of the file, so earlier trades are never replayed. When the file is shorter than the offset or missing, the offset resets to 0 and the scanner state is cleared; a missing file is warned about once and polling continues.
  - Unsent events are appended, one JSON object per line, to `$XDG_STATE_HOME/qf-helper/trade-queue.jsonl`, falling back to `~/.local/state/qf-helper/trade-queue.jsonl`. The queue is replayed oldest first before new events. A 2xx removes the line; 401 waits 60 s as the heartbeat does; 400, 404, 409, 413 and 422 drop the line with a log line; anything else, including network errors, retries every 10 s.
  - `qf-helper --parse <file>` runs the scanner over a whole file, prints the trades as a JSON array and exits 0. It needs no server.
  - Each reported trade logs one stdout line: `trade detected: <purchase|sale|unknown> <platinum>p with <player>, <n> items; server: <status>`.
- **E4 — `POST /helper/trade`.**
  - Authenticated as in D4. Body: `{ event_id: 64 lowercase hex, detected_at: RFC 3339, trade: { player_name, ee_timestamp, offered: [RawItem], received: [RawItem] } }` with `RawItem = { name, quantity, rank? }`.
  - Returns `200 { status: "applied" | "needs_review" | "ignored" | "duplicate", reason? }`. A malformed body returns 400. A known `event_id` returns `duplicate` and changes nothing. The event is processed inside the request.
- **E5 — `helper_events`.** Migration `m20260918_000001_create_helper_events` creates `event_id TEXT PRIMARY KEY, device_name TEXT NOT NULL, received_at TEXT NOT NULL, detected_at TEXT NOT NULL, status TEXT NOT NULL CHECK (status IN ('applied','needs_review','ignored')), reason TEXT, payload TEXT NOT NULL, resolution TEXT, reviewed_at TEXT`. `payload` is the E4 `trade` object. `resolution` is `{ direction, platinum, items: [{ name, slug, item_name, sub_type, quantity, price, matched_by }], extras: [RawItem] }` with `matched_by` one of `name`, `override`, `set` or `review`; it is stored for `needs_review` events too, holding whatever resolved. The hourly maintenance job deletes rows older than 90 days. The §6 row is replaced by this.
- **E6 — Classification.** Sum the platinum on each side. Platinum offered and none received is a **purchase** whose goods are the received items. Platinum received and none offered is a **sale** whose goods are the offered items. Platinum on both sides or on neither is `needs_review` with reason `no_platinum_side`. Non-platinum items on the platinum side are **extras**: recorded in the resolution, never applied, and never a reason for review. The bundles show a fish and junk mods handed over alongside platinum.
- **E7 — Resolution and sets.**
  - Each goods item resolves by E1. A rank becomes `SubType::rank(rank)`, capped at the item's `max_rank` when one is known. Items without a rank get no sub type.
  - Sets fold lazily. When a goods side has two or more resolved items, the candidate roots are the items tagged `set` whose English name without the trailing ` Set` is a prefix of at least two of those items' names. For each candidate the server fetches `GET /v2/item/{slug}` once for its `setParts`, caching the result in memory and in `QF_DATA_DIR/cache/sets.json`. If the goods cover every part other than the root, they are replaced by the root with quantity equal to the smallest part quantity; leftover parts stay as individual items.
  - Any goods item that doesn't resolve makes the event `needs_review` with reason `unresolved: <name>`.
- **E8 — Applying.**
  - An event is applied automatically when `live_scraper.general.auto_trade` is on and every goods item resolved. With `auto_trade` off every event is `needs_review`; the setting is the kill switch.
  - Platinum split. Each item's weight is the price of the user's own current WFM order for that slug and sub type, from the WFM client's cached orders: a sell order for a sale, a buy order for a purchase. Items without one use `item_stats.median` for the item and sub type. If no item has a weight, the weights are equal. Line prices are `total × weight / Σweight` rounded to whole platinum, then adjusted so they sum to the total, with the remainder on the first item. A line price is the total for that line, matching how the handlers already divide by quantity.
  - Per item, in order: for a purchase, `handle_wish_list` with `OrderType::Buy` and `ReturnOn:NotFound`, then `handle_item`; for a sale, `handle_item` with `OrderType::Sell` and `SkipWFMCheck:ItemSell_NotFound`. Every call carries `SetDate:<detected_at>`. Real WFM orders are closed or adjusted through the handlers' existing path regardless of global dry-run, the same as the manual "sold" action, because the trade really happened.
  - If a handler fails part-way, the event becomes `needs_review` with reason `apply_failed: <component>`, the resolution is kept, and the completed items are logged so they aren't applied twice.
  - After an applied event: `RefreshStockItems`, `RefreshWishListItems` and `RefreshTransactions` are broadcast, `notify_gui!("on_trade_event", "green.7", "applied", …)` goes to open browsers, and `notifications.on_new_trade` sends with the upstream variables. A `needs_review` event sends `notify_gui!("on_trade_event", "yellow", "needs_review", …)`.
- **E9 — RPC.**
  - `helper_trades { status?: string, page: i64, limit: i64 }` returns `{ total, results: [HelperEvent] }`, newest first.
  - `helper_trade_apply { event_id, items: [{ slug, sub_type?, quantity, price }] }` applies E8 with the given items, using the stored direction, player and time, then sets `applied`, `matched_by: review` and `reviewed_at`.
  - `helper_trade_ignore { event_id }` sets `ignored` with reason `reviewed`.
  - Apply and ignore are refused unless the event is `needs_review`, and apply is also refused when the stored resolution has no direction (reason `no_platinum_side`).
- **E10 — Browser.**
  - A **Trades** tab on the Live Scraper page lists events with a status filter that defaults to needs_review, and columns for detected time, player, direction, platinum, items, status and reason. Row actions are Review and Ignore; Review is disabled for events without a direction, which can only be ignored.
  - The Review modal shows one row per goods item: the item picker the stock form already uses, sub type, quantity and price. Resolved items are prefilled from the stored resolution; prices default to an equal split of the total, editable. Apply calls `helper_trade_apply`.
  - Toasts use the `on_trade_event.applied` and `on_trade_event.needs_review` notification keys.
  - The `ee_log_path` field is removed from the Advanced settings tab. The setting stays in the struct so saved settings keep loading.
- **E11 — Tests.**
  - `qf_log_parser`: fixtures under `crates/qf_log_parser/tests/fixtures/` are raw lines from the real bundles with the player names replaced. They cover a purchase of four set parts, a sale of an arcane, a mod name split across lines, an end marker split across lines, a line with quantity, extras beside platinum, a failed and a cancelled result, and an `OnTradeAccepted failed`. Every fixture is fed in chunk sizes of 1, 7, 64 and the whole file and must give identical trades and event ids.
  - Server: unit tests for normalisation, overrides, rank capping, classification, extras, set folding against a small item list, and the split rule; an integration test of the trade route covering 401, 400, applied, needs_review and duplicate.
  - Web: `python3 scripts/check-rpc-commands.py` and `pnpm build`.
- **E12 — Acceptance.** One real sale and one real purchase in game apply automatically; a trade with an unresolved name lands in the tab and is applied from the modal; a helper restart with a queued event delivers it; replaying the same event returns `duplicate`.
- **E13 — Out of scope.** Riven trades, prices for item-for-item trades, the Russian dialog strings, the WFCD data source and the review of extras.

## 19. Amendments from phase 4c planning (2026-09-16)

These amendments take precedence over the earlier sections. Phase 4c hardens the server before global dry-run is turned off. It adds no trading behaviour. The evidence behind each item is in `docs/PHASE-4B-ACCEPTANCE.md` (follow-ups 1, 3, 9, 10 and 13) and the phase 4c research notes. This section also edits the §8 "DB migration" row and adds the 4c bullet to §11.

- **H1 — A live start is refused while `auto_delete` is on.** `auto_delete` stays as a setting with its current default (`true`) and its current behaviour: on the first cycle after Start it deletes every non-blacklisted order. `Checklist` gains `auto_delete_off: bool`, filled from `live_scraper.general.auto_delete` through a new `Platform::auto_delete()` accessor. `Checklist::ready()` is unchanged; a new `ready_for(dry_run: bool)` is `ready() && (dry_run || auto_delete_off)`. The controller uses `ready_for(options.dry_run)` for `start()`, for the idle state (`idle_state` gains the dry-run flag as a parameter, since it cannot see `inner.options` today) and for `tick`'s idle branch; the running branch is untouched, so a live trader is never stopped by this. The badge therefore never reads `Ready` while Start would be refused. In dry-run the item is informational. The web checklist renders the new row with the key `pages.live_scraper.trader.checklist.auto_delete_off`, grey rather than red while `options.dry_run` is true. Turning `auto_delete` on while the trader is already running changes nothing, because `just_started` is already false. As the last step of the web change, the dead upstream component `web/src/components/Forms/LiveScraperControl/` (the confirm modal this gate replaces; nothing imports it) is deleted; its en.json strings stay.
- **H2 — The trader tolerates rows that a trade removed mid-cycle.** `ItemEntry::get_stock_item` and `get_wish_list_item` (and their `_or_error` wrappers) return `Ok(None)` when the row no longer exists; a `None` id is still an error. The three call sites in `trader/item.rs` (`progress_buying`, `progress_selling`, `progress_wish_list`) skip the entry with a debug-level line naming the id and continue with the remaining entries. This replaces the `ItemEntry:GetStockItem` warning, which also aborted the rest of that cycle. No event subscription and no cross-cycle cache: the trader already rebuilds its entry list every cycle.
- **H3 — Failure alerts.**
  - Migration `m20260920_000001_add_helper_events_alerted_at`: `ALTER TABLE helper_events ADD COLUMN alerted_at TEXT` (nullable). This extends the E5 schema. `HelperEvent` carries `alerted_at`; the TypeScript type gets it as optional and the Trades tab does not display it.
  - Two kinds of row alert, once each: a row whose `reason` starts with `apply_failed:` alerts at the first sweep after it is written; a row still `needs_review` with `reason = applying` and `received_at` older than **180 s** (past the helper's 120 s request timeout, so the apply is stranded, not slow) has its reason rewritten to `apply_interrupted` and alerts. An `apply_interrupted` row stays reviewable by hand; the alert text and the Trades tab reason say that some items may already have been applied, because nothing recorded how far the stranded apply got. Rows with the ordinary review reasons (`unresolved: …`, `no_goods`, `auto_trade_off`, `no_platinum_side`) keep today's yellow toast and do not alert.
  - Store additions: `events::needing_alert(conn, now)` returns rows with `status = 'needs_review' AND alerted_at IS NULL` and either `reason LIKE 'apply_failed:%'` or (`reason = 'applying'` AND `received_at <= now − 180 s`), using the existing `(status, received_at)` index; `events::mark_alerted(conn, event_id, now)`; `events::set_reason(conn, event_id, reason)` for the rewrite.
  - Delivery: a new `TradeEnv::alert(&self, &HelperEvent)`; `LiveEnv` sends `notify_gui!("on_trade_event", "red", "alert", toast_values(event), {"autoClose": false})` (new en.json keys `common.notifications.on_trade_event.alert.{title,message}`) and `notifications.on_alert.send(...)` with the `on_new_trade` variables plus `<KIND>` (`apply_failed`, `apply_interrupted` or, from H6, `backup_failed`), `<REASON>` and `<EVENT_ID>`. `on_alert` is a new `NotificationSetting` with `#[serde(default = "default_on_alert")]` modelled on `on_trader_stopped`, so existing settings files keep loading; it appears in the Notifications settings tab (`on_alert_title`), and its Discord webhook is configured there by the user like the others. Fakes record alerts.
  - `trades::sweep_alerts(conn, env, now) -> Result<usize, Error>` does the query, the rewrite, the two sends and `mark_alerted` in one pass; it is what the housekeeping tick calls and what the tests exercise.
- **H4 — Helper and cache durability.** `Queue::push` calls `File::sync_all` after the append, so a detected trade is on disk before the helper moves on. `SetCache::save` writes `sets.json.tmp` and renames it over `sets.json`, the same idiom as `Queue::pop`. `quantframe.sqlite_backup`, the cold copy taken at every start, stays as the migration safety net.
- **H5 — Housekeeping runs whether or not the collector does.** New module `qf_core::housekeeping`, started unconditionally from `startup::start` through the existing `collector::runner::supervise`, holding a clone of the database connection and the `LiveEnv`. It ticks every **60 s**: every tick runs `sweep_alerts` (H3); once an hour it runs `helper_link::trades::events::apply_retention` (90 days, E5); once a UTC day it runs the backup (H6). The `helper_events` call leaves `collector::maintenance::hourly`, `HourlyReport` loses `deleted_events`, and the hourly log line in `collector::runner` drops that field, so retention has one owner. `tick(conn, env, now, gates)` is a plain function so the tests drive it without the loop.
- **H6 — Nightly backup, off the volume.** The backup directory is `QF_BACKUP_DIR` (default `<data_dir>/backups`), read in `qf-server/src/config.rs` beside `QF_DATA_DIR`, carried on `CoreConfig`, and stored as a third `Paths` field `backup_dir` that is created on demand like the `subdir` helpers. `compose.yaml` mounts `./backups:/backups` and sets `QF_BACKUP_DIR=/backups`, and the deploy step creates the host directory owned by uid 10001. The daily gate writes `quantframe-<YYYY-MM-DD>.sqlite` (UTC date) with `VACUUM INTO` on the live connection; a file that already exists for today means the day is done, which is what makes the job restart-safe. The new file is opened read-only on a second connection and must answer `ok` to `PRAGMA integrity_check`; otherwise it is deleted. A failed `VACUUM INTO` (typically a bind mount not writable by uid 10001) and a failed integrity check take the same path: a Critical log line, `notify_gui!("on_backup", "red", "failed", …)` (en.json `common.notifications.on_backup.failed.{title,message}`), `notifications.on_alert.send` with `<KIND> = backup_failed`, and no retry until the next UTC day. Retention keeps the 7 most recent dates plus the 4 most recent Sundays among the rest and deletes everything else; it is a pure function over dates. The README replaces its backup line with the backup and restore runbook: stop the container, copy the chosen file over `/data/quantframe.sqlite` in the volume with uid 10001, remove `-wal` and `-shm`, start, and confirm `Database ready` and the Trades tab totals.
- **H7 — Tests.**
  - Lifecycle: `ready_for(true)` ignores `auto_delete_off`; `ready_for(false)` requires it. Controller: with the fake platform, a live start is refused while `auto_delete` is on and a dry-run start is not.
  - Trader: `get_stock_item` on a deleted row is `Ok(None)`; a missing first entry does not stop the remaining entries from being processed (two dry-run log rows).
  - Events and pipeline: `needing_alert` returns only the two alerting shapes, and `mark_alerted` removes a row from the next call; with the recording fake, an `apply_failed` row alerts exactly once across two sweeps; an `applying` row younger than 180 s is left alone, an older one is rewritten and alerted once.
  - Helper and cache: `sets.json.tmp` is never left behind after `save`. The `sync_all` in `Queue::push` is not observable without a crash harness and has no test; H2 and H4 have no acceptance step beyond the local gate.
  - Housekeeping: `tick` with the hourly gate deletes a 91-day-old event; the backup test, written first because it is the one unproven assumption, runs `VACUUM INTO` against the file-backed test database, asserts the file exists with no `-wal`/`-shm` sibling, that a read-only connection returns `ok` from `PRAGMA integrity_check`, and that a second call for the same day is a no-op; the retention rule over a 40-day series keeps exactly 11 files (a 30-day window ending mid-week holds only three Sundays beyond the 7 dailies) and over a series with no Sunday keeps 7.
  - Web: `python3 scripts/check-rpc-commands.py` and `pnpm build`.
- **H8 — Acceptance.** With global dry-run on and `auto_delete` on, Start is available; with dry-run off (flipped briefly for the check and back on afterwards, nothing started) the checklist shows the red row and Start is disabled; turning `auto_delete` off enables it. A queued event whose apply is forced to fail lands as `apply_failed`, produces one Discord message (with `on_alert`'s webhook configured in Settings → Notifications beforehand) and one red toast, and a second sweep sends nothing. A row planted as `applying` with an old `received_at` is rewritten to `apply_interrupted` and alerted once. The first backup file appears under `./backups` on ockohome, passes `sqlite3 … "PRAGMA integrity_check"` on the host, and a restore rehearsal (helper stopped, container stopped, that file restored, container started) shows the same event and transaction totals as before. With `QF_COLLECTOR=off` in a throwaway container, the housekeeping log line still appears.
- **H9 — Out of scope.** Digest alerts, alerts for ordinary review reasons, off-site copies of the backups, a shutdown signal for background tasks, and any change to what `auto_delete` deletes.

## 20. Amendments from phase 4d planning (2026-09-16)

These amendments take precedence over the earlier sections. Phase 4d adds a **Log** tab that shows the server's log output live, the same lines `docker compose logs -f` shows. The user asked to "see it running" (2026-09-16); they chose live streaming over polling.

- **L1 — One tee in the logger.** `utils::core::dolog` is already the single funnel for every level function and for `Error::log`, and it already keeps the last 10 000 ANSI-stripped lines in `CACHED_LOGS`. It gains `static SINK: OnceLock<fn(&LogLevel, &str)>` and `pub fn set_sink(f: fn(&LogLevel, &str))`, called once next to `cache_log_entry` with the clean line after all filters. `utils` has no tokio and no `qf_core` dependency, so the sink is a plain function pointer installed by `qf_core::startup::start` right after `init_logger()`.
- **L2 — The event.** The installed sink calls `qf_core::events::emit("log", json!({"level": level.prefix(), "line": line}))` **directly**. It must never go through `send_event!`/`emit_event!`, which log a line on every emit and would recurse without end. `emit` is a synchronous `tokio::sync::broadcast` send that needs no runtime and returns 0 when nobody listens; `/ws` already drops frames on lag, so a slow browser never blocks the server. No new `UIEvent` variant: `log` is a raw channel, consumed like `play_sound`. The payload is the whole pre-formatted line (timestamp, elapsed, level, component, message), not re-split fields.
- **L3 — Scrollback.** `utils::core::tail(limit: usize) -> Vec<(LogLevel, String)>` reads the newest `limit` entries of `CACHED_LOGS`, oldest first. RPC `log_tail { limit: i64 } -> [{ level, line }]`, `limit` clamped to 1..=2000, in `commands/logs.rs` next to the existing write-only `log` command. It is the only new RPC; its name contains none of the banned substrings.
- **L4 — Browser.** A **Log** tab on the Live Scraper page after Trades (panels there mount only when active, so the socket handler attaches on open and detaches on leave). On mount it calls `log_tail { limit: 500 }`, then subscribes to the raw `log` channel and appends. Rendering: a monospace, dark, scrollable pane; auto-scroll to the bottom that pauses while the user has scrolled up and resumes with a "Jump to latest" button; a level filter (`Debug`, `Info`, `Warning`, `Error`, `Critical` as toggles, default all but Debug/Trace); a client-side cap of 2000 lines (oldest dropped); a Clear button that empties the pane locally. Level colours only: Warning yellow, Error/Critical red, the rest default. Strings under `pages.live_scraper.log.*` in `en.json`.
- **L5 — Not built.** Search, download, per-component filters, a top-level nav entry, server-side rate limiting, and the helper's journal. The Debug page and the Settings → Advanced → Log stub are untouched.
- **L6 — Security note.** The tab is behind the same session as everything else, but its audience is not `docker compose logs`: the sink and `tail` both read `CACHED_LOGS`, which is cached before the console filter and so also holds `console: false` lines — a superset of stdout. The two that matter are the full-context second copy of an error whose context exceeds `MAX_CONTEXT_LENGTH` (1048 chars, `utils::error`), so a big-context error shows as two consecutive lines in the pane with the second much longer, and the `DumpLog` lines cached by `zip_logger`. `RequestError.content` (an upstream error body) is logged unmasked today and therefore appears in full in that second line; that is a pre-existing follow-up, not changed here.
- **L7 — Tests.** `utils`: `tail(n)` returns the newest `n` oldest-first and an installed sink receives the ANSI-free line (assert on a unique marker, since `CACHED_LOGS` is process-global). `qf_core`: an `events` test that a `"log"` frame has `channel = "log"` and `payload.level/line`; an `rpc` test that `log_tail` is routable and rejects a non-numeric `limit`; `startup` is not unit-tested (the sink install is one line). Web: `python3 scripts/check-rpc-commands.py` and `pnpm build`.
- **L8 — Acceptance.** Open Live Scraper → Log: the last lines appear at once; a housekeeping tick line arrives within 60 s without reloading; scrolling up stops the auto-scroll and "Jump to latest" resumes it; turning the Info toggle off hides the housekeeping lines; the pane never exceeds 2000 lines after a few minutes of trading; `docker compose logs` shows no `Emit:SendEvent` line per log line (no recursion).

## 21. Amendments from phase 5 planning (2026-09-16)

These amendments take precedence over the earlier sections. Phase 5 is go-live. The §9 acceptance rule stands: at least 7 days of dry-run on the server with the dry-run log reviewed before global dry-run is turned off. Dry-run started on ockohome on 2026-09-15 and items turn `warm` from about 2026-09-22, so the phase has two halves. The **prep** half (G1–G4) is built, reviewed and deployed now. The **flip** half (G7) happens on or after 2026-09-22 and is appended to the same acceptance record. Decisions taken with the user on 2026-09-16: `auto_delete` is turned **off** in Settings before the flip, so the first live start adopts the existing real orders instead of deleting them (H1 is unchanged); the desktop data import (phase 3 follow-up 2) was already done in phase 4c and is not part of this phase; the Log tab auto-scroll (4d check 3) was re-tested by the user and works.

- **G1 — Simulated deletes say so.** `TradeOrders::delete` returns `Result<Route, Error>`: `Route::DryRun(book_forced_by())` when the id was handled in the simulated book, `Route::Live` after a real warframe.market delete. The trader's auto-delete loop (`trader/item.rs`, `delete_unwanted_orders`) logs `Simulated delete of order <id> (<forced_by>) <i>/<total>` for a dry-run route and keeps `Deleted order with ID: <id> <i>/<total>` for live. The `deleted` UI event is unchanged. The knapsack delete ignores the returned route. Nothing about routing changes; this is the wording fix from the 4d acceptance note.
- **G2 — Idle cycles pause longer.** `ItemTrader::check` returns `Result<usize, Error>`, the number of interesting items the cycle processed (`process_items` returns its `total`). `engine::run_loop` takes `pause` and `idle_pause`: after a cycle that returned `Ok(0)` it sleeps `idle_pause`; after any other cycle, including an error, it sleeps `pause` as today. The idle sleep runs in 1 s slices that stop as soon as `running` is cleared, because `stop` awaits the engine handle and must not wait 30 s. `platform::spawn_engine` passes `CYCLE_PAUSE` (1 s) and the new `IDLE_PAUSE` (30 s), both `pub const` in `engine.rs`. `just_started` is cleared after the sleep as today. This is phase 3 follow-up 4.
- **G3 — Dry-run summary.** New RPC `trader_dry_run_summary { days: i64 }` (`days` clamped to 1..=30, the log's retention) in `commands/trader.rs`, backed by `store::dry_run_summary(conn, since) -> DryRunSummary` with two GROUP BY queries over `dry_run_log` where `at >= since`: `by_action: [{ action, side, forced_by, count }]` ordered by action, side, forced_by; and `by_item: [{ item_id, sub_type, action, count, min_price, max_price }]` ordered by count descending, limited to 25 rows. `since` (UTC ISO-8601 text) is returned in the payload. The Dry-run log tab gains a Mantine `SegmentedControl` with 1, 7 and 30 days (default 7) and two small tables above the existing paged table, fed by `api.live_scraper.dryRunSummary(days)` with the same 10 s refetch and `isActive` gate as the log query; item ids resolve to names through the existing `cache_items` map. Strings under `pages.live_scraper.trader.dry_run_log.summary.*` in `en.json`, added by targeted insertion. This is what the user reads before the flip.
- **G4 — Go-live runbook.** `docs/GO-LIVE-RUNBOOK.md`, linked from the README's homelab section. Sections, in order:
  - *Pre-flight*: 7 days since 2026-09-15 have passed; Market Data shows warm items (`history_days ≥ 7`, ≥ 10 probable trades in 7 days); the 7-day summary shows `create` and `update` rows with plausible prices against the Market Data medians, and no delete storm beyond the `AutoDelete` rows on Start; `auto_delete` is off in Settings → Live Scraper (the checklist row is green); `delete_buy_orders_on_stop` is set the way the user wants; Settings → Notifications → On Alert has the Discord webhook; today's backup file exists on ockohome; the helper is connected and Warframe is running; the token does not expire within 7 days.
  - *The flip*: in the Trader panel turn Dry-run off, confirm the badge reads Ready, press Start, and watch the Log tab and warframe.market through the first cycle: the WFM status goes `ingame`, the first `Processing Item` lines show `Route: Live` for warm items and `Route: DryRun(NotWarm)` for the rest, and the first live create or update appears on the user's warframe.market profile.
  - *First hour*: watch for `Trader stopped` (the reason is in the panel and on Discord), for `OrderFailures`, and for the summary's `not_warm` rows, which keep growing after the flip because not-warm items still route to dry-run.
  - *Rollback*: Stop, turn Dry-run back on, Start. Orders the trader created live stay on warframe.market; delete them by hand or start once with `delete_buy_orders_on_stop` on and Stop.
- **G5 — Not built.** No H1 change (a live start with `auto_delete` on stays refused). No automatic flip, no scheduled flip, no per-item live allow-list, no summary export. The Log tab auto-scroll is not touched.
- **G6 — Tests.** `orders`: `delete` returns `DryRun(Global)` under global dry-run, `DryRun(NotWarm)` for a book id when not global, and the live path is unchanged. `engine`: a `run_loop` test with `Ok(0)` cycles sleeps `idle_pause` and still exits within about a second of `running` being cleared, and a test that a non-zero cycle sleeps `pause`. `item` golden tests are updated for the new return type. `store`: `dry_run_summary` groups the seeded rows, honours `since` and caps `by_item` at 25. `rpc`: `trader_dry_run_summary` is routable and rejects a missing `days`. Web: `python3 scripts/check-rpc-commands.py` and `pnpm build`.
- **G7 — Acceptance.** *Now, after deploying the prep:* a dry-run Start on ockohome logs `Simulated delete of order … (global)` lines instead of `Deleted order with ID`; with nothing to process the `Checking items...` line repeats about every 30 s and Stop still takes effect within about a second; the Dry-run log tab shows the summary tables for 1, 7 and 30 days with counts that match the paged rows; the runbook exists and its pre-flight list matches the UI. *Later, the flip:* the runbook is followed on or after 2026-09-22 and the result is appended to `docs/PHASE-5-ACCEPTANCE.md`, including the first live order id and the stop reason if the trader stopped in the first hour.

## 22. Amendments from phase 6a planning (2026-09-16)

These amendments take precedence over the earlier sections. Phase 6a fills out the **Trading Analytics** page, which today has only the Transaction tab. The user asked for tabs "that I can look at and use" (2026-09-16) and chose four. Every tab is one read-only RPC that aggregates in SQL over the existing `transaction` and `stock_item` tables, plus one web tab. No new tables, no new collection, no new dependencies: tables use `mantine-datatable`, charts use `chart.js` through `react-chartjs-2`, both already installed. The Home page's yearly, today and recent-days cards are not repeated.

- **A1 — Dates.** RPCs that take a range take `from: String` and `to: String`, UTC ISO-8601 dates or timestamps; a row is in range when `created_at >= from AND created_at < to`. The web sends `to` as the day after the picked end date so the picker reads as inclusive. Every ranged tab shares one `DatePickerInput` range (the same component the Transaction tab uses) defaulting to the last 30 days, remembered in `localStorage` under `trading_analytics_range`.
- **A2 — `analytics_items { from, to }`.** One row per `(wfm_url, sub_type)` with any transaction in range: `wfm_id, wfm_url, item_name, sub_type, purchases, bought_qty, spend, sales, sold_qty, revenue, profit, avg_buy, avg_sell, avg_days_held`. `spend` and `revenue` sum `price` (the row total, see the phase 3 handler: per-unit is `price / quantity`); `profit` sums the sale rows' existing `profit` column (written at sale time, so it matches the Transaction tab's report); `avg_buy = spend / bought_qty` and `avg_sell = revenue / sold_qty`, null when the divisor is 0; `avg_days_held` averages, over the sales in range that have one, the days from the most recent purchase of the same `(wfm_url, sub_type)` at or before the sale to the sale (a correlated subquery; null when no sale has a prior purchase). Sorted by `profit` descending. The web tab renders it with `mantine-datatable`, sortable on every numeric column, with the existing `SearchField` filtering `item_name` client-side.
- **A3 — `analytics_stock {}`.** One row per `stock_item` with `owned > 0`: `id, wfm_id, wfm_url, item_name, sub_type, owned, bought, list_price, status, created_at, days_in_stock, median, moving_avg, volume, warm, unrealised, list_vs_median`. Market fields come from `StatsPriceSource::load(conn, &cache)` and `find_by(wfm_id, sub_type)` exactly as the trader reads them (so the sub-type mapping is the trader's, not a second one); they are null for an item the collector has no stats for. `bought` is the per-unit price the row was bought at (the trader compares it directly with the sell price). `unrealised = (median - bought) * owned`, `list_vs_median = list_price - median`, both null when either side is null. Sorted by `unrealised` descending, nulls last. Rendered with `mantine-datatable`; `warm` as the Price History badge, `unrealised` coloured green or red by sign.
- **A4 — `analytics_partners { from, to }`.** One row per non-empty `user_name` with a transaction in range: `user_name, trades, bought_count, bought_plat, sold_count, sold_plat, profit, last_trade_at`, where `bought_*` are the user's sales to us (our `purchase` rows) and `sold_*` our sales to them, `profit` sums our sale rows' `profit`. Sorted by `trades` descending. Rendered with `mantine-datatable`.
- **A5 — `analytics_timeline { from, to, bucket }`.** `bucket` is `"day"` or `"week"` (anything else is a 400 from the dispatcher's enum parse). One row per bucket with at least one transaction: `bucket_start` (the day, or the Monday of the ISO week, as a UTC date), `sales, purchases, revenue, expenses, profit, cumulative_profit`. `profit` sums the sale rows' `profit`; `cumulative_profit` is the running sum in bucket order, computed in Rust after the query. Rendered as a chart.js bar chart of `profit` per bucket with `cumulative_profit` as a line on a second axis, a `SegmentedControl` for day/week, and the three totals above it. Empty buckets are not filled in; the chart shows the buckets that have data.
- **A6 — Placement and strings.** Four new tabs after Transaction, ids `items`, `stock`, `partners`, `timeline` (not `item`: that upstream key already exists in `en.json` for the cut desktop tab and stays untouched). Modules `commands/analytics.rs` (the four commands and their row structs) and `analytics/store.rs` (the SQL) in `qf_core`; web `api/analytics/index.ts`, `pages/trading_analytics/Tabs/{Items,Stock,Partners,Timeline}/index.tsx`; strings under `pages.trading_analytics.tabs.{items,stock,partners,timeline}.*` by targeted insertion.
- **A7 — Not built.** No CSV or JSON export, no per-item drill-down from the tables, no wish-list analytics, no riven or syndicate rows, no caching layer: every query runs on demand against SQLite and the tables are small.
- **A8 — Tests.** `analytics/store.rs` tests seed transactions and stock rows in the test database (`crate::trader::store::tests::db()` runs the full migration set) and assert exact rows for: an item bought twice and sold once (`avg_days_held` from the later purchase), a range boundary (`to` exclusive), a partner with both directions, and a week bucket that spans a month boundary. `rpc.rs`: the four commands are routable, `from`/`to` are required, `bucket: "month"` is rejected. Web: `check-rpc-commands.py` and `pnpm build`.
- **A9 — Acceptance.** On ockohome with the imported desktop data: Items P&L for the full range sums `revenue`, `spend` and `profit` to the same numbers the Transaction tab's financial report shows for that range; Galvanized Shot shows 3 purchases and 2 sales with the expected profit; Stock performance lists the 16 stock rows and shows market fields for those the collector tracks; Trading partners lists the users from the transaction table with correct counts for one spot-checked user; the timeline's monthly totals match the Home page's yearly bar chart for the same months.

## 23. Amendments from phase 6b planning (2026-09-16)

These amendments take precedence over the earlier sections. Phase 6b fills out the **Market Data** page, which today has the Collector health tab and the per-item Price History. The user chose four additions. Every addition reads the collector tables that already exist (`item_stats`, `item_stats_daily`, `vanished_orders`, `sweep_summary`, `sweep_state`) and the tradable-item cache for names. No new tables or collection.

- **M1 — `market_overview {}`.** Every `item_stats` row joined to the cache for `name` and `slug`: `item_id, name, slug, sub_type, volume, avg_price, moving_avg, median, profit, min_price, max_price, history_days, warm, updated_at`. Items missing from the cache are skipped. The whole set is returned in one call (about 9 000 rows today, well under 2 MB); the web tab does all sorting and filtering: a warm-only switch, a minimum-volume number input (default 0), a name search, and `mantine-datatable` column sorting defaulting to `profit` descending, paginated client-side at 50. Clicking a row stores `{ slug, sub_type }` in `localStorage` under `market_data_price_history_selection` and switches to the Price History tab, which reads that key on mount and pre-selects the item.
- **M2 — `market_movers { min_volume }`.** From `item_stats_daily`: for each `(item_id, sub_type)` take the latest day with a non-null `median` as *now*, and compare with the latest day at or before *now − 1 day* and at or before *now − 7 days*. Rows whose current `item_stats.volume < min_volume` (default `3.0`, the web's number input) are excluded, so a single odd trade cannot top the list. Returns `{ day: MoverList, week: MoverList }`, each `{ up: [Mover; ≤25], down: [Mover; ≤25] }` with `Mover = { item_id, name, slug, sub_type, median_now, median_then, change_pct, volume }` sorted by `change_pct` descending for `up` and ascending for `down`. Items with no comparison day are omitted. Rendered as two side-by-side tables per period with a SegmentedControl for 24 h / 7 d.
- **M3 — `market_warmup {}`.** `{ tracked, warm, projected: [{ date, warm_count }], history_days_histogram: [{ bucket, count }], trades_histogram: [{ bucket, count }] }`. `tracked` counts `item_stats` rows; `warm` counts `warm = 1`. `projected` gives, for each of the next 7 UTC dates starting tomorrow, the cumulative number of rows that will satisfy `history_days + d >= 7 AND volume * 7 >= 10` on day `d` (trade counts held constant: a projection, labelled as one in the UI). `history_days_histogram` buckets `history_days` into `0`…`6` and `7+`; `trades_histogram` buckets `round(volume * 7)` into `0`, `1-4`, `5-9`, `10+`. Rendered as two stat cards, a bar chart of `projected`, and two small bar charts for the histograms.
- **M4 — Price History extension.** `market_item_history` gains two fields: `trades: [{ vanished_at, side, platinum, quantity }]`, the newest 50 `vanished_orders` rows with `status = 'trade'` for the item and sub-type, newest first; and `book: { swept_at, top_sells: [[platinum, quantity]], top_buys: [[platinum, quantity]] } | null`, the latest `sweep_summary` row for the item and sub-type with its stored `top_sells` and `top_buys` JSON parsed. The tab renders the book as two short lists under the existing charts and the trades as a compact table below them. The existing fields and charts are unchanged.
- **M5 — Placement and strings.** Market Data tab order becomes Collector, Overview, Movers, Warm-up, Price History; ids `overview`, `movers`, `warmup`, `price_history`. Server: `commands/market.rs` for the three new commands and their structs, SQL in `collector/store.rs` beside the existing history queries, the M4 additions in `commands/collector.rs`. Web: `api/market/index.ts`, `pages/market_data/Tabs/{Overview,Movers,Warmup}/index.tsx`, additions to `Tabs/PriceHistory/index.tsx`; strings under `pages.market_data.tabs.{overview,movers,warmup}.*` and `pages.market_data.tabs.price_history.{book_title,trades_title,side,platinum,quantity,vanished_at}` by targeted insertion.
- **M6 — Not built.** No server-side paging or sorting for the overview, no watch-lists, no alerts on movers, no per-user views over `vanished_orders`, no export.
- **M7 — Tests.** `collector/store.rs`: seeded `item_stats_daily` rows give the expected day and week movers and exclude a low-volume item; `market_warmup` counts a 5-day-old item with 12 trades as warm on the projection's second day and never counts one with 4 trades; the history extension returns the 50 newest trades and parses the book. `rpc.rs`: the three commands are routable and `market_movers` rejects a non-numeric `min_volume`. Web: `check-rpc-commands.py` and `pnpm build`.
- **M8 — Acceptance.** On ockohome: the overview lists about as many rows as `item_stats` has, sorting by profit puts the widest spreads first, and clicking a row lands on Price History with that item selected; movers show plausible items with the change matching Price History's daily chart for one spot-checked item; warm-up shows `warm = 0` before 2026-09-22 with a non-zero projection for the 22nd; Price History for a busy item shows recent trades whose prices sit inside the daily min/max and a book whose top sell is at or above the hourly chart's latest min-sell.

## 24. Amendments from phase 6c planning (2026-09-16)

These amendments take precedence over the earlier sections. warframe.market publishes per-item closed-trade statistics (the series the desktop Quantframe app used) only on its v1 API, at `GET /v1/items/{slug}/statistics`. The user chose to import the 90-day daily series once so the Movers tab and the Price History daily chart have history from day one. This section adds the second permitted v1 call to §4. Probed live 2026-09-16: `payload.statistics_closed["90days"]` holds up to 90 rows per sub-type with `datetime` (midnight UTC), `volume`, `min_price`, `max_price`, `median` (all prices as floats), plus `mod_rank` on ranked mods and `subtype` (for example `intact`) on relics; unranked items carry neither. `statistics_closed["48hours"]` is hourly and `statistics_live` describes open listings; both are ignored.

- **K1 — Mapping.** Each 90-day row becomes one `item_stats_daily` row: `item_id` is the cache item's `wfm_id`, `sub_type` is `sub_type_key(mod_rank, None, subtype, None, None)` (so `""`, `rank=10` or `subtype=intact`, the collector's own key), `day` is the first ten characters of `datetime`, `volume` as is, `median` as is, `min_price` and `max_price` rounded to integers.
- **K2 — Write rule.** Rows are written with `INSERT OR IGNORE` on the table's primary key `(item_id, sub_type, day)`. A day the collector has already produced is never overwritten, and re-running the import after a restore or a partial run is safe. The backfill writes nothing else: `item_stats`, `sweep_state`, `vanished_orders` and `sweep_summary*` are untouched, so `history_days`, `warm` and every trader input stay exactly as the collector computes them. Backfilled days show up in the Price History daily chart and in Movers only.
- **K3 — Job.** `collector::backfill::run` walks every item in the tradable cache (about 3 840 slugs), fetches the statistics through the limiter's `Hot` lane with the collector's headers (`Platform: pc`, `Language: en`), retries transient failures twice with the same jitter rule as `fetch_with_retries`, reports a 429 to the limiter, counts a 404 as `missing` and gives up on an item after the retries as `failed`, and inserts per K2. It logs `[Backfill] Started: N items`, a progress line every 500 items (`done/total, days inserted so far`) and `[Backfill] Finished: items N, days D, missing M, failed F` in the same form. Progress and outcome live in a process-wide status: `{ state: idle | running | done | failed, started_at, finished_at, items_total, items_done, days_inserted, items_missing, items_failed, last_error }`. Only one run at a time; a second start while running is a no-op that returns the current status. A run that cannot list the cache or open the database ends `failed` with `last_error` set. The job is not restarted by the supervisor and does not survive a container restart; the user presses the button again, and K2 makes that harmless.
- **K4 — RPCs and UI.** `market_backfill_start {}` starts the job (or returns the current status if one is running) and `market_backfill_status {}` returns the status; both in `commands/market.rs`. The Collector tab gains a "Historical data" block at the bottom with a button `Import 90-day history from warframe.market`, disabled while running, and a status line: idle, `Importing… done/total items, D days added`, `Finished at T: N items, D days added, M missing, F failed`, or `Failed: <error>`. The web polls `market_backfill_status` every 5 s only while the state is `running`. Strings under `pages.market_data.tabs.collector.backfill.*`.
- **K5 — Not built.** No hourly import, no scheduled or automatic re-run, no per-item import, no cancel button (the run finishes on its own in about twenty minutes at the limiter's pace), no change to warm-up or the trader.
- **K6 — Tests.** `collector/backfill.rs`: parsing a fixture body with a ranked mod (two `mod_rank` values), a relic (`subtype`) and a set (neither) yields the expected `sub_type` keys, days and rounded prices, and an empty or malformed `90days` is an error; `insert_missing` against the test database inserts new days and leaves an existing `(item, sub_type, day)` row untouched; `run` with a fake source over three items (one OK, one 404, one transient-then-OK) produces the expected counts and status transitions. `rpc.rs`: both commands are routable. Web: `check-rpc-commands.py` and `pnpm build`.
- **K7 — Acceptance.** On ockohome: press the button; the Log tab shows `[Backfill] Started` and progress lines about every 500 items; the button stays disabled and the status line counts up; within about 25 minutes the status reads Finished with items ≈ 3 840, days in the hundreds of thousands, a small missing count and failed 0 or near it; Price History for Ash Prime Set shows about 90 daily bars; Movers 7 d lists are populated; Warm-up still shows warm = 0 (before 2026-09-22) and the projection is unchanged; pressing the button again finishes quickly with days added 0.

## 25. Amendments from phase 6d planning (2026-09-20)

These amendments take precedence over the earlier sections. The user's bar for the trader is parity with, or better than, the desktop Quantframe app. The decision logic is already a port (§5.6, C1), so the gap is the price input. Measured on ockohome on 2026-09-20 over 2 698 items, the collector's inferred `moving_avg` against warframe.market's closed-trade medians: median absolute error 6.2 % (2.9 % for items at 20 p or more with 10 or more trades a day), a signed bias of +2 % to +5 %, and inferred `volume` at 0.5× to 0.65× the closed volume. `moving_avg` is the trader's `closed_avg` (`item.rs`), which sets `potential_profit` on every buy, so the bias makes buys look more profitable than they are, and the volume undercount makes the volume filter reject items the desktop app would trade. This phase adds warframe.market's closed-trade daily series as a second, switchable price basis, keeps the collector's live data where it is the better signal, and lets the user compare both before switching. Probed live 2026-09-20: a `statistics_closed["90days"]` row carries `datetime`, `volume`, `min_price`, `max_price`, `open_price`, `closed_price`, `avg_price`, `wa_price`, `median`, `moving_avg`, `donch_top`, `donch_bot` and no `order_type`; yesterday's row was present by 08:05 UTC.

- **P1 — Tables.** A new migration creates `closed_stats_daily (item_id TEXT, sub_type TEXT, day TEXT, volume INTEGER NOT NULL, median REAL, min_price INTEGER, max_price INTEGER, avg_price REAL, wa_price REAL, PRIMARY KEY (item_id, sub_type, day))` with an index on `day`, and `closed_fetch_state (item_id TEXT PRIMARY KEY, fetched_at TEXT NOT NULL, outcome TEXT NOT NULL)` where `outcome` is `ok`, `missing` or `failed`. Rows map as K1 does, plus `avg_price` and `wa_price` as is. This table has a single kind of writer, so rows are upserted (`ON CONFLICT (item_id, sub_type, day) DO UPDATE`). `item_stats_daily` and K2 are unchanged: the charts and Movers keep reading what they read today.
- **P2 — Refresh job.** `collector::closed::refresh_loop` runs supervised as `Collector:ClosedStats`. Every `CLOSED_PACE_S = 10` s it takes the next stale item and fetches its statistics through the limiter's `Cold` lane with K3's headers and retry rule (`fetch_with_retries` gains a lane parameter). An item is stale when it is `active` in `sweep_state` and has no `closed_fetch_state` row, or its `fetched_at` is before the cutoff (the most recent 00:30 UTC), or its outcome is `failed` and `fetched_at` is more than 1 h old. Order among stale items: hot-set items first, then never fetched, then oldest `fetched_at`, then `item_id`. With nothing stale it sleeps 60 s. `ok` upserts the days and writes the state; a 404 writes `missing`; exhausted retries write `failed` and log a warning. When a pass drains (stale count reaches zero after having been above it) it logs `[ClosedStats] Pass complete: ok N, missing M, failed F`. Cost: 0.1 req/s, about 3 % of the shared budget; a full pass of about 3 840 items takes about 10.7 h, and a hot set of about 300 is fresh within an hour of the cutoff. The hourly maintenance deletes `closed_stats_daily` rows older than 90 days. The 6c import button's job also upserts into `closed_stats_daily` and writes the fetch state, so one press fills the new table in about 70 minutes instead of waiting for the first slow pass; its label, status shape and K2 behaviour are unchanged.
- **P3 — Closed stats.** `collector::closed::aggregate` is a pure function over the rows of the window W, the seven UTC days `today − 7 ..= today − 1`. For each `(item_id, sub_type)` with at least one row in W: `volume` = Σ volume ÷ 7 (trades per day, the unit `item_stats.volume` already uses); `moving_avg` = mean of the daily `median` values present; `median` = median of those daily medians; `avg_price` = Σ(`wa_price` × `volume`) ÷ Σ `volume` over the two most recent days in W that have volume, falling back to `moving_avg`; `min_price` and `max_price` over W; `week_price_shift` = the newest day's `median` minus the oldest day's `median` in W, absent with fewer than two days; `days` = days with a row; `trades` = Σ volume; `warm` = `days ≥ 5 AND trades ≥ 10` (the 10 is `StatsConfig::warm_min_trades`). Closed stats count as **fresh** only when the item's fetch state is `ok` and `fetched_at` is within 3 days. If warframe.market retires the endpoint, every item goes unfresh within 3 days and the trader falls back to inferred stats by itself.
- **P4 — Blend.** `trader::blend::blend(inferred, closed, mode, guard_pct)` is pure and returns one `Effective { stats: ItemStats, week_price_shift: Option<f64>, guarded: bool, closed: bool }` per key in the union of both inputs. Mode `inferred`: the inferred rows exactly as today, no shift, never guarded. Mode `closed`: a key with fresh closed stats takes `volume`, `moving_avg`, `median`, `avg_price`, `min_price`, `max_price` and `warm` from them and keeps `profit`, `history_days` and `updated_at` from the inferred row (`profit` absent and `history_days` 0 when there is none); a key without fresh closed stats keeps its inferred row unchanged. `profit` stays inferred on purpose: closed rows carry no order side, and the collector's figure (median vanished sell minus median vanished buy, or the live in-game spread) is the current flip margin, which is the part of the data where the server is ahead of the desktop app. `ItemStats` itself gains no fields, so its ten struct literals are untouched.
- **P5 — Fast-drop guard.** In mode `closed`, when `fast_drop_guard_pct` is not disabled, the key has an inferred row with `volume × 7 ≥ 20` and an `avg_price`, and that inferred 48 h `avg_price` is below the closed `moving_avg` by more than the percentage, `moving_avg` is replaced by the inferred `avg_price` and `guarded` is set. The closed average is a seven-day figure published once a day; the collector sees a falling market within minutes. Because the inferred figure reads high, the guard errs toward not firing. It only ever lowers `closed_avg`: buys get more conservative, and the sell floor `closed_avg − min_sma` follows the market down. The trader adds the informational operation `FastDropGuard` on a guarded item's buy and sell paths, so the dry-run log shows how often it fires. No other use of the inferred series in pricing.
- **P6 — Filters.** `get_interesting_items` applies `week_price_shift ≥ price_shift_threshold` when the setting is not disabled and the item has a shift (it has none in mode `inferred`, so nothing changes there). For this setting alone, disabled means exactly `-1`: a shift threshold is naturally negative ("not falling by more than 5 p" is `-5`), so the shared `is_disabled` rule (`≤ -1`) does not apply to it and is left unchanged for every other threshold. A threshold of exactly −1 p therefore cannot be expressed. `ItemPriceInfo.week_price_shift` is filled from the blend and `ItemPriceInfo` gains `guarded: bool`. `ItemPriceInfo.trading_tax` is filled from the tradable cache's `trade_tax`, and `trading_tax ≤ trading_tax_cap` applies when that setting is not disabled; this one is independent of the mode, and the setting is disabled by default, so nothing changes unless the user has set it. `min_wtb_profit_margin` still always passes (P9).
- **P7 — Settings and callers.** `live_scraper.general.price_source` is `inferred` or `closed`, default `inferred`, so deploying this phase changes no trading behaviour. `live_scraper.general.fast_drop_guard_pct` is an integer, default 10, disabled at −1 like the other thresholds. Both are read on every price load, so a change applies on the next trader cycle and the next hot-set refresh; the runbook tells the user to change the source only in dry-run. `price_source::effective_stats(conn, mode, guard_pct, now)` loads both inputs and blends; `StatsPriceSource::load`, `market_overview` and `market_warmup` use it, so the hot set, the trader, the Warm badge and the Warm-up counts all show the effective values. The Warm-up projection is computed from `history_days` and is meaningful only in mode `inferred`. The Settings → Live Scraper → General tab gains a select and a number input for the two settings.
- **P8 — Comparison RPC and tab.** `market_price_sources {}` returns `{ mode, guard_pct, refresh: { active, ok, missing, failed, stale, oldest_fetched_at }, candidates: { inferred, closed, both }, rows }`. A row is `{ item_id, sub_type, name, wfm_url, inferred_volume, inferred_moving_avg, closed_volume, closed_moving_avg, closed_days, week_price_shift, profit, warm_inferred, warm_closed, candidate_inferred, candidate_closed, guarded, fetched_at }`, one per key that has either source and a tradable name. `candidate_*` is membership in `get_interesting_items` under each mode with the current settings, which is the evidence for switching. Market Data gains a **Price source** tab: the refresh status line, the three candidate counts, and a sortable table with a search box and a "differences only" switch (candidate membership differs, or the two moving averages differ by more than 10 %). Strings under `pages.market_data.tabs.price_source.*`.
- **P9 — Not built.** No profit-margin definition, so `min_wtb_profit_margin` still always passes; it is off by default. No use of the hourly `48hours` series or of `statistics_live`. No automatic switch of `price_source`. No change to which table the charts and Movers read. No per-item source override.
- **P10 — Tests.** `collector/closed.rs`: the parser fills `avg_price` and `wa_price` from the fixture; upsert replaces an existing day; `next_stale` honours the cutoff, the 1 h failed rule, inactive items and the hot-first order; `aggregate` on a hand-built week gives the expected `volume`, `moving_avg`, `median`, volume-weighted `avg_price`, shift, `days`, `trades` and `warm`, and ignores days outside W; `refresh_once` with a fake source writes `ok`, `missing` and `failed` states. `collector/backfill.rs`: a run also fills `closed_stats_daily`. `trader/blend.rs`: mode `inferred` is the identity; mode `closed` takes the closed fields and keeps the inferred `profit`; unfresh closed stats fall back; closed-only keys appear with `profit` absent; the guard fires only past the percentage and the trade count and never when disabled. `trader/price_source.rs`: the shift filter, that it passes items without a shift, and the trading-tax cap. `trader/item.rs`: a guarded price tags the order with `FastDropGuard`. Settings: a pre-6d settings body loads with `inferred` and 10. `rpc.rs`: `market_price_sources` is routable. Web: `check-rpc-commands.py` and `pnpm build`.
- **P11 — Acceptance and the go-live rule.** Deploy with `price_source = inferred`: the Dry-run log keeps the same cadence and the Log tab shows `Collector:ClosedStats` fetching. Press the 6c import button once and wait for Finished; the Price source tab then shows `ok` near 3 840 and closed values for Ash Prime Set close to its warframe.market statistics page. **Calibration:** for five items the user trades, compare the tab's closed `volume` and `moving_avg` with what the desktop Quantframe app shows for the same items; a systematic factor on `volume` (for example a weekly total instead of a daily mean) is a spec error to fix before switching, not something to tune around. Then, in dry-run, set `price_source = closed`; the next cycle's item count follows the tab's `closed` candidate count and `FastDropGuard` appears on few or no items. **Live trading in mode `closed` needs at least 48 h of dry-run in that mode with the Summary reviewed**, whether that falls before or after the phase 5 flip; `docs/GO-LIVE-RUNBOOK.md` gains that pre-flight line and the rollback (set `price_source` back to `inferred`, effective next cycle). The phase 5 flip itself can go ahead on the inferred source as planned.
- **P12 — Amendments from the final whole-branch review (2026-09-20).** These override P2–P4 and P8 where they differ.
  - **Cutoff.** The daily cutoff is **08:30 UTC**, not 00:30. The only evidence of when warframe.market publishes yesterday's row is the P-series probe ("present by 08:05 UTC"); a hot item fetched at 00:31 without that row would be stamped `ok` and keep a stale window for 24 h. A pass now runs from about 08:30 to about 19:15 UTC.
  - **Window anchored on the fetch.** What an item's data can contain depends on when *it* was fetched, not on the clock now. Let `A` be the UTC date of `cutoff(fetched_at)`, the cutoff in force at the item's own fetch; the item's window is the seven days `A − 7 ..= A − 1`. So an item fetched 2026-09-20 16:00 UTC uses 09-13..09-19 whether it is read that evening or at 03:00 the next morning, and an item the pass missed for a day keeps a full, older week instead of a short one. `load_fresh` groups rows by `A` and calls `aggregate` once per group with `today = A`; `aggregate` itself is unchanged, and the query reaches back `WINDOW_DAYS + FRESH_DAYS + 1` days so every fresh item's window is covered. (The first wording of this point compared the fetch with the *current* cutoff, which still left a six-day window between 00:00 and 08:30 UTC; the scoped re-review caught it.) Without this, every item not yet refetched today would read a six-day window and `volume` at 6⁄7 of the truth, a daily sawtooth in the candidate set and a false "systematic factor" in P11's calibration.
  - **Warm gate.** In mode `closed`, `warm` is the closed `warm` **and** the collector knows the key: a key with no inferred row is never warm. `warm` is the live/dry-run gate (`route_for`), and on `main` an unknown key already routes to dry-run; a closed-only key must not change that. The closed bar stays at 5 of 7 days because warframe.market publishes no row for a day without closed trades, so an item with ten trades on five days is liquid; the inferred bar of 7 counts days of observation, which is a different thing.
  - **Warm-up tab.** `Effective` gains `inferred: bool` (the key has an inferred row). `market_warmup` counts only those keys, so `tracked`, the history histogram and the projection stay on the collector's own universe, while `warm` and the trades histogram show effective values. `market_overview` keeps every key.
  - **Sub-type classes that never match.** Closed rows are keyed by rank or relic refinement only. Items whose collector key carries `charges`, `amber` or `cyan`, and mod ranks warframe.market does not publish, keep their inferred row in mode `closed`; the item's closed row sits on a separate key that is never warm (previous point). This is safe and expected.
  - **Price source tab.** The table shows `warm_inferred` and `warm_closed`, and "differences only" also includes a row whose two warm flags differ, and a row that is an inferred candidate but has no closed values.
  - **Import order.** The import job writes `item_stats_daily` first, exactly as before this phase, and only then the closed table and the fetch state, so a failure on the new table cannot cost the old one a row.
  - **Pre-deploy check.** Before deploying, the user confirms that `live_scraper.items.wtb.trading_tax_cap` and `live_scraper.items.wtb.price_shift_threshold` are both `-1` in the server's settings: the first filter is new and mode-independent, the second goes live in mode `closed` for any value other than `-1`. The runbook carries both lines, plus: switching to `closed` re-arms live routing for every item whose closed stats are warm, so switch only with global dry-run on.
- **P13 — Amendments from acceptance (2026-09-20).** The desktop app caches upstream's price data in `~/.local/share/dev.kenya.quantframe/cache/items/ItemPrices.json`; read on 2026-09-20 it settles two things. Its `volume` values are whole numbers divided by seven, so upstream's unit is mean closed trades per day over a week, the unit P3 chose: P11's calibration passes. And it lists mods and arcanes **only at their maximum rank** (`rank` 3, 5 or 10, never 0), while the server's `item_stats` and closed keys include `rank=0` rows, several of which top the candidate list by volume.
  - **Max-rank candidates.** `ItemPriceInfo` gains `max_rank: Option<i64>`, filled in `StatsPriceSource::load` from the tradable cache's `sub_type.max_rank` (absent for items without ranks). `get_interesting_items` drops an item whose `sub_type` carries a `rank` lower than its `max_rank`. Items with no `max_rank`, and sub-types with no `rank` (sets, parts, relic refinements, sculptures), are untouched. This applies in **both** modes on purpose: it is a parity fix, not a property of the closed basis, so it changes the default-mode candidate list and therefore the collector's hot set. It filters buy candidates only: `find_by` still returns price info for a rank-0 stock or wish-list item, so selling and wish-list buying are unchanged. The Price source tab's `candidate_*` flags follow, because `compare` uses the same function with the same lookup (`ItemLookup` gains `max_rank`).
  - **Language file caching.** The web client fetches `/lang/<lang>.json` from script, which browsers cache heuristically because the server sends no `Cache-Control`; a deploy that adds strings then shows raw keys until the cache expires, and a hard refresh does not reach a script-initiated fetch. Both fetches (`App.tsx` `initializeI18n`, `contexts/app.context.tsx` `loadLanguage`) pass `{ cache: "no-cache" }`, which revalidates against the server's `Last-Modified` and costs one conditional request per page load.
  - **Recorded, not built.** Upstream's `profit` is the mean daily closed price range (`max_price − min_price`), not a buy/sell spread, and its `profit_margin` is `profit ÷ avg_price × 100`; upstream applies no 150-candidate cap (213 of its 1 467 rows pass the default filters). The server keeps its live-spread `profit` and its cap; whether to change either is a separate decision for the user.
- **P14 — Orphaned buy orders (2026-09-20, before the phase 5 flip).** A trader cycle only visits the items in `collect_interesting_items`. A buy order for an item that has left that list (it fell under a threshold, out of the 150 cap, or was a rank-0 candidate before P13) is never visited again, and with `auto_delete` off — the user's go-live choice, so the first live start adopts the existing orders — nothing else removes it: `orders_to_delete` returns nothing while Buy, Sell and WishList are all enabled. Live, that is a standing bid at a stale price.
  - **Rule.** At the end of a cycle that ran to completion, a buy order in the order cache is an **orphan** when no entry of that cycle's interesting list carrying the `Buy` or `WishList` operation has the same `(item_id, sub-type key)`, and the item is not blacklisted for `TradeMode::Buy` (the blacklist is how the user tells the trader to leave an item alone, as in the `auto_delete` sweep). Sell orders are never touched by this rule.
  - **Grace.** An order must have been an orphan continuously for `ORPHAN_GRACE = 30 min` before it is deleted; an order that is covered again, or gone, is forgotten. The inferred candidate list is recomputed from statistics that change every few minutes, so an item on a threshold or on the cap boundary can flicker; without the grace that would be a delete and a create on warframe.market every cycle. The first-seen times live in memory on the `ItemTrader` (one per trader run), so a restart begins a new grace period.
  - **Deletion** goes through `TradeOrders::delete` with the reason `NotCandidate`, so it is simulated and logged to `dry_run_log` under global dry-run exactly like every other delete, and it counts toward the consecutive-failure stop like every other live write. A cycle that was cut short (`should_stop`) or that processed no items does not sweep. The rule applies only while `TradeMode::Buy` is enabled; with Buy disabled `orders_to_delete` already removes every buy order.
  - **Readable log.** The sweep's log line names the item, its sub-type key when it has one, and the order's price, next to the order id (`Deleted buy order <id> for <name> [<sub-type>] at <platinum>p: its item is no longer a candidate`), and the delete's `WriteMeta.sub_type` carries the sub-type key like every other order write, so the list can be read from the Log tab without the Dry-run log. An item the tradable cache no longer knows is logged by its item id.
  - **No candidates, no sweep (added after the Task 7 review).** A cycle whose interesting list carries no entry with the `Buy` operation while `TradeMode::Buy` is enabled does not sweep: an empty candidate list is an input failure (an empty or restored `item_stats`, a mis-set filter such as `trading_tax_cap` or `price_shift_threshold`), not a trading decision, and without this guard half an hour of it would delete every standing bid at once. The grace clocks are left as they are on such a cycle.
  - **Blacklist.** An order is spared when its item is blacklisted for `TradeMode::Buy` **or** `TradeMode::WishList`: both mean "leave this item's buy side alone", and a wish-list row blacklisted for `WishList` produces no covering entry.
  - **Clock.** The grace uses the wall clock. A backwards jump cannot make an order due; a forward jump (time correction, resume from suspend) can make one due early, which is accepted for a 30-minute grace.
  - **Consequence to know.** A buy order the user places by hand on warframe.market for an item that is neither a candidate nor on the wish list will be deleted after 30 minutes of the trader running, unless the item is blacklisted for buying. With `auto_delete` on (upstream's default) every order was already wiped at each start, so this is not a new kind of ownership, but it is now continuous.
  - **Tests.** Pure `orphan_buy_orders` (covered by a Buy entry, covered by a WishList entry, a Sell-only entry does not cover, sub-type must match, blacklisted item is skipped, sell orders never returned) and pure `due_orphans` (not due before the grace, due after it, forgotten when covered again, forgotten when the order disappears); one `ItemTrader::check` test under global dry-run showing no delete on the first cycle and a `NotCandidate` delete once the grace has passed.
- **P15 — Correction (2026-09-21): the trading-tax filter of P6 is inert.** The server's `game_data` loader sets `trade_tax` to 0 for every item, because warframe.market's v2 item list carries no tax, so `ItemPriceInfo.trading_tax` is always 0 and the cap never excludes an item. P6's premise that the tradable cache already carried the tax was checked against the desktop app's cache file, not against the server's loader. The filter stays in place and harmless; making it work needs tax data (derived from rarity and type, or read from the v2 item detail) and is a follow-up. P12's pre-deploy check of `trading_tax_cap` is therefore unnecessary until then.
- **P16 — Upstream's profit definition as an option (2026-09-21).** Measured on 2026-09-21 against the desktop app's own cached data and settings, the closed-mode price anchors agree with upstream (median +1.2 %, 90 % within 5 %), but candidate selection does not: the server buys 150 items and the desktop app 213, with 124 in common. All 26 server-only items are ones upstream rejects on `profit`, and most of the 89 desktop-only items are busy ones whose live buy/sell gap is thin. The cause is the meaning of `profit`: upstream's is the mean daily closed price range (`max_price − min_price` per day, averaged over the week; verified on Ash Prime Set, 70.0 − 65.571 = 4.429), while the server's is the live in-game spread or median vanished sell minus buy (§5.5). `ItemPriceInfo.profit` is read by exactly one decision, the `profit > profit_threshold` candidate filter, so the definition can be switched without touching order pricing.
  - **Closed stat.** `ClosedStats` gains `range_profit: Option<f64>`: the mean of `max_price − min_price` over the days in the item's window W that carry both; absent when no day does. `closed_stats_daily` already stores both columns (as rounded integers, K1), so no schema change and no new request.
  - **Setting.** `live_scraper.general.profit_basis` is `spread` or `range`, default `spread`, so deploying changes nothing. It takes effect only in price mode `closed` and only for a key that has **an inferred row**, fresh closed stats and a `range_profit`: there the blend sets `profit` to `range_profit`. A key the collector has never seen keeps no profit, exactly as under `spread`: it can never be `warm` (P12), so as a candidate it would only hold a slot and a hot-set place for an item that cannot trade live. The Overview tab's column for this figure is labelled "Profit", not "Spread", because under `range` it shows the range for closed keys. Every other key, and every key in mode `inferred`, keeps the spread profit. Read on every price load, like `price_source`.
  - **Plumbing.** `blend` takes the basis as a parameter; `source_settings()` returns a struct `SourceSettings { mode, guard_pct, profit_basis }` in place of the tuple. `compare` computes `candidate_closed` with the current basis, so the Price source tab shows what the trader would buy; the tab's row gains `closed_range_profit`, shown as a column next to the existing (spread) profit, and the header line names the basis. The General settings tab gains a select for it.
  - **Not built.** `profit_margin` (`profit ÷ avg_price × 100`) and the `min_wtb_profit_margin` filter stay unbuilt (the desktop app has it at −1); the 150-candidate cap stays. With `range`, more items pass the profit filter than the cap admits, so the cap, which sorts by volume, decides the list: expect the busiest items upstream buys (Voruna Prime Set, Volt Prime Set, …) to enter and lower-volume ones to leave.
  - **Tests.** `aggregate` gives the mean daily range and ignores a day missing either price; `blend` leaves `profit` alone for `spread`, in mode `inferred`, and for a key without closed stats or without a range, and replaces it for `range`; `compare` flips a key's `candidate_closed` with the basis; a pre-P16 settings body loads with `spread`.
- **P17 — The buy-candidate limit becomes a setting (2026-09-21).** C2 capped the buy candidates at a constant 150, sorted by volume. The desktop app has no such limit: with the user's thresholds it works 213 items. `live_scraper.items.wtb.max_buy_candidates` replaces the constant (it lives with the other buy thresholds because `get_interesting_items` already receives `ItemSettings`, so no signature changes and `compare` and the collector's hot set follow by themselves): an integer, default `150`, so deploying changes nothing, and disabled at `-1` like the other thresholds, in which case every item that passes the filters is a candidate. The sort by volume stays, so a finite limit still keeps the busiest items. The constant `MAX_BUY_CANDIDATES` remains only as the default's value. The WTB settings form gains a number input beside the other thresholds. What lifting it costs, for the runbook: one order-book fetch and one order rewrite per candidate per cycle against the shared three-requests-a-second limit (about 430 requests, two and a half to three minutes, at 213 items); a hot set of the same size for the collector's five-minute sweep; and warframe.market's per-account order cap, past which the trader logs `has reached the order limit. Skipping.` and posts nothing more.
- **P18 — Trade tax from warframe.market (2026-09-21); replaces P15's "inert" state.** warframe.market's v2 item *list* carries no tax, which is why `game_data` fills `trade_tax` with 0, but the item *detail*, `GET /v2/item/{slug}`, does: probed 2026-09-21, `data.tradingTax` is 2 100 000 for Arcane Energize, 1 000 000 for Primed Flow, 8 000 for Blind Rage and 8 000 for Ash Prime Set (the desktop app's cache says 2 000 for that set, a per-part figure; the server uses warframe.market's number, the only source it can reach, and for a cap meant to exclude million-credit items the difference is immaterial).
  - **Table.** A new migration creates `item_trade_tax (item_id TEXT PRIMARY KEY, trading_tax INTEGER NOT NULL, fetched_at TEXT NOT NULL)`.
  - **Loop.** `collector::trade_tax` runs supervised as `Collector:TradeTax`: every `TAX_PACE_S = 10` s it takes the next active `sweep_state` item with no row, hot-set items first then by `item_id` (reusing `closed::pick`), fetches its detail through the limiter's `Cold` lane with the collector's headers and the retry rule of `backfill::fetch_with_retries` (generalised over the URL, or a small sibling if that is cleaner), and upserts the tax. A 404 or an unparseable body stores nothing and the item is left out for the rest of the process (a restart retries it): that answer will not change, and re-picking it every ten seconds would block the queue. An exhausted *transient* failure or a 429 stores nothing either, but only sets the item aside for **one hour** (the closed-statistics loop's `failed` rule), so a network outage or a rate-limit storm cannot strand items until a restart. A body without `tradingTax` stores 0; a `tradingTax` that is present but not an integer also stores 0 and logs a warning, so a change in warframe.market's schema is visible rather than a silent universal pass. Taxes do not change, so each item is fetched once; with nothing missing the loop sleeps `TAX_IDLE_S = 600` s, which also picks up items added by the daily item refresh. A full first pass is about 3 840 requests, 10.7 h at this pace, 0.1 req/s beside the closed-statistics loop's 0.1.
  - **Use.** `StatsPriceSource::load` and `market_price_sources`' lookup read a `HashMap<item_id, trading_tax>` loaded from the table on every price load (one small query) instead of the tradable cache's always-zero `trade_tax`; `with_trade_tax`'s closure signature is unchanged. An item with no row yet has tax 0 and passes the cap: **fail open**, because failing closed would stop all buying for the hours the first pass takes. The cap is therefore effective for the hot set about 25 minutes after a deploy and for everything after about eleven hours. `CacheTradableItem.trade_tax` stays 0 and is no longer read by the trader.
  - **Tests.** Parsing a detail body (with and without `tradingTax`; malformed is an error); `missing_items` lists active items without a row and skips inactive ones; `fetch_once` with a fake source stores a tax, stores nothing on 404, and returns `None` when nothing is missing; `load_all` returns the map; `get_interesting_items` with a tax map excludes an item above the cap and keeps an unknown one.
- **P19 — Set folding uses each part's quantity in the set (2026-09-23, amends E7).** Live on 2026-09-23 04:36 UTC a 51 p purchase of a Kogake Prime Set arrived from `EE.log` as Blueprint ×1, Gauntlet ×2, Boot ×2 (Kogake Prime needs two gauntlets and two boots). `sets::fold_sets` treats every part as needed once per set: it took one of each, recorded one set, and left Gauntlet ×1 and Boot ×1 as stock rows with no order behind them, which the trader then tried to list. warframe.market's `GET /v2/item/{slug}` carries `quantityInSet` on each **part** (gauntlet: 2); the root's detail lists only the part ids.
  - **Parts carry a quantity.** `PartsMap` becomes `HashMap<String, Vec<SetPart>>` with `SetPart { slug: String, quantity: i64 }`. When a root is first cached, the cache fetches the root's detail for the part ids as today, then each part's detail for its `quantityInSet` (absent or non-positive → 1), through the same limiter lane. A failure on any part leaves the root uncached, as an empty list does today, so an incomplete set is never folded as complete.
  - **Folding.** `sets = min over parts of (available quantity of that part ÷ needed quantity)`, integer division; each part loses `needed × sets`; leftovers stay, as before. A set whose every part needs one behaves exactly as today.
  - **Cache file.** The on-disk format changes, so the file is `sets_v2.json` (`SETS_FILE`); an old `sets.json` is ignored and can be deleted by hand. Roots are re-fetched once.
  - **Tests.** Kogake-shaped fixture (blueprint ×1, gauntlet ×2, boot ×2 → one set, no leftovers; gauntlet ×3, boot ×2 → one set and Gauntlet ×1 left; gauntlet ×1 → nothing folds); the Wolf Sledge tests unchanged in outcome with quantity 1; `quantityInSet` parsing (present, absent, non-positive); a part-detail failure leaves the root uncached.
- **P20 — Helper trades carry the item's default market variant (2026-09-24, amends E6/E8).** Live on 2026-09-23 04:54 UTC a 60 p purchase of Archon Vitality rank 10 arrived from `EE.log` as name + rank only. `resolve::resolved` built `SubType::rank(10)` and the stock row was stored as `{"rank":10}`. On warframe.market Archon Vitality has variants (`regular`, `atragraph`) and every order carries one; the trader filters orders by exact sub-type equality, so the sell pass matched zero of the 286 rank-10 `regular` sellers and stamped the row `NoSellers` every cycle (six times in two hours, the only stock row affected). The buy side was unaffected because buy candidates come from `item_stats` whose key is `rank=10;subtype=regular`. The same loss hits any helper-detected purchase or sale of an item whose listing has variants (Archon Vitality today; relics and Ayatan sculptures in principle), and a sale of such an item would fail to match a stock row that does carry the variant.
  - **Rule.** When the tradable cache says the item has variants (`CacheTradableItem.sub_type.variants` non-empty) and the trade names none (it never does: the in-game log has no variant), the resolved sub-type takes the **first listed variant** (`regular` for mods, `intact` for relics), alongside the capped rank when the trade has one. An item without variants resolves exactly as today. The trade log cannot say more, so this is the best default; a wrong guess is visible as a stock row whose variant the user can edit.
  - **Data repair.** The existing Archon Vitality stock row (id 36) is set to `{"rank":10,"variant":"regular"}` by hand after the deploy; the purchase transaction (id 314) is left as recorded.
  - **Tests.** A ranked item with variants resolves to rank + first variant; an unranked item with variants resolves to the variant alone; an item without variants is unchanged (existing assertions); `resolve_trade` on an Archon-shaped trade yields the variant.
