# Phase 1: Headless Port — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task, **inline in the main session**. The user has ruled out subagent-driven development for this project: subagents may only explore the repo or write docs. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run quantframe as a headless Rust server on the homelab and use its React UI in a browser. That means stock, wish list, transactions, trade entries and warframe.market orders can be managed without Tauri and without the Quantframe API.

**Architecture:**
- Copy quantframe-react at `3d59c4e7` into a Cargo workspace.
- Replace the three Tauri seams: the global AppHandle becomes OnceLock state accessors, `app.emit` becomes a tokio broadcast channel, and `#[tauri::command]` becomes a `POST /rpc/{name}` dispatch table.
- Replace Quantframe's item cache with warframe.market's v2 item list.
- An axum binary adds password login, `/rpc`, `/ws`, static files and sounds.
- The React app swaps `invoke`/`listen` for fetch/WebSocket, and the removed features are deleted.

**Tech Stack:**
- Rust 1.95 (workspace `edition = "2021"`; `utils` keeps `2024`)
- axum 0.8 (`ws`), tower-http 0.6 (`fs`), tokio 1
- sea-orm 0.12 on SQLite
- wf-market (git rev `aba1d26`)
- aes-gcm 0.10, argon2 0.5
- React 18, Vite, Mantine, pnpm 11.3.0
- Docker Compose on the homelab

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`. Read §14 (amendments from this planning pass) first.

## Global Constraints

- **Upstream source:** quantframe-react commit `3d59c4e7`, read-only, at `~/Projects/Personal/quantframe-react`. Never modify that repo. Its working tree has local edits (`src-tauri/qf_api/src/client.rs`, `src-tauri/tauri.conf.json`), so always copy from the commit with `git archive`, never from the working tree.
- **License:** GPLv3. Keep upstream `LICENSE`; the README credits Kenya-DK.
- **API calls:** warframe.market v1 is allowed **only** for `POST /v1/auth/signin` (via `wf_market::Client::login`). Every other call uses v2. No calls to `api.quantframe.app` may remain (`grep -rn quantframe.app crates web/src` must be empty at the end of phase 1).
- **Token storage:** the WFM token is stored only AES-256-GCM encrypted, in table `wfm_account`. It is never written to `auth.json` or logs, and the password is never stored.
- **Docker:** runs only on the homelab server, never on the desktop. Tasks 1–9 run on the desktop with `cargo` and `pnpm`; Task 10 runs on the server.
- **Commits:** conventional commits, ending with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29
  ```
- **Web access:** LAN only.
  - **Session cookie:** `qf_session` with `HttpOnly; SameSite=Strict`, 30-day Max-Age.
  - **Origin check:** non-GET requests and `/ws` must send `Origin == QF_PUBLIC_ORIGIN`.
- **Dry-run default** applies from phase 3. Phase 1 contains no trading code: `live_scraper` and `log_parser` are **not copied** and are ported from upstream in phases 3 and 4.
- **Paths:** all paths below are relative to `~/Projects/Personal/quantframe-server` unless they start with `upstream:`, which means `~/Projects/Personal/quantframe-react` at `3d59c4e7`.

## File Structure (end of phase 1)

```
Cargo.toml                       workspace
crates/
  entity/ migration/ service/ utils/   copied from upstream:src-tauri/*
  qf_core/                        library: upstream:src-tauri/src minus cut modules
    src/lib.rs                    module list + DATABASE/HAS_STARTED/APP_ERROR globals
    src/paths.rs                  NEW  data/resources dirs, device id
    src/events.rs                 NEW  broadcast channel replacing app.emit
    src/crypto.rs                 NEW  AES-256-GCM SecretKey, JWT expiry
    src/db.rs                     NEW  SQLite connect + WAL + migrations
    src/wfm_account.rs            NEW  encrypted token store
    src/web_auth.rs               NEW  argon2 web password
    src/game_data/mod.rs          NEW  WFM v2 /items → CacheTradableItem
    src/startup.rs                NEW  ordered boot sequence
    src/macros.rs                 MOD  emit_event → events::emit; add_metric/system notification removed
    src/utils/modules/states.rs   MOD  OnceLock accessors
    src/cache/…                   MOD  slim CacheState (tradable_item, theme)
    src/app/…                     MOD  no qf_client, no chat socket, no http_server
    src/commands/rpc.rs           NEW  allowlisted dispatch table
    src/commands/*.rs             MOD  tauri::State params removed
  qf-server/                      binary + lib (for tests)
    src/lib.rs  src/main.rs  src/config.rs  src/auth.rs  src/routes.rs  src/login.html
    tests/http.rs
web/                              React app (upstream:src, public, index.html, package.json, vite.config.ts, …)
  src/api/transport.ts            NEW  fetch-based invoke
  src/api/socket.ts               NEW  WebSocket listen
  src/utils/{openUrl,pickFile,downloadJson}.ts  NEW
resources/sounds/                 upstream:src-tauri/resources/sounds
scripts/check-rpc-commands.py     NEW  frontend command names ⊆ server allowlist
Dockerfile  .dockerignore  compose.yaml  .env.example  README.md
```

---

### Task 1: Import upstream and create the workspace

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `README.md`, `LICENSE` (copied), `crates/{entity,migration,service,utils}/**` (copied), `crates/qf_core/src/**` (copied, pruned), `web/**` (copied), `resources/sounds/*` (copied)

**Interfaces:**
- Produces: workspace members `crates/entity`, `crates/migration`, `crates/service`, `crates/utils`. `crates/qf_core/src` holds source but is not yet a member.

- [ ] **Step 1: Export upstream at the pinned commit into a scratch dir**

```bash
UP=~/Projects/Personal/quantframe-react
SCR=$(mktemp -d)
git -C "$UP" archive 3d59c4e7 | tar -x -C "$SCR"
ls "$SCR"   # expect: LICENSE package.json src src-tauri public index.html vite.config.ts …
```

- [ ] **Step 2: Copy into the new layout**

```bash
cd ~/Projects/Personal/quantframe-server
mkdir -p crates web resources
cp "$SCR/LICENSE" .
for c in entity migration service utils; do cp -r "$SCR/src-tauri/$c" crates/; done
mkdir -p crates/qf_core && cp -r "$SCR/src-tauri/src" crates/qf_core/src
cp -r "$SCR/src" web/src
cp -r "$SCR/public" web/public
cp "$SCR"/{index.html,package.json,pnpm-lock.yaml,tsconfig.json,tsconfig.node.json,vite.config.ts,postcss.config.cjs} web/
cp -r "$SCR/src-tauri/resources/sounds" resources/sounds
```

- [ ] **Step 3: Delete modules that are cut or re-ported in later phases**

Nothing below may be recreated in phase 1.
- `live_scraper` and `log_parser` are re-ported from upstream in phases 3 and 4.
- `http_server`, `wf_inventory` and `qf_api` are gone for good.

```bash
cd crates/qf_core/src
rm -rf live_scraper log_parser wf_inventory http_server main.rs
rm commands/{analytics,alert,auction,chat,item,live_scraper,market,riven,stock_riven,syndicate_price,warframe_gdpr,wf_inventory}.rs
rm types/{chat_link,item_riven,permissions_flags}.rs
rm utils/{auction_ext,auction_list_ext,create_stock_riven_ext,wfm_auction_pagination_query_dto,wfm_chat_pagination_query_dto}.rs
(cd cache/modules && ls | grep -v -E '^(mod|tradable_items|theme)\.rs$' | xargs rm)
(cd cache/types && ls | grep -v -E '^(mod|cache_tradable_item|cache_theme)\.rs$' | xargs rm -r)
cd ~/Projects/Personal/quantframe-server
```

- [ ] **Step 4: Write the workspace manifest**

`Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = ["crates/entity", "crates/migration", "crates/service", "crates/utils"]
```

- [ ] **Step 5: Write `.gitignore` and a minimal README**

`.gitignore`:
```
/target
web/node_modules
web/dist
secrets/
.env
```

`README.md`:
```markdown
# quantframe-server

A headless, self-hosted server version of [Quantframe](https://github.com/Kenya-DK/quantframe-react) by Kenya-DK,
for one user on a home network. Forked from quantframe-react at commit `3d59c4e7` (v1.6.28).

Licensed under GPLv3, like the upstream project.

Design: `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`.
```

- [ ] **Step 6: Verify the copied library crates build on their own**

Run: `cargo check -p entity -p migration -p service -p utils`
Expected: `Finished` with no errors. Warnings are fine.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: import quantframe-react 3d59c4e7 into server workspace

Library crates copied unchanged. Tauri shell, qf_api, live scraper, log
parser, inventory, http server and riven/chat/analytics commands dropped.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 2: `qf_core` foundation — paths, events, crypto, macros

**Files:**
- Create: `crates/qf_core/Cargo.toml`, `crates/qf_core/src/paths.rs`, `crates/qf_core/src/events.rs`, `crates/qf_core/src/crypto.rs`
- Replace: `crates/qf_core/src/lib.rs`, `crates/qf_core/src/macros.rs`
- Modify: `Cargo.toml` (add member)

**Interfaces:**
- **Produces, `paths`:**
  - `Paths::new(data_dir, resources_dir) -> Result<Paths, Error>`
  - `Paths::{sounds_dir, cache_dir, logs_dir}() -> PathBuf`
  - `Paths::device_id() -> Result<String, Error>`
  - `paths::init(Paths)`, `paths::get() -> &'static Paths`
- **Produces, `events`:** `events::emit(channel: &str, payload: impl Serialize) -> usize` and `events::subscribe() -> broadcast::Receiver<serde_json::Value>`. Each frame is `{"channel": String, "payload": Value}`.
- **Produces, `crypto`:**
  - `SecretKey::from_hex(&str) -> Result<SecretKey, Error>`
  - `SecretKey::encrypt(&[u8]) -> Result<(Vec<u8> /*ciphertext*/, Vec<u8> /*nonce*/), Error>`
  - `SecretKey::decrypt(ct: &[u8], nonce: &[u8]) -> Result<Vec<u8>, Error>`
  - `crypto::init_key(Option<SecretKey>)`, `crypto::key() -> Result<&'static SecretKey, Error>`
  - `crypto::jwt_expiry(token: &str) -> Option<DateTime<Utc>>`
- **Produces, macros:** `emit_event!`, `send_event!`, `send_event_update!`, `emit_error!`, `clear_error!`, `emit_startup!`, `emit_update_user!`, `notify_gui!` and `play_sound!`, with the same call syntax as upstream.

- [ ] **Step 1: Create the crate manifest**

`crates/qf_core/Cargo.toml`:
```toml
[package]
name = "qf_core"
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tokio = { version = "1.45.1", features = ["full"] }
reqwest = { version = "0.12.19", features = ["json"] }
chrono = "0.4"
regex = "1.9.1"
uuid = { version = "1.19.0", features = ["v4"] }
aes-gcm = "0.10"
argon2 = "0.5"
base64 = "0.22"
getrandom = "0.2"
hex = "0.4"
wf-market = { git = "https://github.com/KibbeWater/wf-market", rev = "aba1d268a7a0f76d54ba3dcd862d2f3a4f7e3496" }
migration = { path = "../migration" }
service = { path = "../service" }
entity = { path = "../entity" }
utils = { path = "../utils" }

[dev-dependencies]
tempfile = "3"
```

Add `"crates/qf_core"` to `members` in the root `Cargo.toml`.

- [ ] **Step 2: Write failing tests for paths, events and crypto**

Create each file with only its test module for now. The implementation goes above it in Step 4.

`crates/qf_core/src/paths.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_id_is_created_once_and_reused() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path(), dir.path().join("res")).unwrap();
        let first = paths.device_id().unwrap();
        let second = paths.device_id().unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 36);
        assert!(paths.sounds_dir().is_dir());
    }
}
```

`crates/qf_core/src/events.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emitted_frames_reach_subscribers() {
        let mut rx = subscribe();
        let receivers = emit("message", serde_json::json!({"event": "User:Update", "data": 1}));
        assert!(receivers >= 1);
        let frame = rx.recv().await.unwrap();
        assert_eq!(frame["channel"], "message");
        assert_eq!(frame["payload"]["event"], "User:Update");
    }
}
```

`crates/qf_core/src/crypto.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    #[test]
    fn encrypt_then_decrypt_roundtrips() {
        let key = SecretKey::from_hex(KEY).unwrap();
        let (ct, nonce) = key.encrypt(b"jwt-token").unwrap();
        assert_ne!(ct, b"jwt-token");
        assert_eq!(key.decrypt(&ct, &nonce).unwrap(), b"jwt-token");
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let key = SecretKey::from_hex(KEY).unwrap();
        let other = SecretKey::from_hex(&"ab".repeat(32)).unwrap();
        let (ct, nonce) = key.encrypt(b"jwt-token").unwrap();
        assert!(other.decrypt(&ct, &nonce).is_err());
    }

    #[test]
    fn short_key_is_rejected() {
        assert!(SecretKey::from_hex("abcd").is_err());
    }

    #[test]
    fn jwt_expiry_reads_exp_claim_with_optional_prefix() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"exp":1794614400,"sub":"x"}"#);
        let token = format!("eyJhbGciOiJIUzI1NiJ9.{payload}.sig");
        assert_eq!(jwt_expiry(&token).unwrap().timestamp(), 1794614400);
        assert_eq!(jwt_expiry(&format!("JWT {token}")).unwrap().timestamp(), 1794614400);
        assert!(jwt_expiry("not-a-jwt").is_none());
    }
}
```

`crates/qf_core/src/lib.rs` (temporary, extended in later tasks):
```rust
#![allow(non_snake_case)]
#![allow(deprecated)]

pub mod crypto;
pub mod events;
mod macros;
pub mod paths;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p qf_core`
Expected: compile errors such as `cannot find type Paths`, `cannot find function subscribe` and `cannot find type SecretKey`.

- [ ] **Step 4: Implement paths, events and crypto (above the test modules)**

`crates/qf_core/src/paths.rs`:
```rust
use std::{fs, path::PathBuf, sync::OnceLock};

use utils::{get_location, Error};

#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub resources_dir: PathBuf,
}

static PATHS: OnceLock<Paths> = OnceLock::new();

impl Paths {
    pub fn new(data_dir: impl Into<PathBuf>, resources_dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let data_dir = data_dir.into();
        fs::create_dir_all(&data_dir).map_err(|e| {
            Error::new(
                "Paths:New",
                format!("Failed to create data dir {}: {}", data_dir.display(), e),
                get_location!(),
            )
        })?;
        Ok(Self { data_dir, resources_dir: resources_dir.into() })
    }

    fn subdir(&self, name: &str) -> PathBuf {
        let path = self.data_dir.join(name);
        let _ = fs::create_dir_all(&path);
        path
    }

    pub fn sounds_dir(&self) -> PathBuf {
        self.subdir("sounds")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.subdir("cache")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.subdir("logs")
    }

    /// Stable per-installation id, generated once and stored in `<data_dir>/device_id`.
    pub fn device_id(&self) -> Result<String, Error> {
        let file = self.data_dir.join("device_id");
        if let Ok(existing) = fs::read_to_string(&file) {
            let existing = existing.trim().to_string();
            if !existing.is_empty() {
                return Ok(existing);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        fs::write(&file, &id).map_err(|e| {
            Error::new("Paths:DeviceId", format!("Failed to write device id: {}", e), get_location!())
        })?;
        Ok(id)
    }
}

pub fn init(paths: Paths) {
    let _ = PATHS.set(paths);
}

pub fn get() -> &'static Paths {
    PATHS.get().expect("Paths not initialized")
}
```

`crates/qf_core/src/events.rs`:
```rust
use std::sync::OnceLock;

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

static SENDER: OnceLock<broadcast::Sender<Value>> = OnceLock::new();

fn sender() -> &'static broadcast::Sender<Value> {
    SENDER.get_or_init(|| broadcast::channel(1024).0)
}

pub fn subscribe() -> broadcast::Receiver<Value> {
    sender().subscribe()
}

/// Sends `{channel, payload}` to every connected browser. Returns how many receivers got it.
pub fn emit(channel: &str, payload: impl Serialize) -> usize {
    let payload = serde_json::to_value(payload).unwrap_or(Value::Null);
    sender()
        .send(json!({ "channel": channel, "payload": payload }))
        .unwrap_or(0)
}
```

