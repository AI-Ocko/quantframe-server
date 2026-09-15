# quantframe-server — Design Spec

- **Date:** 2026-09-14
- **Status:** Approved 2026-09-14. Amended by §14 (phase 1), §15 (phase 2) and §16 (phase 3) planning.
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

- **warframe.market sign-in** uses `POST /v1/auth/signin`, the only permitted v1 call. Everything else uses v2. Only the token is kept (valid ~60 days), encrypted with AES-256-GCM. The password is never stored.
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
| DB migration | Backup copy first (upstream behaviour); daily backup, keep 7 |

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
5. **Go-live.** Review the dry-run output and turn global dry-run off.

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