`crates/qf_core/src/crypto.rs`:
```rust
use std::sync::OnceLock;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use chrono::{DateTime, Utc};
use utils::{get_location, Error};

pub struct SecretKey([u8; 32]);

static KEY: OnceLock<Option<SecretKey>> = OnceLock::new();

impl SecretKey {
    pub fn from_hex(hex_key: &str) -> Result<Self, Error> {
        let bytes = hex::decode(hex_key.trim()).map_err(|e| {
            Error::new("Crypto:Key", format!("Secret key is not valid hex: {}", e), get_location!())
        })?;
        let key: [u8; 32] = bytes.try_into().map_err(|_| {
            Error::new("Crypto:Key", "Secret key must be 32 bytes (64 hex characters)", get_location!())
        })?;
        Ok(Self(key))
    }

    fn cipher(&self) -> Result<Aes256Gcm, Error> {
        Aes256Gcm::new_from_slice(&self.0)
            .map_err(|e| Error::new("Crypto:Cipher", format!("{:?}", e), get_location!()))
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>), Error> {
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce)
            .map_err(|e| Error::new("Crypto:Encrypt", format!("{:?}", e), get_location!()))?;
        let ciphertext = self
            .cipher()?
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|e| Error::new("Crypto:Encrypt", format!("{:?}", e), get_location!()))?;
        Ok((ciphertext, nonce.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<Vec<u8>, Error> {
        if nonce.len() != 12 {
            return Err(Error::new("Crypto:Decrypt", "Nonce must be 12 bytes", get_location!()));
        }
        self.cipher()?
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| {
                Error::new(
                    "Crypto:Decrypt",
                    "Failed to decrypt; the secret key does not match the stored data",
                    get_location!(),
                )
            })
    }
}

pub fn init_key(key: Option<SecretKey>) {
    let _ = KEY.set(key);
}

pub fn key() -> Result<&'static SecretKey, Error> {
    KEY.get().and_then(|k| k.as_ref()).ok_or_else(|| {
        Error::new(
            "Crypto:Key",
            "No secret key configured (QF_SECRET_KEY_FILE); warframe.market sign-in is disabled",
            get_location!(),
        )
    })
}

/// Reads the `exp` claim of a JWT. Accepts an optional `JWT ` or `Bearer ` prefix.
pub fn jwt_expiry(token: &str) -> Option<DateTime<Utc>> {
    let token = token.trim_start_matches("JWT ").trim_start_matches("Bearer ");
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    DateTime::from_timestamp(claims.get("exp")?.as_i64()?, 0)
}
```

- [ ] **Step 5: Replace `macros.rs`**

Keep upstream's macro bodies except for these changes:
- **`emit_event!`:** now calls `events::emit`.
- **`send_system_notification!`:** deleted (Windows toast only).
- **`add_metric!`:** deleted (Quantframe analytics).

`crates/qf_core/src/macros.rs`:
```rust
/// Macro to emit events with automatic logging
#[macro_export]
macro_rules! emit_event {
    ($event_name:expr, $payload:expr, $log_context:expr) => {{
        let receivers = $crate::events::emit($event_name, $payload);
        ::utils::info(
            &format!("Emit:{}", $log_context),
            &format!("Event: {} ({} receivers)", $event_name, receivers),
            &::utils::LoggerOptions::default(),
        );
    }};
}

#[macro_export]
macro_rules! send_event {
    ($event:expr, $data:expr) => {{
        use serde_json::json;
        use crate::emit_event;
        emit_event!(
            "message",
            json!({ "event": $event.as_str(), "data": $data }),
            format!("SendEvent:{}", $event.as_str())
        );
    }};
}

#[macro_export]
macro_rules! send_event_update {
    ($event:expr, $operation:expr, $data:expr) => {{
        use crate::types::*;
        use serde_json::json;
        use crate::emit_event;
        emit_event!(
            "message_update",
            json!({ "event": $event.as_str(), "operation": $operation.as_str(), "data": $data }),
            format!("SendEventUpdate:{}", $event.as_str())
        );
    }};
}

#[macro_export]
macro_rules! emit_error {
    ($err:expr) => {{
        use crate::send_event;
        use crate::types::*;
        use crate::utils::modules::states;
        send_event!(UIEvent::OnError, Some(json!($err)));
        states::set_app_error(Some($err));
    }};
}

#[macro_export]
macro_rules! clear_error {
    () => {{
        use crate::send_event;
        use crate::types::*;
        use crate::utils::modules::*;
        send_event!(UIEvent::OnError, Some(json!({})));
        states::set_app_error(None);
    }};
}

#[macro_export]
macro_rules! emit_startup {
    ($i18n_key:expr, $Option:expr) => {{
        use crate::types::*;
        use crate::send_event;
        send_event!(UIEvent::OnStartingUp, Some(json!({"i18n_key": $i18n_key, "values": $Option})));
    }};
}

#[macro_export]
macro_rules! emit_update_user {
    ($user:expr) => {{
        use crate::send_event_update;
        send_event_update!(
            UIEvent::UpdateUser,
            UIOperationEvent::CreateOrUpdate,
            Some(json!($user))
        );
    }};
}

#[macro_export]
macro_rules! notify_gui {
    ($i18n_key:expr, $color:expr, $notify_type:expr, $values:expr, $settings:expr) => {{
        use crate::send_event;
        use crate::types::*;
        send_event!(
            UIEvent::OnNotify,
            Some(json!({"i18n_key": $i18n_key, "color": $color, "type": $notify_type, "values": $values, "settings": $settings}))
        );
    }};
}

#[macro_export]
macro_rules! play_sound {
    ($file_name:expr, $volume:expr) => {{
        use crate::emit_event;
        emit_event!(
            "play_sound",
            serde_json::json!({"file_name": $file_name, "volume": $volume}),
            "PlaySound"
        );
    }};
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p qf_core`
Expected: 6 passed (1 paths, 1 events, 4 crypto).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/qf_core/Cargo.toml crates/qf_core/src/{lib,paths,events,crypto,macros}.rs
git commit -m "feat(core): add paths, event bus and token crypto replacing Tauri handles

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 3: Game data from warframe.market v2 `/items` and the slim item cache

**Files:**
- Create: `crates/qf_core/src/game_data/mod.rs`, `crates/qf_core/tests/fixtures/wfm_items.json`
- Replace: `crates/qf_core/src/cache/client.rs`, `crates/qf_core/src/cache/modules/mod.rs`, `crates/qf_core/src/cache/types/mod.rs`
- Modify: `crates/qf_core/src/cache/modules/tradable_items.rs`, `crates/qf_core/src/cache/modules/theme.rs`, `crates/qf_core/src/cache/types/cache_tradable_item.rs`, `crates/qf_core/src/cache/mod.rs`, `crates/qf_core/src/lib.rs`

**Interfaces:**
- **Consumes:** nothing from earlier tasks except the crate manifest.
- **Produces, `game_data`:**
  - `WfmItem` (serde, fields per spec §5.2)
  - `game_data::parse_items_response(json: &str) -> Result<Vec<WfmItem>, Error>`
  - `game_data::to_tradable_item(&WfmItem) -> Option<CacheTradableItem>`
  - `game_data::load_items(cache_dir: &Path, http: &reqwest::Client) -> Result<Vec<CacheTradableItem>, Error>`
  - `game_data::load_items_from(cache_dir, http, url)`
- **Produces, `cache`:**
  - `CacheState::new(base_path: PathBuf) -> CacheState`
  - `CacheState::load(&self, items: Vec<CacheTradableItem>) -> Result<(), Error>`
  - `CacheState::tradable_item() -> Arc<TradableItemModule>`, `CacheState::theme() -> Arc<ThemeModule>`
  - `TradableItemModule::{set_items, get_items, get_by}`
  - `ThemeModule::{new(base_path: &Path), load, get_items, get_theme_folder}`

- [ ] **Step 1: Record the fixture from the live API**

```bash
mkdir -p crates/qf_core/tests/fixtures
curl -s -H 'Language: en' -H 'Platform: pc' https://api.warframe.market/v2/items -o /tmp/wfm_items_full.json
python3 - <<'EOF'
import json
full = json.load(open('/tmp/wfm_items_full.json'))
want = {"arcane_energize", "axi_a1_relic", "mesa_prime_set", "ayatan_anasa_sculpture", "secura_dual_cestra"}
data = [i for i in full["data"] if i["slug"] in want]
assert len(data) == len(want), sorted(want - {i["slug"] for i in data})
json.dump({"apiVersion": full.get("apiVersion"), "data": data, "error": None},
          open('crates/qf_core/tests/fixtures/wfm_items.json', 'w'), indent=1)
print(len(data), "items written")
EOF
```
Expected: `5 items written`.

Also check the item-shape assumptions the tests rely on:
```bash
python3 -c "
import json;d={i['slug']:i for i in json.load(open('crates/qf_core/tests/fixtures/wfm_items.json'))['data']}
print(d['arcane_energize'].get('maxRank'), d['axi_a1_relic'].get('subtypes'), d['ayatan_anasa_sculpture'].get('maxAmberStars'), [k for k in ('maxRank','subtypes','maxAmberStars','maxCyanStars') if k in d['mesa_prime_set']])"
```
Expected output: `5 [...'intact'...] <number> []`. If it differs, adjust the matching assertion in Step 2 to the recorded data before continuing.

- [ ] **Step 2: Write failing tests**

`crates/qf_core/src/game_data/mod.rs`, test module only:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../tests/fixtures/wfm_items.json");

    fn items() -> Vec<CacheTradableItem> {
        parse_items_response(FIXTURE)
            .unwrap()
            .iter()
            .filter_map(to_tradable_item)
            .collect()
    }

    fn by_slug(slug: &str) -> CacheTradableItem {
        items().into_iter().find(|i| i.wfm_url == slug).unwrap()
    }

    #[test]
    fn maps_identity_fields() {
        let item = by_slug("arcane_energize");
        assert_eq!(item.name, "Arcane Energize");
        assert_eq!(
            item.unique_name,
            "/Lotus/Upgrades/CosmeticEnhancers/Utility/GolemArcaneRadialEnergyOnEnergyPickup"
        );
        assert!(!item.wfm_id.is_empty());
        assert_eq!(item.trade_tax, 0);
        assert_eq!(item.sub_type.as_ref().unwrap().max_rank, Some(5));
    }

    #[test]
    fn maps_variants_and_stars() {
        let relic = by_slug("axi_a1_relic");
        assert!(relic.sub_type.as_ref().unwrap().has_variant("intact"));
        let ayatan = by_slug("ayatan_anasa_sculpture");
        assert!(ayatan.sub_type.as_ref().unwrap().amber_stars.is_some());
    }

    #[test]
    fn items_without_ranks_or_variants_have_no_sub_type() {
        assert!(by_slug("mesa_prime_set").sub_type.is_none());
    }

    #[tokio::test]
    async fn load_items_falls_back_to_last_good_copy() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(LAST_GOOD_FILE), FIXTURE).unwrap();
        // Port 9 (discard) on localhost refuses the connection, so the fetch fails fast.
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(500))
            .build()
            .unwrap();
        let loaded = load_items_from(dir.path(), &http, "http://127.0.0.1:9/v2/items").await.unwrap();
        assert_eq!(loaded.len(), 5);
    }

    #[tokio::test]
    async fn load_items_errors_without_fetch_or_last_good_copy() {
        let dir = tempfile::tempdir().unwrap();
        let http = reqwest::Client::new();
        assert!(load_items_from(dir.path(), &http, "http://127.0.0.1:9/v2/items").await.is_err());
    }
}
```

`crates/qf_core/src/cache/client.rs`: write the test module now, at the end of the file, and put the new implementation above it in Step 4:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tradable_items_are_found_by_every_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CacheState::new(dir.path().to_path_buf());
        let items = crate::game_data::parse_items_response(include_str!("../../tests/fixtures/wfm_items.json"))
            .unwrap()
            .iter()
            .filter_map(crate::game_data::to_tradable_item)
            .collect::<Vec<_>>();
        let energize = items.iter().find(|i| i.wfm_url == "arcane_energize").unwrap().clone();
        cache.load(items).unwrap();
        let module = cache.tradable_item();
        assert_eq!(module.get_by("arcane_energize").unwrap().wfm_id, energize.wfm_id);
        assert_eq!(module.get_by(&energize.wfm_id).unwrap().wfm_url, "arcane_energize");
        assert_eq!(module.get_by("Arcane Energize").unwrap().wfm_url, "arcane_energize");
        assert_eq!(module.get_by(&energize.unique_name).unwrap().wfm_url, "arcane_energize");
        assert!(module.get_by("does_not_exist").is_err());
        assert_eq!(module.get_items().unwrap().len(), 5);
    }
}
```

Add `pub mod cache;` and `pub mod game_data;` to `lib.rs`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p qf_core`
Expected: compile errors (`cannot find function parse_items_response`, and the upstream `cache/client.rs` referencing removed modules).

- [ ] **Step 4: Implement**

`crates/qf_core/src/game_data/mod.rs` (above the tests):
```rust
use std::{collections::HashMap, path::Path};

use serde::{Deserialize, Serialize};
use utils::{get_location, info, warning, Error, LoggerOptions};

use crate::cache::types::{CacheTradableItem, SubType};

pub const WFM_ITEMS_URL: &str = "https://api.warframe.market/v2/items";
pub const LAST_GOOD_FILE: &str = "wfm_items.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WfmItemI18n {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub thumb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WfmItem {
    pub id: String,
    pub slug: String,
    #[serde(default)]
    pub game_ref: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub i18n: HashMap<String, WfmItemI18n>,
    pub max_rank: Option<i64>,
    pub bulk_tradable: Option<bool>,
    pub subtypes: Option<Vec<String>>,
    pub max_amber_stars: Option<i64>,
    pub max_cyan_stars: Option<i64>,
    pub req_mastery_rank: Option<i64>,
}

#[derive(Deserialize)]
struct ItemsResponse {
    data: Vec<WfmItem>,
}

pub fn parse_items_response(json: &str) -> Result<Vec<WfmItem>, Error> {
    serde_json::from_str::<ItemsResponse>(json)
        .map(|r| r.data)
        .map_err(|e| Error::new("GameData:Parse", format!("Invalid /v2/items response: {}", e), get_location!()))
}

pub fn to_tradable_item(item: &WfmItem) -> Option<CacheTradableItem> {
    let en = item.i18n.get("en")?;
    let has_sub_type = item.max_rank.is_some()
        || item.subtypes.is_some()
        || item.max_amber_stars.is_some()
        || item.max_cyan_stars.is_some();
    Some(CacheTradableItem {
        name: en.name.clone(),
        unique_name: item.game_ref.clone(),
        wfm_id: item.id.clone(),
        wfm_url: item.slug.clone(),
        trade_tax: 0,
        mr_requirement: item.req_mastery_rank.unwrap_or(0),
        tags: item.tags.clone(),
        icon: en.icon.clone(),
        bulk_tradable: item.bulk_tradable.unwrap_or(false),
        sub_type: has_sub_type.then(|| SubType {
            max_rank: item.max_rank,
            variants: item.subtypes.clone(),
            amber_stars: item.max_amber_stars,
            cyan_stars: item.max_cyan_stars,
        }),
        variant_to_unique_name: HashMap::new(),
    })
}

pub async fn load_items(cache_dir: &Path, http: &reqwest::Client) -> Result<Vec<CacheTradableItem>, Error> {
    load_items_from(cache_dir, http, WFM_ITEMS_URL).await
}

/// Fetches the item list, saving it as the last good copy. Falls back to that copy when the fetch fails.
pub async fn load_items_from(
    cache_dir: &Path,
    http: &reqwest::Client,
    url: &str,
) -> Result<Vec<CacheTradableItem>, Error> {
    let last_good = cache_dir.join(LAST_GOOD_FILE);
    let fetched: Result<String, String> = async {
        let body = http
            .get(url)
            .header("Language", "en")
            .header("Platform", "pc")
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        parse_items_response(&body).map_err(|e| e.message.clone())?;
        Ok(body)
    }
    .await;

    let body = match fetched {
        Ok(body) => {
            if let Err(e) = std::fs::write(&last_good, &body) {
                warning(
                    "GameData:Load",
                    format!("Could not save last good item list: {}", e),
                    &LoggerOptions::default(),
                );
            }
            body
        }
        Err(fetch_error) => {
            warning(
                "GameData:Load",
                format!("Fetching {} failed ({}); using last good copy", url, fetch_error),
                &LoggerOptions::default(),
            );
            std::fs::read_to_string(&last_good).map_err(|e| {
                Error::new(
                    "GameData:Load",
                    format!("Item list fetch failed ({}) and no last good copy exists: {}", fetch_error, e),
                    get_location!(),
                )
            })?
        }
    };

    let items: Vec<CacheTradableItem> =
        parse_items_response(&body)?.iter().filter_map(to_tradable_item).collect();
    info("GameData:Load", format!("Loaded {} tradable items", items.len()), &LoggerOptions::default());
    Ok(items)
}
```

`crates/qf_core/src/cache/types/cache_tradable_item.rs`:
- Delete the `impl CacheTradableItem { fn translate … }` block.
- Delete the `use crate::cache::modules::LanguageModule;` line.

`crates/qf_core/src/cache/types/mod.rs`:
```rust
pub mod cache_theme;
pub mod cache_tradable_item;

pub use cache_theme::*;
pub use cache_tradable_item::*;
```

`crates/qf_core/src/cache/modules/mod.rs`:
```rust
pub mod theme;
pub mod tradable_items;

pub use theme::*;
pub use tradable_items::*;
```

`crates/qf_core/src/cache/mod.rs`:
```rust
pub mod client;
pub mod modules;
pub mod types;
pub use client::*;
pub use types::*;
```
Then `rm -rf crates/qf_core/src/cache/enums`.

`crates/qf_core/src/cache/modules/tradable_items.rs`: replace the imports, struct, `new` and `load` with the code below. Keep upstream's `get_items` and `get_by` bodies unchanged below `set_items`.
```rust
use std::sync::{Arc, Mutex};

use utils::{get_location, Error, MultiKeyMap};

use crate::cache::types::CacheTradableItem;

#[derive(Debug)]
pub struct TradableItemModule {
    items: Mutex<Vec<CacheTradableItem>>,
    item_lookup: Mutex<MultiKeyMap<CacheTradableItem>>,
}

impl TradableItemModule {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            items: Mutex::new(Vec::new()),
            item_lookup: Mutex::new(MultiKeyMap::new()),
        })
    }

    pub fn set_items(&self, items: Vec<CacheTradableItem>) {
        let mut lookup = MultiKeyMap::new();
        for item in items.iter() {
            let mut keys = vec![item.wfm_id.clone(), item.name.clone(), item.wfm_url.clone()];
            if !item.unique_name.is_empty() {
                keys.push(item.unique_name.clone());
            }
            keys.extend(item.variant_to_unique_name.values().cloned());
            lookup.insert_value(item.clone(), keys);
        }
        *self.item_lookup.lock().unwrap() = lookup;
        *self.items.lock().unwrap() = items;
    }

    // get_items and get_by: unchanged from upstream
}
```

`crates/qf_core/src/cache/modules/theme.rs`:
- Change `new(client: Arc<CacheState>)` to take a path:
  ```rust
  pub fn new(base_path: &std::path::Path) -> Arc<Self> {
      Arc::new(Self { path: base_path.join("themePresets"), items: Mutex::new(Vec::new()) })
  }
  ```
- Delete `pick_icon`, `create_theme` and anything used only by them (`BASE64_CHARS`, base64 helpers) once the compiler reports them unused.
- Delete the `tauri_plugin_dialog`, `APP`, `helper` and `client::CacheState` imports.

`crates/qf_core/src/cache/client.rs` (full replacement above the test module):
```rust
use std::{path::PathBuf, sync::Arc};

use utils::Error;

use super::modules::{ThemeModule, TradableItemModule};
use super::types::CacheTradableItem;

#[derive(Clone, Debug)]
pub struct CacheState {
    pub base_path: PathBuf,
    tradable_item_module: Arc<TradableItemModule>,
    theme_module: Arc<ThemeModule>,
}

impl CacheState {
    pub fn new(base_path: PathBuf) -> Self {
        let theme_module = ThemeModule::new(&base_path);
        Self { base_path, tradable_item_module: TradableItemModule::new(), theme_module }
    }

    pub fn load(&self, items: Vec<CacheTradableItem>) -> Result<(), Error> {
        self.tradable_item_module.set_items(items);
        self.theme_module.load()
    }

    pub fn tradable_item(&self) -> Arc<TradableItemModule> {
        self.tradable_item_module.clone()
    }

    pub fn theme(&self) -> Arc<ThemeModule> {
        self.theme_module.clone()
    }
}
```

If `ThemeModule::load` or `CacheTheme` use more upstream helpers, keep what compiles. If the compiler complains about a type `SubType` name clash between `utils::SubType` and `cache::types::SubType`, refer to the cache one as `crate::cache::types::cache_tradable_item::SubType` in `game_data`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p qf_core`
Expected: all tests pass (6 from Task 2, plus 5 game_data and 1 cache).

- [ ] **Step 6: Commit**

```bash
git add -A crates/qf_core
git commit -m "feat(core): load tradable items from warframe.market v2 instead of Quantframe cache

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 4: Database, encrypted WFM account store and web password

**Files:**
- Create: `crates/qf_core/src/db.rs`, `crates/qf_core/src/wfm_account.rs`, `crates/qf_core/src/web_auth.rs`, `crates/migration/src/m20260914_000001_create_wfm_account.rs`, `crates/migration/src/m20260914_000002_create_web_auth.rs`
- Modify: `crates/migration/src/lib.rs`, `crates/qf_core/src/lib.rs`

**Interfaces:**
- **Consumes:** `crypto::SecretKey`, `crypto::jwt_expiry` (Task 2).
- **Produces, `db`:** `db::DB_FILE = "quantframe.sqlite"` and `db::connect(data_dir: &Path) -> Result<DatabaseConnection, Error>`, which backs up the existing file, turns on WAL and runs the migrations.
- **Produces, `wfm_account`:**
  - `StoredAccount { token: String, token_expires_at: Option<DateTime<Utc>>, wfm_user_id: String, username: String }`
  - `wfm_account::save(conn: &DatabaseConnection, key: &SecretKey, token: &str, wfm_user_id: &str, username: &str) -> Result<(), Error>`
  - `wfm_account::load(conn, key) -> Result<Option<StoredAccount>, Error>`
  - `wfm_account::delete(conn) -> Result<(), Error>`
- **Produces, `web_auth`:**
  - `web_auth::hash_password(&str) -> Result<String, Error>`
  - `web_auth::verify_password(password: &str, hash: &str) -> bool`
  - `web_auth::ensure_password(conn, password_file: &Path) -> Result<String /*hash*/, Error>`

- [ ] **Step 1: Add the migrations**

`crates/migration/src/m20260914_000001_create_wfm_account.rs`:
```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "CREATE TABLE IF NOT EXISTS wfm_account (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                token_ciphertext BLOB NOT NULL,
                nonce BLOB NOT NULL,
                token_expires_at TEXT,
                wfm_user_id TEXT NOT NULL,
                username TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"
            .to_string(),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(db.get_database_backend(), "DROP TABLE wfm_account".to_string()))
            .await?;
        Ok(())
    }
}
```

`crates/migration/src/m20260914_000002_create_web_auth.rs`:
```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(
            db.get_database_backend(),
            "CREATE TABLE IF NOT EXISTS web_auth (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                password_hash TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )"
            .to_string(),
        ))
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute(Statement::from_string(db.get_database_backend(), "DROP TABLE web_auth".to_string()))
            .await?;
        Ok(())
    }
}
```

`crates/migration/src/lib.rs`: add `mod m20260914_000001_create_wfm_account;` and `mod m20260914_000002_create_web_auth;`. Append `Box::new(m20260914_000001_create_wfm_account::Migration),` and `Box::new(m20260914_000002_create_web_auth::Migration),` to the end of the `vec!`.

Upstream migrations use `use sea_orm::…` from `sea_orm_migration`'s re-export. If `sea_orm` isn't resolvable, change the first line of both new files to `use sea_orm_migration::sea_orm::{ConnectionTrait, Statement};`.

Run: `cargo check -p migration`
Expected: `Finished`.

- [ ] **Step 2: Write failing tests**

`crates/qf_core/src/wfm_account.rs`, test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::SecretKey;

    fn key(byte: &str) -> SecretKey {
        SecretKey::from_hex(&byte.repeat(32)).unwrap()
    }

    #[tokio::test]
    async fn save_load_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        let k = key("11");
        assert!(load(&conn, &k).await.unwrap().is_none());

        save(&conn, &k, "JWT header.eyJleHAiOjE3OTQ2MTQ0MDB9.sig", "u1", "Tenno").await.unwrap();
        let account = load(&conn, &k).await.unwrap().unwrap();
        assert_eq!(account.token, "JWT header.eyJleHAiOjE3OTQ2MTQ0MDB9.sig");
        assert_eq!(account.username, "Tenno");
        assert_eq!(account.wfm_user_id, "u1");
        assert_eq!(account.token_expires_at.unwrap().timestamp(), 1794614400);

        assert!(load(&conn, &key("22")).await.is_err(), "wrong key must not decrypt");

        delete(&conn).await.unwrap();
        assert!(load(&conn, &k).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn token_is_not_stored_in_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        save(&conn, &key("11"), "super-secret-token", "u1", "Tenno").await.unwrap();
        conn.close().await.unwrap();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let raw = std::fs::read(&path).unwrap();
                let needle = b"super-secret-token";
                assert!(!raw.windows(needle.len()).any(|w| w == needle), "{} contains the token", path.display());
            }
        }
    }
}
```

`crates/qf_core/src/web_auth.rs`, test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify() {
        let hash = hash_password("correct horse battery").unwrap();
        assert!(verify_password("correct horse battery", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[tokio::test]
    async fn file_is_source_of_truth() {
        let dir = tempfile::tempdir().unwrap();
        let conn = crate::db::connect(dir.path()).await.unwrap();
        let file = dir.path().join("pw");

        assert!(ensure_password(&conn, &file).await.is_err(), "no file and no row");

        std::fs::write(&file, "first-password-123\n").unwrap();
        let h1 = ensure_password(&conn, &file).await.unwrap();
        assert!(verify_password("first-password-123", &h1));

        std::fs::write(&file, "second-password-456").unwrap();
        let h2 = ensure_password(&conn, &file).await.unwrap();
        assert!(verify_password("second-password-456", &h2));

        std::fs::remove_file(&file).unwrap();
        let h3 = ensure_password(&conn, &file).await.unwrap();
        assert!(verify_password("second-password-456", &h3), "row is used when file is gone");

        std::fs::write(&file, "short").unwrap();
        assert!(ensure_password(&conn, &file).await.is_err(), "passwords under 12 chars rejected");
    }
}
```

Add `pub mod db;`, `pub mod wfm_account;` and `pub mod web_auth;` to `lib.rs`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p qf_core`
Expected: compile errors for the missing `connect`, `save`, `load`, `delete`, `hash_password`, `verify_password` and `ensure_password`.

- [ ] **Step 4: Implement**

`crates/qf_core/src/db.rs`:
```rust
use std::path::Path;

use migration::{Migrator, MigratorTrait};
use service::sea_orm::{ConnectionTrait, Database, DatabaseConnection};
use utils::{get_location, info, Error, LoggerOptions};

pub const DB_FILE: &str = "quantframe.sqlite";

pub async fn connect(data_dir: &Path) -> Result<DatabaseConnection, Error> {
    let file = data_dir.join(DB_FILE);
    if file.exists() {
        let backup = data_dir.join(format!("{}_backup", DB_FILE));
        std::fs::copy(&file, &backup).map_err(|e| {
            Error::new("Db:Backup", format!("Failed to back up database: {}", e), get_location!())
        })?;
    }
    let url = format!("sqlite://{}?mode=rwc", file.display());
    let conn = Database::connect(url)
        .await
        .map_err(|e| Error::new("Db:Connect", e.to_string(), get_location!()))?;
    conn.execute_unprepared("PRAGMA journal_mode=WAL;")
        .await
        .map_err(|e| Error::new("Db:Wal", e.to_string(), get_location!()))?;
    Migrator::up(&conn, None)
        .await
        .map_err(|e| Error::new("Db:Migrate", format!("Failed to apply migrations: {}", e), get_location!()))?;
    info("Db:Connect", "Database ready", &LoggerOptions::default());
    Ok(conn)
}
```

`crates/qf_core/src/wfm_account.rs` (above the tests):
```rust
use chrono::{DateTime, Utc};
use service::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value};
use utils::{get_location, Error};

use crate::crypto::{jwt_expiry, SecretKey};

#[derive(Debug, Clone)]
pub struct StoredAccount {
    pub token: String,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub wfm_user_id: String,
    pub username: String,
}

fn db_err(component: &str, e: impl std::fmt::Display) -> Error {
    Error::new(component, e.to_string(), get_location!())
}

pub async fn save(
    conn: &DatabaseConnection,
    key: &SecretKey,
    token: &str,
    wfm_user_id: &str,
    username: &str,
) -> Result<(), Error> {
    let (ciphertext, nonce) = key.encrypt(token.as_bytes())?;
    let expires = jwt_expiry(token).map(|d| d.to_rfc3339());
    conn.execute(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT OR REPLACE INTO wfm_account
            (id, token_ciphertext, nonce, token_expires_at, wfm_user_id, username, created_at)
         VALUES (1, ?, ?, ?, ?, ?, ?)",
        [
            Value::Bytes(Some(Box::new(ciphertext))),
            Value::Bytes(Some(Box::new(nonce))),
            expires.into(),
            wfm_user_id.to_string().into(),
            username.to_string().into(),
            Utc::now().to_rfc3339().into(),
        ],
    ))
    .await
    .map_err(|e| db_err("WfmAccount:Save", e))?;
    Ok(())
}

pub async fn load(conn: &DatabaseConnection, key: &SecretKey) -> Result<Option<StoredAccount>, Error> {
    let row = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT token_ciphertext, nonce, token_expires_at, wfm_user_id, username FROM wfm_account WHERE id = 1"
                .to_string(),
        ))
        .await
        .map_err(|e| db_err("WfmAccount:Load", e))?;
    let Some(row) = row else { return Ok(None) };
    let ciphertext: Vec<u8> = row.try_get("", "token_ciphertext").map_err(|e| db_err("WfmAccount:Load", e))?;
    let nonce: Vec<u8> = row.try_get("", "nonce").map_err(|e| db_err("WfmAccount:Load", e))?;
    let expires: Option<String> = row.try_get("", "token_expires_at").map_err(|e| db_err("WfmAccount:Load", e))?;
    let token = String::from_utf8(key.decrypt(&ciphertext, &nonce)?).map_err(|e| db_err("WfmAccount:Load", e))?;
    Ok(Some(StoredAccount {
        token,
        token_expires_at: expires
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|d| d.with_timezone(&Utc)),
        wfm_user_id: row.try_get("", "wfm_user_id").map_err(|e| db_err("WfmAccount:Load", e))?,
        username: row.try_get("", "username").map_err(|e| db_err("WfmAccount:Load", e))?,
    }))
}

pub async fn delete(conn: &DatabaseConnection) -> Result<(), Error> {
    conn.execute(Statement::from_string(DbBackend::Sqlite, "DELETE FROM wfm_account".to_string()))
        .await
        .map_err(|e| db_err("WfmAccount:Delete", e))?;
    Ok(())
}
```

`crates/qf_core/src/web_auth.rs` (above the tests):
```rust
use std::path::Path;

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};
use utils::{get_location, Error};

const MIN_PASSWORD_LEN: usize = 12;

pub fn hash_password(password: &str) -> Result<String, Error> {
    let mut salt_bytes = [0u8; 16];
    getrandom::getrandom(&mut salt_bytes)
        .map_err(|e| Error::new("WebAuth:Hash", format!("{:?}", e), get_location!()))?;
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|e| Error::new("WebAuth:Hash", format!("{:?}", e), get_location!()))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| Error::new("WebAuth:Hash", format!("{:?}", e), get_location!()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

async fn stored_hash(conn: &DatabaseConnection) -> Result<Option<String>, Error> {
    let row = conn
        .query_one(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT password_hash FROM web_auth WHERE id = 1".to_string(),
        ))
        .await
        .map_err(|e| Error::new("WebAuth:Load", e.to_string(), get_location!()))?;
    match row {
        Some(row) => Ok(Some(
            row.try_get("", "password_hash")
                .map_err(|e| Error::new("WebAuth:Load", e.to_string(), get_location!()))?,
        )),
        None => Ok(None),
    }
}

/// The password file is the source of truth. Its password is re-hashed whenever it no longer
/// matches the stored hash. If the file is missing, the stored hash is used.
pub async fn ensure_password(conn: &DatabaseConnection, password_file: &Path) -> Result<String, Error> {
    let existing = stored_hash(conn).await?;
    let from_file = std::fs::read_to_string(password_file).ok().map(|s| s.trim().to_string());

    match (from_file, existing) {
        (Some(password), existing) => {
            if password.chars().count() < MIN_PASSWORD_LEN {
                return Err(Error::new(
                    "WebAuth:Ensure",
                    format!("Web password must be at least {} characters", MIN_PASSWORD_LEN),
                    get_location!(),
                ));
            }
            if let Some(hash) = existing.filter(|h| verify_password(&password, h)) {
                return Ok(hash);
            }
            let hash = hash_password(&password)?;
            conn.execute(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR REPLACE INTO web_auth (id, password_hash, updated_at) VALUES (1, ?, ?)",
                [hash.clone().into(), Utc::now().to_rfc3339().into()],
            ))
            .await
            .map_err(|e| Error::new("WebAuth:Ensure", e.to_string(), get_location!()))?;
            Ok(hash)
        }
        (None, Some(hash)) => Ok(hash),
        (None, None) => Err(Error::new(
            "WebAuth:Ensure",
            format!("No web password set: create {}", password_file.display()),
            get_location!(),
        )),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p qf_core`
Expected: all pass (12 previous, plus 2 wfm_account and 2 web_auth).

- [ ] **Step 6: Commit**

```bash
git add -A crates/migration crates/qf_core
git commit -m "feat(core): add encrypted warframe.market token store and web password

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 5: App state, handlers and helpers without Tauri or the Quantframe API

This task makes every non-command module compile. The work is mostly mechanical, so **run `cargo check -p qf_core` after each step**. The goal is zero errors with `commands` not yet declared. The upstream file:line references come from the inventory made while planning.

**Files:**
- Replace: `crates/qf_core/src/utils/modules/states.rs`, `crates/qf_core/src/app/client.rs`, `crates/qf_core/src/lib.rs`
- Modify:
  - `crates/qf_core/src/helper.rs`
  - `crates/qf_core/src/app/types/{app_state,user}.rs`
  - `crates/qf_core/src/app/modules/{auth,ws}.rs`
  - `crates/qf_core/src/app/types/settings/*.rs`
  - `crates/qf_core/src/utils/{mod,error_ext,order_list_ext,create_trade_entry_ext}.rs`
  - `crates/qf_core/src/types/mod.rs`
  - `crates/qf_core/src/handlers/*.rs`

**Interfaces:**
- **Consumes:** `paths`, `crypto`, `wfm_account`, `cache::CacheState`, `DATABASE` (Tasks 2–4).
- **Produces, `states`:**
  - `states::init_app_state(AppState)`, `states::init_cache_state(CacheState)`
  - `states::app_mutex() -> &'static Mutex<AppState>`, `states::cache_mutex() -> &'static Mutex<CacheState>`
  - `states::app_state() -> Result<AppState, Error>`, `states::cache_client() -> Result<CacheState, Error>`, `states::get_settings()`, `states::get_app_error()`, `states::set_app_error(Option<Error>)`
- **Produces, `AppState`:**
  - `AppState::new(use_temp_db: bool) -> Result<AppState, Error>`
  - `AppState::login(&self, email: &str, password: &str) -> Result<(WFClient<Authenticated>, User, WsClient), Error>`
  - `AppState::validate(&mut self) -> Result<WFUserPrivate, Error>`
  - `auth::update_user(User, &WFUserPrivate) -> User`
  - `ws::setup_socket(WFClient<Authenticated>) -> Result<WsClient, Error>`
- **Produces, crate root:** `DATABASE`, `HAS_STARTED` and `APP_ERROR` statics, plus `SENSITIVE_FIELDS`.

- [ ] **Step 1: Crate root globals and module list**

`crates/qf_core/src/lib.rs`:
```rust
#![allow(non_snake_case)]
#![allow(deprecated)]

use std::sync::{Mutex, OnceLock};

use service::sea_orm::DatabaseConnection;
use ::utils::Error;

pub mod app;
pub mod cache;
pub mod crypto;
pub mod db;
pub mod enums;
pub mod events;
pub mod game_data;
pub mod handlers;
pub mod helper;
mod macros;
pub mod paths;
pub mod types;
pub mod utils;
pub mod web_auth;
pub mod wfm_account;

pub static DATABASE: OnceLock<DatabaseConnection> = OnceLock::new();
pub static HAS_STARTED: OnceLock<bool> = OnceLock::new();
pub static APP_ERROR: OnceLock<Mutex<Option<Error>>> = OnceLock::new();
pub static SENSITIVE_FIELDS: &[&str] = &[
    "email",
    "password",
    "authorization",
    "wfm_token",
    "webhook",
    "slug",
    "device_key",
    "token_ciphertext",
];
```

If anything in `src/enums` references removed modules (the live scraper), delete those files and their `mod` lines. Keep the enums that settings and handlers use.

- [ ] **Step 2: Replace `states.rs`**

`crates/qf_core/src/utils/modules/states.rs`:
```rust
use std::sync::{Mutex, OnceLock};

use utils::Error;

use crate::{
    app::{AppState, Settings},
    cache::client::CacheState,
    APP_ERROR,
};

static APP_STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
static CACHE_STATE: OnceLock<Mutex<CacheState>> = OnceLock::new();

pub fn init_app_state(state: AppState) {
    let _ = APP_STATE.set(Mutex::new(state));
}

pub fn init_cache_state(state: CacheState) {
    let _ = CACHE_STATE.set(Mutex::new(state));
}

pub fn app_mutex() -> &'static Mutex<AppState> {
    APP_STATE.get().expect("App state not initialized")
}

pub fn cache_mutex() -> &'static Mutex<CacheState> {
    CACHE_STATE.get().expect("Cache state not initialized")
}

pub fn app_state() -> Result<AppState, Error> {
    Ok(app_mutex().lock()?.clone())
}

pub fn get_settings() -> Result<Settings, Error> {
    Ok(app_state()?.settings)
}

pub fn cache_client() -> Result<CacheState, Error> {
    Ok(cache_mutex().lock()?.clone())
}

pub fn get_app_error() -> Option<Error> {
    let app_error = APP_ERROR.get_or_init(|| Mutex::new(None));
    let guard = app_error.lock().expect("Failed to lock APP_ERROR");
    guard.clone()
}

pub fn set_app_error(error: Option<Error>) {
    let app_error = APP_ERROR.get_or_init(|| Mutex::new(None));
    let mut guard = app_error.lock().expect("Failed to lock APP_ERROR");
    *guard = error;
}
```

Also replace `cache/client.rs:177`-style direct `app.state::<…>()` lookups if any remain. Find them with `grep -rn "\.state::<" crates/qf_core/src`.

- [ ] **Step 3: `helper.rs`**

- Delete `APP_PATH`, `get_device_id`, `get_desktop_path`, `get_local_data_path`, `get_or_create_window` and `populate_riven_market_properties`.
- Delete the imports that become unused: `tauri::{…}`, `wf_market::types::AuctionLike`, `crate::cache::{derive_riven_summary_attributes, grade_riven, scale_attributes, CacheState, CacheWeaponBase}` (re-add `CacheState` alone if `populate_item_market_properties` needs it), `AuctionWithOwnerListExt`, `RivenGrade`, `RivenAttribute`, `APP`.
- Replace the two remaining path functions with:

```rust
pub fn get_app_storage_path() -> PathBuf {
    crate::paths::get().data_dir.clone()
}

pub fn get_sounds_path() -> PathBuf {
    crate::paths::get().sounds_dir()
}
```

Keep `generate_transaction_summary`, `paginate`, `populate_item_market_properties` and every other function that compiles.

- [ ] **Step 4: `User` without Quantframe fields**

In `crates/qf_core/src/app/types/user.rs`:
- Remove these fields, their `Default` entries and every use: `qf_banned`, `qf_banned_until`, `qf_banned_reason`, `qf_token`, `check_code`, `unread_messages`, `permissions`, `patreon_tier`.
- Delete `has_permission`.
- Change `is_banned` to `self.wfm_banned`.
- Replace `use crate::{helper, types::PermissionsFlags, utils::ErrorFromExt};` with `use crate::{helper, utils::ErrorFromExt};`.
- Change `wfm_token` so it is never persisted or sent to the browser:

```rust
    #[serde(skip)]
    pub wfm_token: String,
```

`User::load` validates the file against the default JSON. Because `wfm_token` is now skipped on both sides, this stays consistent.

Add at the end of `user.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_never_serialized() {
        let mut user = User::default();
        user.wfm_token = "secret".into();
        let json = serde_json::to_string(&user).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("wfm_token"));
    }
}
```

- [ ] **Step 5: `AppState` struct and `new`**

In `crates/qf_core/src/app/types/app_state.rs`:
- Remove `qf_client`, `wfm_chat_socket` and `http_server`, and `ACTIVE_CHAT_ID` with its get/set functions.
- Remove the imports of `HttpServer`, `QFClient`, `Arc` and `OnceLock` if they become unused.

Keep upstream's `derive` list on the struct:
```rust
pub struct AppState {
    pub user: User,
    pub settings: Settings,
    pub wfm_client: WFClient<WFAuthenticated>,
    pub is_development: bool,
    pub is_pre_release: bool,
    pub use_temp_db: bool,
    pub wfm_socket: Option<WsClient>,
}
```

`crates/qf_core/src/app/client.rs` (full replacement):
```rust
use utils::{Error, LogLevel};
use wf_market::Client as WFClient;

use crate::app::modules::auth::update_user;
use crate::app::{AppState, Settings, User};

impl AppState {
    pub async fn new(use_temp_db: bool) -> Result<Self, Error> {
        let user = User::load().unwrap_or_else(|e| {
            e.log("app_init.log");
            User::default()
        });
        let settings = Settings::load().unwrap_or_else(|e| {
            e.log("app_init.log");
            Settings::default()
        });
        let mut state = AppState {
            wfm_client: WFClient::new_default("", "N/A")
                .await
                .expect("Failed to create WFM client"),
            user,
            is_development: cfg!(debug_assertions),
            use_temp_db,
            is_pre_release: false,
            settings,
            wfm_socket: None,
        };
        match state.validate().await {
            Ok(wfm_user) => {
                state.user = update_user(state.user, &wfm_user);
            }
            Err(e) => {
                e.log("user_validation.log");
                if e.log_level != LogLevel::Warning {
                    state.user = User::default();
                }
            }
        }
        state.user.save()?;
        Ok(state)
    }

    pub fn update_settings(&mut self, settings: Settings) -> Result<(), Error> {
        self.settings = settings;
        self.settings.save()?;
        Ok(())
    }
}
```

- [ ] **Step 6: `auth.rs` — WFM only, token from the encrypted store**

In `crates/qf_core/src/app/modules/auth.rs`:
- Delete every `qf_api` import, `authenticate_qf_user` and the `QFUserPrivate` parameter.
- Keep `new_base_wfm_client` unchanged (upstream lines ~94–125).
- Replace the imports, `update_user`, `login` and `validate` with:

```rust
use serde_json::json;
use utils::{get_location, info, log_json, Error, LoggerOptions};
use wf_market::client::Authenticated as WFAuthenticated;
use wf_market::types::websocket::WsClient;
use wf_market::types::UserPrivate as WFUserPrivate;
use wf_market::Client as WFClient;

use crate::app::modules::ws::setup_socket;
use crate::app::{AppState, User};
use crate::utils::ErrorFromExt;
use crate::{crypto, emit_startup, paths, wfm_account, DATABASE, SENSITIVE_FIELDS};

pub fn update_user(mut cu_user: User, user: &WFUserPrivate) -> User {
    cu_user.anonymous = false;
    cu_user.verification = user.verification;
    cu_user.wfm_banned = user.banned.unwrap_or(false);
    cu_user.wfm_banned_reason = user.ban_message.clone();
    cu_user.wfm_banned_until = user.ban_until.clone();
    cu_user.wfm_id = user.id.to_string();
    cu_user.wfm_username = user.ingame_name.clone();
    cu_user.locale = user.locale.clone();
    cu_user.platform = user.platform.clone();
    cu_user.wfm_avatar = user.avatar.clone();
    cu_user
}

impl AppState {
    pub async fn login(
        &self,
        email: &str,
        password: &str,
    ) -> Result<(WFClient<WFAuthenticated>, User, WsClient), Error> {
        let key = crypto::key()?;
        let device_id = paths::get().device_id()?;
        let wfm_client = self
            .new_base_wfm_client()
            .login(email, password, &device_id)
            .await
            .map_err(|e| Error::from_wfm("AppState:Login", "Failed to login to WFM client", e, get_location!()))?;
        let wfm_user = wfm_client
            .get_user()
            .map_err(|e| Error::from_wfm("AppState:Login", "Failed to get WFM user", e, get_location!()))?;
        wfm_account::save(
            DATABASE.get().expect("Database not initialized"),
            key,
            &wfm_client.get_token(),
            &wfm_user.id.to_string(),
            &wfm_user.ingame_name,
        )
        .await?;
        let updated_user = update_user(self.user.clone(), &wfm_user);
        let ws = setup_socket(wfm_client.clone()).await?;
        updated_user.save()?;
        Ok((wfm_client, updated_user, ws))
    }

    pub async fn validate(&mut self) -> Result<WFUserPrivate, Error> {
        let key = crypto::key()?;
        let account = wfm_account::load(DATABASE.get().expect("Database not initialized"), key)
            .await?
            .ok_or_else(|| {
                Error::new(
                    "AppState:Validate",
                    "No warframe.market account stored, please sign in.",
                    get_location!(),
                )
            })?;
        let device_id = paths::get().device_id()?;
        let wfm_client = self
            .new_base_wfm_client()
            .login_with_token(&account.token, &device_id)
            .await
            .map_err(|e| Error::from_wfm("AppState:Validate", "Failed to login with WFM token", e, get_location!()))?;
        let wfm_user = wfm_client
            .get_user()
            .map_err(|e| Error::from_wfm("AppState:Validate", "Failed to get WFM user", e, get_location!()))?;
        let ws = setup_socket(wfm_client.clone()).await?;
        self.wfm_socket = Some(ws);
        self.wfm_client = wfm_client;
        Ok(wfm_user)
    }

    // new_base_wfm_client: unchanged from upstream
}
```

If `get_token()` returns a value with a prefix such as `JWT `, store it exactly as returned: `login_with_token` expects the same form, and `jwt_expiry` strips the prefix.

- [ ] **Step 7: `ws.rs` — v2 socket only**

In `crates/qf_core/src/app/modules/ws.rs`:
- Delete `handle_new_message` and the whole `ws_client_chat` builder (upstream lines ~263–297).
- Delete the imports of `Chat`, `ChatMessage`, `get_active_chat_id`, `tauri::Manager`, `APP` and `HAS_STARTED` (if unused).
- Change the `setup_socket` signature and final return:

```rust
pub async fn setup_socket(wfm_client: WFClient<WFAuthenticated>) -> Result<WsClient, Error> {
    // … unchanged V2 builder …
    Ok(ws_client)
}
```

- In `update_user_status` (upstream lines ~60–68), replace the `APP.get()` / `app.state::<Mutex<AppState>>()` lines with the line below, and keep the rest of the body:

```rust
    let app = crate::utils::modules::states::app_mutex();
```

- [ ] **Step 8: Settings and notifications**

- **`settings.rs`:**
  - Remove the `advanced_settings` and `wf_inventory` fields, their defaults, and the legacy-remapping branches that touch them (upstream lines 94–184).
  - In `live_scraper_settings.rs`, remove `rivens` and `syndicate`, and delete their settings files and `mod`/`pub use` lines.
  - Keep `live_scraper.general`, because `handlers/base.rs` reads `report_to_wfm`.
  - Delete `http_server_settings.rs`, `advanced_settings.rs`, `wf_inventory_settings.rs` and the riven/syndicate settings files.
- **`debugging_settings.rs`:** replace the `crate::live_scraper::ItemEntry` import and field type with `pub entries: Vec<serde_json::Value>`.
- **`discord_notify.rs:50–52` and `webhook_notify.rs:24–26`:**
  - Replace `APP.get()…package_info()` usage with `"Quantframe Server"` for the name and `env!("CARGO_PKG_VERSION")` for the version.
  - Replace `tauri::async_runtime::spawn` with `tokio::spawn`.
- **`system_notify.rs:40–44`:** delete the `send_system_notification!` call and its `#[cfg(windows)]` block, and keep the `play_sound!` call.

- [ ] **Step 9: Utils, types and handlers**

- **`utils/mod.rs`:** remove the `mod`/`pub use` lines for the files deleted in Task 1.
- **`utils/error_ext.rs`:** delete `from_qf` (lines ~19, 46–54) and the `qf_api` import.
- **`utils/order_list_ext.rs`:** delete `apply_trade_info` from the trait and both impls.
- **`utils/create_trade_entry_ext.rs`:** replace the `"riven"` group branch that calls `cache.weapon()` (line ~25) with:

```rust
            return Err(Error::new(
                "CreateTradeEntry:Validate",
                "Riven trade entries are not supported in this version",
                get_location!(),
            ));
```

- **`types/mod.rs`:** remove `chat_link`, `item_riven` and `permissions_flags`.
- **`handlers/stock_riven.rs`:** delete the file and its `mod`/`pub use` lines in `handlers/mod.rs`.
- **Every file:** remove each `add_metric!(…);` line. Run `grep -rn "add_metric!" crates/qf_core/src` and expect no output when done.

- [ ] **Step 10: Compile and run the tests**

Run: `cargo check -p qf_core 2>&1 | grep -E '^error' | sort | uniq -c | head -40`
Fix each error by deleting code that belongs to a removed feature, or by swapping in the replacements above. Never re-add a Quantframe API, riven, chat, analytics or Tauri dependency.

Run: `cargo test -p qf_core`
Expected: all previous tests pass, plus `token_is_never_serialized`.

Run: `grep -rn "tauri\|qf_api\|qf_client\|APP\.get" crates/qf_core/src`
Expected: no output.

- [ ] **Step 11: Commit**

```bash
git add -A crates/qf_core
git commit -m "refactor(core): remove Tauri handle and Quantframe API from app state and handlers

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 6: Commands, RPC dispatch table and startup

**Files:**
- Create: `crates/qf_core/src/commands/rpc.rs`, `crates/qf_core/src/startup.rs`
- Modify: `crates/qf_core/src/commands/{mod,app,auth,user,cache,logs,dashboard,stock_item,wish_list,transaction,trade_entry,order,debug,sound,handlers}.rs`, `crates/qf_core/src/lib.rs`

**Interfaces:**
- **Consumes:** `states::{app_mutex, cache_mutex, app_state, cache_client, init_app_state, init_cache_state}` (Task 5), `game_data::load_items`, `CacheState` (Task 3), `db::connect`, `web_auth::ensure_password`, `crypto::{SecretKey, init_key}` (Tasks 2 and 4).
- **Produces, `rpc`:**
  - `qf_core::commands::rpc::COMMANDS: &[&str]`
  - `qf_core::commands::rpc::dispatch(name: &str, args: serde_json::Value) -> Option<Result<serde_json::Value, utils::Error>>`. It returns `None` for an unknown name. Argument keys are camelCase, which is what upstream `sendInvoke` sends.
- **Produces, `startup`:**
  - `qf_core::startup::CoreConfig { data_dir: PathBuf, resources_dir: PathBuf, secret_key_hex: Option<String>, web_password_file: PathBuf }`
  - `qf_core::startup::start(CoreConfig) -> Result<CoreHandles, Error>`
  - `CoreHandles { web_password_hash: String }`

**How to convert a command:** apply these rules to every function listed in the Step 3 table.

1. Remove `#[tauri::command]`.
2. For each `name: tauri::State<'_, Mutex<AppState>>` parameter, delete it and add `let name = crate::utils::modules::states::app_mutex();` as the first line of the body. `Mutex<CacheState>` becomes `cache_mutex()` in the same way. The body keeps calling `name.lock()` exactly as before.
3. Parameters of type `tauri::State<'_, Arc<LiveScraperState>>` or `…LogParserState…` are deleted together with every line that uses them.
4. Synchronous commands (`pub fn` without `async`) become `pub async fn`.
5. `panic!` on socket send errors in `user_set_status` becomes `return Err(Error::new("User:SetStatus", format!("{:?}", e), get_location!()))`.
6. Remove `PermissionsFlags` imports, every `has_permission` check, and any `APP`/`tauri_plugin_dialog` imports.

Before and after, for `commands/order.rs::order_delete_by_id`:
```rust
// before
#[tauri::command]
pub async fn order_delete_by_id(id: String, app: tauri::State<'_, Mutex<AppState>>) -> Result<(), Error> {
    let app = app.lock()?.clone();
// after
pub async fn order_delete_by_id(id: String) -> Result<(), Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app = app.lock()?.clone();
```

- [ ] **Step 1: Write failing tests for the dispatch table**

`crates/qf_core/src/commands/rpc.rs`, test module:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn unknown_command_returns_none() {
        assert!(dispatch("does_not_exist", json!({})).await.is_none());
    }

    #[tokio::test]
    async fn no_arg_command_accepts_empty_object() {
        let result = dispatch("initialized", json!({})).await.unwrap().unwrap();
        assert!(result.is_boolean());
    }

    #[tokio::test]
    async fn camel_case_args_are_mapped_and_bad_args_are_errors() {
        let ok = dispatch(
            "log",
            json!({"cause": "c", "component": "Test", "location": "l", "logLevel": "Info", "message": "m"}),
        )
        .await
        .unwrap();
        assert!(ok.is_ok(), "{:?}", ok.err());
        let bad = dispatch("log", json!({"cause": 1})).await.unwrap();
        assert!(bad.is_err());
    }

    #[test]
    fn allowlist_has_no_removed_features() {
        for name in COMMANDS {
            for banned in [
                "riven", "auction", "chat", "analytics", "alert", "syndicate", "wfgdpr",
                "wf_inventory", "live_scraper", "permission", "exit", "calculate_tax",
            ] {
                assert!(!name.contains(banned), "{name} must not be exposed");
            }
        }
    }
}
```

`crates/qf_core/src/commands/mod.rs`:
```rust
pub mod app;
pub mod auth;
pub mod cache;
pub mod dashboard;
pub mod debug;
pub mod handlers;
pub mod logs;
pub mod order;
pub mod rpc;
pub mod sound;
pub mod stock_item;
pub mod trade_entry;
pub mod transaction;
pub mod user;
pub mod wish_list;
```

Add `pub mod commands;` and `pub mod startup;` to `lib.rs`. Create `startup.rs` empty for now.

- [ ] **Step 2: Convert the command files**

Apply the conversion rules to every file above. Beyond the rules, these per-file edits apply:

- **`app.rs`:**
  - Delete `app_exit`.
  - In `app_get_app_info`, replace `APP.get()…package_info()` with `"Quantframe Server"` / `env!("CARGO_PKG_VERSION")`, and delete the `patreon_usernames` field.
  - In `app_update_settings`, delete the `log_parser` parameter, the `log_parser.set_path(...)` call and every `http_server` line.
- **`auth.rs`:**
  - Delete `auth_has_permission`.
  - Replace `auth_login` and `auth_logout` with the code below, and keep `auth_me` converted by the rules.

```rust
pub async fn auth_login(email: String, password: String) -> Result<User, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let cache = crate::utils::modules::states::cache_client()?;
    let app_state = app.lock()?.clone();
    let (wfm_client, updated_user, ws) = app_state
        .login(&email, &password)
        .await
        .map_err(|e| e.log("auth_login.log"))?;
    info(
        "Commands:AuthLogin",
        &format!("User {} logged in successfully", updated_user.wfm_username),
        &LoggerOptions::default(),
    );
    wfm_client.order().cache_orders_mut().apply_item_info(&cache)?;
    let mut app = app.lock()?;
    app.wfm_client = wfm_client;
    app.user = updated_user.clone();
    app.wfm_socket = Some(ws);
    send_event!(UIEvent::RefreshCache, "Cache refreshed successfully");
    Ok(updated_user)
}

pub async fn auth_logout() -> Result<User, Error> {
    let app = crate::utils::modules::states::app_mutex();
    let app_state = app.lock()?.clone();
    if let Some(ws) = &app_state.wfm_socket {
        if let Err(e) = ws.disconnect() {
            let err = Error::new(
                "Commands:AuthLogout",
                format!("Failed to close WebSocket: {:?}", e),
                get_location!(),
            );
            err.log("auth_logout.log");
            return Err(err);
        }
    }
    crate::wfm_account::delete(crate::DATABASE.get().expect("Database not initialized")).await?;
    let new_user = User::default();
    new_user.save()?;
    let mut app = app.lock()?;
    app.user = new_user.clone();
    app.wfm_socket = None;
    Ok(new_user)
}
```

- **`cache.rs`:** keep only `cache_get_tradable_items` and `cache_get_theme_presets`.
- **`logs.rs`:** keep only `log`. Delete `log_export`.
- **`debug.rs`:** keep only `debug_get_wfm_state`, and delete the auction-cache lines inside it.
- **`order.rs`:** `order_delete_all` loses its `live_scraper` parameter and the `live_scraper.stop()` line.
- **`transaction.rs`:** delete `transaction_calculate_tax` and `export_transaction_json`.
- **`stock_item.rs`:** replace `export_stock_item_json` with the version below. Do the same in `wish_list.rs` for `export_wish_list_json` (`WishListQuery`, `wish_list::Model`) and in `trade_entry.rs` for `export_trade_entry_json` (`TradeEntryQuery`, `trade_entry::Model`).

```rust
pub async fn export_stock_item_json(
    mut query: StockItemPaginationQueryDto,
) -> Result<Vec<stock_item::Model>, Error> {
    let conn = DATABASE.get().unwrap();
    query.pagination.limit = -1; // fetch all
    StockItemQuery::get_all(conn, query)
        .await
        .map(|page| page.results)
        .map_err(|e| e.with_location(get_location!()))
}
```

- **`sound.rs`:**
  - Delete `sound_get_custom_sounds_path` and `validate_sound_file` (it checked a local path).
  - Replace `sound_add_custom_sound` with:

```rust
pub async fn sound_add_custom_sound(
    name: String,
    file_name: String,
    data_base64: String,
) -> Result<Vec<CustomSound>, Error> {
    use base64::Engine;
    let app = crate::utils::modules::states::app_mutex();
    let mut app = app.lock()?;

    let normalized_name = normalize_sound_name(&name)?;
    let normalized_name_key = normalized_name.to_lowercase();
    if app
        .settings
        .notifications
        .custom_sounds
        .iter()
        .any(|sound| sound.name_key == normalized_name_key)
    {
        return Err(Error::new("Sound", "Sound name already exists.", utils::get_location!()));
    }
    let extension = file_name
        .rsplit('.')
        .next()
        .map(|e| e.to_lowercase())
        .filter(|e| ["mp3", "wav", "ogg"].contains(&e.as_str()))
        .ok_or_else(|| Error::new("Sound", "Only mp3, wav and ogg files are allowed.", utils::get_location!()))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| Error::new("Sound", format!("Invalid file data: {}", e), utils::get_location!()))?;

    let stored_name = format!("{}.{}", uuid::Uuid::new_v4(), extension);
    fs::write(helper::get_sounds_path().join(&stored_name), bytes).map_err(|e| {
        Error::new("Sound", format!("Failed to save sound file: {}", e), utils::get_location!())
    })?;

    app.settings
        .notifications
        .custom_sounds
        .push(CustomSound::new(normalized_name, stored_name));
    app.settings.save()?;
    Ok(app.settings.notifications.custom_sounds.clone())
}
```

- **`user.rs`:** apply rule 5 to `user_set_status`.
- **`handlers.rs`:** remove `use crate::commands::item;`.

Run `cargo check -p qf_core` after each file.

- [ ] **Step 3: Write the dispatch table**

`crates/qf_core/src/commands/rpc.rs` (above the tests). Take the argument types' `use` paths from the headers of the corresponding command files. If a path is wrong, find the type with `grep -rn "pub struct <Type>" crates`. **Never rename an argument**: the names must match what the frontend sends.
```rust
use serde_json::Value;
use utils::{get_location, Error, SubType};
use wf_market::enums::OrderType;

use crate::app::Settings;
use crate::handlers::ItemEntity;
use crate::utils::WfmOrderPaginationQueryDto;
use entity::{stock_item::*, trade_entry::*, transaction::*, wish_list::*};

macro_rules! rpc_table {
    ($( $name:ident => $module:ident :: $func:ident { $( $arg:ident : $ty:ty ),* $(,)? } ),* $(,)?) => {
        pub const COMMANDS: &[&str] = &[$( stringify!($name) ),*];

        pub async fn dispatch(name: &str, args: Value) -> Option<Result<Value, Error>> {
            match name {
                $(
                    stringify!($name) => {
                        #[derive(serde::Deserialize)]
                        #[serde(rename_all = "camelCase")]
                        #[allow(dead_code)]
                        struct Args { $( $arg: $ty ),* }
                        let parsed: Args = match serde_json::from_value(args) {
                            Ok(parsed) => parsed,
                            Err(e) => {
                                return Some(Err(Error::new(
                                    "Rpc:Args",
                                    format!("Invalid arguments for {}: {}", name, e),
                                    get_location!(),
                                )))
                            }
                        };
                        let result = crate::commands::$module::$func($( parsed.$arg ),*).await;
                        Some(result.and_then(|value| {
                            serde_json::to_value(value)
                                .map_err(|e| Error::new("Rpc:Serialize", e.to_string(), get_location!()))
                        }))
                    }
                )*
                _ => None,
            }
        }
    };
}

rpc_table! {
    initialized => app::initialized {},
    app_get_app_info => app::app_get_app_info {},
    app_get_settings => app::app_get_settings {},
    app_update_settings => app::app_update_settings { settings: Settings },
    app_accept_tos => app::app_accept_tos { id: String },
    app_notify_reset => app::app_notify_reset { id: String },
    app_get_default_settings => app::app_get_default_settings {},
    auth_me => auth::auth_me {},
    auth_login => auth::auth_login { email: String, password: String },
    auth_logout => auth::auth_logout {},
    user_set_status => user::user_set_status { status: String },
    dashboard_summary => dashboard::dashboard_summary {},
    cache_get_tradable_items => cache::cache_get_tradable_items {},
    cache_get_theme_presets => cache::cache_get_theme_presets {},
    log => logs::log { cause: String, component: String, location: String, log_level: String, message: String, context: Option<Value> },
    get_stock_item_pagination => stock_item::get_stock_item_pagination { query: StockItemPaginationQueryDto },
    get_stock_item_financial_report => stock_item::get_stock_item_financial_report { query: StockItemPaginationQueryDto },
    get_stock_item_status_counts => stock_item::get_stock_item_status_counts { query: StockItemPaginationQueryDto },
    stock_item_create => stock_item::stock_item_create { input: CreateStockItem },
    stock_item_delete => stock_item::stock_item_delete { id: i64 },
    stock_item_sell => stock_item::stock_item_sell { wfm_url: String, sub_type: Option<SubType>, quantity: i64, price: i64 },
    stock_item_update => stock_item::stock_item_update { input: UpdateStockItem },
    stock_item_get_by_id => stock_item::stock_item_get_by_id { id: i64, operations: Option<Vec<String>> },
    stock_item_update_multiple => stock_item::stock_item_update_multiple { ids: Vec<i64>, input: UpdateStockItem },
    stock_item_delete_multiple => stock_item::stock_item_delete_multiple { ids: Vec<i64> },
    export_stock_item_json => stock_item::export_stock_item_json { query: StockItemPaginationQueryDto },
    get_wish_list_pagination => wish_list::get_wish_list_pagination { query: WishListPaginationQueryDto },
    get_wish_list_financial_report => wish_list::get_wish_list_financial_report { query: WishListPaginationQueryDto },
    get_wish_list_status_counts => wish_list::get_wish_list_status_counts { query: WishListPaginationQueryDto },
    wish_list_create => wish_list::wish_list_create { input: CreateWishListItem },
    wish_list_bought => wish_list::wish_list_bought { wfm_url: String, sub_type: Option<SubType>, quantity: i64, price: i64 },
    wish_list_delete => wish_list::wish_list_delete { id: i64 },
    wish_list_update => wish_list::wish_list_update { input: UpdateWishList },
    wish_list_get_by_id => wish_list::wish_list_get_by_id { id: i64, operations: Option<Vec<String>> },
    export_wish_list_json => wish_list::export_wish_list_json { query: WishListPaginationQueryDto },
    wish_list_update_multiple => wish_list::wish_list_update_multiple { ids: Vec<i64>, input: UpdateWishList },
    wish_list_delete_multiple => wish_list::wish_list_delete_multiple { ids: Vec<i64> },
    get_transaction_pagination => transaction::get_transaction_pagination { query: TransactionPaginationQueryDto },
    get_transaction_financial_report => transaction::get_transaction_financial_report { query: TransactionPaginationQueryDto },
    transaction_update => transaction::transaction_update { input: UpdateTransaction },
    transaction_delete => transaction::transaction_delete { id: i64 },
    transaction_delete_bulk => transaction::transaction_delete_bulk { ids: Vec<i64> },
    get_trade_entry_pagination => trade_entry::get_trade_entry_pagination { query: TradeEntryPaginationQueryDto },
    trade_entry_get_by_id => trade_entry::trade_entry_get_by_id { id: i64 },
    trade_entry_create => trade_entry::trade_entry_create { input: CreateTradeEntry },
    trade_entry_create_multiple => trade_entry::trade_entry_create_multiple { inputs: Vec<CreateTradeEntry> },
    trade_entry_delete => trade_entry::trade_entry_delete { id: i64 },
    trade_entry_delete_multiple => trade_entry::trade_entry_delete_multiple { ids: Vec<i64> },
    trade_entry_update => trade_entry::trade_entry_update { input: UpdateTradeEntry },
    trade_entry_update_multiple => trade_entry::trade_entry_update_multiple { ids: Vec<i64>, input: UpdateTradeEntry },
    export_trade_entry_json => trade_entry::export_trade_entry_json { query: TradeEntryPaginationQueryDto },
    get_wfm_orders_pagination => order::get_wfm_orders_pagination { query: WfmOrderPaginationQueryDto },
    get_wfm_orders_status_counts => order::get_wfm_orders_status_counts { query: WfmOrderPaginationQueryDto },
    order_refresh => order::order_refresh {},
    order_delete_all => order::order_delete_all { order_type: Option<OrderType> },
    order_delete_by_id => order::order_delete_by_id { id: String },
    get_wfm_order_by_id => order::get_wfm_order_by_id { id: String, operations: Option<Vec<String>> },
    debug_get_wfm_state => debug::debug_get_wfm_state {},
    sound_get_custom_sounds => sound::sound_get_custom_sounds {},
    sound_add_custom_sound => sound::sound_add_custom_sound { name: String, file_name: String, data_base64: String },
    sound_delete_custom_sound => sound::sound_delete_custom_sound { file_name: String },
    handles_handle_items => handlers::handles_handle_items { items: Vec<ItemEntity> },
}
```

The table has 62 commands. `Option<T>` arguments missing from the JSON deserialize as `None`, which matches Tauri's behaviour.

- [ ] **Step 4: Write `startup.rs`**

`crates/qf_core/src/startup.rs`:
```rust
use std::path::PathBuf;

use utils::{error, info, init_logger, set_base_path, warning, Error, LoggerOptions};

use crate::app::AppState;
use crate::cache::CacheState;
use crate::crypto::{self, SecretKey};
use crate::paths::{self, Paths};
use crate::utils::modules::states;
use crate::utils::OrderListExt;
use crate::{db, game_data, web_auth, DATABASE, HAS_STARTED};

pub struct CoreConfig {
    pub data_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub secret_key_hex: Option<String>,
    pub web_password_file: PathBuf,
}

pub struct CoreHandles {
    pub web_password_hash: String,
}

pub async fn start(cfg: CoreConfig) -> Result<CoreHandles, Error> {
    paths::init(Paths::new(&cfg.data_dir, &cfg.resources_dir)?);
    init_logger();
    set_base_path(paths::get().logs_dir().to_string_lossy().to_string());

    let conn = db::connect(&paths::get().data_dir).await?;
    let _ = DATABASE.set(conn);
    let conn = DATABASE.get().expect("Database just set");

    let web_password_hash = web_auth::ensure_password(conn, &cfg.web_password_file).await?;

    let key = match cfg.secret_key_hex.as_deref() {
        Some(hex) => Some(SecretKey::from_hex(hex)?),
        None => {
            warning(
                "Startup",
                "QF_SECRET_KEY_FILE not readable; warframe.market sign-in is disabled",
                &LoggerOptions::default(),
            );
            None
        }
    };
    crypto::init_key(key);

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| Error::new("Startup:Http", e.to_string(), utils::get_location!()))?;
    let items = game_data::load_items(&paths::get().cache_dir(), &http).await?;
    let cache = CacheState::new(paths::get().cache_dir());
    cache.load(items)?;
    states::init_cache_state(cache);

    states::init_app_state(AppState::new(false).await?);
    {
        let cache = states::cache_client()?;
        let app = states::app_mutex().lock()?;
        app.wfm_client.order().cache_orders_mut().apply_item_info(&cache)?;
    }
    if states::app_state()?.wfm_socket.is_some() {
        if let Err(e) = crate::commands::user::user_set_status("invisible".to_string()).await {
            error(
                "Startup",
                format!("Could not force invisible status: {:?}", e),
                &LoggerOptions::default(),
            );
        }
    }

    let _ = HAS_STARTED.set(true);
    info("Startup", "Core started", &LoggerOptions::default());
    Ok(CoreHandles { web_password_hash })
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p qf_core`
Expected: all pass, including the 4 rpc tests.

Run: `grep -rn "tauri" crates/qf_core`
Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add -A crates/qf_core
git commit -m "feat(core): expose allowlisted commands through rpc dispatch and add startup sequence

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 7: `qf-server` binary — login, `/rpc`, `/ws`, static files

**Files:**
- Create: `crates/qf-server/Cargo.toml`, `crates/qf-server/src/{lib,main,config,auth,routes}.rs`, `crates/qf-server/src/login.html`, `crates/qf-server/tests/http.rs`
- Modify: `Cargo.toml` (add member)

**Interfaces:**
- **Consumes:** `qf_core::startup::{start, CoreConfig, CoreHandles}`, `qf_core::commands::rpc::dispatch`, `qf_core::events::subscribe`, `qf_core::web_auth::{verify_password, hash_password}` (Tasks 2, 4 and 6).
- **Produces, `config`:** `Config::from_lookup(impl Fn(&str) -> Option<String>) -> Result<Config, String>` and `Config::core() -> CoreConfig`.
- **Produces, `auth`:**
  - `Sessions::{new(Duration), create() -> String, is_valid(&str) -> bool, remove(&str)}`
  - `LoginLimiter::{new(usize, Duration), try_acquire() -> bool}`
- **Produces, `routes`:** `routes::ServerState` and `routes::router(ServerState) -> axum::Router`.
- **Produces, HTTP:**
  - `GET /healthz`
  - `GET|POST /login`
  - `POST /logout`
  - `POST /rpc/{name}`
  - `GET /ws`
  - `GET /sounds/builtin/*`, `GET /sounds/custom/*`
  - SPA fallback to `index.html`

- [ ] **Step 1: Crate manifest and library layout for tests**

`crates/qf-server/Cargo.toml`:
```toml
[package]
name = "qf-server"
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"

[lib]
name = "qf_server"
path = "src/lib.rs"

[[bin]]
name = "qf-server"
path = "src/main.rs"

[dependencies]
axum = { version = "0.8", features = ["ws"] }
tower-http = { version = "0.6", features = ["fs"] }
tower = "0.5"
tokio = { version = "1.45.1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
getrandom = "0.2"
hex = "0.4"
qf_core = { path = "../qf_core" }

[dev-dependencies]
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
tempfile = "3"
```

`crates/qf-server/src/lib.rs`:
```rust
pub mod auth;
pub mod config;
pub mod routes;
```

Add `"crates/qf-server"` to the workspace members.

- [ ] **Step 2: Write failing integration tests**

`crates/qf-server/tests/http.rs`:
```rust
use std::{sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    Router,
};
use http_body_util::BodyExt;
use qf_server::{
    auth::{LoginLimiter, Sessions},
    routes::{router, ServerState},
};
use tower::ServiceExt;

const ORIGIN: &str = "http://test.local";
const PASSWORD: &str = "correct horse battery";

fn app() -> (Router, ServerState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("web")).unwrap();
    std::fs::write(dir.path().join("web/index.html"), "<html>app</html>").unwrap();
    let state = ServerState {
        sessions: Arc::new(Sessions::new(Duration::from_secs(3600))),
        limiter: Arc::new(LoginLimiter::new(5, Duration::from_secs(60))),
        password_hash: Arc::new(qf_core::web_auth::hash_password(PASSWORD).unwrap()),
        public_origin: Arc::new(ORIGIN.to_string()),
        web_dir: dir.path().join("web"),
        resources_dir: dir.path().join("resources"),
        data_dir: dir.path().to_path_buf(),
    };
    (router(state.clone()), state, dir)
}

fn rpc(name: &str, cookie: Option<&str>, origin: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri(format!("/rpc/{name}"))
        .header(header::ORIGIN, origin)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = cookie {
        req = req.header(header::COOKIE, format!("qf_session={token}"));
    }
    req.body(Body::from("{}")).unwrap()
}

fn login(password: &str) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri("/login")
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(format!("password={}", password.replace(' ', "+"))))
        .unwrap()
}

#[tokio::test]
async fn healthz_needs_no_session() {
    let (app, _, _dir) = app();
    let res = app.oneshot(Request::get("/healthz").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn rpc_without_session_is_unauthorized() {
    let (app, _, _dir) = app();
    let res = app.oneshot(rpc("initialized", None, ORIGIN)).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_origin_is_forbidden_even_with_session() {
    let (app, state, _dir) = app();
    let token = state.sessions.create();
    let res = app.oneshot(rpc("initialized", Some(&token), "http://evil.example")).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn login_sets_cookie_only_for_correct_password() {
    let (app, _, _dir) = app();
    let bad = app.clone().oneshot(login("nope")).await.unwrap();
    assert_eq!(bad.status(), StatusCode::SEE_OTHER);
    assert_eq!(bad.headers()[header::LOCATION], "/login?error=1");
    assert!(bad.headers().get(header::SET_COOKIE).is_none());

    let good = app.oneshot(login(PASSWORD)).await.unwrap();
    assert_eq!(good.status(), StatusCode::SEE_OTHER);
    let cookie = good.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.starts_with("qf_session="));
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
}

#[tokio::test]
async fn sixth_login_attempt_in_a_minute_is_rate_limited() {
    let (app, _, _dir) = app();
    for _ in 0..5 {
        app.clone().oneshot(login("nope")).await.unwrap();
    }
    let res = app.oneshot(login(PASSWORD)).await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn app_page_redirects_to_login_without_session() {
    let (app, _, _dir) = app();
    let res = app.oneshot(Request::get("/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);
    assert_eq!(res.headers()[header::LOCATION], "/login");
}

#[tokio::test]
async fn session_serves_app_and_dispatches_rpc() {
    let (app, state, _dir) = app();
    let token = state.sessions.create();

    let page = app
        .clone()
        .oneshot(
            Request::get("/stock")
                .header(header::COOKIE, format!("qf_session={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(page.status(), StatusCode::OK);
    let body = page.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], b"<html>app</html>");

    let unknown = app.clone().oneshot(rpc("does_not_exist", Some(&token), ORIGIN)).await.unwrap();
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);

    let ok = app.oneshot(rpc("initialized", Some(&token), ORIGIN)).await.unwrap();
    assert_eq!(ok.status(), StatusCode::OK);
    let body = ok.into_body().collect().await.unwrap().to_bytes();
    assert!(&body[..] == b"false" || &body[..] == b"true");
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p qf-server`
Expected: compile errors (`unresolved import qf_server::auth`).

- [ ] **Step 4: Implement `auth.rs`**

`crates/qf-server/src/auth.rs`:
```rust
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::http::{header, HeaderMap};

pub const COOKIE_NAME: &str = "qf_session";
const COOKIE_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;

pub struct Sessions {
    inner: Mutex<HashMap<String, Instant>>,
    ttl: Duration,
}

impl Sessions {
    pub fn new(ttl: Duration) -> Self {
        Self { inner: Mutex::new(HashMap::new()), ttl }
    }

    pub fn create(&self) -> String {
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).expect("OS random number generator unavailable");
        let token = hex::encode(bytes);
        self.inner.lock().unwrap().insert(token.clone(), Instant::now() + self.ttl);
        token
    }

    pub fn is_valid(&self, token: &str) -> bool {
        let mut sessions = self.inner.lock().unwrap();
        let now = Instant::now();
        sessions.retain(|_, expires| *expires > now);
        sessions.contains_key(token)
    }

    pub fn remove(&self, token: &str) {
        self.inner.lock().unwrap().remove(token);
    }
}

/// Allows at most `max` login attempts per `window`, across all clients.
pub struct LoginLimiter {
    attempts: Mutex<VecDeque<Instant>>,
    max: usize,
    window: Duration,
}

impl LoginLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        Self { attempts: Mutex::new(VecDeque::new()), max, window }
    }

    pub fn try_acquire(&self) -> bool {
        let mut attempts = self.attempts.lock().unwrap();
        let now = Instant::now();
        while attempts.front().is_some_and(|t| now.duration_since(*t) > self.window) {
            attempts.pop_front();
        }
        if attempts.len() >= self.max {
            return false;
        }
        attempts.push_back(now);
        true
    }
}

pub fn session_cookie(token: &str) -> String {
    format!("{COOKIE_NAME}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={COOKIE_MAX_AGE_SECS}")
}

pub fn cleared_cookie() -> String {
    format!("{COOKIE_NAME}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

pub fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE_NAME)
        .map(|(_, value)| value.to_string())
}
```

- [ ] **Step 5: Implement `config.rs`, `routes.rs`, `login.html` and `main.rs`**

`crates/qf-server/src/config.rs`:
```rust
use std::path::PathBuf;

use qf_core::startup::CoreConfig;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: String,
    pub data_dir: PathBuf,
    pub web_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub public_origin: String,
    pub secret_key_file: PathBuf,
    pub web_password_file: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let or = |key: &str, default: &str| get(key).unwrap_or_else(|| default.to_string());
        let public_origin = get("QF_PUBLIC_ORIGIN")
            .ok_or("QF_PUBLIC_ORIGIN is required, e.g. http://homelab.lan:8080")?;
        Ok(Self {
            bind: or("QF_BIND", "0.0.0.0:8080"),
            data_dir: or("QF_DATA_DIR", "/data").into(),
            web_dir: or("QF_WEB_DIR", "/app/web").into(),
            resources_dir: or("QF_RESOURCES_DIR", "/app/resources").into(),
            public_origin: public_origin.trim_end_matches('/').to_string(),
            secret_key_file: or("QF_SECRET_KEY_FILE", "/run/secrets/qf_secret_key").into(),
            web_password_file: or("QF_WEB_PASSWORD_FILE", "/run/secrets/qf_web_password").into(),
        })
    }

    pub fn core(&self) -> CoreConfig {
        CoreConfig {
            data_dir: self.data_dir.clone(),
            resources_dir: self.resources_dir.clone(),
            secret_key_hex: std::fs::read_to_string(&self.secret_key_file).ok(),
            web_password_file: self.web_password_file.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_is_required_and_trailing_slash_trimmed() {
        assert!(Config::from_lookup(|_| None).is_err());
        let cfg = Config::from_lookup(|k| (k == "QF_PUBLIC_ORIGIN").then(|| "http://h:8080/".to_string())).unwrap();
        assert_eq!(cfg.public_origin, "http://h:8080");
        assert_eq!(cfg.bind, "0.0.0.0:8080");
    }
}
```

`crates/qf-server/src/routes.rs`:
```rust
use std::{path::PathBuf, sync::Arc};

use axum::{
    body::Bytes,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Path, Request, State,
    },
    http::{header, HeaderMap, Method, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast::error::RecvError;
use tower_http::services::{ServeDir, ServeFile};

use crate::auth::{cleared_cookie, session_cookie, token_from_headers, LoginLimiter, Sessions};

#[derive(Clone)]
pub struct ServerState {
    pub sessions: Arc<Sessions>,
    pub limiter: Arc<LoginLimiter>,
    pub password_hash: Arc<String>,
    pub public_origin: Arc<String>,
    pub web_dir: PathBuf,
    pub resources_dir: PathBuf,
    pub data_dir: PathBuf,
}

pub fn router(state: ServerState) -> Router {
    let spa = ServeDir::new(&state.web_dir).fallback(ServeFile::new(state.web_dir.join("index.html")));
    let protected = Router::new()
        .route("/rpc/{name}", post(rpc))
        .route("/ws", get(ws))
        .nest_service("/sounds/builtin", ServeDir::new(state.resources_dir.join("sounds")))
        .nest_service("/sounds/custom", ServeDir::new(state.data_dir.join("sounds")))
        .fallback_service(spa)
        .layer(middleware::from_fn_with_state(state.clone(), require_session));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", post(logout))
        .merge(protected)
        .layer(middleware::from_fn_with_state(state.clone(), check_origin))
        .layer(DefaultBodyLimit::max(20 * 1024 * 1024))
        .with_state(state)
}

async fn check_origin(State(state): State<ServerState>, req: Request, next: Next) -> Response {
    let is_read = req.method() == Method::GET || req.method() == Method::HEAD;
    if !is_read || req.uri().path() == "/ws" {
        let allowed = req
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|origin| origin == state.public_origin.as_str());
        if !allowed {
            return (StatusCode::FORBIDDEN, "Origin not allowed").into_response();
        }
    }
    next.run(req).await
}

async fn require_session(State(state): State<ServerState>, req: Request, next: Next) -> Response {
    let valid = token_from_headers(req.headers()).is_some_and(|t| state.sessions.is_valid(&t));
    if valid {
        return next.run(req).await;
    }
    let path = req.uri().path();
    if path.starts_with("/rpc/") || path == "/ws" || path.starts_with("/sounds/") {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Redirect::to("/login").into_response()
}

async fn login_page() -> Html<&'static str> {
    Html(include_str!("login.html"))
}

#[derive(Deserialize)]
struct LoginForm {
    password: String,
}

async fn login_submit(State(state): State<ServerState>, Form(form): Form<LoginForm>) -> Response {
    if !state.limiter.try_acquire() {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many login attempts; wait a minute.").into_response();
    }
    if !qf_core::web_auth::verify_password(&form.password, &state.password_hash) {
        return Redirect::to("/login?error=1").into_response();
    }
    let token = state.sessions.create();
    ([(header::SET_COOKIE, session_cookie(&token))], Redirect::to("/")).into_response()
}

async fn logout(State(state): State<ServerState>, headers: HeaderMap) -> Response {
    if let Some(token) = token_from_headers(&headers) {
        state.sessions.remove(&token);
    }
    ([(header::SET_COOKIE, cleared_cookie())], Redirect::to("/login")).into_response()
}

async fn rpc(Path(name): Path<String>, body: Bytes) -> Response {
    let args: Value = if body.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&body) {
            Ok(value) => value,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"component": "Rpc", "message": format!("Invalid JSON body: {e}")})),
                )
                    .into_response()
            }
        }
    };
    match qf_core::commands::rpc::dispatch(&name, args).await {
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"component": "Rpc", "message": format!("Unknown command {name}")})),
        )
            .into_response(),
        Some(Ok(value)) => Json(value).into_response(),
        Some(Err(error)) => (StatusCode::UNPROCESSABLE_ENTITY, Json(error)).into_response(),
    }
}

async fn ws(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(forward_events)
}

async fn forward_events(mut socket: WebSocket) {
    let mut events = qf_core::events::subscribe();
    loop {
        tokio::select! {
            frame = events.recv() => match frame {
                Ok(value) => {
                    if socket.send(Message::Text(value.to_string().into())).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                None | Some(Err(_)) | Some(Ok(Message::Close(_))) => break,
                _ => {}
            },
        }
    }
}
```

`crates/qf-server/src/login.html`:
```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Quantframe Server — Sign in</title>
  <style>
    :root { color-scheme: dark; }
    body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #1a1b1e; color: #c1c2c5; font: 15px system-ui, sans-serif; }
    form { width: min(320px, calc(100% - 32px)); display: grid; gap: 12px; }
    h1 { font-size: 20px; margin: 0 0 8px; color: #fff; }
    input, button { font: inherit; padding: 10px 12px; border-radius: 6px; border: 1px solid #373a40; }
    input { background: #25262b; color: #fff; }
    button { background: #228be6; border-color: #228be6; color: #fff; cursor: pointer; }
    .error { color: #fa5252; margin: 0; }
  </style>
</head>
<body>
  <form method="post" action="/login">
    <h1>Quantframe Server</h1>
    <p class="error" id="error" hidden>Wrong password.</p>
    <input type="password" name="password" placeholder="Server password" autocomplete="current-password" required autofocus>
    <button type="submit">Sign in</button>
  </form>
  <script>
    if (new URLSearchParams(location.search).has("error")) document.getElementById("error").hidden = false;
  </script>
</body>
</html>
```

`crates/qf-server/src/main.rs`:
```rust
use std::{sync::Arc, time::Duration};

use qf_server::{
    auth::{LoginLimiter, Sessions},
    config::Config,
    routes::{router, ServerState},
};

#[tokio::main]
async fn main() {
    let cfg = Config::from_env().unwrap_or_else(|e| {
        eprintln!("Configuration error: {e}");
        std::process::exit(2);
    });
    let handles = match qf_core::startup::start(cfg.core()).await {
        Ok(handles) => handles,
        Err(e) => {
            eprintln!("Startup failed: {} ({})", e.message, e.component);
            std::process::exit(1);
        }
    };
    let state = ServerState {
        sessions: Arc::new(Sessions::new(Duration::from_secs(30 * 24 * 60 * 60))),
        limiter: Arc::new(LoginLimiter::new(5, Duration::from_secs(60))),
        password_hash: Arc::new(handles.web_password_hash),
        public_origin: Arc::new(cfg.public_origin.clone()),
        web_dir: cfg.web_dir.clone(),
        resources_dir: cfg.resources_dir.clone(),
        data_dir: cfg.data_dir.clone(),
    };
    let listener = tokio::net::TcpListener::bind(&cfg.bind).await.unwrap_or_else(|e| {
        eprintln!("Cannot bind {}: {e}", cfg.bind);
        std::process::exit(1);
    });
    println!("quantframe-server listening on {} (origin {})", cfg.bind, cfg.public_origin);
    axum::serve(listener, router(state)).await.expect("server error");
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p qf-server`
Expected: 8 passed (7 integration tests and 1 config test).

If `app_page_redirects_to_login_without_session` returns `200`, the session layer is not wrapping the SPA fallback after `merge`. Remove `.fallback_service(spa)` from `protected` and add this to the outer router instead:
```rust
        .fallback_service(
            tower::ServiceBuilder::new()
                .layer(middleware::from_fn_with_state(state.clone(), require_session))
                .service(spa),
        )
```
Then re-run the tests.

- [ ] **Step 7: Run the server locally against a scratch data dir**

This is a smoke test, not Docker.
```bash
mkdir -p /tmp/qfs/{data,secrets,web} && echo '<html>placeholder</html>' > /tmp/qfs/web/index.html
openssl rand -hex 32 > /tmp/qfs/secrets/key && echo 'local-dev-password' > /tmp/qfs/secrets/pw
QF_PUBLIC_ORIGIN=http://localhost:8080 QF_BIND=127.0.0.1:8080 QF_DATA_DIR=/tmp/qfs/data \
QF_WEB_DIR=/tmp/qfs/web QF_RESOURCES_DIR=$PWD/resources \
QF_SECRET_KEY_FILE=/tmp/qfs/secrets/key QF_WEB_PASSWORD_FILE=/tmp/qfs/secrets/pw \
cargo run -p qf-server
```
Expected: a `Loaded 38xx tradable items` log line, then `quantframe-server listening on 127.0.0.1:8080`.

In a second terminal, run `curl -s localhost:8080/healthz`. Expected: `ok`. Stop the server with Ctrl+C.

- [ ] **Step 8: Commit**

```bash
git add -A Cargo.toml Cargo.lock crates/qf-server
git commit -m "feat(server): add axum server with password login, rpc, event socket and static files

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 8: Frontend transport — replace Tauri `invoke`, `listen` and plugins

**Files:**
- Create: `web/src/api/transport.ts`, `web/src/api/socket.ts`, `web/src/utils/openUrl.ts`, `web/src/utils/pickFile.ts`, `web/src/utils/downloadJson.ts`
- Modify:
  - `web/package.json`, `web/pnpm-lock.yaml`, `web/vite.config.ts`
  - `web/src/api/index.ts`, `web/src/api/events/index.ts`
  - `web/src/contexts/app.context.tsx`, `web/src/contexts/liveScraper.context.tsx`
  - `web/src/utils/helper.ts`, `web/src/utils/logger.helper.ts`
  - `web/src/components/ThemeEditor/index.tsx`
  - `web/src/components/Forms/EditNotificationSetting/views/ManageSoundsView.tsx`
  - `web/src/api/sound/index.ts`, `web/src/api/{stack_item,wish_list,trade_entry}/index.ts`
  - the five shell-`open` importers
- Delete: `web/src/components/Modals/UpdateAvailable/`, `web/src/components/Modals/TermsAndConditions/`

**Interfaces:**
- **Consumes:** HTTP `POST /rpc/{name}` (JSON body; 200 with the value, 401, 404, or 422 with the `Error` JSON) and `GET /ws` (frames `{channel, payload}`), from Task 7.
- **Produces:**
  - `rpcInvoke<T>(command: string, args?: Record<string, any>): Promise<T>`
  - `listen<T>(channel: string, handler: (event: { payload: T }) => void): Promise<() => void>`
  - `open(url: string, target?: string): void`
  - `pickFile(accept: string): Promise<File | null>`, `fileToBase64(file: File): Promise<string>`
  - `downloadJson(fileName: string, data: unknown): void`

- [ ] **Step 1: Dependencies and dev proxy**

```bash
cd ~/Projects/Personal/quantframe-server/web
pnpm remove @tauri-apps/api @tauri-apps/plugin-clipboard-manager @tauri-apps/plugin-dialog @tauri-apps/plugin-fs @tauri-apps/plugin-http @tauri-apps/plugin-notification @tauri-apps/plugin-os @tauri-apps/plugin-process @tauri-apps/plugin-shell @tauri-apps/plugin-updater @tauri-apps/cli
```

In `web/package.json` `scripts`, delete `tauri:build`, `tauri:dev` and `tauri:dev-temp-db`.

In `web/vite.config.ts`, replace everything from the `// Vite options tailored for Tauri development` comment to the end of the returned object with:
```ts
  server: {
    port: 1420,
    strictPort: true,
    proxy: {
      "/rpc": "http://127.0.0.1:8080",
      "/login": "http://127.0.0.1:8080",
      "/logout": "http://127.0.0.1:8080",
      "/sounds": "http://127.0.0.1:8080",
      "/ws": { target: "ws://127.0.0.1:8080", ws: true },
    },
  },
  envPrefix: ["VITE_"],
}));
```

- [ ] **Step 2: Write the transport, socket and helper modules**

`web/src/api/transport.ts`:
```ts
/** Calls a server command. Resolves with the result; rejects with the server's Error JSON, like Tauri's invoke. */
export async function rpcInvoke<T>(command: string, args?: Record<string, any>): Promise<T> {
  const res = await fetch(`/rpc/${command}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    credentials: "same-origin",
    body: JSON.stringify(args ?? {}),
  });
  if (res.status === 401) {
    window.location.href = "/login";
    throw { component: "Rpc", message: "Not signed in" };
  }
  const text = await res.text();
  const body = text ? JSON.parse(text) : null;
  if (!res.ok) throw body ?? { component: "Rpc", message: `Request failed with status ${res.status}` };
  return body as T;
}
```

`web/src/api/socket.ts`:
```ts
type Handler = (event: { payload: any }) => void;

const handlers = new Map<string, Set<Handler>>();
let socket: WebSocket | undefined;
let retries = 0;

function connect() {
  const protocol = window.location.protocol === "https:" ? "wss" : "ws";
  socket = new WebSocket(`${protocol}://${window.location.host}/ws`);
  socket.onopen = () => {
    retries = 0;
  };
  socket.onmessage = (message) => {
    try {
      const { channel, payload } = JSON.parse(message.data);
      handlers.get(channel)?.forEach((handler) => handler({ payload }));
    } catch (error) {
      console.error("Invalid event frame", error);
    }
  };
  socket.onclose = () => {
    const delay = Math.min(30_000, 1000 * 2 ** retries++);
    setTimeout(connect, delay);
  };
}

/** Same shape as Tauri's `listen`: resolves to an unlisten function. */
export function listen<T = any>(channel: string, handler: (event: { payload: T }) => void): Promise<() => void> {
  if (!socket) connect();
  const set = handlers.get(channel) ?? new Set<Handler>();
  set.add(handler as Handler);
  handlers.set(channel, set);
  return Promise.resolve(() => {
    set.delete(handler as Handler);
  });
}
```

`web/src/utils/openUrl.ts`:
```ts
export const open = (url: string, _target?: string) => {
  window.open(url, "_blank", "noopener,noreferrer");
};
```

`web/src/utils/pickFile.ts`:
```ts
export function pickFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.onchange = () => resolve(input.files?.[0] ?? null);
    input.click();
  });
}

export async function fileToBase64(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
```

`web/src/utils/downloadJson.ts`:
```ts
export function downloadJson(fileName: string, data: unknown) {
  const blob = new Blob([JSON.stringify(data, null, 2)], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
```

- [ ] **Step 3: Swap imports at every Tauri call site**

| File | Change |
|---|---|
| `web/src/api/index.ts:2` | `import { invoke } from "@tauri-apps/api/core";` → `import { rpcInvoke as invoke } from "./transport";` |
| `web/src/api/events/index.ts:1` | `import { listen } from "@tauri-apps/api/event";` → `import { listen } from "../socket";` |
| `web/src/contexts/liveScraper.context.tsx:4` | → `import { rpcInvoke as invoke } from "@api/transport";` |
| `web/src/contexts/app.context.tsx:13` | → `import { rpcInvoke as invoke } from "@api/transport";` |
| `web/src/contexts/app.context.tsx:14` | → `import { listen } from "@api/socket";` |
| `components/Layouts/LogIn/index.tsx:11`, `components/Shared/TooltipIcon/index.tsx:4`, `components/Modals/RivenDetails/Tabs/WFM/index.tsx:8`, `components/Modals/ItemDetails/Tabs/WFM/index.tsx:5`, `pages/about/index.tsx:12` | `import { open } from "@tauri-apps/plugin-shell";` → `import { open } from "@utils/openUrl";` |
| `components/ThemeEditor/index.tsx:10` | Replace the clipboard import with `const writeText = (text: string) => navigator.clipboard.writeText(text);` and `const readText = () => navigator.clipboard.readText();` |

`navigator.clipboard` only works on secure origins (HTTPS or localhost). On plain-HTTP LAN access the theme copy/paste buttons will fail, and they log the error rather than crashing. This is acceptable for phase 1.

- [ ] **Step 4: Remove the updater and ToS from `app.context.tsx`, and delete their modals**

In `web/src/contexts/app.context.tsx`:
- Delete the imports of `TermsAndConditions` (line 4), `UpdateAvailable` (5), `resolveResource` (15), `readTextFile` (16) and `check` (17).
- Delete `checkForUpdates` (lines 108–125) and `checkForTosUpdates` (127–155), plus their calls (177, 184–187).
- Delete `checkForUpdates` from the context type (52), the default value (64) and the provided value (224, 228).

```bash
rm -rf web/src/components/Modals/UpdateAvailable web/src/components/Modals/TermsAndConditions
```

- [ ] **Step 5: Sounds, logger, file picker and exports**

In `web/src/utils/helper.ts`:
- Delete lines 4–5 (the Tauri imports) and `cachedCustomSoundsPath`.
- Replace `PlaySound` (keep the `(window as any).PlaySound = PlaySound;` line after it) with:
```ts
export const PlaySound = async (fileName: string, volume: number = 1.0) => {
  const url = isCustomSound(fileName)
    ? `/sounds/custom/${encodeURIComponent(stripCustomSoundPrefix(fileName))}`
    : `/sounds/builtin/${encodeURIComponent(fileName)}`;
  try {
    const audio = new Audio(url);
    audio.volume = volume;
    await audio.play();
  } catch (error) {
    console.error(`Error playing sound ${fileName}:`, error);
  }
};
```

`web/src/utils/logger.helper.ts`: delete the Tauri import (line 1). Replace `await invoke("log_send", { component, msg, level, console, file })` (line 18) with `console.log(`[${level}] ${component}: ${msg}`);`. Upstream never had a `log_send` command.

In `web/src/api/sound/index.ts`:
- Delete `getCustomSoundsPath`.
- Add `import { fileToBase64 } from "@utils/pickFile";`.
- Replace the add method with:
```ts
  async addCustomSound(name: string, file: File) {
    return this.client.sendInvoke<TauriTypes.CustomSound[]>("sound_add_custom_sound", {
      name,
      file_name: file.name,
      data_base64: await fileToBase64(file),
    });
  }
```

In `web/src/components/Forms/EditNotificationSetting/views/ManageSoundsView.tsx`:
- Delete the `@tauri-apps/plugin-dialog` import (line 8) and add `import { pickFile } from "@utils/pickFile";`.
- Replace the `openFile({...})` call (line ~227) with `const selected = await pickFile(".mp3,.wav,.ogg");`.
- Store `selected` (a `File | null`) where the path string was stored. Change that state's type from `string` to `File | null`, and pass it to `api.sound.addCustomSound(name, selected)` at line ~74.
- Where the component shows the chosen path, show `selected?.name`.

In `web/src/api/stack_item/index.ts` (line ~49), `web/src/api/wish_list/index.ts` (~46) and `web/src/api/trade_entry/index.ts` (~35):
- Add `import { downloadJson } from "@utils/downloadJson";`.
- Change the export method body to download the returned rows, keeping its existing argument object. For stack_item:
```ts
    const rows = await this.client.sendInvoke<unknown[]>("export_stock_item_json", { query });
    downloadJson("quantframe_stock_items.json", rows);
    return "quantframe_stock_items.json";
```
- Use file names `quantframe_wish_list.json` and `quantframe_trade_entries.json` for the other two.

- [ ] **Step 6: List the remaining Tauri imports for Task 9**

Run: `grep -rln "@tauri-apps" web/src`
Expected: only files that Task 9 deletes: `components/Popups/ProcessTrade`, `pages/trading_analytics/Tabs/WFGDPR`, `hooks/useTauriDragDrop.hook.ts`, `components/Modals/PatreonModal`. Any other file listed must be fixed now using the Step 3 patterns.

- [ ] **Step 7: Commit**

`pnpm build` is verified in Task 9, because pages for removed features still import deleted modules.
```bash
git add -A web
git commit -m "refactor(web): replace Tauri invoke, events and plugins with fetch and WebSocket

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 9: Frontend feature cuts and command allowlist check

**Files:**
- Create: `scripts/check-rpc-commands.py`
- Delete/modify: the `web/src` files listed below

**Interfaces:**
- **Consumes:** the table rows in `crates/qf_core/src/commands/rpc.rs` (Task 6), in the format `name => module::func {…},`.
- **Produces:** `python3 scripts/check-rpc-commands.py` exits 0 only when every command name the frontend invokes is on the server allowlist.

- [ ] **Step 1: Write the allowlist check (the failing "test")**

`scripts/check-rpc-commands.py`:
```python
#!/usr/bin/env python3
"""Fail if the web app invokes any command the server does not expose."""
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parent.parent
table = (root / "crates/qf_core/src/commands/rpc.rs").read_text()
server = set(re.findall(r"^\s*(\w+)\s*=>\s*\w+::\w+", table, re.M))

pattern = re.compile(r"""(?:sendInvoke|invoke)(?:<.*?>)?\(\s*["'](\w+)["']""")
used: dict[str, list[str]] = {}
for path in (root / "web/src").rglob("*.ts*"):
    for name in pattern.findall(path.read_text()):
        used.setdefault(name, []).append(str(path.relative_to(root)))

missing = {name: files for name, files in used.items() if name not in server}
for name, files in sorted(missing.items()):
    print(f"{name}: {', '.join(sorted(set(files)))}")
print(f"{len(server)} server commands, {len(used)} used by web, {len(missing)} missing")
sys.exit(1 if missing else 0)
```

Run: `python3 scripts/check-rpc-commands.py`
Expected: exit 1, listing names such as `alert_get_alerts`, `chat_refresh`, `riven_prices_lookup`, `wfgdpr_load`. The first line of the summary reads `62 server commands`.

- [ ] **Step 2: Delete removed features**

```bash
cd ~/Projects/Personal/quantframe-server/web/src
rm -rf components/Modals/PatreonModal components/Shared/PatreonOverlay hooks/useGetPatreonInfo.hook.ts pages/banned
rm -rf pages/chat components/DataDisplay/ChatRome components/DataDisplay/ChatMessage api/chat
rm -rf pages/live_scraper/Tabs/Riven components/Forms/Settings/Tabs/LiveTrading/Tabs/Riven
rm -rf pages/live_scraper/Tabs/Syndicate components/Forms/Settings/Tabs/LiveTrading/Tabs/Syndicate
rm -rf pages/trading_analytics/Tabs/Syndicate api/syndicate
rm -rf pages/wf_inventory api/wf_inventory
rm -rf pages/trading_analytics/Tabs/WFGDPR hooks/useTauriDragDrop.hook.ts api/log_parser
rm -rf api/alert hooks/useHasAlert.hook.ts
rm -rf pages/trading_analytics/Tabs/User api/market api/analytics
rm -rf pages/trading_analytics/Tabs/Item pages/trading_analytics/Tabs/Riven api/item api/riven
rm -rf pages/trade_messages/Tabs/Riven/FindInterestingRivensModal
rm -rf api/stack_riven api/auction pages/warframe_market/Tabs/Auctions
rm -rf components/Popups/ProcessTrade pages/clean pages/debug/tabs/EELog
cd ~/Projects/Personal/quantframe-server
```

- [ ] **Step 3: Remove references, guided by the compiler**

Run: `cd web && pnpm build 2>&1 | grep -E "error TS" | head -60`

Fix every error by **removing** the reference (import, tab entry, route, nav item, `TauriClient` field or constructor line). Never re-add a deleted feature. Known edits:
- **`api/index.ts`:**
  - Remove the imports, fields and constructor lines for `alert`, `analytics`, `chat`, `auction`, `item`, `riven`, `market`, `log_parser`, `wf_inventory`, `syndicate` and `stock_riven`.
  - Replace `AddMetric` with `export const AddMetric = async (_key: string, _value: string) => {};`.
  - Replace `HasPermission` with `export const HasPermission = async (_flag: TauriTypes.PermissionsFlags) => true;`.
- **`App.tsx`:** remove the `PatreonModal` import (line 12) and the `patreon:` modal entry (74). Remove the `window.onclick = … setLastUserActivity` block (113–116).
- **`components/Layouts/Routes.tsx`:** remove the `clean`, `chat`, `wf_inventory` and banned routes, and the `IsUserBanned` usage (lines 166, 178, 185–200, 227, 231, 242–246). In **`routeLoaders.ts`**, remove `chat`, `banned` and `wf_inventory`.
- **`components/Layouts/LogIn/index.tsx`:** remove the chat nav entry (72–90), the wf_inventory nav entry (100–109), the `AddMetric` call (36) and the `qf_banned` part of the ban check (143–145). **`LogOut/index.tsx`:** keep only the `wfm_banned` check.
- **`components/Shared/UserMenu/index.tsx`:** remove `qf_banned` from the `IsAuthenticated` check (62–73).
- **`pages/auth/login.tsx`:** change `if (u.qf_banned || !u.verification)` to `if (!u.verification)`.
- **`types/tauri.type.ts`:**
  - Remove the `User` fields `check_code`, `qf_access_token`, `qf_banned`, `qf_banned_reason`, `qf_banned_until`, `unread_messages`, `patreon_tier` and `permissions`. **Keep `anonymous`**, which the backend still sends.
  - Remove `Events.OnChatMessage`, `TradeMode.Syndicate`, `wf_inventory` settings and the `WFGDPR*` types.
- **`pages/live_scraper/index.tsx`:** remove the `RivenPanel` and `SyndicatePanel` tab entries and the `<LiveScraperControl />` element and import. **`pages/live_scraper/Tabs/index.ts`:** export only `ItemPanel` and `WishListPanel`.
- **`components/Forms/Settings/Tabs/LiveTrading/index.tsx`:** remove the Riven and Syndicate tabs.
- **`pages/trading_analytics/index.tsx` and `Tabs/index.ts`:** keep only the Transaction tab. **`pages/warframe_market`:** keep only the Orders tab.
- **`pages/trade_messages/Tabs/Riven/index.tsx`:** remove the `FindInterestingRivensModal` import and usage.
- **`pages/about/index.tsx`:** remove the Patreon card (41, 71–88, 97–101) and the "check for updates" button (42, 104–116).
- **`api/live_scraper/index.ts`:** replace with phase-1 stubs:
```ts
import { TauriClient } from "..";
import { TauriTypes } from "$types";
// Live trading returns in phase 3; these stubs keep the stock and wish list screens working.
export class LiveScraperModule {
  constructor(private readonly client: TauriClient) {}
  async toggle(): Promise<void> {}
  async get_interesting_wtb_items(_settings: TauriTypes.ItemSettings): Promise<TauriTypes.ItemPriceInfo[]> {
    return [];
  }
  async get_state(): Promise<{ is_running: boolean }> {
    return { is_running: false };
  }
}
```
- **`useHasAlert()` call sites** (home, debug, live_scraper, trade_messages, trading_analytics, warframe_market): replace `data-has-alert={useHasAlert()}` with `data-has-alert={false}` and delete the import.
- **`api/cache/index.ts`:** remove `getSyndicates`, `getChatIcons`, `getChatLink`, `getRivenAttributes`, `getRivenWeapons`, `openThemeFolder` and `createTheme`.
- **`contexts/cache.context.tsx`:** provide `weapons: []` and remove its query.
- **`components/ThemeEditor/index.tsx`:** remove the "open theme folder" button (277) and the "save theme" button.
- **`contexts/app.context.tsx`:** remove the alerts query, `refetchAlerts`, its interval and `alerts` from the context value (49, 62, 157–178, 191, 196–198, 221, 228).
- **`components/Layouts/Shared/Header/index.tsx`:** remove the alert Ticker (14–26, 34–44). **`pages/debug/tabs/states/index.tsx`:** remove the alerts block (67–71).
- **`components/Forms/Settings/Tabs/Advanced/Tabs/Log/index.tsx` and `components/Modals/Error/index.tsx`:** remove the export-logs button, and delete `export_logs` from `api/log/index.ts`.
- **Settings:** remove the http-server advanced tab.
- **Transactions:** remove any "calculate tax" button calling `transaction_calculate_tax`.

Repeat `pnpm build` until it succeeds.

- [ ] **Step 4: Make the allowlist check pass**

Run: `python3 scripts/check-rpc-commands.py`

For every name still listed, delete the frontend method that invokes it and that method's callers. Expected examples: `debug_test`, `debug_get_ee_logs`, `debug_export_ee_logs`, `auth_has_permission`, `app_exit`, `cache_create_theme`, `log_export`, `sound_get_custom_sounds_path`.

Repeat until the output ends with `0 missing` and the exit code is 0.

Run: `cd web && pnpm build`
Expected: build succeeds.

Run: `grep -rni "quantframe.app\|@tauri-apps\|patreon" web/src crates`
Expected: no output.

- [ ] **Step 5: Manual smoke test against the local server**

```bash
# terminal 1: server, with the Vite dev origin
QF_PUBLIC_ORIGIN=http://localhost:1420 QF_BIND=127.0.0.1:8080 QF_DATA_DIR=/tmp/qfs/data \
QF_WEB_DIR=/tmp/qfs/web QF_RESOURCES_DIR=$PWD/resources \
QF_SECRET_KEY_FILE=/tmp/qfs/secrets/key QF_WEB_PASSWORD_FILE=/tmp/qfs/secrets/pw cargo run -p qf-server
# terminal 2
cd web && pnpm dev
```

Open `http://localhost:1420/login` and sign in with `local-dev-password`. Check that:
1. The browser ends up in the React app showing the warframe.market sign-in form (no account stored yet).
2. There are no `/rpc/*` 404s in the browser devtools Network tab.
3. The `/ws` request shows status 101.

Signing in to warframe.market with real credentials is optional here; it is done in Task 10.

- [ ] **Step 6: Commit**

```bash
git add -A web scripts
git commit -m "feat(web): cut removed features and check frontend commands against server allowlist

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

### Task 10: Docker image, Compose file and server acceptance

**Files:**
- Create: `Dockerfile`, `.dockerignore`, `compose.yaml`, `.env.example`, `docs/PHASE-1-ACCEPTANCE.md`
- Modify: `README.md`

**Interfaces:**
- **Consumes:** the `qf-server` binary (Task 7), the `web` build (Task 9) and `resources/sounds` (Task 1).
- **Produces:** a container listening on 8080 with a `/data` volume and the Compose secrets `qf_secret_key` and `qf_web_password`.

- [ ] **Step 1: Write the Dockerfile and ignore file**

`Dockerfile`:
```dockerfile
# syntax=docker/dockerfile:1
FROM node:22-bookworm-slim AS web
WORKDIR /src/web
RUN npm i -g pnpm@11.3.0
COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM rust:1-bookworm AS server
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p qf-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --home-dir /data qf && mkdir -p /data && chown qf /data
COPY --from=server /src/target/release/qf-server /usr/local/bin/qf-server
COPY --from=web /src/web/dist /app/web
COPY resources /app/resources
USER qf
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=60s CMD curl -fsS http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["qf-server"]
```

`.dockerignore`:
```
target
web/node_modules
web/dist
secrets
.env
.git
```

- [ ] **Step 2: Write the Compose and env files**

`compose.yaml`:
```yaml
services:
  quantframe-server:
    build: .
    image: quantframe-server:local
    restart: unless-stopped
    ports:
      - "8080:8080"
    environment:
      QF_PUBLIC_ORIGIN: ${QF_PUBLIC_ORIGIN:?set QF_PUBLIC_ORIGIN in .env}
    volumes:
      - qf-data:/data
    secrets:
      - qf_secret_key
      - qf_web_password

secrets:
  qf_secret_key:
    file: ./secrets/qf_secret_key
  qf_web_password:
    file: ./secrets/qf_web_password

volumes:
  qf-data: {}
```

`.env.example`:
```
# The exact URL you type into the browser on your LAN (scheme, host, port; no trailing slash).
QF_PUBLIC_ORIGIN=http://homelab.lan:8080
```

- [ ] **Step 3: Document setup in the README**

Append to `README.md`:
````markdown
## Running on the homelab

Docker runs on the server only.

```bash
git clone <this repo> && cd quantframe-server
cp .env.example .env            # edit QF_PUBLIC_ORIGIN
mkdir -p secrets
openssl rand -hex 32 > secrets/qf_secret_key
printf '%s' 'choose-a-long-password' > secrets/qf_web_password   # at least 12 characters
sudo chown 10001:10001 secrets/* && sudo chmod 600 secrets/*
docker compose up -d --build
docker compose logs -f
```

Open `QF_PUBLIC_ORIGIN` in a browser, sign in with the web password, then sign in to warframe.market.
Only the warframe.market token is kept, encrypted with `qf_secret_key`. Your warframe.market password is never stored.

- **Changing the web password:** edit `secrets/qf_web_password` and restart the container.
- **Losing `qf_secret_key`:** the stored token can't be decrypted, so sign in to warframe.market again.
- **Backups:** the `qf-data` volume holds `quantframe.sqlite`, and a `quantframe.sqlite_backup` copy is made on every start.
````

- [ ] **Step 4: Commit**

```bash
git add Dockerfile .dockerignore compose.yaml .env.example README.md
git commit -m "build: add Docker image and Compose file for homelab deployment

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

- [ ] **Step 5: Server acceptance (run on the homelab)**

Get the repo onto the server (push to a remote it can pull from, or `rsync`). Follow the README steps there, then check:

1. `docker compose ps` shows `healthy` within 2 minutes.
2. `docker compose logs` contains `Loaded` … `tradable items`, and has no `Startup failed`.
3. From the desktop browser, `QF_PUBLIC_ORIGIN` redirects to `/login`. A wrong password shows the error, and the right one loads the app.
4. warframe.market sign-in with real credentials succeeds, and the user menu shows your in-game name.
5. **Spec §13.3 check:**
   - In the user menu, set status to **Online**, and confirm on your warframe.market profile, from another device, that it shows Online.
   - Set **Invisible**, and confirm it changes back.
6. `docker compose restart`, then:
   - you are still signed in to warframe.market, since the token was decrypted from the DB;
   - your profile shows invisible, since it is forced on boot.
7. Stock items:
   - Create one (e.g. Arcane Energize, rank 0) and sell 1 of it. A transaction appears.
   - Delete the stock item.
   - Export stock JSON; the browser downloads `quantframe_stock_items.json`.
8. Upload a custom sound in notification settings and play it.
9. `docker compose exec quantframe-server grep -c wfm_token /data/auth.json` prints `0`.
10. `docker compose down` then `docker volume ls` still lists the `qf-data` volume.

Write `docs/PHASE-1-ACCEPTANCE.md` with one line per check (number, pass/fail, note) and the date, then commit it:
```bash
git add docs/PHASE-1-ACCEPTANCE.md
git commit -m "docs: record phase 1 server acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01NjXob2hagm4DWnMb3eLe29"
```

---

## Self-Review

**Spec coverage (phase 1 scope, spec §11 as amended in §14):**

| Spec requirement | Covered by |
|---|---|
| Three Tauri seams replaced (§4.2) | Tasks 2, 5 and 6 |
| `qf_api` removed, no `api.quantframe.app` calls | Tasks 1, 5 and 6; grep checks in Tasks 5 and 9 |
| WfmSession: encrypted token, v1 sign-in only, v2 `/me`, forced invisible on boot (§5.1) | Tasks 4, 5 and 6; acceptance in Task 10 (checks 4–6, 9) |
| warframe.market item list as game data (§14 A1) | Task 3 |
| Web login, `/rpc` allowlist, `/ws`, Origin check, cookie flags, rate limit (§7.1–7.3, §14 A5–A7) | Task 7 |
| React UI ported, §10 cuts, exports and uploads via RPC (§7.4, §14 A2–A3) | Tasks 8 and 9 |
| Dockerfile and Compose, LAN only, secrets (§2 defaults, §7.2) | Task 10 |
| Periodic `/me` check and 7-day expiry warning (§5.1) | Not in phase 1: phase 3 with the lifecycle. `token_expires_at` is already stored (Task 4) |
| Upstream `wts.max_price_drop` bug (§5.6) | Not in phase 1: phase 3 |

**Placeholder scan:** no TBD, TODO or "similar to". Tasks 5, 6 and 9 are mechanical ports of upstream code. Each gives exact rules, before/after code, file:line targets, and a finishing check that must come back clean: `cargo check`, a grep with no output, `pnpm build`, and `check-rpc-commands.py`.

**Type consistency:**
- `CacheState::new(PathBuf)` / `load(Vec<CacheTradableItem>)`: defined in Task 3, used in Task 6.
- `states::app_mutex()` / `cache_client()`: defined in Task 5, used in Task 6.
- `wfm_account::{save, load, delete}`: defined in Task 4, used in Tasks 5 and 6.
- `web_auth::{hash_password, verify_password, ensure_password}`: defined in Task 4, used in Tasks 6 and 7.
- `CoreConfig` / `CoreHandles.web_password_hash`: defined in Task 6, used in Task 7.
- `rpc::{COMMANDS, dispatch}`: defined in Task 6, used in Task 7 and by the Task 9 script, whose regex matches the `name => module::func` rows.
- Export commands return `Vec<Model>` (Task 6), which `downloadJson` consumes in Task 8.
- `sound_add_custom_sound { name, file_name, data_base64 }` (Task 6) receives `fileName` / `dataBase64` after `sendInvoke`'s camelCase conversion in Task 8.
