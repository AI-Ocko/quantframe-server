# Phase 4b: Trade Events — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task, **inline in the main session**. The user has ruled out subagent-driven development for this project: subagents may only explore the repo or write docs. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a trade completes in Warframe on the gaming PC, `qf-helper` reads it from EE.log and reports it, and the server resolves the items, records the transaction, updates stock and the real warframe.market order, or parks the trade for review in the browser.

**Architecture:**
- **`crates/qf_log_parser`** (new library): a chunk-based scanner over EE.log bytes that finds the trade-accept dialog, its result, and turns the dialog into raw items. Pure functions, no I/O.
- **`crates/qf-helper`**: tails EE.log from the end, feeds the scanner, queues each success to a JSONL file and POSTs it to `/helper/trade`.
- **Server (`qf_core::helper_link::trades`)**: `events` stores `helper_events`; `resolve` matches raw names to WFM items (English name, then `overrides.toml`) and classifies the trade; `sets` folds a full set of parts into the set item; `split` divides the platinum; `apply` drives the existing stock and wish-list handlers. `handle_incoming` runs the pipeline inside the request. Everything outside the database (settings, caches, WFM orders, handlers, notifications) sits behind the `TradeEnv` trait, so the pipeline is unit-tested with a fake and `live::LiveEnv` is the only code that touches global state.
- **Web server (`qf-server`)**: `POST /helper/trade` behind the existing device-key middleware.
- **Browser**: a Trades tab on the Live Scraper page. Review opens a modal from a row; nothing pops up by itself. Toasts announce applied and needs-review events.

**Tech Stack:** Rust (tokio, axum 0.8, sea-orm 0.12 raw SQL on SQLite, reqwest 0.12, sha2 0.10, toml 0.8, async-trait 0.1), React 19 with Mantine 9 and TanStack Query 5, pnpm 11.3.0, Docker Compose on the homelab, `systemd --user` on the gaming PC.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md`. Read §5.8, §6, §7.1, §8, §9, §17 and **§18 (E1–E13)** first. §18 takes precedence.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-4b`, branch `phase-4b-trade-events`. All paths are relative to it. Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target` to reuse the build cache.
- **Dry-run stays on** (`trader_state.dry_run = 1`). Nothing in this plan turns it off. Applied trades still close or adjust **real** WFM orders through the existing handlers (E8), the same as the manual "sold" button.
- **Markers (E2), copied exactly:**
  - start: `Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:`
  - end: `, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)`
  - success: `description=The trade was successful!`
  - discard: `description=The trade failed.`, `description=The trade was cancelled`, `OnTradeAccepted failed`
  - separator: `and will receive from ` … ` the following:`
- **`event_id`** = SHA-256 hex of `<ee_timestamp>\n<dialog text between the markers>` (E2). Private-use characters U+E000–U+F8FF are stripped from names; on arcanes their count is the rank.
- **Timing:** the helper polls EE.log every **1 s**; queue retries every **10 s**; a 401 waits **60 s**.
- **Auto-apply** needs `live_scraper.general.auto_trade = true` and every goods item resolved (E8). Otherwise the event is `needs_review`.
- **Platinum split:** own WFM order price, then `item_stats.median`, then equal; whole platinum; remainder on the first item (E8).
- **RPC names** must not contain the banned substrings in `allowlist_has_no_removed_features` (`riven, auction, chat, analytics, alert, syndicate, wfgdpr, wf_inventory, live_scraper, permission, exit, calculate_tax`). The new names are `helper_trades`, `helper_trade_apply`, `helper_trade_ignore`.
- **Fixtures** come from real bundles under `~/.local/share/dev.kenya.quantframe/logs/`. Player names are replaced with `PlayerA`…`PlayerF` before anything is committed. Never commit a bundle itself.
- **Tests:** `cargo test -p qf_log_parser`, `cargo test -p qf-helper`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`.
- **Docker** runs on ockohome only (`ssh christopher@ockohome`, `~/stacks/quantframe-server`). The image still builds only `-p qf-server`. `qf-helper` is built and installed natively on this desktop.
- **Commits:** conventional commits, ending with exactly this trailer:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  ```
- **Pushes:** push `phase-4b-trade-events` after each task; never push `main` from this plan.

## File Structure (end of phase 4b)

```
Cargo.toml                                                  MOD  workspace member crates/qf_log_parser
crates/qf_log_parser/Cargo.toml                             NEW
crates/qf_log_parser/src/lib.rs                             NEW  RawItem, RawTrade, TradeEvent, parse_line, parse_dialog, event_id, scan_all
crates/qf_log_parser/src/scanner.rs                         NEW  Scanner (chunk-safe state machine)
crates/qf_log_parser/tests/fixtures/*.log                   NEW  raw EE.log excerpts, names replaced
crates/qf_log_parser/tests/fixtures.rs                      NEW  fixture and chunk-size tests
crates/qf-helper/Cargo.toml                                 MOD  qf_log_parser, chrono
crates/qf-helper/src/{lib.rs,main.rs}                       MOD  --parse, trade loop
crates/qf-helper/src/ee_log.rs                              NEW  Tail (poll, truncation, start at end)
crates/qf-helper/src/queue.rs                               NEW  JSONL queue, default_queue_path
crates/qf-helper/src/trade.rs                               NEW  TradeClient, TradeOutcome, describe_trade
crates/migration/src/m20260918_000001_create_helper_events.rs NEW
crates/migration/src/lib.rs                                 MOD  register
crates/qf_core/Cargo.toml                                   MOD  async-trait, toml
crates/qf_core/src/db.rs                                    MOD  table test
crates/qf_core/src/helper_link/mod.rs                       MOD  pub mod trades
crates/qf_core/src/helper_link/trades/mod.rs                NEW  IncomingTrade, Resolution, TradeEnv, Outcome, validate, handle_incoming, apply_reviewed, ignore
crates/qf_core/src/helper_link/trades/events.rs             NEW  helper_events store
crates/qf_core/src/helper_link/trades/resolve.rs            NEW  normalise, Overrides, ItemIndex, classify, resolve_item, resolve_trade
crates/qf_core/src/helper_link/trades/sets.rs               NEW  set_candidates, fold_sets, PartsMap, SetSource, SetCache
crates/qf_core/src/helper_link/trades/split.rs              NEW  split_platinum, weights_for, price_items, medians
crates/qf_core/src/helper_link/trades/apply.rs              NEW  ItemApplier, HandlerApplier, apply_items, handler flags
crates/qf_core/src/helper_link/trades/live.rs               NEW  LiveEnv, trade_variables, toast_values
crates/qf_core/src/collector/maintenance.rs                 MOD  helper_events retention in hourly()
crates/qf_core/src/commands/helper_link.rs                  MOD  helper_trades / helper_trade_apply / helper_trade_ignore
crates/qf_core/src/commands/rpc.rs                          MOD  allowlist + tests
crates/qf-server/Cargo.toml                                 MOD  dev-deps async-trait, utils, wf-market
crates/qf-server/src/routes.rs                              MOD  ServerState.trade_env, POST /helper/trade
crates/qf-server/src/main.rs                                MOD  trade_env: LiveEnv
crates/qf-server/tests/http.rs                              MOD  FakeTrades, trade route tests
web/src/types/tauri.type.ts                                 MOD  helper event types
web/src/api/helper_link/index.ts                            MOD  trades(), applyTrade(), ignoreTrade()
web/src/pages/live_scraper/Tabs/Trades/index.tsx            NEW  TradesPanel
web/src/pages/live_scraper/Tabs/Trades/ReviewTradeModal.tsx NEW
web/src/pages/live_scraper/Tabs/index.ts                    MOD
web/src/pages/live_scraper/index.tsx                        MOD  Trades tab
web/src/components/Forms/Settings/Tabs/Advanced/Tabs/Log/index.tsx MOD  ee_log_path field removed
web/public/lang/en.json                                     MOD  strings
README.md                                                   MOD  trade reporting, overrides.toml
docs/PHASE-4B-ACCEPTANCE.md                                 NEW
```

---

### Task 1: `qf_log_parser` crate

**Files:**
- Modify: `Cargo.toml` (workspace members)
- Create: `crates/qf_log_parser/Cargo.toml`, `crates/qf_log_parser/src/lib.rs`, `crates/qf_log_parser/src/scanner.rs`
- Create: `crates/qf_log_parser/tests/fixtures/` (nine files, see Step 4) and `crates/qf_log_parser/tests/fixtures.rs`

**Interfaces:**
- Consumes: nothing from the workspace.
- Produces (all `pub` in `qf_log_parser`):
  - `RawItem { name: String, quantity: i64, rank: Option<i64> }` (Serialize, Deserialize, Clone, Debug, PartialEq)
  - `RawTrade { player_name: String, ee_timestamp: String, offered: Vec<RawItem>, received: Vec<RawItem> }` (same derives)
  - `TradeEvent { event_id: String, trade: RawTrade }` (same derives)
  - `parse_line(piece: &str) -> RawItem`
  - `parse_dialog(ee_timestamp: &str, dialog: &str) -> RawTrade`
  - `event_id(ee_timestamp: &str, dialog: &str) -> String`
  - `scan_all(bytes: &[u8]) -> Vec<TradeEvent>`
  - `scanner::Scanner::new()`, `Scanner::feed(&mut self, chunk: &[u8]) -> Vec<TradeEvent>`, `Scanner::reset(&mut self)`
  - constants `START`, `END`, `SUCCESS`, `FAILED`, `CANCELLED`, `ACCEPT_FAILED`

- [ ] **Step 1: Crate skeleton**

In the root `Cargo.toml`, replace:

```toml
members = ["crates/entity", "crates/migration", "crates/service", "crates/utils", "crates/qf_core", "crates/wf-market", "crates/qf-server", "crates/qf-helper"]
```

with:

```toml
members = ["crates/entity", "crates/migration", "crates/service", "crates/utils", "crates/qf_core", "crates/wf-market", "crates/qf-server", "crates/qf-helper", "crates/qf_log_parser"]
```

`crates/qf_log_parser/Cargo.toml`:

```toml
[package]
name = "qf_log_parser"
version = "0.1.0"
edition = "2021"
license = "GPL-3.0-only"

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sha2 = "0.10"
```

- [ ] **Step 2: Line and dialog parsing with unit tests**

`crates/qf_log_parser/src/lib.rs`:

```rust
//! Parses Warframe trade dialogs out of EE.log (spec §5.8, amendment E2).
//!
//! Warframe writes the accept dialog in arbitrary chunks, so nothing here assumes line boundaries:
//! the [`scanner::Scanner`] accumulates bytes and acts only on complete markers.

pub mod scanner;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use scanner::Scanner;

pub const START: &str = "Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:";
pub const END: &str = ", title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)";
pub const SUCCESS: &str = "description=The trade was successful!";
pub const FAILED: &str = "description=The trade failed.";
pub const CANCELLED: &str = "description=The trade was cancelled";
pub const ACCEPT_FAILED: &str = "OnTradeAccepted failed";
const RECEIVE_FROM: &str = "and will receive from ";
const THE_FOLLOWING: &str = " the following:";
const PLATINUM_PREFIX: &str = "Platinum x ";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawItem {
    pub name: String,
    pub quantity: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawTrade {
    pub player_name: String,
    /// The EE.log timestamp of the dialog's start line, e.g. `422.424`.
    pub ee_timestamp: String,
    pub offered: Vec<RawItem>,
    pub received: Vec<RawItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeEvent {
    pub event_id: String,
    pub trade: RawTrade,
}

/// Warframe appends rank icons and name badges from the private-use area.
fn is_private_use(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// Removes trailing private-use characters and returns how many there were.
fn strip_glyphs(text: &str) -> (&str, usize) {
    let trimmed = text.trim_end_matches(is_private_use);
    (trimmed, text[trimmed.len()..].chars().count())
}

/// One dialog piece: `Platinum x 30`, `Adaptation (RARE RANK 10)`, `Arcane Energize <5 glyphs>`, `Wolf Sledge Handle`.
pub fn parse_line(piece: &str) -> RawItem {
    let piece = piece.trim();
    if let Some(amount) = piece.strip_prefix(PLATINUM_PREFIX) {
        return RawItem { name: "Platinum".into(), quantity: amount.trim().parse().unwrap_or(1), rank: None };
    }
    let (without_glyphs, glyphs) = strip_glyphs(piece);
    let without_glyphs = without_glyphs.trim();
    if glyphs > 0 {
        return RawItem { name: without_glyphs.to_string(), quantity: 1, rank: Some(glyphs as i64) };
    }
    if let Some(open) = without_glyphs.rfind(" (") {
        if let Some(inner) = without_glyphs[open + 2..].strip_suffix(')') {
            if let Some((_, rank)) = inner.rsplit_once(" RANK ") {
                if let Ok(rank) = rank.trim().parse::<i64>() {
                    return RawItem { name: without_glyphs[..open].trim().to_string(), quantity: 1, rank: Some(rank) };
                }
            }
        }
    }
    RawItem { name: without_glyphs.to_string(), quantity: 1, rank: None }
}

fn parse_pieces(text: &str) -> Vec<RawItem> {
    let mut items: Vec<RawItem> = Vec::new();
    for piece in text.split(['\r', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
        let item = parse_line(piece);
        match items.iter_mut().find(|i| i.name == item.name && i.rank == item.rank) {
            Some(existing) => existing.quantity += item.quantity,
            None => items.push(item),
        }
    }
    items
}

/// Splits the text between [`START`] and [`END`] into the offered and received items.
pub fn parse_dialog(ee_timestamp: &str, dialog: &str) -> RawTrade {
    let (offered_text, rest) = dialog.split_once(RECEIVE_FROM).unwrap_or((dialog, ""));
    let (player, received_text) = rest.split_once(THE_FOLLOWING).unwrap_or((rest, ""));
    let (player, _) = strip_glyphs(player.trim());
    RawTrade {
        player_name: player.trim().to_string(),
        ee_timestamp: ee_timestamp.to_string(),
        offered: parse_pieces(offered_text),
        received: parse_pieces(received_text),
    }
}

/// SHA-256 hex of `<ee_timestamp>\n<dialog>` (amendment E2).
pub fn event_id(ee_timestamp: &str, dialog: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ee_timestamp.as_bytes());
    hasher.update(b"\n");
    hasher.update(dialog.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Scans a whole file. Same result as feeding it to a [`Scanner`] in any chunking.
pub fn scan_all(bytes: &[u8]) -> Vec<TradeEvent> {
    Scanner::new().feed(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platinum_lines_carry_the_amount() {
        assert_eq!(parse_line("Platinum x 30"), RawItem { name: "Platinum".into(), quantity: 30, rank: None });
        assert_eq!(parse_line("Platinum x 70\r").quantity, 70);
    }

    #[test]
    fn mod_ranks_come_from_the_suffix_whatever_the_word() {
        assert_eq!(parse_line("Adaptation (RARE RANK 10)"), RawItem { name: "Adaptation".into(), quantity: 1, rank: Some(10) });
        assert_eq!(parse_line("Galvanized Shot (GALVANIZED RANK 10)").name, "Galvanized Shot");
        assert_eq!(parse_line("Archon Vitality (KAHL RANK 10)").rank, Some(10));
        assert_eq!(parse_line("Pistol Gambit (COMMON RANK 0)").rank, Some(0));
    }

    #[test]
    fn arcane_glyph_count_is_the_rank() {
        let five = "Arcane Nullifier \u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}";
        assert_eq!(parse_line(five), RawItem { name: "Arcane Nullifier".into(), quantity: 1, rank: Some(5) });
        let three = "Exodia Contagion \u{e0dc}\u{e0dc}\u{e0dc}";
        assert_eq!(parse_line(three).rank, Some(3));
    }

    #[test]
    fn plain_names_and_names_with_parentheses_but_no_rank_are_kept() {
        assert_eq!(parse_line("Wolf Sledge Handle"), RawItem { name: "Wolf Sledge Handle".into(), quantity: 1, rank: None });
        assert_eq!(parse_line("Mortus Lungfish (L)"), RawItem { name: "Mortus Lungfish (L)".into(), quantity: 1, rank: None });
    }

    #[test]
    fn dialog_splits_sides_folds_duplicates_and_cleans_the_player() {
        let dialog = "\r\nPlatinum x 300\r\n\r\nand will receive from CloudStepKing\u{e000} the following:\
                      \r\nGalvanized Shot (GALVANIZED RANK 10)\r\nGalvanized Shot (GALVANIZED RANK 10)\r\nParry (COMMON RANK 0)";
        let trade = parse_dialog("5380.793", dialog);
        assert_eq!(trade.player_name, "CloudStepKing");
        assert_eq!(trade.ee_timestamp, "5380.793");
        assert_eq!(trade.offered, vec![RawItem { name: "Platinum".into(), quantity: 300, rank: None }]);
        assert_eq!(
            trade.received,
            vec![
                RawItem { name: "Galvanized Shot".into(), quantity: 2, rank: Some(10) },
                RawItem { name: "Parry".into(), quantity: 1, rank: Some(0) },
            ]
        );
    }

    #[test]
    fn event_ids_are_stable_hex_and_depend_on_the_timestamp() {
        let a = event_id("1.000", "x");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, event_id("1.000", "x"));
        assert_ne!(a, event_id("2.000", "x"));
    }
}
```

- [ ] **Step 3: The scanner**

`crates/qf_log_parser/src/scanner.rs`:

```rust
use crate::{event_id, parse_dialog, RawTrade, TradeEvent, ACCEPT_FAILED, CANCELLED, END, FAILED, START, SUCCESS};

/// Longest tail worth keeping while waiting for a marker: a dialog line plus slack.
const KEEP_TAIL: usize = 64 * 1024;

struct Pending {
    ee_timestamp: String,
    dialog: String,
}

/// A chunk-safe state machine: feed it EE.log bytes in any sizes and it emits successful trades.
#[derive(Default)]
pub struct Scanner {
    buf: Vec<u8>,
    pending: Option<Pending>,
}

fn find(haystack: &[u8], needle: &str, from: usize) -> Option<usize> {
    let needle = needle.as_bytes();
    if from > haystack.len() || needle.is_empty() {
        return None;
    }
    haystack[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Start of the line that contains `at`.
fn line_start(buf: &[u8], at: usize) -> usize {
    buf[..at].iter().rposition(|b| *b == b'\n').map_or(0, |p| p + 1)
}

/// `422.424 Script [Info]: ...` -> `422.424`
fn timestamp_of(line: &[u8]) -> String {
    let text = String::from_utf8_lossy(line);
    text.split_whitespace().next().unwrap_or("").to_string()
}

impl Scanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets everything, e.g. after EE.log was truncated.
    pub fn reset(&mut self) {
        self.buf.clear();
        self.pending = None;
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<TradeEvent> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        loop {
            if self.pending.is_none() {
                let Some(start) = find(&self.buf, START, 0) else {
                    self.keep_tail_from_line_start();
                    break;
                };
                let dialog_from = start + START.len();
                let Some(end) = find(&self.buf, END, dialog_from) else {
                    // The dialog is still being written: keep from its line start and wait.
                    let keep = line_start(&self.buf, start);
                    self.buf.drain(..keep);
                    break;
                };
                let ee_timestamp = timestamp_of(&self.buf[line_start(&self.buf, start)..start]);
                let dialog = String::from_utf8_lossy(&self.buf[dialog_from..end]).into_owned();
                self.pending = Some(Pending { ee_timestamp, dialog });
                self.buf.drain(..end + END.len());
                continue;
            }

            // Waiting for the result of the pending dialog.
            let candidates = [
                (find(&self.buf, SUCCESS, 0), "success"),
                (find(&self.buf, FAILED, 0), "discard"),
                (find(&self.buf, CANCELLED, 0), "discard"),
                (find(&self.buf, ACCEPT_FAILED, 0), "discard"),
                (find(&self.buf, START, 0), "restart"),
            ];
            let Some((pos, kind, len)) = candidates
                .iter()
                .zip([SUCCESS.len(), FAILED.len(), CANCELLED.len(), ACCEPT_FAILED.len(), START.len()])
                .filter_map(|((pos, kind), len)| pos.map(|p| (p, *kind, len)))
                .min_by_key(|(p, _, _)| *p)
            else {
                self.keep_tail_from_line_start();
                break;
            };
            let pending = self.pending.take().expect("pending checked above");
            match kind {
                "success" => {
                    let trade: RawTrade = parse_dialog(&pending.ee_timestamp, &pending.dialog);
                    events.push(TradeEvent { event_id: event_id(&pending.ee_timestamp, &pending.dialog), trade });
                    self.buf.drain(..pos + len);
                }
                "discard" => {
                    self.buf.drain(..pos + len);
                }
                _ => {
                    // A new dialog before any result: the loop picks it up with `pending == None`.
                }
            }
        }
        events
    }

    /// Keeps the current (possibly partial) last line so a marker split across chunks is still found.
    fn keep_tail_from_line_start(&mut self) {
        let keep = line_start(&self.buf, self.buf.len());
        let keep = keep.min(self.buf.len());
        let keep = if self.buf.len() - keep > KEEP_TAIL { self.buf.len() - KEEP_TAIL } else { keep };
        self.buf.drain(..keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PURCHASE: &str = "422.424 Sys [Info]: Created /Lotus/Interface/Dialog.swf\n\
422.424 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:\n\
\rPlatinum x 30\r\n\
\r\n\
and will receive from PlayerA\u{e000} the following:\n\
\rWolf Sledge Blueprint\n\
\rWolf Sledge Motor\n\
\rWolf Sledge Head\n\
\rWolf Sledge Handle, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)\n\
424.897 Net [Info]: Updating session (params changed)\n\
426.113 Script [Info]: Dialog.lua: Dialog::SendResult(4)\n\
427.411 Sys [Info]: Created /Lotus/Interface/Dialog.swf\n\
427.411 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n";

    #[test]
    fn a_successful_purchase_is_one_event() {
        let events = Scanner::new().feed(PURCHASE.as_bytes());
        assert_eq!(events.len(), 1);
        let trade = &events[0].trade;
        assert_eq!(trade.ee_timestamp, "422.424");
        assert_eq!(trade.player_name, "PlayerA");
        assert_eq!(trade.offered[0].quantity, 30);
        assert_eq!(trade.received.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Wolf Sledge Blueprint", "Wolf Sledge Motor", "Wolf Sledge Head", "Wolf Sledge Handle"]);
        assert_eq!(events[0].event_id.len(), 64);
    }

    #[test]
    fn byte_by_byte_feeding_gives_the_same_event() {
        let whole = Scanner::new().feed(PURCHASE.as_bytes());
        let mut scanner = Scanner::new();
        let mut events = Vec::new();
        for byte in PURCHASE.as_bytes() {
            events.extend(scanner.feed(std::slice::from_ref(byte)));
        }
        assert_eq!(events, whole);
    }

    #[test]
    fn failed_cancelled_and_accept_failed_results_are_dropped() {
        for result in [
            "1.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade failed., title= leftItem=/Menu/Confirm_Item_Ok)\n",
            "1.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was cancelled, title= leftItem=/Menu/Confirm_Item_Ok)\n",
            "849.134 Sys [Info]: OnTradeAccepted failed: -13\n",
        ] {
            let text = PURCHASE.replace(
                "427.411 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n",
                result,
            );
            let mut scanner = Scanner::new();
            assert!(scanner.feed(text.as_bytes()).is_empty(), "{result}");
            // A later, unrelated success must not resurrect the dropped dialog.
            assert!(scanner.feed(b"2.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=)\n").is_empty());
        }
    }

    #[test]
    fn a_new_dialog_before_a_result_replaces_the_pending_one() {
        let first_half = PURCHASE.split("424.897").next().unwrap();
        let text = format!("{first_half}{PURCHASE}");
        let events = Scanner::new().feed(text.as_bytes());
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn a_success_without_a_dialog_is_ignored_and_reset_clears_state() {
        let mut scanner = Scanner::new();
        assert!(scanner.feed(b"1.0 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=)\n").is_empty());
        let first_half = PURCHASE.split("424.897").next().unwrap();
        scanner.feed(first_half.as_bytes());
        scanner.reset();
        assert!(scanner.feed(b"9.0 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=)\n").is_empty());
    }
}
```

- [ ] **Step 4: Fixture files from the real bundles**

Create the nine fixtures with this script (run from the worktree root). It writes the raw lines with the same `\r` bytes Warframe wrote, player names replaced, and the result line appended, because the bundles cut the raw excerpt before it. The upstream watcher saw `Galvanized Scope (GALVAN` and `IZED RANK 10)` as two lines because it read a partial write; the file itself has no newline there, so the fixtures don't either. The chunk-size test is what exercises those splits.

```bash
mkdir -p crates/qf_log_parser/tests/fixtures && python3 - <<'PY'
import pathlib
d = pathlib.Path("crates/qf_log_parser/tests/fixtures")
OK = "{t} Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n"
START = "{t} Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:\n"
END = ", title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)\n"
SEND = "{t} Script [Info]: Dialog.lua: SendResult_MENU_SELECT()\n{t} Script [Info]: Dialog.lua: Dialog::SendResult(4)\n"
def w(name, t, body, result=None, t2=None):
    text = f"{t} Sys [Info]: Created /Lotus/Interface/Dialog.swf\n" + START.format(t=t) + body + SEND.format(t=t) + (result if result is not None else OK.format(t=t2 or t))
    (d / name).write_bytes(text.encode())
# 1 purchase of four set parts (2026-09-07 20:37)
w("purchase_set_parts.log", "422.424", "\rPlatinum x 30\r\n\r\nand will receive from PlayerA\ue000 the following:\n\rWolf Sledge Blueprint\n\rWolf Sledge Motor\n\rWolf Sledge Head\n\rWolf Sledge Handle" + END, t2="427.411")
# 2 sale of an arcane (2026-09-07 20:49)
w("sale_arcane.log", "1170.388", "\rArcane Nullifier \ue0b9\ue0b9\ue0b9\ue0b9\ue0b9\r\n\r\nand will receive from PlayerB\ue000 the following:\n\rPlatinum x 70" + END, t2="1174.495")
# 3 mod name that Warframe wrote in two chunks (2026-09-10 22:28)
w("wrapped_mod_name.log", "8145.240", "\rPlatinum x 50\r\n\r\nand will receive from PlayerC\ue000 the following:\n\rGalvanized Scope (GALVANIZED RANK 10)" + END, t2="8149.500")
# 4 end marker that Warframe wrote in two chunks (2026-09-10 20:04)
w("split_end_marker.log", "2002.840", "\rArcane Ice Storm \ue09a\ue09a\ue09a\ue09a\ue09a\r\n\r\nand will receive from PlayerD\ue002 the following:\n\rPlatinum x 70" + END, t2="2007.000")
# 5 quantity via repeated lines (2026-09-14 05:13)
w("quantity_repeated_lines.log", "5380.793", "\rPlatinum x 300\r\n\r\nand will receive from PlayerE\ue000 the following:\n\rGalvanized Shot (GALVANIZED RANK 10)\n\rGalvanized Shot (GALVANIZED RANK 10)\n\rGalvanized Shot (GALVANIZED RANK 10)\n\rGalvanized Shot (GALVANIZED RANK 10)\n\rGalvanized Shot (GALVANIZED RANK 10)\n\rGalvanized Shot (GALVANIZED RANK 10)" + END, t2="5390.000")
# 6 extras beside platinum (2026-09-14 04:33)
w("extras_with_platinum.log", "3020.824", "\rSevagoth Prime Blueprint\n\rSevagoth Prime Systems Blueprint\n\rSevagoth Prime Neuroptics Blueprint\n\rSevagoth Prime Chassis Blueprint\r\n\r\nand will receive from PlayerF\ue004 the following:\n\rPlatinum x 43\n\rParry (COMMON RANK 0)\n\rPistol Gambit (COMMON RANK 0)\n\rPistol Gambit (COMMON RANK 0)\n\rPistol Gambit (COMMON RANK 0)" + END, t2="3026.000")
# 7 failed result
w("result_failed.log", "100.000", "\rPlatinum x 40\r\n\r\nand will receive from PlayerA\ue000 the following:\n\rAdaptation (RARE RANK 10)" + END, result="103.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade failed., title= leftItem=/Menu/Confirm_Item_Ok)\n")
# 8 cancelled result
w("result_cancelled.log", "200.000", "\rPlatinum x 40\r\n\r\nand will receive from PlayerA\ue000 the following:\n\rAdaptation (RARE RANK 10)" + END, result="203.000 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was cancelled, title= leftItem=/Menu/Confirm_Item_Ok)\n")
# 9 OnTradeAccepted failed (2026-09-13 19:12)
w("result_accept_failed.log", "845.518", "\rGalvanized Diffusion (GALVANIZED RANK 10)\r\n\r\nand will receive from PlayerB\ue000 the following:\n\rPlatinum x 65" + END, result="849.129 Sys [Warning]: HTTP/1.1 409 Conflict\n849.134 Sys [Info]: OnTradeAccepted failed: -13\n")
PY
ls crates/qf_log_parser/tests/fixtures
```

Expected: nine `.log` files. `grep -c 'Player' crates/qf_log_parser/tests/fixtures/*.log` shows only `PlayerA`–`PlayerF`.

- [ ] **Step 5: Fixture tests, including chunk-size invariance**

`crates/qf_log_parser/tests/fixtures.rs`:

```rust
use qf_log_parser::{scan_all, RawItem, Scanner, TradeEvent};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

fn item(name: &str, quantity: i64, rank: Option<i64>) -> RawItem {
    RawItem { name: name.into(), quantity, rank }
}

fn only(events: Vec<TradeEvent>) -> TradeEvent {
    assert_eq!(events.len(), 1, "expected exactly one trade");
    events.into_iter().next().unwrap()
}

#[test]
fn purchase_of_set_parts() {
    let event = only(scan_all(&fixture("purchase_set_parts.log")));
    assert_eq!(event.trade.player_name, "PlayerA");
    assert_eq!(event.trade.offered, vec![item("Platinum", 30, None)]);
    assert_eq!(
        event.trade.received,
        vec![
            item("Wolf Sledge Blueprint", 1, None),
            item("Wolf Sledge Motor", 1, None),
            item("Wolf Sledge Head", 1, None),
            item("Wolf Sledge Handle", 1, None),
        ]
    );
}

#[test]
fn sale_of_an_arcane_at_rank_five() {
    let event = only(scan_all(&fixture("sale_arcane.log")));
    assert_eq!(event.trade.offered, vec![item("Arcane Nullifier", 1, Some(5))]);
    assert_eq!(event.trade.received, vec![item("Platinum", 70, None)]);
    assert_eq!(event.trade.ee_timestamp, "1170.388");
}

#[test]
fn a_mod_name_written_in_two_chunks_is_one_item() {
    let event = only(scan_all(&fixture("wrapped_mod_name.log")));
    assert_eq!(event.trade.received, vec![item("Galvanized Scope", 1, Some(10))]);
}

#[test]
fn an_end_marker_written_in_two_chunks_still_ends_the_dialog() {
    let event = only(scan_all(&fixture("split_end_marker.log")));
    assert_eq!(event.trade.offered, vec![item("Arcane Ice Storm", 1, Some(5))]);
    assert_eq!(event.trade.received, vec![item("Platinum", 70, None)]);
}

#[test]
fn repeated_lines_fold_into_quantity() {
    let event = only(scan_all(&fixture("quantity_repeated_lines.log")));
    assert_eq!(event.trade.received, vec![item("Galvanized Shot", 6, Some(10))]);
}

#[test]
fn extras_beside_platinum_are_kept_as_raw_items() {
    let event = only(scan_all(&fixture("extras_with_platinum.log")));
    assert_eq!(event.trade.offered.len(), 4);
    assert_eq!(
        event.trade.received,
        vec![item("Platinum", 43, None), item("Parry", 1, Some(0)), item("Pistol Gambit", 3, Some(0))]
    );
}

#[test]
fn failed_cancelled_and_accept_failed_produce_nothing() {
    for name in ["result_failed.log", "result_cancelled.log", "result_accept_failed.log"] {
        assert!(scan_all(&fixture(name)).is_empty(), "{name}");
    }
}

#[test]
fn every_fixture_is_chunk_size_invariant() {
    for name in [
        "purchase_set_parts.log",
        "sale_arcane.log",
        "wrapped_mod_name.log",
        "split_end_marker.log",
        "quantity_repeated_lines.log",
        "extras_with_platinum.log",
        "result_failed.log",
        "result_cancelled.log",
        "result_accept_failed.log",
    ] {
        let bytes = fixture(name);
        let whole = scan_all(&bytes);
        for size in [1usize, 7, 64] {
            let mut scanner = Scanner::new();
            let mut events = Vec::new();
            for chunk in bytes.chunks(size) {
                events.extend(scanner.feed(chunk));
            }
            assert_eq!(events, whole, "{name} at chunk size {size}");
        }
    }
}

#[test]
fn event_ids_differ_between_fixtures_and_repeat_for_the_same_bytes() {
    let a = only(scan_all(&fixture("purchase_set_parts.log"))).event_id;
    let b = only(scan_all(&fixture("sale_arcane.log"))).event_id;
    assert_ne!(a, b);
    assert_eq!(a, only(scan_all(&fixture("purchase_set_parts.log"))).event_id);
}
```

- [ ] **Step 6: Run the tests**

```bash
export CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target
cargo test -p qf_log_parser
```

Expected: all unit tests and the nine fixture tests pass. If `a_new_dialog_before_a_result_replaces_the_pending_one` fails with two events, the `"restart"` arm drained the buffer; it must not.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/qf_log_parser
git commit -m "feat(log-parser): add qf_log_parser with a chunk-safe trade dialog scanner

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push -u origin phase-4b-trade-events
```

---

### Task 2: `qf-helper` tails EE.log and can parse a file

**Files:**
- Modify: `crates/qf-helper/Cargo.toml`, `crates/qf-helper/src/lib.rs`, `crates/qf-helper/src/main.rs`
- Create: `crates/qf-helper/src/ee_log.rs`

**Interfaces:**
- Consumes: `qf_log_parser::{Scanner, TradeEvent, scan_all}` (Task 1); `Config.ee_log_path` (phase 4a).
- Produces:
  - `ee_log::POLL_EVERY: Duration` (1 s)
  - `ee_log::Tail::start_at_end(path: &Path) -> Tail`, `Tail::offset(&self) -> u64`, `Tail::poll(&mut self) -> Vec<TradeEvent>`
  - `qf-helper --parse <file>` prints a JSON array of `TradeEvent` and exits 0.

- [ ] **Step 1: Dependencies and module list**

In `crates/qf-helper/Cargo.toml` `[dependencies]`, add after `toml = "0.8"`:

```toml
chrono = "0.4"
qf_log_parser = { path = "../qf_log_parser" }
```

Replace `crates/qf-helper/src/lib.rs` with:

```rust
//! qf-helper: tells quantframe-server whether Warframe is running (spec §5.8, amendment D7)
//! and reports completed trades from EE.log (amendments E2–E3).

pub mod config;
pub mod ee_log;
pub mod heartbeat;
pub mod process;
```

- [ ] **Step 2: Write the failing tail tests and the tail**

`crates/qf-helper/src/ee_log.rs`:

```rust
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use qf_log_parser::{Scanner, TradeEvent};

pub const POLL_EVERY: Duration = Duration::from_secs(1);

/// Follows EE.log from its current end (amendment E3). Earlier trades are never replayed.
pub struct Tail {
    path: PathBuf,
    offset: u64,
    scanner: Scanner,
    warned_missing: bool,
}

impl Tail {
    pub fn start_at_end(path: &Path) -> Self {
        let offset = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Self { path: path.to_path_buf(), offset, scanner: Scanner::new(), warned_missing: false }
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Reads whatever was appended since the last poll and returns the trades completed in it.
    /// A file shorter than the last offset means a new game session: reading restarts from 0.
    pub fn poll(&mut self) -> Vec<TradeEvent> {
        let len = match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(_) => {
                if !self.warned_missing {
                    eprintln!("EE.log not found at {}; waiting for Warframe to create it", self.path.display());
                    self.warned_missing = true;
                }
                return Vec::new();
            }
        };
        self.warned_missing = false;
        if len < self.offset {
            println!("EE.log was truncated (new game session); reading it from the start");
            self.offset = 0;
            self.scanner.reset();
        }
        if len == self.offset {
            return Vec::new();
        }
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("cannot open {}: {e}", self.path.display());
                return Vec::new();
            }
        };
        if let Err(e) = file.seek(SeekFrom::Start(self.offset)) {
            eprintln!("cannot seek {}: {e}", self.path.display());
            return Vec::new();
        }
        let mut bytes = Vec::with_capacity((len - self.offset) as usize);
        if let Err(e) = file.take(len - self.offset).read_to_end(&mut bytes) {
            eprintln!("cannot read {}: {e}", self.path.display());
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.scanner.feed(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const DIALOG: &str = "422.424 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:\n\
\rPlatinum x 30\r\n\r\nand will receive from PlayerA\u{e000} the following:\n\
\rWolf Sledge Handle, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)\n";
    const OK: &str = "427.411 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)\n";

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn starts_at_the_end_and_reports_trades_completed_across_polls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        append(&path, "0.001 Sys [Info]: old session\n");
        append(&path, DIALOG);
        append(&path, OK);
        let mut tail = Tail::start_at_end(&path);
        assert_eq!(tail.offset(), std::fs::metadata(&path).unwrap().len());
        assert!(tail.poll().is_empty(), "existing trades are never replayed");

        append(&path, DIALOG);
        assert!(tail.poll().is_empty(), "no result yet");
        append(&path, OK);
        let events = tail.poll();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].trade.received[0].name, "Wolf Sledge Handle");
        assert!(tail.poll().is_empty());
    }

    #[test]
    fn a_shorter_file_is_a_new_session_and_is_read_from_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        append(&path, &"x".repeat(5000));
        let mut tail = Tail::start_at_end(&path);
        std::fs::write(&path, format!("{DIALOG}{OK}")).unwrap();
        assert_eq!(tail.poll().len(), 1);
        assert_eq!(tail.offset(), (DIALOG.len() + OK.len()) as u64);
    }

    #[test]
    fn a_missing_file_yields_nothing_until_it_appears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("EE.log");
        let mut tail = Tail::start_at_end(&path);
        assert!(tail.poll().is_empty());
        std::fs::write(&path, format!("{DIALOG}{OK}")).unwrap();
        assert_eq!(tail.poll().len(), 1, "a file that appears is read from offset 0");
    }
}
```

- [ ] **Step 3: `--parse` in `main.rs`**

In `crates/qf-helper/src/main.rs`, replace:

```rust
const USAGE: &str = "Usage: qf-helper [--config <path>] [--once]";
```

with:

```rust
const USAGE: &str = "Usage: qf-helper [--config <path>] [--once] | qf-helper --parse <EE.log>";
```

Replace:

```rust
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
```

with:

```rust
    let mut config_path: Option<PathBuf> = None;
    let mut once = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => match args.next() {
                Some(path) => config_path = Some(PathBuf::from(path)),
                None => exit_with(2, &format!("--config needs a path\n{USAGE}")),
            },
            "--parse" => match args.next() {
                Some(path) => {
                    let bytes = std::fs::read(&path).unwrap_or_else(|e| exit_with(2, &format!("Cannot read {path}: {e}")));
                    let events = qf_log_parser::scan_all(&bytes);
                    println!("{}", serde_json::to_string_pretty(&events).expect("events serialize"));
                    return;
                }
                None => exit_with(2, &format!("--parse needs a file\n{USAGE}")),
            },
            "--once" => once = true,
```

- [ ] **Step 4: Run the tests and try `--parse` on a fixture**

```bash
cargo test -p qf-helper
cargo run -q -p qf-helper -- --parse crates/qf_log_parser/tests/fixtures/sale_arcane.log
```

Expected: the tail tests pass; `--parse` prints one event with `"player_name": "PlayerB"` and `"rank": 5`.

- [ ] **Step 5: Commit**

```bash
git add crates/qf-helper
git commit -m "feat(helper): tail EE.log from the end and add --parse

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: `qf-helper` queue, trade client and trade loop

**Files:**
- Create: `crates/qf-helper/src/queue.rs`, `crates/qf-helper/src/trade.rs`
- Modify: `crates/qf-helper/src/lib.rs`, `crates/qf-helper/src/main.rs`, `README.md`

**Interfaces:**
- Consumes: `ee_log::{Tail, POLL_EVERY}` (Task 2), `heartbeat::REJECTED_BACKOFF` (phase 4a), `qf_log_parser::RawTrade`.
- Produces:
  - `queue::QueuedEvent { event_id: String, detected_at: String, trade: RawTrade }` (Serialize, Deserialize, Clone, Debug, PartialEq) — this is exactly the `POST /helper/trade` body (E4).
  - `queue::Queue::new(path: PathBuf)`, `push(&self, &QueuedEvent) -> Result<(), String>`, `peek(&self) -> Result<Option<QueuedEvent>, String>`, `pop(&self) -> Result<(), String>`, `len(&self) -> usize`
  - `queue::default_queue_path(xdg_state_home: Option<&str>, home: &Path) -> PathBuf`
  - `trade::RETRY_EVERY: Duration` (10 s)
  - `trade::TradeOutcome::{Accepted(String), Rejected, Dropped(String), Failed(String)}`, `removes_from_queue(&self) -> bool`, `next_delay(&self) -> Option<Duration>`
  - `trade::TradeClient::new(server_url, device_key)`, `send(&self, &QueuedEvent) -> TradeOutcome`
  - `trade::describe(trade: &RawTrade, server: &str) -> String`
  - `trade::run(config: Config, queue: Queue) -> !` (async loop)

- [ ] **Step 1: Queue with tests**

`crates/qf-helper/src/queue.rs`:

```rust
use std::io::Write;
use std::path::{Path, PathBuf};

use qf_log_parser::RawTrade;
use serde::{Deserialize, Serialize};

/// One line of the queue file and the body of `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueuedEvent {
    pub event_id: String,
    /// RFC 3339 UTC, when the helper saw the success line.
    pub detected_at: String,
    pub trade: RawTrade,
}

/// `$XDG_STATE_HOME/qf-helper/trade-queue.jsonl`, or `~/.local/state/qf-helper/trade-queue.jsonl`.
pub fn default_queue_path(xdg_state_home: Option<&str>, home: &Path) -> PathBuf {
    let base = match xdg_state_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.join(".local").join("state"),
    };
    base.join("qf-helper").join("trade-queue.jsonl")
}

/// Append-only JSONL, replayed oldest first (amendment E3).
pub struct Queue {
    path: PathBuf,
}

impl Queue {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn lines(&self) -> Result<Vec<String>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(text.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(format!("cannot read {}: {e}", self.path.display())),
        }
    }

    pub fn push(&self, event: &QueuedEvent) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let line = serde_json::to_string(event).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|e| format!("cannot open {}: {e}", self.path.display()))?;
        writeln!(file, "{line}").map_err(|e| format!("cannot write {}: {e}", self.path.display()))
    }

    /// The oldest event. A corrupt line is dropped with a message so the queue can't wedge.
    pub fn peek(&self) -> Result<Option<QueuedEvent>, String> {
        loop {
            let Some(first) = self.lines()?.into_iter().next() else { return Ok(None) };
            match serde_json::from_str::<QueuedEvent>(&first) {
                Ok(event) => return Ok(Some(event)),
                Err(e) => {
                    eprintln!("dropping corrupt queue line: {e}");
                    self.pop()?;
                }
            }
        }
    }

    pub fn pop(&self) -> Result<(), String> {
        let mut lines = self.lines()?;
        if lines.is_empty() {
            return Ok(());
        }
        lines.remove(0);
        let mut text = lines.join("\n");
        if !text.is_empty() {
            text.push('\n');
        }
        std::fs::write(&self.path, text).map_err(|e| format!("cannot write {}: {e}", self.path.display()))
    }

    pub fn len(&self) -> usize {
        self.lines().map(|l| l.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qf_log_parser::RawItem;

    fn event(id: &str) -> QueuedEvent {
        QueuedEvent {
            event_id: id.into(),
            detected_at: "2026-09-15T10:00:00Z".into(),
            trade: RawTrade {
                player_name: "PlayerA".into(),
                ee_timestamp: "1.000".into(),
                offered: vec![RawItem { name: "Platinum".into(), quantity: 30, rank: None }],
                received: vec![RawItem { name: "Wolf Sledge Handle".into(), quantity: 1, rank: None }],
            },
        }
    }

    #[test]
    fn events_replay_oldest_first_and_pop_removes_the_head() {
        let dir = tempfile::tempdir().unwrap();
        let queue = Queue::new(dir.path().join("state/qf-helper/trade-queue.jsonl"));
        assert!(queue.peek().unwrap().is_none());
        queue.push(&event("a")).unwrap();
        queue.push(&event("b")).unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "a");
        queue.pop().unwrap();
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "b");
        queue.pop().unwrap();
        assert!(queue.is_empty());
        queue.pop().unwrap();
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("trade-queue.jsonl");
        std::fs::write(&path, "not json\n").unwrap();
        let queue = Queue::new(path);
        queue.push(&event("c")).unwrap();
        assert_eq!(queue.peek().unwrap().unwrap().event_id, "c");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn queue_path_prefers_xdg_state_home() {
        let home = Path::new("/home/player");
        assert_eq!(default_queue_path(Some("/st"), home), PathBuf::from("/st/qf-helper/trade-queue.jsonl"));
        assert_eq!(default_queue_path(None, home), PathBuf::from("/home/player/.local/state/qf-helper/trade-queue.jsonl"));
    }
}
```

- [ ] **Step 2: Trade client, description and loop with tests**

`crates/qf-helper/src/trade.rs`:

```rust
use std::time::{Duration, Instant};

use chrono::{SecondsFormat, Utc};
use qf_log_parser::RawTrade;
use serde::Deserialize;

use crate::config::Config;
use crate::ee_log::{Tail, POLL_EVERY};
use crate::heartbeat::REJECTED_BACKOFF;
use crate::queue::{Queue, QueuedEvent};

pub const RETRY_EVERY: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq)]
pub enum TradeOutcome {
    /// 2xx with the server's status: applied, needs_review, ignored or duplicate.
    Accepted(String),
    /// 401: the device key is wrong or revoked.
    Rejected,
    /// A 4xx the server will never accept; the event is dropped.
    Dropped(String),
    /// Network trouble or a 5xx; retried.
    Failed(String),
}

impl TradeOutcome {
    pub fn removes_from_queue(&self) -> bool {
        matches!(self, TradeOutcome::Accepted(_) | TradeOutcome::Dropped(_))
    }

    pub fn next_delay(&self) -> Option<Duration> {
        match self {
            TradeOutcome::Rejected => Some(REJECTED_BACKOFF),
            TradeOutcome::Failed(_) => Some(RETRY_EVERY),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        match self {
            TradeOutcome::Accepted(status) => status.clone(),
            TradeOutcome::Rejected => "device key rejected (401)".into(),
            TradeOutcome::Dropped(reason) => format!("dropped ({reason})"),
            TradeOutcome::Failed(reason) => format!("failed ({reason})"),
        }
    }
}

#[derive(Deserialize)]
struct TradeResponse {
    status: String,
}

pub struct TradeClient {
    http: reqwest::Client,
    url: String,
    key: String,
}

impl TradeClient {
    pub fn new(server_url: &str, device_key: &str) -> Self {
        let http = reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build().expect("HTTP client");
        Self { http, url: format!("{server_url}/helper/trade"), key: device_key.to_string() }
    }

    pub async fn send(&self, event: &QueuedEvent) -> TradeOutcome {
        use reqwest::StatusCode;
        match self.http.post(&self.url).bearer_auth(&self.key).json(event).send().await {
            Ok(res) if res.status().is_success() => match res.json::<TradeResponse>().await {
                Ok(body) => TradeOutcome::Accepted(body.status),
                Err(e) => TradeOutcome::Accepted(format!("accepted, unreadable reply: {e}")),
            },
            Ok(res) if res.status() == StatusCode::UNAUTHORIZED => TradeOutcome::Rejected,
            Ok(res)
                if matches!(
                    res.status(),
                    StatusCode::BAD_REQUEST
                        | StatusCode::NOT_FOUND
                        | StatusCode::CONFLICT
                        | StatusCode::PAYLOAD_TOO_LARGE
                        | StatusCode::UNPROCESSABLE_ENTITY
                ) =>
            {
                TradeOutcome::Dropped(format!("server returned {}", res.status()))
            }
            Ok(res) => TradeOutcome::Failed(format!("server returned {}", res.status())),
            Err(e) => TradeOutcome::Failed(e.to_string()),
        }
    }
}

fn platinum_of(items: &[qf_log_parser::RawItem]) -> i64 {
    items.iter().filter(|i| i.name == "Platinum").map(|i| i.quantity).sum()
}

/// `trade detected: sale 70p with PlayerB, 1 items; server: applied` (amendment E3).
pub fn describe(trade: &RawTrade, server: &str) -> String {
    let offered = platinum_of(&trade.offered);
    let received = platinum_of(&trade.received);
    let (kind, platinum, goods) = match (offered > 0, received > 0) {
        (true, false) => ("purchase", offered, trade.received.len()),
        (false, true) => ("sale", received, trade.offered.len()),
        _ => ("unknown", offered.max(received), trade.offered.len() + trade.received.len()),
    };
    format!("trade detected: {kind} {platinum}p with {}, {goods} items; server: {server}", trade.player_name)
}

pub fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Polls EE.log every second, queues successes and drains the queue oldest first.
pub async fn run(config: Config, queue: Queue) -> ! {
    let client = TradeClient::new(&config.server_url, &config.device_key);
    let mut tail = Tail::start_at_end(&config.ee_log_path);
    println!("watching {} from byte {} ({} queued trade(s))", config.ee_log_path.display(), tail.offset(), queue.len());
    let mut wait_until: Option<Instant> = None;
    loop {
        for event in tail.poll() {
            let queued = QueuedEvent { event_id: event.event_id, detected_at: now_rfc3339(), trade: event.trade };
            match queue.push(&queued) {
                Ok(()) => println!("{}", describe(&queued.trade, "queued")),
                Err(e) => eprintln!("cannot queue trade: {e}"),
            }
        }
        if wait_until.is_none_or(|until| Instant::now() >= until) {
            wait_until = None;
            loop {
                let next = match queue.peek() {
                    Ok(Some(next)) => next,
                    Ok(None) => break,
                    Err(e) => {
                        eprintln!("cannot read the trade queue: {e}");
                        wait_until = Some(Instant::now() + RETRY_EVERY);
                        break;
                    }
                };
                let outcome = client.send(&next).await;
                println!("{}", describe(&next.trade, &outcome.label()));
                if outcome.removes_from_queue() {
                    if let Err(e) = queue.pop() {
                        eprintln!("cannot update the trade queue: {e}");
                        wait_until = Some(Instant::now() + RETRY_EVERY);
                        break;
                    }
                }
                if let Some(delay) = outcome.next_delay() {
                    wait_until = Some(Instant::now() + delay);
                    break;
                }
            }
        }
        tokio::time::sleep(POLL_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::Path, http::{HeaderMap, StatusCode}, response::IntoResponse, routing::post, Json, Router};
    use qf_log_parser::RawItem;
    use std::sync::{Arc, Mutex};

    fn event(id: &str) -> QueuedEvent {
        QueuedEvent {
            event_id: id.into(),
            detected_at: "2026-09-15T10:00:00Z".into(),
            trade: RawTrade {
                player_name: "PlayerB".into(),
                ee_timestamp: "1170.388".into(),
                offered: vec![RawItem { name: "Arcane Nullifier".into(), quantity: 1, rank: Some(5) }],
                received: vec![RawItem { name: "Platinum".into(), quantity: 70, rank: None }],
            },
        }
    }

    /// Replies according to the event id: `ok-*` 200, `dup-*` 200 duplicate, `bad-*` 422, `boom-*` 500.
    async fn mock_server() -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorder = seen.clone();
        let app = Router::new().route(
            "/helper/trade",
            post(move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
                let recorder = recorder.clone();
                async move {
                    if headers.get("authorization").and_then(|v| v.to_str().ok()) != Some("Bearer qfh_good") {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    recorder.lock().unwrap().push(body.clone());
                    let id = body["event_id"].as_str().unwrap_or("");
                    if id.starts_with("bad-") {
                        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
                    }
                    if id.starts_with("boom-") {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    let status = if id.starts_with("dup-") { "duplicate" } else { "applied" };
                    Json(serde_json::json!({ "status": status })).into_response()
                }
            }),
        );
        let _ = Path::<String>::from; // keeps the import used if axum changes the prelude
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, seen)
    }

    #[tokio::test]
    async fn accepted_events_send_the_e4_body_and_return_the_status() {
        let (url, seen) = mock_server().await;
        let client = TradeClient::new(&url, "qfh_good");
        assert_eq!(client.send(&event("ok-1")).await, TradeOutcome::Accepted("applied".into()));
        assert_eq!(client.send(&event("dup-1")).await, TradeOutcome::Accepted("duplicate".into()));
        let body = &seen.lock().unwrap()[0];
        assert_eq!(body["event_id"], "ok-1");
        assert_eq!(body["detected_at"], "2026-09-15T10:00:00Z");
        assert_eq!(body["trade"]["player_name"], "PlayerB");
        assert_eq!(body["trade"]["offered"][0]["rank"], 5);
        assert_eq!(body["trade"]["received"][0]["quantity"], 70);
    }

    #[tokio::test]
    async fn outcomes_decide_queue_removal_and_delay() {
        let (url, _) = mock_server().await;
        let rejected = TradeClient::new(&url, "qfh_bad").send(&event("ok-2")).await;
        assert_eq!(rejected, TradeOutcome::Rejected);
        assert!(!rejected.removes_from_queue());
        assert_eq!(rejected.next_delay(), Some(REJECTED_BACKOFF));

        let client = TradeClient::new(&url, "qfh_good");
        let dropped = client.send(&event("bad-1")).await;
        assert!(matches!(dropped, TradeOutcome::Dropped(_)));
        assert!(dropped.removes_from_queue());
        assert_eq!(dropped.next_delay(), None);

        let failed = client.send(&event("boom-1")).await;
        assert!(matches!(failed, TradeOutcome::Failed(_)));
        assert!(!failed.removes_from_queue());
        assert_eq!(failed.next_delay(), Some(RETRY_EVERY));
    }

    #[test]
    fn descriptions_name_the_direction_platinum_and_player() {
        assert_eq!(describe(&event("x").trade, "applied"), "trade detected: sale 70p with PlayerB, 1 items; server: applied");
        let mut purchase = event("y").trade;
        std::mem::swap(&mut purchase.offered, &mut purchase.received);
        assert_eq!(describe(&purchase, "queued"), "trade detected: purchase 70p with PlayerB, 1 items; server: queued");
    }
}
```

Add `pub mod queue;` and `pub mod trade;` to `crates/qf-helper/src/lib.rs` (alphabetical: after `process`).

- [ ] **Step 3: Start the trade loop from `main.rs`**

In `crates/qf-helper/src/main.rs`, replace:

```rust
use qf_helper::config::{default_config_path, Config};
use qf_helper::heartbeat::{describe, Client, Heartbeat, Outcome};
use qf_helper::process;
```

with:

```rust
use qf_helper::config::{default_config_path, Config};
use qf_helper::heartbeat::{describe, Client, Heartbeat, Outcome};
use qf_helper::queue::{default_queue_path, Queue};
use qf_helper::{process, trade};
```

Replace:

```rust
    let client = Client::new(&config.server_url, &config.device_key);
    let own_pid = std::process::id();
```

with:

```rust
    let client = Client::new(&config.server_url, &config.device_key);
    if !once {
        let queue = Queue::new(default_queue_path(std::env::var("XDG_STATE_HOME").ok().as_deref(), &home));
        tokio::spawn(trade::run(config.clone(), queue));
    }
    let own_pid = std::process::id();
```

The heartbeat loop and its Ctrl-C handling stay as they are; the process exit ends the trade task.

- [ ] **Step 4: README**

In `README.md`, after the `qf-helper.toml` code block at the end of the "qf-helper (gaming PC)" section, append:

```markdown

### Trade reporting

`qf-helper` also follows `EE.log` from the moment it starts (earlier trades are never replayed) and reports every trade Warframe confirms with "The trade was successful!" to the server. The server records the transaction, updates stock and your real warframe.market order, or parks the trade under **Live Scraper → Trades** for review when a name doesn't resolve.

- Events the server hasn't accepted yet wait in `~/.local/state/qf-helper/trade-queue.jsonl` and are replayed in order.
- `qf-helper --parse /path/to/EE.log` prints the trades found in a log file as JSON, without a server.
- The journal shows one line per trade: `trade detected: sale 70p with <player>, 1 items; server: applied`.
```

- [ ] **Step 5: Run the tests and a manual smoke test**

```bash
cargo test -p qf-helper
cargo build --release -p qf-helper
```

Expected: queue, trade and tail tests pass. The binary builds.

- [ ] **Step 6: Commit**

```bash
git add crates/qf-helper README.md Cargo.lock
git commit -m "feat(helper): queue completed trades and post them to the server

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: `helper_events` table and store

**Files:**
- Create: `crates/migration/src/m20260918_000001_create_helper_events.rs`
- Modify: `crates/migration/src/lib.rs`, `crates/qf_core/Cargo.toml`, `crates/qf_core/src/db.rs`, `crates/qf_core/src/helper_link/mod.rs`, `crates/qf_core/src/collector/maintenance.rs`
- Create: `crates/qf_core/src/helper_link/trades/mod.rs`, `crates/qf_core/src/helper_link/trades/events.rs`

**Interfaces:**
- Consumes: `collector::{db_err, stmt, ts}`, `collector::store::{exec, count}`, `trader::store::tests::db()`, `qf_log_parser::{RawItem, RawTrade}` (Task 1).
- Produces:
  - `trades::{RawItem, RawTrade}` (re-exports), `trades::IncomingTrade { event_id, detected_at, trade: RawTrade }` (Deserialize)
  - `trades::Direction::{Purchase, Sale}` (Copy, serde snake_case), `Direction::as_str(self) -> &'static str`
  - `trades::ResolvedItem { name, slug, wfm_id, item_name, sub_type: Option<SubType>, quantity: i64, price: i64, matched_by: String }`
  - `trades::Resolution { direction: Option<Direction>, platinum: i64, items: Vec<ResolvedItem>, extras: Vec<RawItem> }` (Default)
  - `events::{APPLIED, NEEDS_REVIEW, IGNORED, RETENTION_DAYS}`
  - `events::HelperEvent { event_id, device_name, received_at, detected_at, status, reason: Option<String>, payload: RawTrade, resolution: Option<Resolution>, reviewed_at: Option<String> }` (Serialize, Clone, PartialEq)
  - `events::EventPage { total, page, limit, results: Vec<HelperEvent> }`
  - Async, each `Result<_, Error>`: `exists(conn, id) -> bool`, `insert(conn, &HelperEvent)`, `get(conn, id) -> Option<HelperEvent>`, `list(conn, status: Option<&str>, page, limit) -> EventPage`, `set_status(conn, id, status, reason: Option<&str>, resolution: Option<&Resolution>, reviewed_at: Option<DateTime<Utc>>) -> bool`, `apply_retention(conn, now) -> u64`
  - `maintenance::HourlyReport.deleted_events: u64`

- [ ] **Step 1: Migration**

`crates/migration/src/m20260918_000001_create_helper_events.rs`:

```rust
use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

const UP: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS helper_events (
        event_id TEXT PRIMARY KEY,
        device_name TEXT NOT NULL,
        received_at TEXT NOT NULL,
        detected_at TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('applied', 'needs_review', 'ignored')),
        reason TEXT,
        payload TEXT NOT NULL,
        resolution TEXT,
        reviewed_at TEXT
    )",
    "CREATE INDEX IF NOT EXISTS idx_helper_events_status ON helper_events (status, received_at)",
];

const DOWN: &[&str] = &["DROP TABLE IF EXISTS helper_events"];

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

In `crates/migration/src/lib.rs`, add `mod m20260918_000001_create_helper_events;` after `mod m20260917_000002_drop_helper_override;`, and `Box::new(m20260918_000001_create_helper_events::Migration),` after `Box::new(m20260917_000002_drop_helper_override::Migration),`.

In `crates/qf_core/src/db.rs` test `migrations_create_collector_tables`, add `"helper_events",` after `"helper_keys",`.

- [ ] **Step 2: Dependencies and types**

In `crates/qf_core/Cargo.toml` `[dependencies]`, add after `sha2 = "0.10"`:

```toml
async-trait = "0.1"
toml = "0.8"
qf_log_parser = { path = "../qf_log_parser" }
```

Replace `crates/qf_core/src/helper_link/mod.rs` with:

```rust
//! Link to `qf-helper` on the gaming PC: device keys, heartbeat presence and trade events
//! (spec §5.8, amendments D1–D9 and E1–E9).

pub mod keys;
pub mod presence;
pub mod trades;
```

`crates/qf_core/src/helper_link/trades/mod.rs` (Task 6 extends it):

```rust
//! Trade events reported by `qf-helper` (spec §5.8, amendments E1–E9).

pub mod events;

use serde::{Deserialize, Serialize};
use utils::SubType;

pub use qf_log_parser::{RawItem, RawTrade};

/// Body of `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct IncomingTrade {
    pub event_id: String,
    /// RFC 3339, from the helper's clock.
    pub detected_at: String,
    pub trade: RawTrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Purchase,
    Sale,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Purchase => "purchase",
            Direction::Sale => "sale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolvedItem {
    /// The in-game name as reported.
    pub name: String,
    pub slug: String,
    pub wfm_id: String,
    pub item_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub_type: Option<SubType>,
    pub quantity: i64,
    /// Platinum for the whole line, the way the handlers expect it.
    pub price: i64,
    /// `name`, `override`, `set` or `review` (amendment E5).
    pub matched_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    #[serde(default)]
    pub direction: Option<Direction>,
    #[serde(default)]
    pub platinum: i64,
    #[serde(default)]
    pub items: Vec<ResolvedItem>,
    /// Non-platinum items on the platinum side; recorded, never applied (amendment E6).
    #[serde(default)]
    pub extras: Vec<RawItem>,
}
```

- [ ] **Step 3: Write `events.rs` with its tests**

`crates/qf_core/src/helper_link/trades/events.rs`:

```rust
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection, QueryResult, Value};
use utils::Error;

use super::{RawTrade, Resolution};
use crate::collector::store::{count, exec};
use crate::collector::{db_err, stmt, ts};

pub const RETENTION_DAYS: i64 = 90;
pub const APPLIED: &str = "applied";
pub const NEEDS_REVIEW: &str = "needs_review";
pub const IGNORED: &str = "ignored";

const COLUMNS: &str = "event_id, device_name, received_at, detected_at, status, reason, payload, resolution, reviewed_at";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HelperEvent {
    pub event_id: String,
    pub device_name: String,
    pub received_at: String,
    pub detected_at: String,
    pub status: String,
    pub reason: Option<String>,
    pub payload: RawTrade,
    pub resolution: Option<Resolution>,
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EventPage {
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    pub results: Vec<HelperEvent>,
}

fn from_row(c: &str, row: &QueryResult) -> Result<HelperEvent, Error> {
    let payload: String = row.try_get("", "payload").map_err(|e| db_err(c, e))?;
    let resolution: Option<String> = row.try_get("", "resolution").map_err(|e| db_err(c, e))?;
    Ok(HelperEvent {
        event_id: row.try_get("", "event_id").map_err(|e| db_err(c, e))?,
        device_name: row.try_get("", "device_name").map_err(|e| db_err(c, e))?,
        received_at: row.try_get("", "received_at").map_err(|e| db_err(c, e))?,
        detected_at: row.try_get("", "detected_at").map_err(|e| db_err(c, e))?,
        status: row.try_get("", "status").map_err(|e| db_err(c, e))?,
        reason: row.try_get("", "reason").map_err(|e| db_err(c, e))?,
        payload: serde_json::from_str(&payload).map_err(|e| db_err(c, e))?,
        resolution: resolution.as_deref().map(serde_json::from_str).transpose().map_err(|e| db_err(c, e))?,
        reviewed_at: row.try_get("", "reviewed_at").map_err(|e| db_err(c, e))?,
    })
}

pub async fn exists(conn: &DatabaseConnection, event_id: &str) -> Result<bool, Error> {
    Ok(count(conn, "HelperEvents:Exists", "SELECT COUNT(*) AS n FROM helper_events WHERE event_id = ?", vec![event_id.into()]).await? > 0)
}

pub async fn insert(conn: &DatabaseConnection, event: &HelperEvent) -> Result<(), Error> {
    const C: &str = "HelperEvents:Insert";
    let payload = serde_json::to_string(&event.payload).map_err(|e| db_err(C, e))?;
    let resolution = event.resolution.as_ref().map(serde_json::to_string).transpose().map_err(|e| db_err(C, e))?;
    exec(
        conn,
        C,
        &format!("INSERT INTO helper_events ({COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"),
        vec![
            event.event_id.clone().into(),
            event.device_name.clone().into(),
            event.received_at.clone().into(),
            event.detected_at.clone().into(),
            event.status.clone().into(),
            event.reason.clone().into(),
            payload.into(),
            resolution.into(),
            event.reviewed_at.clone().into(),
        ],
    )
    .await?;
    Ok(())
}

pub async fn get(conn: &DatabaseConnection, event_id: &str) -> Result<Option<HelperEvent>, Error> {
    const C: &str = "HelperEvents:Get";
    conn.query_one(stmt(&format!("SELECT {COLUMNS} FROM helper_events WHERE event_id = ?"), vec![event_id.into()]))
        .await
        .map_err(|e| db_err(C, e))?
        .map(|row| from_row(C, &row))
        .transpose()
}

/// Newest first. `status = None` lists everything. `limit` is clamped to 1..=200.
pub async fn list(conn: &DatabaseConnection, status: Option<&str>, page: i64, limit: i64) -> Result<EventPage, Error> {
    const C: &str = "HelperEvents:List";
    let page = page.max(1);
    let limit = limit.clamp(1, 200);
    let (filter, values): (&str, Vec<Value>) = match status {
        Some(status) => ("WHERE status = ?", vec![status.into()]),
        None => ("", vec![]),
    };
    let total = count(conn, C, &format!("SELECT COUNT(*) AS n FROM helper_events {filter}"), values.clone()).await?;
    let mut values = values;
    values.push(limit.into());
    values.push(((page - 1) * limit).into());
    let results = conn
        .query_all(stmt(
            &format!("SELECT {COLUMNS} FROM helper_events {filter} ORDER BY received_at DESC, rowid DESC LIMIT ? OFFSET ?"),
            values,
        ))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|row| from_row(C, row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EventPage { total, page, limit, results })
}

/// Returns false when no row has that id.
pub async fn set_status(
    conn: &DatabaseConnection,
    event_id: &str,
    status: &str,
    reason: Option<&str>,
    resolution: Option<&Resolution>,
    reviewed_at: Option<DateTime<Utc>>,
) -> Result<bool, Error> {
    const C: &str = "HelperEvents:SetStatus";
    let resolution = resolution.map(serde_json::to_string).transpose().map_err(|e| db_err(C, e))?;
    let changed = exec(
        conn,
        C,
        "UPDATE helper_events SET status = ?, reason = ?, resolution = ?, reviewed_at = ? WHERE event_id = ?",
        vec![
            status.into(),
            reason.map(str::to_string).into(),
            resolution.into(),
            reviewed_at.map(ts).into(),
            event_id.into(),
        ],
    )
    .await?;
    Ok(changed > 0)
}

/// Deletes events received more than 90 days ago (amendment E5).
pub async fn apply_retention(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<u64, Error> {
    exec(
        conn,
        "HelperEvents:Retention",
        "DELETE FROM helper_events WHERE received_at < ?",
        vec![ts(now - Duration::days(RETENTION_DAYS)).into()],
    )
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::helper_link::trades::{Direction, RawItem, ResolvedItem};
    use crate::trader::store::tests::db;

    pub(crate) fn at(text: &str) -> DateTime<Utc> {
        parse_ts(text).unwrap()
    }

    pub(crate) fn sale_trade() -> RawTrade {
        RawTrade {
            player_name: "PlayerB".into(),
            ee_timestamp: "1170.388".into(),
            offered: vec![RawItem { name: "Arcane Nullifier".into(), quantity: 1, rank: Some(5) }],
            received: vec![RawItem { name: "Platinum".into(), quantity: 70, rank: None }],
        }
    }

    pub(crate) fn event(id: &str, received_at: &str, status: &str) -> HelperEvent {
        HelperEvent {
            event_id: id.into(),
            device_name: "gaming-pc".into(),
            received_at: received_at.into(),
            detected_at: received_at.into(),
            status: status.into(),
            reason: None,
            payload: sale_trade(),
            resolution: None,
            reviewed_at: None,
        }
    }

    fn resolution() -> Resolution {
        Resolution {
            direction: Some(Direction::Sale),
            platinum: 70,
            items: vec![ResolvedItem {
                name: "Arcane Nullifier".into(),
                slug: "arcane_nullifier".into(),
                wfm_id: "id_nullifier".into(),
                item_name: "Arcane Nullifier".into(),
                sub_type: Some(utils::SubType::rank(5)),
                quantity: 1,
                price: 70,
                matched_by: "name".into(),
            }],
            extras: vec![],
        }
    }

    #[tokio::test]
    async fn insert_get_and_exists_round_trip_payload_and_resolution() {
        let (_dir, conn) = db().await;
        let mut stored = event("a".repeat(64).as_str(), "2026-09-15T10:00:00Z", APPLIED);
        stored.resolution = Some(resolution());
        assert!(!exists(&conn, &stored.event_id).await.unwrap());
        insert(&conn, &stored).await.unwrap();
        assert!(exists(&conn, &stored.event_id).await.unwrap());
        assert_eq!(get(&conn, &stored.event_id).await.unwrap(), Some(stored.clone()));
        assert!(insert(&conn, &stored).await.is_err(), "the primary key rejects a duplicate");
        assert_eq!(get(&conn, "missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn list_filters_by_status_and_pages_newest_first() {
        let (_dir, conn) = db().await;
        insert(&conn, &event("e1", "2026-09-15T10:00:00Z", APPLIED)).await.unwrap();
        insert(&conn, &event("e2", "2026-09-15T10:01:00Z", NEEDS_REVIEW)).await.unwrap();
        insert(&conn, &event("e3", "2026-09-15T10:02:00Z", NEEDS_REVIEW)).await.unwrap();
        let all = list(&conn, None, 1, 50).await.unwrap();
        assert_eq!(all.total, 3);
        assert_eq!(all.results.iter().map(|e| e.event_id.as_str()).collect::<Vec<_>>(), ["e3", "e2", "e1"]);
        let review = list(&conn, Some(NEEDS_REVIEW), 1, 1).await.unwrap();
        assert_eq!((review.total, review.results.len(), review.results[0].event_id.as_str()), (2, 1, "e3"));
        let second = list(&conn, Some(NEEDS_REVIEW), 2, 1).await.unwrap();
        assert_eq!(second.results[0].event_id, "e2");
        assert_eq!(list(&conn, Some(NEEDS_REVIEW), 0, 0).await.unwrap().limit, 1, "page and limit are clamped");
    }

    #[tokio::test]
    async fn set_status_updates_reason_resolution_and_reviewed_at() {
        let (_dir, conn) = db().await;
        insert(&conn, &event("e1", "2026-09-15T10:00:00Z", NEEDS_REVIEW)).await.unwrap();
        assert!(set_status(&conn, "e1", APPLIED, None, Some(&resolution()), Some(at("2026-09-15T11:00:00Z"))).await.unwrap());
        let updated = get(&conn, "e1").await.unwrap().unwrap();
        assert_eq!(updated.status, APPLIED);
        assert_eq!(updated.resolution, Some(resolution()));
        assert_eq!(updated.reviewed_at.as_deref(), Some("2026-09-15T11:00:00Z"));
        assert!(set_status(&conn, "e1", IGNORED, Some("reviewed"), None, None).await.unwrap());
        assert_eq!(get(&conn, "e1").await.unwrap().unwrap().reason.as_deref(), Some("reviewed"));
        assert!(!set_status(&conn, "nope", IGNORED, None, None, None).await.unwrap());
    }

    #[tokio::test]
    async fn retention_deletes_events_older_than_90_days() {
        let (_dir, conn) = db().await;
        insert(&conn, &event("old", "2026-06-01T10:00:00Z", APPLIED)).await.unwrap();
        insert(&conn, &event("new", "2026-09-15T10:00:00Z", APPLIED)).await.unwrap();
        assert_eq!(apply_retention(&conn, at("2026-09-15T12:00:00Z")).await.unwrap(), 1);
        assert_eq!(list(&conn, None, 1, 10).await.unwrap().results[0].event_id, "new");
    }
}
```

- [ ] **Step 4: Hourly retention hook**

In `crates/qf_core/src/collector/maintenance.rs`, replace:

```rust
pub struct HourlyReport {
    pub hourly_rows: u64,
    pub daily_rows: u64,
    pub deleted_summaries: u64,
    pub deleted_vanished: u64,
}
```

with:

```rust
pub struct HourlyReport {
    pub hourly_rows: u64,
    pub daily_rows: u64,
    pub deleted_summaries: u64,
    pub deleted_vanished: u64,
    /// `helper_events` older than 90 days (amendment E5).
    pub deleted_events: u64,
}
```

and replace:

```rust
    let (deleted_summaries, deleted_vanished) = apply_retention(conn, now).await?;
    Ok(HourlyReport { hourly_rows, daily_rows, deleted_summaries, deleted_vanished })
```

with:

```rust
    let (deleted_summaries, deleted_vanished) = apply_retention(conn, now).await?;
    let deleted_events = crate::helper_link::trades::events::apply_retention(conn, now).await?;
    Ok(HourlyReport { hourly_rows, daily_rows, deleted_summaries, deleted_vanished, deleted_events })
```

- [ ] **Step 5: Run the tests**

```bash
cargo test -p qf_core --lib helper_link::trades::events
cargo test -p qf_core --lib db::
cargo test -p qf_core --lib collector::maintenance
```

Expected: all pass; the table test now lists `helper_events`.

- [ ] **Step 6: Commit**

```bash
git add crates/migration crates/qf_core Cargo.lock
git commit -m "feat(core): store helper trade events with 90-day retention

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 5: Resolution, sets and the platinum split

**Files:**
- Create: `crates/qf_core/src/helper_link/trades/resolve.rs`, `crates/qf_core/src/helper_link/trades/sets.rs`, `crates/qf_core/src/helper_link/trades/split.rs`
- Modify: `crates/qf_core/src/helper_link/trades/mod.rs` (module list), `README.md`

**Interfaces:**
- Consumes: `cache::types::CacheTradableItem` (phase 1), `market::limiter` (phase 2), `trader::price_source::key_of` (phase 3), `paths::get()`, Task 4 types.
- Produces:
  - `resolve::OVERRIDES_FILE = "overrides.toml"`, `resolve::normalise(&str) -> String`
  - `resolve::Overrides::{parse(&str) -> Result<Self, Error>, load(&Path) -> Self, slug_for(&self, name) -> Option<&str>}` (Default)
  - `resolve::ItemIndex::{from_items(Vec<CacheTradableItem>), by_name(&self, name), by_slug(&self, slug), by_id(&self, id), set_roots(&self)}`
  - `resolve::Classified { direction, platinum, goods, extras }`, `resolve::classify(&RawTrade) -> Result<Classified, String>`
  - `resolve::resolve_item(&RawItem, &ItemIndex, &Overrides) -> Option<ResolvedItem>`
  - `resolve::Resolved { resolution: Resolution, unresolved: Vec<String> }`, `resolve::resolve_trade(&RawTrade, &ItemIndex, &Overrides) -> Result<Resolved, String>`
  - `sets::PartsMap = HashMap<String, Vec<String>>`, `#[async_trait] sets::SetSource { async fn parts_for(&self, roots: &[String], index: &ItemIndex) -> PartsMap }` implemented by `PartsMap` and `SetCache`
  - `sets::set_candidates(&[ResolvedItem], &ItemIndex) -> Vec<String>`, `sets::fold_sets(Vec<ResolvedItem>, &[String], &PartsMap, &ItemIndex) -> Vec<ResolvedItem>`, `sets::parse_set_parts(json, &ItemIndex) -> Result<Vec<String>, String>`, `sets::cache() -> &'static SetCache`
  - `split::split_platinum(total: i64, weights: &[Option<f64>]) -> Vec<i64>`, `split::weights_for(&[ResolvedItem], Direction, own_price: &dyn Fn(&ResolvedItem, OrderType) -> Option<i64>, median: &dyn Fn(&ResolvedItem) -> Option<f64>) -> Vec<Option<f64>>`, `split::price_items(&mut [ResolvedItem], total, &[Option<f64>])`, `split::medians(conn, &[ResolvedItem]) -> Result<HashMap<(String, String), f64>, Error>`

- [ ] **Step 1: `resolve.rs` with tests**

Add `pub mod resolve;` (and, for Steps 2 and 3, `pub mod sets;` and `pub mod split;`) after `pub mod events;` in `trades/mod.rs`.

`crates/qf_core/src/helper_link/trades/resolve.rs`:

```rust
//! In-game names to WFM items (amendments E1, E6, E7).

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use utils::{get_location, warning, Error, LoggerOptions, SubType};

use super::{Direction, RawItem, RawTrade, ResolvedItem, Resolution};
use crate::cache::types::CacheTradableItem;

pub const OVERRIDES_FILE: &str = "overrides.toml";
pub const PLATINUM: &str = "Platinum";

fn is_private_use(c: char) -> bool {
    ('\u{e000}'..='\u{f8ff}').contains(&c)
}

/// Trim, drop trailing private-use glyphs, collapse whitespace, lowercase.
pub fn normalise(name: &str) -> String {
    name.trim().trim_end_matches(is_private_use).split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[derive(Debug, Default, Deserialize)]
struct OverridesFile {
    #[serde(default)]
    names: HashMap<String, String>,
}

/// `[names]` table of in-game display name to WFM slug.
#[derive(Debug, Default, Clone)]
pub struct Overrides {
    names: HashMap<String, String>,
}

impl Overrides {
    pub fn parse(text: &str) -> Result<Self, Error> {
        let file: OverridesFile = toml::from_str(text)
            .map_err(|e| Error::new("HelperLink:Overrides", format!("Invalid overrides.toml: {e}"), get_location!()))?;
        Ok(Self { names: file.names.into_iter().map(|(name, slug)| (normalise(&name), slug.trim().to_string())).collect() })
    }

    /// A missing file is empty; an invalid one is logged and treated as empty.
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|e| {
                warning("HelperLink:Overrides", format!("{} ignored: {}", path.display(), e.message), &LoggerOptions::default());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn slug_for(&self, name: &str) -> Option<&str> {
        self.names.get(&normalise(name)).map(String::as_str)
    }
}

/// The tradable item list keyed three ways.
pub struct ItemIndex {
    by_name: HashMap<String, CacheTradableItem>,
    by_slug: HashMap<String, CacheTradableItem>,
    by_id: HashMap<String, CacheTradableItem>,
}

impl ItemIndex {
    pub fn from_items(items: Vec<CacheTradableItem>) -> Self {
        let mut index = Self { by_name: HashMap::new(), by_slug: HashMap::new(), by_id: HashMap::new() };
        for item in items {
            index.by_name.entry(normalise(&item.name)).or_insert_with(|| item.clone());
            index.by_id.insert(item.wfm_id.clone(), item.clone());
            index.by_slug.insert(item.wfm_url.clone(), item);
        }
        index
    }

    pub fn by_name(&self, name: &str) -> Option<&CacheTradableItem> {
        self.by_name.get(&normalise(name))
    }

    pub fn by_slug(&self, slug: &str) -> Option<&CacheTradableItem> {
        self.by_slug.get(slug)
    }

    pub fn by_id(&self, id: &str) -> Option<&CacheTradableItem> {
        self.by_id.get(id)
    }

    pub fn set_roots(&self) -> impl Iterator<Item = &CacheTradableItem> {
        self.by_slug.values().filter(|item| item.tags.iter().any(|t| t == "set"))
    }
}

pub struct Classified {
    pub direction: Direction,
    pub platinum: i64,
    pub goods: Vec<RawItem>,
    pub extras: Vec<RawItem>,
}

fn platinum_of(items: &[RawItem]) -> i64 {
    items.iter().filter(|i| i.name == PLATINUM).map(|i| i.quantity).sum()
}

fn without_platinum(items: &[RawItem]) -> Vec<RawItem> {
    items.iter().filter(|i| i.name != PLATINUM).cloned().collect()
}

/// Amendment E6. The error is the review reason.
pub fn classify(trade: &RawTrade) -> Result<Classified, String> {
    let offered = platinum_of(&trade.offered);
    let received = platinum_of(&trade.received);
    match (offered > 0, received > 0) {
        (true, false) => Ok(Classified {
            direction: Direction::Purchase,
            platinum: offered,
            goods: without_platinum(&trade.received),
            extras: without_platinum(&trade.offered),
        }),
        (false, true) => Ok(Classified {
            direction: Direction::Sale,
            platinum: received,
            goods: without_platinum(&trade.offered),
            extras: without_platinum(&trade.received),
        }),
        _ => Err("no_platinum_side".to_string()),
    }
}

fn resolved(raw: &RawItem, item: &CacheTradableItem, matched_by: &str) -> ResolvedItem {
    let sub_type = raw.rank.map(|rank| {
        let capped = item.sub_type.as_ref().and_then(|s| s.max_rank).map_or(rank, |max| rank.min(max));
        SubType::rank(capped)
    });
    ResolvedItem {
        name: raw.name.clone(),
        slug: item.wfm_url.clone(),
        wfm_id: item.wfm_id.clone(),
        item_name: item.name.clone(),
        sub_type,
        quantity: raw.quantity,
        price: 0,
        matched_by: matched_by.into(),
    }
}

/// English name first, then `overrides.toml`.
pub fn resolve_item(raw: &RawItem, index: &ItemIndex, overrides: &Overrides) -> Option<ResolvedItem> {
    if let Some(item) = index.by_name(&raw.name) {
        return Some(resolved(raw, item, "name"));
    }
    let slug = overrides.slug_for(&raw.name)?;
    index.by_slug(slug).map(|item| resolved(raw, item, "override"))
}

pub struct Resolved {
    pub resolution: Resolution,
    /// Raw names that resolved to nothing; empty means every goods item resolved.
    pub unresolved: Vec<String>,
}

pub fn resolve_trade(trade: &RawTrade, index: &ItemIndex, overrides: &Overrides) -> Result<Resolved, String> {
    let classified = classify(trade)?;
    let mut items = Vec::new();
    let mut unresolved = Vec::new();
    for raw in &classified.goods {
        match resolve_item(raw, index, overrides) {
            Some(item) => items.push(item),
            None => unresolved.push(raw.name.clone()),
        }
    }
    Ok(Resolved {
        resolution: Resolution {
            direction: Some(classified.direction),
            platinum: classified.platinum,
            items,
            extras: classified.extras,
        },
        unresolved,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cache::types::SubType as CacheSubType;

    pub(crate) fn item(name: &str, slug: &str, max_rank: Option<i64>, tags: &[&str]) -> CacheTradableItem {
        CacheTradableItem {
            name: name.into(),
            unique_name: String::new(),
            wfm_id: format!("id_{slug}"),
            wfm_url: slug.into(),
            trade_tax: 0,
            mr_requirement: 0,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            icon: String::new(),
            bulk_tradable: false,
            sub_type: max_rank.map(|max| CacheSubType { max_rank: Some(max), variants: None, amber_stars: None, cyan_stars: None }),
            variant_to_unique_name: HashMap::new(),
        }
    }

    /// The small item list every trades test shares (Task 6 reuses it).
    pub(crate) fn items() -> Vec<CacheTradableItem> {
        vec![
            item("Arcane Nullifier", "arcane_nullifier", Some(5), &["arcane_enhancement"]),
            item("Adaptation", "adaptation", Some(10), &["mod"]),
            item("Wolf Sledge Set", "wolf_sledge_set", None, &["set", "weapon"]),
            item("Wolf Sledge Blueprint", "wolf_sledge_blueprint", None, &["component"]),
            item("Wolf Sledge Motor", "wolf_sledge_motor", None, &["component"]),
            item("Wolf Sledge Head", "wolf_sledge_head", None, &["component"]),
            item("Wolf Sledge Handle", "wolf_sledge_handle", None, &["component"]),
            item("Mesa Prime Set", "mesa_prime_set", None, &["set", "prime"]),
            item("Primed Firestorm", "primed_firestorm", Some(10), &["mod"]),
        ]
    }

    pub(crate) fn index() -> ItemIndex {
        ItemIndex::from_items(items())
    }

    fn raw(name: &str, quantity: i64, rank: Option<i64>) -> RawItem {
        RawItem { name: name.into(), quantity, rank }
    }

    #[test]
    fn normalise_trims_glyphs_whitespace_and_case() {
        assert_eq!(normalise("  Arcane  Energize \u{e0b9}\u{e0b9} "), "arcane energize");
        assert_eq!(normalise("Wolf Sledge Handle"), "wolf sledge handle");
    }

    #[test]
    fn overrides_parse_load_and_lookup() {
        let overrides = Overrides::parse("[names]\n\"Primed Fir\" = \"primed_firestorm\"\n").unwrap();
        assert_eq!(overrides.slug_for("primed fir \u{e000}"), Some("primed_firestorm"));
        assert_eq!(overrides.slug_for("other"), None);
        assert!(Overrides::parse("names = 3").is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(Overrides::load(&dir.path().join("missing.toml")).slug_for("x").is_none());
        std::fs::write(dir.path().join("bad.toml"), "not = [toml").unwrap();
        assert!(Overrides::load(&dir.path().join("bad.toml")).slug_for("x").is_none(), "invalid files are ignored");
    }

    #[test]
    fn classify_finds_the_platinum_side_and_extras() {
        let purchase = RawTrade {
            player_name: "P".into(),
            ee_timestamp: "1".into(),
            offered: vec![raw("Platinum", 43, None), raw("Mortus Lungfish (L)", 1, None)],
            received: vec![raw("Adaptation", 1, Some(10))],
        };
        let c = classify(&purchase).unwrap();
        assert_eq!((c.direction, c.platinum), (Direction::Purchase, 43));
        assert_eq!(c.goods, vec![raw("Adaptation", 1, Some(10))]);
        assert_eq!(c.extras, vec![raw("Mortus Lungfish (L)", 1, None)]);

        let sale = RawTrade { offered: purchase.received.clone(), received: purchase.offered.clone(), ..purchase.clone() };
        assert_eq!(classify(&sale).unwrap().direction, Direction::Sale);

        let swap = RawTrade { offered: vec![raw("Adaptation", 1, Some(10))], received: vec![raw("Primed Firestorm", 1, Some(10))], ..purchase.clone() };
        assert_eq!(classify(&swap).unwrap_err(), "no_platinum_side");
        let both = RawTrade { offered: vec![raw("Platinum", 5, None)], received: vec![raw("Platinum", 9, None)], ..purchase.clone() };
        assert_eq!(classify(&both).unwrap_err(), "no_platinum_side");
    }

    #[test]
    fn items_resolve_by_name_then_override_with_capped_ranks() {
        let index = index();
        let overrides = Overrides::parse("[names]\n\"Primed Fir\" = \"primed_firestorm\"\n").unwrap();
        let arcane = resolve_item(&raw("Arcane Nullifier \u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}\u{e0b9}", 1, Some(7)), &index, &overrides).unwrap();
        assert_eq!((arcane.slug.as_str(), arcane.matched_by.as_str()), ("arcane_nullifier", "name"));
        assert_eq!(arcane.sub_type, Some(SubType::rank(5)), "capped at max_rank");
        let part = resolve_item(&raw("wolf sledge HANDLE", 2, None), &index, &overrides).unwrap();
        assert_eq!((part.slug.as_str(), part.quantity, part.sub_type.is_none()), ("wolf_sledge_handle", 2, true));
        let overridden = resolve_item(&raw("Primed Fir", 1, Some(10)), &index, &overrides).unwrap();
        assert_eq!((overridden.slug.as_str(), overridden.matched_by.as_str()), ("primed_firestorm", "override"));
        assert!(resolve_item(&raw("Mortus Lungfish (L)", 1, None), &index, &overrides).is_none());
    }

    #[test]
    fn resolve_trade_keeps_the_unresolved_names() {
        let index = index();
        let trade = RawTrade {
            player_name: "P".into(),
            ee_timestamp: "1".into(),
            offered: vec![raw("Platinum", 30, None)],
            received: vec![raw("Wolf Sledge Handle", 1, None), raw("Mystery Thing", 1, None)],
        };
        let resolved = resolve_trade(&trade, &index, &Overrides::default()).unwrap();
        assert_eq!(resolved.unresolved, vec!["Mystery Thing".to_string()]);
        assert_eq!(resolved.resolution.direction, Some(Direction::Purchase));
        assert_eq!(resolved.resolution.platinum, 30);
        assert_eq!(resolved.resolution.items.len(), 1);
    }
}
```

- [ ] **Step 2: `sets.rs` with tests**

`crates/qf_core/src/helper_link/trades/sets.rs`:

```rust
//! Folds a full set of parts into the set item, with WFM `setParts` fetched lazily (amendment E7).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use utils::{warning, LoggerOptions};

use super::resolve::ItemIndex;
use super::ResolvedItem;
use crate::market::limiter::{self, Lane};

pub const SETS_FILE: &str = "sets.json";
pub const WFM_ITEM_URL: &str = "https://api.warframe.market/v2/item";

/// Part slugs per set-root slug, without the root itself.
pub type PartsMap = HashMap<String, Vec<String>>;

#[async_trait]
pub trait SetSource: Send + Sync {
    /// Parts for the given roots. Roots it can't provide are absent from the result.
    async fn parts_for(&self, roots: &[String], index: &ItemIndex) -> PartsMap;
}

#[async_trait]
impl SetSource for PartsMap {
    async fn parts_for(&self, roots: &[String], _index: &ItemIndex) -> PartsMap {
        roots.iter().filter_map(|root| self.get(root).map(|parts| (root.clone(), parts.clone()))).collect()
    }
}

/// Set roots whose English name without ` Set` prefixes at least two of the items' names.
pub fn set_candidates(items: &[ResolvedItem], index: &ItemIndex) -> Vec<String> {
    if items.len() < 2 {
        return Vec::new();
    }
    let names: Vec<String> = items.iter().map(|i| i.item_name.to_lowercase()).collect();
    let mut roots: Vec<String> = index
        .set_roots()
        .filter_map(|root| {
            let prefix = format!("{} ", root.name.strip_suffix(" Set")?.to_lowercase());
            (names.iter().filter(|n| n.starts_with(&prefix)).count() >= 2).then(|| root.wfm_url.clone())
        })
        .collect();
    roots.sort();
    roots
}

/// Replaces every complete set of parts with the set item; leftover parts stay.
pub fn fold_sets(mut items: Vec<ResolvedItem>, candidates: &[String], parts: &PartsMap, index: &ItemIndex) -> Vec<ResolvedItem> {
    for root in candidates {
        let (Some(part_slugs), Some(root_item)) = (parts.get(root), index.by_slug(root)) else { continue };
        let part_slugs: Vec<&String> = part_slugs.iter().filter(|p| *p != root).collect();
        if part_slugs.is_empty() {
            continue;
        }
        let sets = part_slugs
            .iter()
            .map(|slug| items.iter().filter(|i| &i.slug == *slug).map(|i| i.quantity).sum::<i64>())
            .min()
            .unwrap_or(0);
        if sets <= 0 {
            continue;
        }
        for slug in &part_slugs {
            let mut remaining = sets;
            for item in items.iter_mut().filter(|i| &i.slug == *slug) {
                let take = remaining.min(item.quantity);
                item.quantity -= take;
                remaining -= take;
            }
        }
        items.retain(|i| i.quantity > 0);
        items.push(ResolvedItem {
            name: root_item.name.clone(),
            slug: root_item.wfm_url.clone(),
            wfm_id: root_item.wfm_id.clone(),
            item_name: root_item.name.clone(),
            sub_type: None,
            quantity: sets,
            price: 0,
            matched_by: "set".into(),
        });
    }
    items
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemDetail {
    #[serde(default)]
    set_parts: Vec<String>,
}

#[derive(Deserialize)]
struct ItemDetailResponse {
    data: ItemDetail,
}

/// `GET /v2/item/{slug}` lists `setParts` as item ids; ids the index doesn't know are dropped.
pub fn parse_set_parts(json: &str, index: &ItemIndex) -> Result<Vec<String>, String> {
    let detail: ItemDetailResponse = serde_json::from_str(json).map_err(|e| e.to_string())?;
    Ok(detail.data.set_parts.iter().filter_map(|id| index.by_id(id).map(|i| i.wfm_url.clone())).collect())
}

/// Memory, then `QF_DATA_DIR/cache/sets.json`, then one WFM fetch per unknown root.
pub struct SetCache {
    file: PathBuf,
    parts: Mutex<PartsMap>,
    http: reqwest::Client,
}

impl SetCache {
    pub fn new(cache_dir: &Path) -> Self {
        let file = cache_dir.join(SETS_FILE);
        let parts = std::fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().expect("HTTP client");
        Self { file, parts: Mutex::new(parts), http }
    }

    fn save(&self) {
        let snapshot = self.parts.lock().unwrap().clone();
        match serde_json::to_string(&snapshot) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&self.file, text) {
                    warning("HelperLink:Sets", format!("Could not save {}: {e}", self.file.display()), &LoggerOptions::default());
                }
            }
            Err(e) => warning("HelperLink:Sets", format!("Could not serialise sets: {e}"), &LoggerOptions::default()),
        }
    }

    async fn fetch(&self, root: &str, index: &ItemIndex) -> Result<Vec<String>, String> {
        limiter::global().acquire(Lane::Hot).await;
        let response = self
            .http
            .get(format!("{WFM_ITEM_URL}/{root}"))
            .header("Language", "en")
            .header("Platform", "pc")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.status().as_u16() == 429 {
            limiter::global().report_429();
        }
        let body = response.error_for_status().map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())?;
        parse_set_parts(&body, index)
    }
}

#[async_trait]
impl SetSource for SetCache {
    async fn parts_for(&self, roots: &[String], index: &ItemIndex) -> PartsMap {
        let mut fetched_any = false;
        for root in roots {
            if self.parts.lock().unwrap().contains_key(root) {
                continue;
            }
            match self.fetch(root, index).await {
                Ok(parts) => {
                    self.parts.lock().unwrap().insert(root.clone(), parts);
                    fetched_any = true;
                }
                Err(e) => warning("HelperLink:Sets", format!("Could not fetch set parts for {root}: {e}"), &LoggerOptions::default()),
            }
        }
        if fetched_any {
            self.save();
        }
        let parts = self.parts.lock().unwrap();
        roots.iter().filter_map(|root| parts.get(root).map(|p| (root.clone(), p.clone()))).collect()
    }
}

static SET_CACHE: OnceLock<SetCache> = OnceLock::new();

pub fn cache() -> &'static SetCache {
    SET_CACHE.get_or_init(|| SetCache::new(&crate::paths::get().cache_dir()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::trades::resolve::tests::index;

    fn part(slug: &str, quantity: i64) -> ResolvedItem {
        let index = index();
        let item = index.by_slug(slug).unwrap();
        ResolvedItem {
            name: item.name.clone(),
            slug: slug.into(),
            wfm_id: item.wfm_id.clone(),
            item_name: item.name.clone(),
            sub_type: None,
            quantity,
            price: 0,
            matched_by: "name".into(),
        }
    }

    fn wolf_parts() -> PartsMap {
        PartsMap::from([(
            "wolf_sledge_set".to_string(),
            vec!["wolf_sledge_blueprint".into(), "wolf_sledge_motor".into(), "wolf_sledge_head".into(), "wolf_sledge_handle".into()],
        )])
    }

    #[test]
    fn candidates_need_two_items_sharing_the_root_prefix() {
        let index = index();
        assert!(set_candidates(&[part("wolf_sledge_handle", 1)], &index).is_empty());
        assert_eq!(set_candidates(&[part("wolf_sledge_handle", 1), part("wolf_sledge_head", 1)], &index), vec!["wolf_sledge_set".to_string()]);
        assert!(set_candidates(&[part("wolf_sledge_handle", 1), part("adaptation", 1)], &index).is_empty());
    }

    #[test]
    fn a_full_set_folds_and_leftovers_stay() {
        let index = index();
        let items = vec![part("wolf_sledge_blueprint", 1), part("wolf_sledge_motor", 2), part("wolf_sledge_head", 1), part("wolf_sledge_handle", 1)];
        let folded = fold_sets(items, &["wolf_sledge_set".into()], &wolf_parts(), &index);
        let mut slugs: Vec<(String, i64, String)> = folded.iter().map(|i| (i.slug.clone(), i.quantity, i.matched_by.clone())).collect();
        slugs.sort();
        assert_eq!(slugs, vec![("wolf_sledge_motor".into(), 1, "name".into()), ("wolf_sledge_set".into(), 1, "set".into())]);
    }

    #[test]
    fn two_sets_fold_into_quantity_two() {
        let index = index();
        let items = vec![part("wolf_sledge_blueprint", 2), part("wolf_sledge_motor", 2), part("wolf_sledge_head", 2), part("wolf_sledge_handle", 2)];
        let folded = fold_sets(items, &["wolf_sledge_set".into()], &wolf_parts(), &index);
        assert_eq!(folded.len(), 1);
        assert_eq!((folded[0].slug.as_str(), folded[0].quantity), ("wolf_sledge_set", 2));
    }

    #[test]
    fn an_incomplete_set_or_unknown_parts_change_nothing() {
        let index = index();
        let items = vec![part("wolf_sledge_blueprint", 1), part("wolf_sledge_motor", 1)];
        assert_eq!(fold_sets(items.clone(), &["wolf_sledge_set".into()], &wolf_parts(), &index), items);
        assert_eq!(fold_sets(items.clone(), &["wolf_sledge_set".into()], &PartsMap::new(), &index), items);
    }

    #[test]
    fn set_parts_parse_from_the_wfm_item_response() {
        let index = index();
        let json = r#"{"data":{"slug":"wolf_sledge_set","setRoot":true,"setParts":["id_wolf_sledge_handle","id_wolf_sledge_set","unknown"]}}"#;
        assert_eq!(parse_set_parts(json, &index).unwrap(), vec!["wolf_sledge_handle".to_string(), "wolf_sledge_set".to_string()]);
        assert!(parse_set_parts("nope", &index).is_err());
    }

    #[tokio::test]
    async fn the_cache_serves_from_disk_without_fetching() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SETS_FILE), serde_json::to_string(&wolf_parts()).unwrap()).unwrap();
        let cache = SetCache::new(dir.path());
        let parts = cache.parts_for(&["wolf_sledge_set".into()], &index()).await;
        assert_eq!(parts["wolf_sledge_set"].len(), 4);
    }
}
```

- [ ] **Step 3: `split.rs` with tests**

`crates/qf_core/src/helper_link/trades/split.rs`:

```rust
//! Platinum split across the items of one trade (amendment E8).

use std::collections::HashMap;

use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;
use wf_market::enums::OrderType;

use super::{Direction, ResolvedItem};
use crate::collector::{db_err, stmt};
use crate::trader::price_source::key_of;

/// Whole-platinum shares proportional to `weights`. An unknown or zero weight takes the mean of
/// the known ones, so it is neither starved nor favoured; all unknown means equal shares.
/// The rounding remainder lands on the first item.
pub fn split_platinum(total: i64, weights: &[Option<f64>]) -> Vec<i64> {
    if weights.is_empty() {
        return Vec::new();
    }
    let known: Vec<f64> = weights.iter().flatten().copied().filter(|w| *w > 0.0).collect();
    let fill = if known.is_empty() { 1.0 } else { known.iter().sum::<f64>() / known.len() as f64 };
    let filled: Vec<f64> = weights.iter().map(|w| match w { Some(v) if *v > 0.0 => *v, _ => fill }).collect();
    let sum: f64 = filled.iter().sum();
    let mut shares: Vec<i64> = filled.iter().map(|w| (total as f64 * w / sum).round() as i64).collect();
    let remainder = total - shares.iter().sum::<i64>();
    shares[0] += remainder;
    shares
}

/// Line weight = unit price × quantity. The unit price is the user's own order for that item and
/// sub type (sell order for a sale, buy order for a purchase), else the collector median.
pub fn weights_for(
    items: &[ResolvedItem],
    direction: Direction,
    own_price: &dyn Fn(&ResolvedItem, OrderType) -> Option<i64>,
    median: &dyn Fn(&ResolvedItem) -> Option<f64>,
) -> Vec<Option<f64>> {
    let order_type = match direction {
        Direction::Sale => OrderType::Sell,
        Direction::Purchase => OrderType::Buy,
    };
    items
        .iter()
        .map(|item| own_price(item, order_type).map(|p| p as f64).or_else(|| median(item)).map(|unit| unit * item.quantity as f64))
        .collect()
}

pub fn price_items(items: &mut [ResolvedItem], total: i64, weights: &[Option<f64>]) {
    for (item, price) in items.iter_mut().zip(split_platinum(total, weights)) {
        item.price = price;
    }
}

/// `item_stats.median` keyed by `(wfm_id, sub-type key)` for the given items.
pub async fn medians(conn: &DatabaseConnection, items: &[ResolvedItem]) -> Result<HashMap<(String, String), f64>, Error> {
    const C: &str = "HelperLink:Medians";
    let mut out = HashMap::new();
    for item in items {
        let key = key_of(&item.sub_type);
        let row = conn
            .query_one(stmt(
                "SELECT median FROM item_stats WHERE item_id = ? AND sub_type = ?",
                vec![item.wfm_id.clone().into(), key.clone().into()],
            ))
            .await
            .map_err(|e| db_err(C, e))?;
        if let Some(median) = row.and_then(|r| r.try_get::<Option<f64>>("", "median").ok().flatten()) {
            out.insert((item.wfm_id.clone(), key), median);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(slug: &str, quantity: i64) -> ResolvedItem {
        ResolvedItem {
            name: slug.into(),
            slug: slug.into(),
            wfm_id: format!("id_{slug}"),
            item_name: slug.into(),
            sub_type: None,
            quantity,
            price: 0,
            matched_by: "name".into(),
        }
    }

    #[test]
    fn equal_split_when_nothing_is_known_with_remainder_first() {
        assert_eq!(split_platinum(100, &[None, None, None]), vec![34, 33, 33]);
        assert_eq!(split_platinum(70, &[None]), vec![70]);
        assert!(split_platinum(70, &[]).is_empty());
    }

    #[test]
    fn weighted_split_rounds_to_whole_platinum() {
        assert_eq!(split_platinum(105, &[Some(100.0), Some(5.0)]), vec![100, 5]);
        assert_eq!(split_platinum(100, &[Some(2.0), Some(1.0)]), vec![67, 33]);
    }

    #[test]
    fn unknown_weights_take_the_mean_of_the_known_ones() {
        assert_eq!(split_platinum(90, &[Some(20.0), None, Some(40.0)]), vec![20, 30, 40]);
        assert_eq!(split_platinum(90, &[Some(0.0), Some(30.0), Some(30.0)]), vec![30, 30, 30]);
    }

    #[test]
    fn weights_prefer_own_orders_then_medians_and_scale_by_quantity() {
        let items = [item("a", 2), item("b", 1), item("c", 3)];
        let own = |i: &ResolvedItem, ot: OrderType| (i.slug == "a" && ot == OrderType::Sell).then_some(10);
        let median = |i: &ResolvedItem| (i.slug == "b").then_some(7.0);
        assert_eq!(weights_for(&items, Direction::Sale, &own, &median), vec![Some(20.0), Some(7.0), None]);
        assert_eq!(weights_for(&items, Direction::Purchase, &own, &median), vec![None, Some(7.0), None], "buy orders only for purchases");
        let mut items = items;
        price_items(&mut items, 60, &[Some(20.0), Some(7.0), None]);
        assert_eq!(items.iter().map(|i| i.price).collect::<Vec<_>>(), vec![30, 10, 20]);
    }
}
```

The last assertion: known weights 20 and 7 have mean 13.5, so the weights are 20, 7 and 13.5 over a sum of 40.5; the shares round to 30, 10 and 20.

- [ ] **Step 4: README section for overrides**

In `README.md`, append to the "Trade reporting" section from Task 3:

```markdown

**Names that don't resolve.** The server matches in-game names against warframe.market's English names. For the rare miss, create `overrides.toml` in the data volume (`docker compose exec quantframe-server sh -c 'cat > /data/overrides.toml'` or edit it on the host) mapping the in-game name to the warframe.market slug:

```toml
[names]
"Primed Fir" = "primed_firestorm"
```

It is read on every trade, so no restart is needed. The review modal shows the exact name the game sent.
```

- [ ] **Step 5: Run the tests**

```bash
cargo test -p qf_core --lib helper_link::trades
```

Expected: the resolve, sets and split tests pass alongside the events tests.

- [ ] **Step 6: Commit**

```bash
git add crates/qf_core README.md
git commit -m "feat(core): resolve helper trades by WFM name, fold sets and split platinum

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 6: Trade pipeline: apply, handle_incoming, review and ignore

**Files:**
- Create: `crates/qf_core/src/helper_link/trades/apply.rs`
- Modify: `crates/qf_core/src/helper_link/trades/mod.rs`

**Interfaces:**
- Consumes: Task 4 (`events::*`, `Direction`, `ResolvedItem`, `Resolution`, `IncomingTrade`), Task 5 (`resolve::{resolve_trade, ItemIndex, Overrides}`, `resolve::tests::items()`, `sets::{set_candidates, fold_sets, SetSource, PartsMap}`, `split::{medians, weights_for, price_items}`), `handlers::{handle_item, handle_wish_list}`, `trader::price_source::key_of`, `collector::ts`.
- Produces:
  - `apply::WISH_LIST_NOT_FOUND = "WishListItemBought_NotFound"`
  - `apply::wish_list_flags(detected_at: &str) -> OperationSet`, `apply::item_flags(direction: Direction, detected_at: &str) -> OperationSet`
  - `#[async_trait] apply::ItemApplier: Send + Sync { async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error> }`
  - `apply::HandlerApplier` (unit struct, implements `ItemApplier` over the real handlers)
  - `apply::ApplyFailure { completed: Vec<String>, error: Error }`, `apply::apply_items(&dyn ItemApplier, Direction, &[ResolvedItem], player: &str, detected_at: &str) -> Result<(), ApplyFailure>`
  - `trades::{DUPLICATE, APPLYING, AUTO_TRADE_OFF, NO_GOODS, NO_PLATINUM_SIDE, REVIEWED}` string constants
  - `trades::Outcome { status: String, reason: Option<String> }` (Serialize; `reason` skipped when `None`)
  - `trades::TradeEnv: Send + Sync` with `auto_trade(&self) -> bool`, `tradable_items(&self) -> Vec<CacheTradableItem>`, `overrides(&self) -> Overrides`, `own_price(&self, &ResolvedItem, OrderType) -> Option<i64>`, `sets(&self) -> &dyn SetSource`, `applier(&self) -> &dyn ItemApplier`, `notify(&self, &HelperEvent)`
  - `trades::ReviewItem { slug: String, sub_type: Option<SubType>, quantity: i64, price: i64 }` (Deserialize, snake_case fields)
  - `trades::validate(&IncomingTrade) -> Result<DateTime<Utc>, String>`
  - Async: `trades::resolve_event(conn, &dyn TradeEnv, &RawTrade) -> Result<(Resolution, Option<String>), Error>`, `trades::handle_incoming(conn, &dyn TradeEnv, device_name: &str, IncomingTrade, now) -> Result<Outcome, Error>`, `trades::apply_reviewed(conn, &dyn TradeEnv, event_id: &str, Vec<ReviewItem>, now) -> Result<HelperEvent, Error>`, `trades::ignore(conn, event_id: &str, now) -> Result<HelperEvent, Error>`

Notes for the implementer:
- **A purchase goes through the wish list first.** `handle_wish_list` with `ReturnOn:NotFound` returns early, with `WishListItemBought_NotFound` in its operations, when no wish-list row matches. Only then does `handle_item` run. When a row matches, the wish-list handler has already closed the buy order and written the transaction, so calling `handle_item` as well would record the purchase twice.
- **An event is stored before anything is applied.** It is inserted as `needs_review` with reason `applying`, then flipped to `applied`. A crash in the middle leaves a visible row that the user can review, and a retry from the helper is a `duplicate` rather than a second application.
- **Reason precedence:** `no_platinum_side`, then `unresolved: <names>`, then `no_goods` (a platinum-only trade), then `auto_trade_off`. `no_goods` and `auto_trade_off` aren't named in E6–E8; they are the reasons for the two cases E8 parks without naming one.

- [ ] **Step 1: `apply.rs` with tests**

Add `pub mod apply;` before `pub mod events;` in `trades/mod.rs`.

`crates/qf_core/src/helper_link/trades/apply.rs`:

```rust
//! Drives the existing stock and wish-list handlers for one trade (amendment E8).

use async_trait::async_trait;
use utils::{Error, OperationSet};
use wf_market::enums::OrderType;

use super::{Direction, ResolvedItem};
use crate::handlers::{handle_item, handle_wish_list};

/// What `handle_wish_list` reports when no wish-list row matched a purchase.
pub const WISH_LIST_NOT_FOUND: &str = "WishListItemBought_NotFound";

/// A purchase asks the wish list first and stops there when nothing matches.
pub fn wish_list_flags(detected_at: &str) -> OperationSet {
    OperationSet::from(vec!["ReturnOn:NotFound".to_string(), format!("SetDate:{detected_at}")])
}

/// A sale skips the WFM check when no stock row matched; a purchase only sets the date.
pub fn item_flags(direction: Direction, detected_at: &str) -> OperationSet {
    match direction {
        Direction::Sale => OperationSet::from(vec!["SkipWFMCheck:ItemSell_NotFound".to_string(), format!("SetDate:{detected_at}")]),
        Direction::Purchase => OperationSet::from(vec![format!("SetDate:{detected_at}")]),
    }
}

#[async_trait]
pub trait ItemApplier: Send + Sync {
    /// `item.price` is the platinum for the whole line.
    async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error>;
}

/// The real handlers. They close or adjust real WFM orders whatever the global dry-run says,
/// the same as the manual "sold" action, because the trade really happened.
pub struct HandlerApplier;

#[async_trait]
impl ItemApplier for HandlerApplier {
    async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error> {
        match direction {
            Direction::Purchase => {
                let (operations, _) = handle_wish_list(
                    item.slug.clone(),
                    &item.sub_type,
                    item.quantity,
                    item.price,
                    player,
                    OrderType::Buy,
                    &wish_list_flags(detected_at),
                )
                .await?;
                if operations.has(WISH_LIST_NOT_FOUND) {
                    handle_item(
                        item.slug.clone(),
                        item.sub_type.clone(),
                        item.quantity,
                        item.price,
                        player,
                        OrderType::Buy,
                        &item_flags(direction, detected_at),
                    )
                    .await?;
                }
            }
            Direction::Sale => {
                handle_item(
                    item.slug.clone(),
                    item.sub_type.clone(),
                    item.quantity,
                    item.price,
                    player,
                    OrderType::Sell,
                    &item_flags(direction, detected_at),
                )
                .await?;
            }
        }
        Ok(())
    }
}

pub struct ApplyFailure {
    /// `<item name> x<quantity>` for every item applied before the failure.
    pub completed: Vec<String>,
    pub error: Error,
}

/// Applies items in order and stops at the first failure.
pub async fn apply_items(
    applier: &dyn ItemApplier,
    direction: Direction,
    items: &[ResolvedItem],
    player: &str,
    detected_at: &str,
) -> Result<(), ApplyFailure> {
    let mut completed = Vec::new();
    for item in items {
        if let Err(error) = applier.apply_item(direction, item, player, detected_at).await {
            return Err(ApplyFailure { completed, error });
        }
        completed.push(format!("{} x{}", item.item_name, item.quantity));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use utils::get_location;

    #[test]
    fn flags_carry_the_date_and_the_per_direction_rules() {
        let date = "2026-09-15T10:00:00Z";
        let wish = wish_list_flags(date);
        assert!(wish.has("ReturnOn:NotFound"));
        assert_eq!(wish.get_value_after("SetDate").as_deref(), Some(date));
        let sale = item_flags(Direction::Sale, date);
        assert_eq!(sale.get_value_after("SkipWFMCheck").as_deref(), Some("ItemSell_NotFound"));
        assert_eq!(sale.get_value_after("SetDate").as_deref(), Some(date));
        let purchase = item_flags(Direction::Purchase, date);
        assert_eq!(purchase.get_value_after("SkipWFMCheck"), None);
        assert_eq!(purchase.get_value_after("SetDate").as_deref(), Some(date));
    }

    struct Recorder {
        fail_on: &'static str,
        seen: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl ItemApplier for Recorder {
        async fn apply_item(&self, _direction: Direction, item: &ResolvedItem, _player: &str, _detected_at: &str) -> Result<(), Error> {
            self.seen.lock().unwrap().push(item.slug.clone());
            if item.slug == self.fail_on {
                return Err(Error::new("HandleItem", "boom", get_location!()));
            }
            Ok(())
        }
    }

    fn item(slug: &str) -> ResolvedItem {
        ResolvedItem {
            name: slug.into(),
            slug: slug.into(),
            wfm_id: format!("id_{slug}"),
            item_name: slug.into(),
            sub_type: None,
            quantity: 2,
            price: 10,
            matched_by: "name".into(),
        }
    }

    #[tokio::test]
    async fn apply_items_stops_at_the_first_failure_and_reports_what_was_done() {
        let recorder = Recorder { fail_on: "b", seen: Mutex::new(Vec::new()) };
        let items = [item("a"), item("b"), item("c")];
        let failure = apply_items(&recorder, Direction::Sale, &items, "PlayerA", "2026-09-15T10:00:00Z").await.err().unwrap();
        assert_eq!(failure.completed, vec!["a x2".to_string()]);
        assert_eq!(failure.error.component, "HandleItem");
        assert_eq!(*recorder.seen.lock().unwrap(), vec!["a".to_string(), "b".to_string()], "c is never attempted");

        let ok = Recorder { fail_on: "none", seen: Mutex::new(Vec::new()) };
        assert!(apply_items(&ok, Direction::Purchase, &items, "PlayerA", "2026-09-15T10:00:00Z").await.is_ok());
        assert_eq!(ok.seen.lock().unwrap().len(), 3);
    }
}
```

- [ ] **Step 2: Run the apply tests**

```bash
cargo test -p qf_core --lib helper_link::trades::apply
```

Expected: both tests pass. If `HandlerApplier` doesn't compile because a handler future isn't `Send`, check the handler signature with `grep -n "pub async fn handle_item\b" -A10 crates/qf_core/src/handlers/stock_item.rs`. The RPC route already awaits these handlers inside axum, so they are `Send`.

- [ ] **Step 3: Write the failing pipeline tests**

Append to `crates/qf_core/src/helper_link/trades/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use async_trait::async_trait;
    use utils::get_location;

    use crate::helper_link::trades::events::tests::sale_trade;
    use crate::helper_link::trades::resolve::tests::items;
    use crate::helper_link::trades::sets::PartsMap;
    use crate::trader::store::tests::db;

    #[derive(Default)]
    struct Fake {
        auto_trade: bool,
        parts: PartsMap,
        fail_on: Option<String>,
        /// (direction, slug, quantity, price, player, detected_at)
        applied: Mutex<Vec<(Direction, String, i64, i64, String, String)>>,
        notified: Mutex<Vec<(String, Option<String>)>>,
    }

    fn fake(auto_trade: bool) -> Fake {
        Fake {
            auto_trade,
            parts: PartsMap::from([(
                "wolf_sledge_set".to_string(),
                vec!["wolf_sledge_blueprint".into(), "wolf_sledge_motor".into(), "wolf_sledge_head".into(), "wolf_sledge_handle".into()],
            )]),
            ..Default::default()
        }
    }

    #[async_trait]
    impl ItemApplier for Fake {
        async fn apply_item(&self, direction: Direction, item: &ResolvedItem, player: &str, detected_at: &str) -> Result<(), Error> {
            if self.fail_on.as_deref() == Some(item.slug.as_str()) {
                return Err(Error::new("HandleItem", "WFM rejected the order", get_location!()));
            }
            self.applied.lock().unwrap().push((direction, item.slug.clone(), item.quantity, item.price, player.into(), detected_at.into()));
            Ok(())
        }
    }

    impl TradeEnv for Fake {
        fn auto_trade(&self) -> bool {
            self.auto_trade
        }
        fn tradable_items(&self) -> Vec<CacheTradableItem> {
            items()
        }
        fn overrides(&self) -> Overrides {
            Overrides::default()
        }
        fn own_price(&self, _item: &ResolvedItem, _order_type: OrderType) -> Option<i64> {
            None
        }
        fn sets(&self) -> &dyn SetSource {
            &self.parts
        }
        fn applier(&self) -> &dyn ItemApplier {
            self
        }
        fn notify(&self, event: &HelperEvent) {
            self.notified.lock().unwrap().push((event.status.clone(), event.reason.clone()));
        }
    }

    fn raw(name: &str, quantity: i64, rank: Option<i64>) -> RawItem {
        RawItem { name: name.into(), quantity, rank }
    }

    fn trade(offered: Vec<RawItem>, received: Vec<RawItem>) -> RawTrade {
        RawTrade { player_name: "PlayerA".into(), ee_timestamp: "422.424".into(), offered, received }
    }

    fn incoming(id: char, trade: RawTrade) -> IncomingTrade {
        IncomingTrade { event_id: id.to_string().repeat(64), detected_at: "2026-09-15T12:00:00+02:00".into(), trade }
    }

    fn now() -> DateTime<Utc> {
        crate::collector::parse_ts("2026-09-15T10:00:05Z").unwrap()
    }

    fn outcome(status: &str, reason: Option<&str>) -> Outcome {
        Outcome { status: status.into(), reason: reason.map(str::to_string) }
    }

    #[test]
    fn validate_wants_lowercase_hex_ids_and_rfc3339_times() {
        let ok = incoming('a', sale_trade());
        assert_eq!(validate(&ok).unwrap(), crate::collector::parse_ts("2026-09-15T10:00:00Z").unwrap());
        assert!(validate(&IncomingTrade { event_id: "A".repeat(64), ..ok.clone() }).is_err());
        assert!(validate(&IncomingTrade { event_id: "a".repeat(63), ..ok.clone() }).is_err());
        assert!(validate(&IncomingTrade { event_id: "g".repeat(64), ..ok.clone() }).is_err());
        assert!(validate(&IncomingTrade { detected_at: "yesterday".into(), ..ok }).is_err());
    }

    #[tokio::test]
    async fn a_resolved_sale_is_applied_with_the_whole_platinum_and_the_game_time() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('a', sale_trade()), now()).await.unwrap();
        assert_eq!(result, outcome(events::APPLIED, None));
        assert_eq!(
            *env.applied.lock().unwrap(),
            vec![(Direction::Sale, "arcane_nullifier".to_string(), 1, 70, "PlayerB".to_string(), "2026-09-15T10:00:00Z".to_string())]
        );
        let stored = events::get(&conn, &"a".repeat(64)).await.unwrap().unwrap();
        assert_eq!((stored.status.as_str(), stored.reason.as_deref()), (events::APPLIED, None));
        assert_eq!((stored.device_name.as_str(), stored.received_at.as_str()), ("gaming-pc", "2026-09-15T10:00:05Z"));
        let resolution = stored.resolution.unwrap();
        assert_eq!(resolution.items[0].sub_type, Some(SubType::rank(5)));
        assert_eq!(*env.notified.lock().unwrap(), vec![(events::APPLIED.to_string(), None)]);
    }

    #[tokio::test]
    async fn a_purchase_of_every_part_applies_the_set() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let parts = trade(
            vec![raw("Platinum", 30, None)],
            vec![raw("Wolf Sledge Blueprint", 1, None), raw("Wolf Sledge Motor", 1, None), raw("Wolf Sledge Head", 1, None), raw("Wolf Sledge Handle", 1, None)],
        );
        assert_eq!(handle_incoming(&conn, &env, "gaming-pc", incoming('b', parts), now()).await.unwrap().status, events::APPLIED);
        let applied = env.applied.lock().unwrap();
        assert_eq!(applied.len(), 1);
        assert_eq!((applied[0].0, applied[0].1.as_str(), applied[0].2, applied[0].3), (Direction::Purchase, "wolf_sledge_set", 1, 30));
        let stored = events::get(&conn, &"b".repeat(64)).await.unwrap().unwrap();
        assert_eq!(stored.resolution.unwrap().items[0].matched_by, "set");
    }

    #[tokio::test]
    async fn unresolved_names_park_the_event_without_applying() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let mixed = trade(vec![raw("Platinum", 40, None)], vec![raw("Wolf Sledge Handle", 1, None), raw("Mystery Thing", 2, None)]);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('c', mixed), now()).await.unwrap();
        assert_eq!(result, outcome(events::NEEDS_REVIEW, Some("unresolved: Mystery Thing")));
        assert!(env.applied.lock().unwrap().is_empty());
        let stored = events::get(&conn, &"c".repeat(64)).await.unwrap().unwrap();
        let resolution = stored.resolution.unwrap();
        assert_eq!((resolution.direction, resolution.platinum, resolution.items.len()), (Some(Direction::Purchase), 40, 1));
        assert_eq!(*env.notified.lock().unwrap(), vec![(events::NEEDS_REVIEW.to_string(), Some("unresolved: Mystery Thing".to_string()))]);
    }

    #[tokio::test]
    async fn auto_trade_off_parks_every_event() {
        let (_dir, conn) = db().await;
        let env = fake(false);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('d', sale_trade()), now()).await.unwrap();
        assert_eq!(result, outcome(events::NEEDS_REVIEW, Some(AUTO_TRADE_OFF)));
        assert!(env.applied.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_platinum_side_and_platinum_only_trades_are_parked() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let swap = trade(vec![raw("Adaptation", 1, Some(10))], vec![raw("Primed Firestorm", 1, Some(10))]);
        assert_eq!(handle_incoming(&conn, &env, "gaming-pc", incoming('e', swap), now()).await.unwrap(), outcome(events::NEEDS_REVIEW, Some(NO_PLATINUM_SIDE)));
        let stored = events::get(&conn, &"e".repeat(64)).await.unwrap().unwrap();
        assert_eq!(stored.resolution.unwrap().direction, None);

        let gift = trade(vec![raw("Platinum", 5, None)], vec![]);
        assert_eq!(handle_incoming(&conn, &env, "gaming-pc", incoming('f', gift), now()).await.unwrap(), outcome(events::NEEDS_REVIEW, Some(NO_GOODS)));
        assert!(env.applied.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_known_event_id_is_a_duplicate_and_changes_nothing() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        handle_incoming(&conn, &env, "gaming-pc", incoming('1', sale_trade()), now()).await.unwrap();
        let again = handle_incoming(&conn, &env, "gaming-pc", incoming('1', sale_trade()), now()).await.unwrap();
        assert_eq!(again, outcome(DUPLICATE, None));
        assert_eq!(env.applied.lock().unwrap().len(), 1);
        assert_eq!(events::list(&conn, None, 1, 10).await.unwrap().total, 1);
    }

    #[tokio::test]
    async fn invalid_input_is_an_error_and_stores_nothing() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let bad = IncomingTrade { event_id: "nope".into(), ..incoming('2', sale_trade()) };
        assert!(handle_incoming(&conn, &env, "gaming-pc", bad, now()).await.is_err());
        assert_eq!(events::list(&conn, None, 1, 10).await.unwrap().total, 0);
    }

    #[tokio::test]
    async fn a_failing_handler_parks_the_event_as_apply_failed_and_keeps_the_resolution() {
        let (_dir, conn) = db().await;
        let env = Fake { fail_on: Some("arcane_nullifier".into()), ..fake(true) };
        let two = trade(vec![raw("Platinum", 100, None)], vec![raw("Adaptation", 1, Some(10)), raw("Arcane Nullifier", 1, Some(5))]);
        let result = handle_incoming(&conn, &env, "gaming-pc", incoming('3', two), now()).await.unwrap();
        assert_eq!(result, outcome(events::NEEDS_REVIEW, Some("apply_failed: HandleItem")));
        let applied = env.applied.lock().unwrap();
        assert_eq!((applied.len(), applied[0].1.as_str(), applied[0].3), (1, "adaptation", 50));
        let stored = events::get(&conn, &"3".repeat(64)).await.unwrap().unwrap();
        assert_eq!(stored.resolution.unwrap().items.len(), 2);
    }

    #[tokio::test]
    async fn review_applies_the_given_items_once_and_marks_the_event_reviewed() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let mixed = trade(vec![raw("Platinum", 40, None)], vec![raw("Wolf Sledge Handle", 1, None), raw("Mystery Thing", 2, None)]);
        handle_incoming(&conn, &env, "gaming-pc", incoming('4', mixed), now()).await.unwrap();
        let id = "4".repeat(64);
        let items = vec![
            ReviewItem { slug: "wolf_sledge_handle".into(), sub_type: None, quantity: 1, price: 10 },
            ReviewItem { slug: "adaptation".into(), sub_type: Some(SubType::rank(10)), quantity: 2, price: 30 },
        ];
        let reviewed = apply_reviewed(&conn, &env, &id, items.clone(), now()).await.unwrap();
        assert_eq!((reviewed.status.as_str(), reviewed.reason.as_deref()), (events::APPLIED, None));
        assert_eq!(reviewed.reviewed_at.as_deref(), Some("2026-09-15T10:00:05Z"));
        let resolution = reviewed.resolution.clone().unwrap();
        assert!(resolution.items.iter().all(|i| i.matched_by == "review"));
        assert_eq!(resolution.items[1].item_name, "Adaptation");
        assert_eq!(events::get(&conn, &id).await.unwrap().unwrap(), reviewed);
        assert_eq!(
            env.applied.lock().unwrap().iter().map(|a| (a.0, a.1.clone(), a.2, a.3)).collect::<Vec<_>>(),
            vec![(Direction::Purchase, "wolf_sledge_handle".to_string(), 1, 10), (Direction::Purchase, "adaptation".to_string(), 2, 30)]
        );

        assert!(apply_reviewed(&conn, &env, &id, items, now()).await.is_err(), "an applied event can't be applied again");
        assert!(ignore(&conn, &id, now()).await.is_err(), "nor ignored");
        assert_eq!(env.applied.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn review_is_refused_without_a_direction_or_for_bad_items_and_ignore_works() {
        let (_dir, conn) = db().await;
        let env = fake(true);
        let swap = trade(vec![raw("Adaptation", 1, Some(10))], vec![raw("Primed Firestorm", 1, Some(10))]);
        handle_incoming(&conn, &env, "gaming-pc", incoming('5', swap), now()).await.unwrap();
        let swap_id = "5".repeat(64);
        let one = vec![ReviewItem { slug: "adaptation".into(), sub_type: None, quantity: 1, price: 0 }];
        assert!(apply_reviewed(&conn, &env, &swap_id, one.clone(), now()).await.is_err(), "no platinum side");

        let parked = trade(vec![raw("Platinum", 40, None)], vec![raw("Mystery Thing", 1, None)]);
        handle_incoming(&conn, &env, "gaming-pc", incoming('6', parked), now()).await.unwrap();
        let parked_id = "6".repeat(64);
        let unknown = vec![ReviewItem { slug: "nope".into(), sub_type: None, quantity: 1, price: 40 }];
        assert!(apply_reviewed(&conn, &env, &parked_id, unknown, now()).await.is_err());
        let zero = vec![ReviewItem { slug: "adaptation".into(), sub_type: None, quantity: 0, price: 40 }];
        assert!(apply_reviewed(&conn, &env, &parked_id, zero, now()).await.is_err());
        assert!(apply_reviewed(&conn, &env, &parked_id, vec![], now()).await.is_err());
        assert!(apply_reviewed(&conn, &env, &"7".repeat(64), one, now()).await.is_err(), "unknown event");
        assert_eq!(events::get(&conn, &parked_id).await.unwrap().unwrap().status, events::NEEDS_REVIEW);
        assert!(env.applied.lock().unwrap().is_empty());

        let ignored = ignore(&conn, &swap_id, now()).await.unwrap();
        assert_eq!((ignored.status.as_str(), ignored.reason.as_deref()), (events::IGNORED, Some(REVIEWED)));
        assert_eq!(events::get(&conn, &swap_id).await.unwrap().unwrap(), ignored);
    }
}
```

Run `cargo test -p qf_core --lib helper_link::trades::tests`. Expected: compile errors for `TradeEnv`, `handle_incoming` and the other missing names.

- [ ] **Step 4: Implement the pipeline**

In `crates/qf_core/src/helper_link/trades/mod.rs`, replace the header, from the doc comment down to and including `pub use qf_log_parser::{RawItem, RawTrade};`, with:

```rust
//! Trade events reported by `qf-helper` (spec §5.8, amendments E1–E9).

pub mod apply;
pub mod events;
pub mod resolve;
pub mod sets;
pub mod split;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use service::sea_orm::DatabaseConnection;
use utils::{get_location, warning, Error, LoggerOptions, SubType};
use wf_market::enums::OrderType;

pub use qf_log_parser::{RawItem, RawTrade};

use crate::cache::types::CacheTradableItem;
use crate::collector::ts;
use crate::trader::price_source::key_of;
use apply::{apply_items, ItemApplier};
use events::{HelperEvent, APPLIED, IGNORED, NEEDS_REVIEW};
use resolve::{resolve_trade, ItemIndex, Overrides};
use sets::{fold_sets, set_candidates, SetSource};
use split::{medians, price_items, weights_for};

/// `status` for a replayed event_id; never stored (amendment E4).
pub const DUPLICATE: &str = "duplicate";
/// Reason while an event is being applied; left behind only if the server stops mid-way.
pub const APPLYING: &str = "applying";
pub const AUTO_TRADE_OFF: &str = "auto_trade_off";
pub const NO_GOODS: &str = "no_goods";
pub const NO_PLATINUM_SIDE: &str = "no_platinum_side";
pub const REVIEWED: &str = "reviewed";
```

Keep `IncomingTrade`, `Direction`, `ResolvedItem` and `Resolution` from Task 4 as they are. Then, between `Resolution` and the `#[cfg(test)] mod tests` from Step 3, add:

```rust
/// Reply to `POST /helper/trade` (amendment E4).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Outcome {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl Outcome {
    fn duplicate() -> Self {
        Self { status: DUPLICATE.into(), reason: None }
    }

    fn of(event: &HelperEvent) -> Self {
        Self { status: event.status.clone(), reason: event.reason.clone() }
    }
}

/// Everything the pipeline needs besides the database. `live::LiveEnv` is the real one.
pub trait TradeEnv: Send + Sync {
    /// `live_scraper.general.auto_trade`, the kill switch (amendment E8).
    fn auto_trade(&self) -> bool;
    fn tradable_items(&self) -> Vec<CacheTradableItem>;
    fn overrides(&self) -> Overrides;
    /// Price of the user's own cached WFM order for this item, sub type and order type.
    fn own_price(&self, item: &ResolvedItem, order_type: OrderType) -> Option<i64>;
    fn sets(&self) -> &dyn SetSource;
    fn applier(&self) -> &dyn ItemApplier;
    /// Called once for each stored outcome: applied, needs_review or apply_failed.
    fn notify(&self, event: &HelperEvent);
}

/// One row of the Review modal (amendment E9).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReviewItem {
    pub slug: String,
    #[serde(default)]
    pub sub_type: Option<SubType>,
    pub quantity: i64,
    pub price: i64,
}

/// Checks the E4 body beyond its shape. Returns `detected_at` in UTC.
pub fn validate(incoming: &IncomingTrade) -> Result<DateTime<Utc>, String> {
    let id = &incoming.event_id;
    if id.len() != 64 || !id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) {
        return Err("event_id must be 64 lowercase hex characters".into());
    }
    DateTime::parse_from_rfc3339(&incoming.detected_at)
        .map(|at| at.with_timezone(&Utc))
        .map_err(|e| format!("detected_at is not RFC 3339: {e}"))
}

async fn price(conn: &DatabaseConnection, env: &dyn TradeEnv, resolution: &mut Resolution) -> Result<(), Error> {
    let Some(direction) = resolution.direction else { return Ok(()) };
    let medians = medians(conn, &resolution.items).await?;
    let weights = weights_for(
        &resolution.items,
        direction,
        &|item, order_type| env.own_price(item, order_type),
        &|item| medians.get(&(item.wfm_id.clone(), key_of(&item.sub_type))).copied(),
    );
    price_items(&mut resolution.items, resolution.platinum, &weights);
    Ok(())
}

/// Classify, resolve, fold sets and price (amendments E6–E8). The second value is the review
/// reason; `None` means the event can be applied.
pub async fn resolve_event(conn: &DatabaseConnection, env: &dyn TradeEnv, trade: &RawTrade) -> Result<(Resolution, Option<String>), Error> {
    let index = ItemIndex::from_items(env.tradable_items());
    let resolved = match resolve_trade(trade, &index, &env.overrides()) {
        Ok(resolved) => resolved,
        Err(reason) => return Ok((Resolution::default(), Some(reason))),
    };
    let mut resolution = resolved.resolution;
    let candidates = set_candidates(&resolution.items, &index);
    if !candidates.is_empty() {
        let parts = env.sets().parts_for(&candidates, &index).await;
        resolution.items = fold_sets(resolution.items, &candidates, &parts, &index);
    }
    price(conn, env, &mut resolution).await?;
    let reason = if !resolved.unresolved.is_empty() {
        Some(format!("unresolved: {}", resolved.unresolved.join(", ")))
    } else if resolution.items.is_empty() {
        Some(NO_GOODS.to_string())
    } else if !env.auto_trade() {
        Some(AUTO_TRADE_OFF.to_string())
    } else {
        None
    };
    Ok((resolution, reason))
}

/// Applies `resolution.items` and records the result on `event`. The outer error is the database;
/// the inner one is the handler failure, already recorded as `apply_failed`.
async fn apply_and_record(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    event: &mut HelperEvent,
    direction: Direction,
    resolution: Resolution,
    reviewed_at: Option<DateTime<Utc>>,
) -> Result<Result<(), Error>, Error> {
    match apply_items(env.applier(), direction, &resolution.items, &event.payload.player_name, &event.detected_at).await {
        Ok(()) => {
            events::set_status(conn, &event.event_id, APPLIED, None, Some(&resolution), reviewed_at).await?;
            event.status = APPLIED.into();
            event.reason = None;
            event.resolution = Some(resolution);
            event.reviewed_at = reviewed_at.map(ts);
            env.notify(event);
            Ok(Ok(()))
        }
        Err(failure) => {
            let reason = format!("apply_failed: {}", failure.error.component);
            let done = if failure.completed.is_empty() { "nothing".to_string() } else { failure.completed.join(", ") };
            warning(
                "HelperLink:Trade",
                format!("Trade {} with {} stopped: {}. Already applied: {done}", event.event_id, event.payload.player_name, failure.error.message),
                &LoggerOptions::default(),
            );
            events::set_status(conn, &event.event_id, NEEDS_REVIEW, Some(&reason), Some(&resolution), None).await?;
            event.status = NEEDS_REVIEW.into();
            event.reason = Some(reason);
            event.resolution = Some(resolution);
            env.notify(event);
            Ok(Err(failure.error))
        }
    }
}

/// `POST /helper/trade`, processed inside the request (amendments E4–E8).
pub async fn handle_incoming(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    device_name: &str,
    incoming: IncomingTrade,
    now: DateTime<Utc>,
) -> Result<Outcome, Error> {
    let detected_at = validate(&incoming).map_err(|message| Error::new("HelperLink:Trade", message, get_location!()))?;
    if events::exists(conn, &incoming.event_id).await? {
        return Ok(Outcome::duplicate());
    }
    let (resolution, review_reason) = resolve_event(conn, env, &incoming.trade).await?;
    let mut event = HelperEvent {
        event_id: incoming.event_id,
        device_name: device_name.to_string(),
        received_at: ts(now),
        detected_at: ts(detected_at),
        status: NEEDS_REVIEW.into(),
        reason: Some(review_reason.clone().unwrap_or_else(|| APPLYING.to_string())),
        payload: incoming.trade,
        resolution: Some(resolution.clone()),
        reviewed_at: None,
    };
    if let Err(error) = events::insert(conn, &event).await {
        // A concurrent request with the same event_id won the insert.
        if events::exists(conn, &event.event_id).await? {
            return Ok(Outcome::duplicate());
        }
        return Err(error);
    }
    match (review_reason, resolution.direction) {
        (None, Some(direction)) => {
            // A handler failure is already recorded on the event; the helper just hears needs_review.
            let _ = apply_and_record(conn, env, &mut event, direction, resolution, None).await?;
        }
        _ => env.notify(&event),
    }
    Ok(Outcome::of(&event))
}

fn review_error(message: impl Into<String>) -> Error {
    Error::new("HelperLink:Review", message, get_location!())
}

async fn reviewable(conn: &DatabaseConnection, event_id: &str) -> Result<HelperEvent, Error> {
    let event = events::get(conn, event_id).await?.ok_or_else(|| review_error(format!("Unknown trade event {event_id}")))?;
    if event.status != NEEDS_REVIEW {
        return Err(review_error(format!("This trade is already {}; only trades that need review can change", event.status)));
    }
    Ok(event)
}

/// `helper_trade_apply` (amendment E9): the user's items, the stored direction, player and time.
pub async fn apply_reviewed(
    conn: &DatabaseConnection,
    env: &dyn TradeEnv,
    event_id: &str,
    items: Vec<ReviewItem>,
    now: DateTime<Utc>,
) -> Result<HelperEvent, Error> {
    let mut event = reviewable(conn, event_id).await?;
    let mut resolution = event.resolution.clone().unwrap_or_default();
    let Some(direction) = resolution.direction else {
        return Err(review_error("This trade has no platinum side, so it can only be ignored"));
    };
    if items.is_empty() {
        return Err(review_error("Add at least one item"));
    }
    let index = ItemIndex::from_items(env.tradable_items());
    let mut resolved = Vec::with_capacity(items.len());
    for item in items {
        let Some(found) = index.by_slug(&item.slug) else {
            return Err(review_error(format!("Unknown item {}", item.slug)));
        };
        if item.quantity < 1 || item.price < 0 {
            return Err(review_error(format!("{}: quantity must be at least 1 and price at least 0", found.name)));
        }
        resolved.push(ResolvedItem {
            name: found.name.clone(),
            slug: found.wfm_url.clone(),
            wfm_id: found.wfm_id.clone(),
            item_name: found.name.clone(),
            sub_type: item.sub_type,
            quantity: item.quantity,
            price: item.price,
            matched_by: "review".into(),
        });
    }
    resolution.items = resolved;
    apply_and_record(conn, env, &mut event, direction, resolution, Some(now)).await??;
    Ok(event)
}

/// `helper_trade_ignore` (amendment E9).
pub async fn ignore(conn: &DatabaseConnection, event_id: &str, now: DateTime<Utc>) -> Result<HelperEvent, Error> {
    let mut event = reviewable(conn, event_id).await?;
    events::set_status(conn, event_id, IGNORED, Some(REVIEWED), event.resolution.as_ref(), Some(now)).await?;
    event.status = IGNORED.into();
    event.reason = Some(REVIEWED.into());
    event.reviewed_at = Some(ts(now));
    Ok(event)
}
```

Task 5's `classify` writes `"no_platinum_side".to_string()`, which equals `NO_PLATINUM_SIDE`, so it needs no change.

- [ ] **Step 5: Run the tests**

```bash
cargo test -p qf_core --lib helper_link::trades
```

Expected: all apply, pipeline, events, resolve, sets and split tests pass. If `a_resolved_sale_is_applied…` fails on `detected_at`, check that `validate` converts `+02:00` to UTC before `ts`.

- [ ] **Step 6: Commit**

```bash
git add crates/qf_core
git commit -m "feat(core): apply helper trades through the stock and wish-list handlers

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 7: Live environment and trade RPC commands

**Files:**
- Create: `crates/qf_core/src/helper_link/trades/live.rs`
- Modify: `crates/qf_core/src/helper_link/trades/mod.rs` (module list), `crates/qf_core/src/commands/helper_link.rs`, `crates/qf_core/src/commands/rpc.rs`

**Interfaces:**
- Consumes: Task 6 (`TradeEnv`, `ReviewItem`, `apply_reviewed`, `ignore`, `apply::HandlerApplier`), `events::{list, EventPage, HelperEvent, APPLIED}`, `sets::cache()`, `resolve::{Overrides, OVERRIDES_FILE, PLATINUM}`, `utils::modules::states`, `utils::SubTypeExt`, `notify_gui!`, `send_event!`, `UIEvent::{RefreshStockItems, RefreshWishListItems, RefreshTransactions}`, `NotificationsSetting.on_new_trade`.
- Produces:
  - `live::LiveEnv` (unit struct, `impl TradeEnv`)
  - `live::trade_variables(&HelperEvent) -> HashMap<String, String>` with the upstream `on_new_trade` variables `<PLAYER_NAME> <TIME> <TR_TYPE> <TOTAL_PLAT> <OF_ITEMS> <RE_ITEMS> <OF_COUNT> <RE_COUNT>`
  - `live::toast_values(&HelperEvent) -> serde_json::Value` = `{ player_name, direction, platinum, items, reason }` (`direction` is `purchase`, `sale` or `trade`; `items` is the summed quantity)
  - RPC `helper_trades { status?: string, page, limit } -> EventPage`, `helper_trade_apply { eventId, items: [ReviewItem] } -> HelperEvent`, `helper_trade_ignore { eventId } -> HelperEvent`

- [ ] **Step 1: `live.rs` with tests for the pure parts**

Add `pub mod live;` after `pub mod events;` in `trades/mod.rs`.

`crates/qf_core/src/helper_link/trades/live.rs`:

```rust
//! The real `TradeEnv`: settings, caches, cached WFM orders, the handlers and notifications (amendment E8).

use std::collections::HashMap;

use serde_json::{json, Value};
use utils::{warning, LoggerOptions};
use wf_market::enums::OrderType;

use super::apply::{HandlerApplier, ItemApplier};
use super::events::{HelperEvent, APPLIED};
use super::resolve::{Overrides, OVERRIDES_FILE, PLATINUM};
use super::sets::{self, SetSource};
use super::{Direction, RawItem, ResolvedItem, TradeEnv};
use crate::cache::types::CacheTradableItem;
use crate::types::UIEvent;
use crate::utils::modules::states;
use crate::utils::SubTypeExt;
use crate::{notify_gui, send_event};

pub struct LiveEnv;

impl TradeEnv for LiveEnv {
    fn auto_trade(&self) -> bool {
        states::try_app_state().is_some_and(|app| app.settings.live_scraper.general.auto_trade)
    }

    fn tradable_items(&self) -> Vec<CacheTradableItem> {
        match states::cache_client().and_then(|cache| cache.tradable_item().get_items()) {
            Ok(items) => items,
            Err(e) => {
                warning("HelperLink:Items", format!("Tradable items unavailable: {}", e.message), &LoggerOptions::default());
                Vec::new()
            }
        }
    }

    fn overrides(&self) -> Overrides {
        Overrides::load(&crate::paths::get().data_dir.join(OVERRIDES_FILE))
    }

    fn own_price(&self, item: &ResolvedItem, order_type: OrderType) -> Option<i64> {
        let app = states::try_app_state()?;
        let sub_type: wf_market::types::SubType = SubTypeExt::from_entity(item.sub_type.clone());
        app.wfm_client.order().cache_orders().find_order(&item.wfm_id, &sub_type, order_type).map(|order| order.platinum as i64)
    }

    fn sets(&self) -> &dyn SetSource {
        sets::cache()
    }

    fn applier(&self) -> &dyn ItemApplier {
        &HandlerApplier
    }

    fn notify(&self, event: &HelperEvent) {
        let values = toast_values(event);
        if event.status == APPLIED {
            let source = json!({"source": "HelperLink:Trade"});
            send_event!(UIEvent::RefreshStockItems, source.clone());
            send_event!(UIEvent::RefreshWishListItems, source.clone());
            send_event!(UIEvent::RefreshTransactions, source);
            notify_gui!("on_trade_event", "green.7", "applied", values, json!({}));
            if let Some(app) = states::try_app_state() {
                app.settings.notifications.on_new_trade.send(
                    &trade_variables(event),
                    Some(json!({"event": "trade", "event_id": event.event_id, "status": event.status})),
                );
            }
        } else {
            notify_gui!("on_trade_event", "yellow", "needs_review", values, json!({"autoClose": false}));
        }
    }
}

fn item_lines(items: &[RawItem]) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.name != PLATINUM)
        .map(|item| match item.rank {
            Some(rank) => format!("{} x{} (rank {rank})", item.name, item.quantity),
            None => format!("{} x{}", item.name, item.quantity),
        })
        .collect()
}

/// The upstream `on_new_trade` variables, filled from a stored event.
pub fn trade_variables(event: &HelperEvent) -> HashMap<String, String> {
    let resolution = event.resolution.clone().unwrap_or_default();
    let offered = item_lines(&event.payload.offered);
    let received = item_lines(&event.payload.received);
    let kind = match resolution.direction {
        Some(Direction::Sale) => "Sale",
        Some(Direction::Purchase) => "Purchase",
        None => "Trade",
    };
    HashMap::from([
        ("<PLAYER_NAME>".to_string(), event.payload.player_name.clone()),
        ("<TIME>".to_string(), event.detected_at.clone()),
        ("<TR_TYPE>".to_string(), kind.to_string()),
        ("<TOTAL_PLAT>".to_string(), resolution.platinum.to_string()),
        ("<OF_COUNT>".to_string(), offered.len().to_string()),
        ("<RE_COUNT>".to_string(), received.len().to_string()),
        ("<OF_ITEMS>".to_string(), offered.join("\n")),
        ("<RE_ITEMS>".to_string(), received.join("\n")),
    ])
}

/// Values for the `on_trade_event.applied` and `on_trade_event.needs_review` toasts.
pub fn toast_values(event: &HelperEvent) -> Value {
    let resolution = event.resolution.clone().unwrap_or_default();
    json!({
        "player_name": event.payload.player_name,
        "direction": resolution.direction.map_or("trade", Direction::as_str),
        "platinum": resolution.platinum,
        "items": resolution.items.iter().map(|item| item.quantity).sum::<i64>(),
        "reason": event.reason.clone().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helper_link::trades::events::tests::event;
    use crate::helper_link::trades::Resolution;

    fn applied_sale() -> HelperEvent {
        let mut stored = event("e1", "2026-09-15T10:00:00Z", APPLIED);
        stored.resolution = Some(Resolution {
            direction: Some(Direction::Sale),
            platinum: 70,
            items: vec![ResolvedItem {
                name: "Arcane Nullifier".into(),
                slug: "arcane_nullifier".into(),
                wfm_id: "id_arcane_nullifier".into(),
                item_name: "Arcane Nullifier".into(),
                sub_type: Some(utils::SubType::rank(5)),
                quantity: 1,
                price: 70,
                matched_by: "name".into(),
            }],
            extras: vec![],
        });
        stored
    }

    #[test]
    fn trade_variables_follow_the_upstream_template() {
        let vars = trade_variables(&applied_sale());
        assert_eq!(vars["<PLAYER_NAME>"], "PlayerB");
        assert_eq!(vars["<TIME>"], "2026-09-15T10:00:00Z");
        assert_eq!(vars["<TR_TYPE>"], "Sale");
        assert_eq!(vars["<TOTAL_PLAT>"], "70");
        assert_eq!(vars["<OF_ITEMS>"], "Arcane Nullifier x1 (rank 5)");
        assert_eq!((vars["<OF_COUNT>"].as_str(), vars["<RE_COUNT>"].as_str(), vars["<RE_ITEMS>"].as_str()), ("1", "0", ""));
    }

    #[test]
    fn toast_values_name_the_direction_or_fall_back_to_trade() {
        let applied = toast_values(&applied_sale());
        assert_eq!(applied, json!({"player_name": "PlayerB", "direction": "sale", "platinum": 70, "items": 1, "reason": ""}));
        let mut parked = event("e2", "2026-09-15T10:00:00Z", "needs_review");
        parked.reason = Some("no_platinum_side".into());
        let values = toast_values(&parked);
        assert_eq!((values["direction"].as_str(), values["reason"].as_str()), (Some("trade"), Some("no_platinum_side")));
    }
}
```

If `states::cache_client().and_then(...)` doesn't compile, use the same two-step lock that `commands::cache::cache_get_tradable_items` uses: `cache_mutex().lock()`, then `tradable_item().get_items()`. The imports `crate::types::UIEvent` and `crate::send_event` match `crates/qf_core/src/trader/platform.rs`, which sends the same kind of events.

- [ ] **Step 2: RPC commands**

In `crates/qf_core/src/commands/helper_link.rs`, add to the imports:

```rust
use crate::helper_link::trades::{
    self,
    events::{self, EventPage, HelperEvent},
    live::LiveEnv,
    ReviewItem,
};
```

and append:

```rust
/// Newest first; `status` `None` or empty lists every event (amendment E9).
pub async fn helper_trades(status: Option<String>, page: i64, limit: i64) -> Result<EventPage, Error> {
    events::list(conn()?, status.as_deref().filter(|s| !s.is_empty()), page, limit).await
}

pub async fn helper_trade_apply(event_id: String, items: Vec<ReviewItem>) -> Result<HelperEvent, Error> {
    trades::apply_reviewed(conn()?, &LiveEnv, &event_id, items, Utc::now()).await
}

pub async fn helper_trade_ignore(event_id: String) -> Result<HelperEvent, Error> {
    trades::ignore(conn()?, &event_id, Utc::now()).await
}
```

In `crates/qf_core/src/commands/rpc.rs`, add `use crate::helper_link::trades::ReviewItem;` after `use crate::handlers::ItemEntity;`, then replace:

```rust
    helper_device_revoke => helper_link::helper_device_revoke { id: i64 },
}
```

with:

```rust
    helper_device_revoke => helper_link::helper_device_revoke { id: i64 },
    helper_trades => helper_link::helper_trades { status: Option<String>, page: i64, limit: i64 },
    helper_trade_apply => helper_link::helper_trade_apply { event_id: String, items: Vec<ReviewItem> },
    helper_trade_ignore => helper_link::helper_trade_ignore { event_id: String },
}
```

In the same file's tests, after `helper_device_commands_are_routable_and_validate_args`, add:

```rust
    #[tokio::test]
    async fn helper_trade_commands_are_routable_and_validate_args() {
        for name in ["helper_trades", "helper_trade_apply", "helper_trade_ignore"] {
            assert!(COMMANDS.contains(&name), "{name}");
        }
        assert!(dispatch("helper_trades", json!({"page": 1})).await.unwrap().is_err(), "limit is required");
        assert!(dispatch("helper_trade_apply", json!({"eventId": "x"})).await.unwrap().is_err(), "items are required");
        assert!(
            dispatch("helper_trade_apply", json!({"eventId": "x", "items": [{"slug": "a", "quantity": "one", "price": 1}]})).await.unwrap().is_err(),
            "quantity must be a number"
        );
        assert!(dispatch("helper_trade_ignore", json!({})).await.unwrap().is_err(), "eventId is required");
    }
```

- [ ] **Step 3: Run the tests**

```bash
cargo test -p qf_core --lib helper_link::trades::live
cargo test -p qf_core --lib commands::rpc
python3 scripts/check-rpc-commands.py
```

Expected: the live and RPC tests pass, including `allowlist_has_no_removed_features`. The script still reports 0 missing.

- [ ] **Step 4: Commit**

```bash
git add crates/qf_core
git commit -m "feat(core): expose helper trades over rpc with live notifications

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 8: `POST /helper/trade`

**Files:**
- Modify: `crates/qf-server/Cargo.toml`, `crates/qf-server/src/routes.rs`, `crates/qf-server/src/main.rs`, `crates/qf-server/tests/http.rs`

**Interfaces:**
- Consumes: `trades::{validate, handle_incoming, IncomingTrade, TradeEnv, Outcome}` (Task 6), `trades::live::LiveEnv` (Task 7), `require_device_key` and `DeviceIdentity` (phase 4a).
- Produces:
  - `ServerState.trade_env: Arc<dyn TradeEnv>`
  - `POST /helper/trade`, which answers:
    - `200 {status, reason?}` for a processed or duplicate event
    - `400 {component, message}` for a body that isn't valid JSON of the E4 shape, or has a bad `event_id` or `detected_at`
    - `401` from the middleware
    - `500` with the error JSON for a database failure

An invalid body is answered with 400, not axum's `Json` rejection. That rejection gives 422 for a missing field, and E4 says 400.

- [ ] **Step 1: Write the failing route tests**

In `crates/qf-server/Cargo.toml` `[dev-dependencies]`, add:

```toml
async-trait = "0.1"
utils = { path = "../utils" }
wf-market = { path = "../wf-market" }
```

In `crates/qf-server/tests/http.rs`, replace:

```rust
use qf_core::helper_link::{keys, presence};
```

with:

```rust
use std::collections::HashMap;

use async_trait::async_trait;
use qf_core::cache::types::{CacheTradableItem, SubType as CacheSubType};
use qf_core::helper_link::trades::{
    apply::ItemApplier,
    events::{self, HelperEvent},
    resolve::Overrides,
    sets::{PartsMap, SetSource},
    Direction, ResolvedItem, TradeEnv,
};
use qf_core::helper_link::{keys, presence};
use serde_json::{json, Value};
use wf_market::enums::OrderType;
```

In `fn app()`, add `trade_env: Arc::new(FakeTrades::default()),` after `db: None,`. Then, after `helper_app`, add:

```rust
/// Resolves "Arcane Nullifier" only and applies everything without touching handlers.
#[derive(Default)]
struct FakeTrades {
    parts: PartsMap,
}

#[async_trait]
impl ItemApplier for FakeTrades {
    async fn apply_item(&self, _direction: Direction, _item: &ResolvedItem, _player: &str, _detected_at: &str) -> Result<(), utils::Error> {
        Ok(())
    }
}

impl TradeEnv for FakeTrades {
    fn auto_trade(&self) -> bool {
        true
    }
    fn tradable_items(&self) -> Vec<CacheTradableItem> {
        vec![CacheTradableItem {
            name: "Arcane Nullifier".into(),
            unique_name: String::new(),
            wfm_id: "id_arcane_nullifier".into(),
            wfm_url: "arcane_nullifier".into(),
            trade_tax: 0,
            mr_requirement: 0,
            tags: vec!["arcane_enhancement".into()],
            icon: String::new(),
            bulk_tradable: false,
            sub_type: Some(CacheSubType { max_rank: Some(5), variants: None, amber_stars: None, cyan_stars: None }),
            variant_to_unique_name: HashMap::new(),
        }]
    }
    fn overrides(&self) -> Overrides {
        Overrides::default()
    }
    fn own_price(&self, _item: &ResolvedItem, _order_type: OrderType) -> Option<i64> {
        None
    }
    fn sets(&self) -> &dyn SetSource {
        &self.parts
    }
    fn applier(&self) -> &dyn ItemApplier {
        self
    }
    fn notify(&self, _event: &HelperEvent) {}
}

fn trade(key: Option<&str>, body: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/helper/trade")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(key) = key {
        req = req.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    req.body(Body::from(body.to_string())).unwrap()
}

fn sale_body(event_id: &str, item: &str) -> String {
    json!({
        "event_id": event_id,
        "detected_at": "2026-09-15T10:00:00Z",
        "trade": {
            "player_name": "PlayerB",
            "ee_timestamp": "1170.388",
            "offered": [{"name": item, "quantity": 1, "rank": 5}],
            "received": [{"name": "Platinum", "quantity": 70}]
        }
    })
    .to_string()
}

async fn json_of(res: axum::response::Response) -> Value {
    serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn trade_requires_a_valid_device_key() {
    let (app, _, _dir) = helper_app().await;
    let res = app.clone().oneshot(trade(None, &sale_body(&"a".repeat(64), "Arcane Nullifier"))).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = app.oneshot(trade(Some("qfh_0000"), &sale_body(&"a".repeat(64), "Arcane Nullifier"))).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn malformed_trade_bodies_are_400() {
    let (app, state, _dir) = helper_app().await;
    let created = keys::create(state.db.as_ref().unwrap(), "trade-400-pc", Utc::now()).await.unwrap();
    for body in ["not json".to_string(), "{}".to_string(), sale_body("ABC", "Arcane Nullifier")] {
        let res = app.clone().oneshot(trade(Some(&created.key), &body)).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST, "{body}");
        assert_eq!(json_of(res).await["component"], "Helper");
    }
}

#[tokio::test]
async fn a_resolved_trade_is_applied_and_a_replay_is_a_duplicate() {
    let (app, state, _dir) = helper_app().await;
    let db = state.db.as_ref().unwrap();
    let created = keys::create(db, "trade-pc", Utc::now()).await.unwrap();
    let id = "a".repeat(64);

    let res = app.clone().oneshot(trade(Some(&created.key), &sale_body(&id, "Arcane Nullifier"))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_of(res).await, json!({"status": "applied"}));
    let stored = events::get(db, &id).await.unwrap().unwrap();
    assert_eq!((stored.status.as_str(), stored.device_name.as_str()), ("applied", "trade-pc"));

    let res = app.oneshot(trade(Some(&created.key), &sale_body(&id, "Arcane Nullifier"))).await.unwrap();
    assert_eq!(json_of(res).await, json!({"status": "duplicate"}));
    assert_eq!(events::list(db, None, 1, 10).await.unwrap().total, 1);
}

#[tokio::test]
async fn an_unresolved_trade_needs_review() {
    let (app, state, _dir) = helper_app().await;
    let created = keys::create(state.db.as_ref().unwrap(), "review-pc", Utc::now()).await.unwrap();
    let res = app.oneshot(trade(Some(&created.key), &sale_body(&"b".repeat(64), "Mystery Thing"))).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(json_of(res).await, json!({"status": "needs_review", "reason": "unresolved: Mystery Thing"}));
}
```

Run `cargo test -p qf-server`. Expected: compile error, because `ServerState` has no field `trade_env`.

- [ ] **Step 2: The route**

In `crates/qf-server/src/routes.rs`, replace:

```rust
use qf_core::helper_link::{
    keys::{self, DeviceIdentity},
    presence::{self, Heartbeat},
};
```

with:

```rust
use qf_core::helper_link::{
    keys::{self, DeviceIdentity},
    presence::{self, Heartbeat},
    trades::{self, IncomingTrade, TradeEnv},
};
```

Replace:

```rust
    /// `None` only in tests that don't exercise the helper routes.
    pub db: Option<qf_core::db::DatabaseConnection>,
}
```

with:

```rust
    /// `None` only in tests that don't exercise the helper routes.
    pub db: Option<qf_core::db::DatabaseConnection>,
    /// Resolves and applies helper trades (amendment E8); tests use a fake.
    pub trade_env: Arc<dyn TradeEnv>,
}
```

Replace:

```rust
        .route("/helper/heartbeat", post(helper_heartbeat))
```

with:

```rust
        .route("/helper/heartbeat", post(helper_heartbeat))
        .route("/helper/trade", post(helper_trade))
```

After `helper_heartbeat`, add:

```rust
/// Amendment E4. The body is parsed by hand so a wrong shape is 400, as the helper expects.
async fn helper_trade(State(state): State<ServerState>, Extension(device): Extension<DeviceIdentity>, body: Bytes) -> Response {
    let bad_request =
        |message: String| (StatusCode::BAD_REQUEST, Json(json!({"component": "Helper", "message": message}))).into_response();
    let incoming: IncomingTrade = match serde_json::from_slice(&body) {
        Ok(incoming) => incoming,
        Err(e) => return bad_request(format!("Invalid trade body: {e}")),
    };
    if let Err(message) = trades::validate(&incoming) {
        return bad_request(message);
    }
    let Some(db) = state.db.as_ref() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "Database not ready").into_response();
    };
    match trades::handle_incoming(db, state.trade_env.as_ref(), &device.name, incoming, Utc::now()).await {
        Ok(outcome) => (StatusCode::OK, Json(outcome)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response(),
    }
}
```

In `crates/qf-server/src/main.rs`, replace:

```rust
        db: qf_core::DATABASE.get().cloned(),
    };
```

with:

```rust
        db: qf_core::DATABASE.get().cloned(),
        trade_env: Arc::new(qf_core::helper_link::trades::live::LiveEnv),
    };
```

- [ ] **Step 3: Run the tests**

```bash
cargo test -p qf-server
```

Expected: the four new trade tests pass alongside the existing unit and integration tests. If the platinum line fails to deserialize because it has no `rank`, check that Task 1's `RawItem.rank` has `#[serde(default)]`.

- [ ] **Step 4: Commit**

```bash
git add crates/qf-server Cargo.lock
git commit -m "feat(server): accept helper trade events at POST /helper/trade

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 9: Trades tab, Review modal and toasts in the web UI

**Files:**
- Modify: `web/src/types/tauri.type.ts`, `web/src/api/helper_link/index.ts`, `web/src/pages/live_scraper/Tabs/index.ts`, `web/src/pages/live_scraper/index.tsx`, `web/src/components/Forms/Settings/Tabs/Advanced/Tabs/Log/index.tsx`, `web/public/lang/en.json`
- Create: `web/src/pages/live_scraper/Tabs/Trades/index.tsx`, `web/src/pages/live_scraper/Tabs/Trades/ReviewTradeModal.tsx`

**Interfaces:**
- Consumes:
  - RPC `helper_trades`, `helper_trade_apply` and `helper_trade_ignore` (Task 7)
  - toast keys `on_trade_event.applied` and `on_trade_event.needs_review` with values `{player_name, direction, platinum, items, reason}` (Task 7)
  - `SelectTradableItem` and `SelectSubType` (existing)
- Produces:
  - `TauriTypes.HelperEvent`, `HelperEventPage`, `HelperEventStatus`, `HelperReviewItem` (and their parts)
  - `api.helper_link.trades()`, `applyTrade()`, `ignoreTrade()`
  - `TradesPanel`, `ReviewTradeModal`, `equalSplit`

`sendInvoke` camel-cases only top-level argument keys (`convertToCamelCase` in `web/src/api/index.ts`), so `eventId` reaches the RPC table as expected. The nested `sub_type` inside `items` stays snake_case, which is what `ReviewItem` reads.

`en.json` doesn't round-trip through `json.dump` byte for byte. Edit it with targeted replacements only.

- [ ] **Step 1: Types and API**

In `web/src/types/tauri.type.ts`, replace:

```ts
  export interface DryRunPage {
    total: number;
    page: number;
    limit: number;
    results: DryRunEntry[];
  }
}
```

with:

```ts
  export interface DryRunPage {
    total: number;
    page: number;
    limit: number;
    results: DryRunEntry[];
  }
  export interface HelperRawItem {
    name: string;
    quantity: number;
    rank?: number | null;
  }
  export interface HelperRawTrade {
    player_name: string;
    ee_timestamp: string;
    offered: HelperRawItem[];
    received: HelperRawItem[];
  }
  export type HelperTradeDirection = "purchase" | "sale";
  export interface HelperResolvedItem {
    name: string;
    slug: string;
    wfm_id: string;
    item_name: string;
    sub_type?: SubType | null;
    quantity: number;
    price: number;
    matched_by: "name" | "override" | "set" | "review";
  }
  export interface HelperResolution {
    direction?: HelperTradeDirection | null;
    platinum: number;
    items: HelperResolvedItem[];
    extras: HelperRawItem[];
  }
  export type HelperEventStatus = "applied" | "needs_review" | "ignored";
  export interface HelperEvent {
    event_id: string;
    device_name: string;
    received_at: string;
    detected_at: string;
    status: HelperEventStatus;
    reason?: string | null;
    payload: HelperRawTrade;
    resolution?: HelperResolution | null;
    reviewed_at?: string | null;
  }
  export interface HelperEventPage {
    total: number;
    page: number;
    limit: number;
    results: HelperEvent[];
  }
  export interface HelperReviewItem {
    slug: string;
    sub_type?: SubType;
    quantity: number;
    price: number;
  }
}
```

In `web/src/api/helper_link/index.ts`, replace:

```ts
  revoke(id: number) {
    return this.client.sendInvoke<boolean>("helper_device_revoke", { id });
  }
}
```

with:

```ts
  revoke(id: number) {
    return this.client.sendInvoke<boolean>("helper_device_revoke", { id });
  }
  trades(status: TauriTypes.HelperEventStatus | null, page: number, limit: number) {
    return this.client.sendInvoke<TauriTypes.HelperEventPage>("helper_trades", { status, page, limit });
  }
  applyTrade(eventId: string, items: TauriTypes.HelperReviewItem[]) {
    return this.client.sendInvoke<TauriTypes.HelperEvent>("helper_trade_apply", { eventId, items });
  }
  ignoreTrade(eventId: string) {
    return this.client.sendInvoke<TauriTypes.HelperEvent>("helper_trade_ignore", { eventId });
  }
}
```

- [ ] **Step 2: Review modal**

`web/src/pages/live_scraper/Tabs/Trades/ReviewTradeModal.tsx`:

```tsx
import api from "@api/index";
import { TauriTypes } from "$types";
import { SelectSubType } from "@components/Forms/SelectSubType";
import { SelectTradableItem } from "@components/Forms/SelectTradableItem";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Button, Code, Group, Modal, NumberInput, Stack, Table, Text } from "@mantine/core";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

const UNRESOLVED_PREFIX = "unresolved: ";

type Row = { key: number; name: string; slug: string; sub_type?: TauriTypes.SubType; quantity: number; price: number };

/** Whole-platinum shares with the remainder on the first row, like the server's equal split. */
export function equalSplit(total: number, count: number): number[] {
  if (count <= 0) return [];
  const base = Math.floor(total / count);
  const shares = Array<number>(count).fill(base);
  shares[0] += total - base * count;
  return shares;
}

const goodsOf = (event: TauriTypes.HelperEvent) =>
  (event.resolution?.direction === "sale" ? event.payload.offered : event.payload.received).filter((item) => item.name !== "Platinum");

function initialRows(event: TauriTypes.HelperEvent): Row[] {
  const resolved = (event.resolution?.items ?? []).map((item) => ({
    name: item.name,
    slug: item.slug,
    sub_type: item.sub_type ?? undefined,
    quantity: item.quantity,
  }));
  const unresolvedNames = event.reason?.startsWith(UNRESOLVED_PREFIX) ? event.reason.slice(UNRESOLVED_PREFIX.length).split(", ") : [];
  const goods = goodsOf(event);
  const unresolved = unresolvedNames.map((name) => ({
    name,
    slug: "",
    sub_type: undefined,
    quantity: goods.find((item) => item.name === name)?.quantity ?? 1,
  }));
  const rows = [...resolved, ...unresolved];
  const prices = equalSplit(event.resolution?.platinum ?? 0, rows.length);
  return rows.map((row, index) => ({ ...row, key: index, price: prices[index] }));
}

export type ReviewTradeModalProps = {
  event: TauriTypes.HelperEvent;
  onClose(): void;
  onApplied(): void;
};

export function ReviewTradeModal({ event, onClose, onApplied }: ReviewTradeModalProps) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trades.review_modal.${key}`, context);
  const tDirection = (direction: string) => useTranslatePages(`live_scraper.trades.direction.${direction}`);
  const [rows, setRows] = useState<Row[]>(() => initialRows(event));
  const [nextKey, setNextKey] = useState(1000);
  const { data: items } = useQuery({ queryKey: ["cache_items"], queryFn: () => api.cache.getTradableItems() });
  const bySlug = useMemo(() => new Map((items ?? []).map((item) => [item.wfmUrl, item])), [items]);
  const platinum = event.resolution?.platinum ?? 0;
  const sum = rows.reduce((total, row) => total + row.price, 0);
  const valid = rows.length > 0 && rows.every((row) => row.slug && row.quantity >= 1 && row.price >= 0);

  const apply = useMutation({
    mutationFn: () =>
      api.helper_link.applyTrade(
        event.event_id,
        rows.map(({ slug, sub_type, quantity, price }) => ({ slug, sub_type, quantity, price })),
      ),
    onSuccess: onApplied,
  });
  const update = (key: number, patch: Partial<Row>) => setRows((current) => current.map((row) => (row.key === key ? { ...row, ...patch } : row)));
  const addRow = () => {
    setRows((current) => [...current, { key: nextKey, name: "", slug: "", quantity: 1, price: 0 }]);
    setNextKey((key) => key + 1);
  };
  const splitEqually = () =>
    setRows((current) => {
      const prices = equalSplit(platinum, current.length);
      return current.map((row, index) => ({ ...row, price: prices[index] }));
    });

  return (
    <Modal opened onClose={onClose} size="xl" title={t("title", { player: event.payload.player_name })}>
      <Stack>
        <Text size="sm">{t("summary", { direction: tDirection(event.resolution?.direction ?? "purchase"), platinum })}</Text>
        <Text size="sm" c="dimmed">
          {t("game_sent")}
        </Text>
        <Code block>
          {goodsOf(event)
            .map((item) => `${item.name} ×${item.quantity}${item.rank != null ? ` (rank ${item.rank})` : ""}`)
            .join("\n")}
        </Code>
        <Table withTableBorder>
          <Table.Thead>
            <Table.Tr>
              <Table.Th>{t("columns.item")}</Table.Th>
              <Table.Th>{t("columns.sub_type")}</Table.Th>
              <Table.Th>{t("columns.quantity")}</Table.Th>
              <Table.Th>{t("columns.price")}</Table.Th>
              <Table.Th />
            </Table.Tr>
          </Table.Thead>
          <Table.Tbody>
            {rows.map((row) => {
              const available = bySlug.get(row.slug)?.subTypes;
              return (
                <Table.Tr key={row.key}>
                  <Table.Td>
                    <Stack gap={2}>
                      <SelectTradableItem hideSubType value={row.slug} onChange={(item) => update(row.key, { slug: item.wfmUrl, sub_type: item.sub_type })} />
                      {row.name && (
                        <Text size="xs" c="dimmed">
                          {t("in_game", { name: row.name })}
                        </Text>
                      )}
                    </Stack>
                  </Table.Td>
                  <Table.Td>
                    <SelectSubType
                      showLabel={false}
                      value={row.sub_type ?? (available ? {} : undefined)}
                      availableSubTypes={available}
                      onChange={(sub_type) => update(row.key, { sub_type })}
                    />
                  </Table.Td>
                  <Table.Td>
                    <NumberInput w={90} min={1} value={row.quantity} onChange={(value) => update(row.key, { quantity: Number(value) || 0 })} />
                  </Table.Td>
                  <Table.Td>
                    <NumberInput w={110} min={0} value={row.price} onChange={(value) => update(row.key, { price: Number(value) || 0 })} />
                  </Table.Td>
                  <Table.Td>
                    <Button size="xs" variant="subtle" color="red" onClick={() => setRows((current) => current.filter((r) => r.key !== row.key))}>
                      {t("remove")}
                    </Button>
                  </Table.Td>
                </Table.Tr>
              );
            })}
          </Table.Tbody>
        </Table>
        <Group>
          <Button variant="light" onClick={addRow}>
            {t("add_item")}
          </Button>
          <Button variant="light" onClick={splitEqually}>
            {t("split_equally")}
          </Button>
        </Group>
        {sum !== platinum && <Alert color="yellow">{t("sum_mismatch", { sum, platinum })}</Alert>}
        {apply.error && <Alert color="red">{String((apply.error as any)?.message ?? apply.error)}</Alert>}
        <Group justify="flex-end">
          <Button variant="default" onClick={onClose}>
            {t("cancel")}
          </Button>
          <Button disabled={!valid} loading={apply.isPending} onClick={() => apply.mutate()}>
            {t("apply")}
          </Button>
        </Group>
      </Stack>
    </Modal>
  );
}
```

- [ ] **Step 3: Trades panel and tab**

`web/src/pages/live_scraper/Tabs/Trades/index.tsx`:

```tsx
import api from "@api/index";
import { TauriTypes } from "$types";
import { useTranslatePages } from "@hooks/useTranslate.hook";
import { Alert, Badge, Button, Group, Pagination, SegmentedControl, Stack, Table, Text } from "@mantine/core";
import { modals } from "@mantine/modals";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { ReviewTradeModal } from "./ReviewTradeModal";

const LIMIT = 25;
const FILTERS = ["needs_review", "applied", "ignored", "all"] as const;
const STATUS_COLORS: Record<TauriTypes.HelperEventStatus, string> = { applied: "green", needs_review: "yellow", ignored: "gray" };

function itemsSummary(event: TauriTypes.HelperEvent) {
  const resolved = event.resolution?.items ?? [];
  if (resolved.length > 0) return resolved.map((item) => `${item.item_name} ×${item.quantity}`).join(", ");
  return [...event.payload.offered, ...event.payload.received]
    .filter((item) => item.name !== "Platinum")
    .map((item) => `${item.name} ×${item.quantity}`)
    .join(", ");
}

export function TradesPanel({ isActive }: { isActive?: boolean }) {
  const t = (key: string, context?: { [key: string]: any }) => useTranslatePages(`live_scraper.trades.${key}`, context);
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<string>("needs_review");
  const [page, setPage] = useState(1);
  const [reviewing, setReviewing] = useState<TauriTypes.HelperEvent | null>(null);
  const status = filter === "all" ? null : (filter as TauriTypes.HelperEventStatus);
  const { data } = useQuery({
    queryKey: ["helper_trades", filter, page],
    queryFn: () => api.helper_link.trades(status, page, LIMIT),
    refetchInterval: 10_000,
    enabled: !!isActive,
  });
  const refresh = () => queryClient.invalidateQueries({ queryKey: ["helper_trades"] });
  const ignore = useMutation({ mutationFn: (eventId: string) => api.helper_link.ignoreTrade(eventId), onSettled: refresh });
  const pages = Math.max(1, Math.ceil((data?.total ?? 0) / LIMIT));

  const confirmIgnore = (event: TauriTypes.HelperEvent) =>
    modals.openConfirmModal({
      title: t("ignore_title"),
      children: <Text size="sm">{t("ignore_message", { player: event.payload.player_name })}</Text>,
      labels: { confirm: t("ignore"), cancel: t("cancel") },
      onConfirm: () => ignore.mutate(event.event_id),
    });

  return (
    <Stack mt="md">
      <Group justify="space-between">
        <SegmentedControl
          value={filter}
          onChange={(value) => {
            setFilter(value);
            setPage(1);
          }}
          data={FILTERS.map((value) => ({ value, label: t(`status.${value}`) }))}
        />
        <Text size="sm" c="dimmed">
          {t("total", { total: data?.total ?? 0 })}
        </Text>
      </Group>
      {ignore.error && <Alert color="red">{String((ignore.error as any)?.message ?? ignore.error)}</Alert>}
      <Table striped withTableBorder>
        <Table.Thead>
          <Table.Tr>
            <Table.Th>{t("columns.detected_at")}</Table.Th>
            <Table.Th>{t("columns.player")}</Table.Th>
            <Table.Th>{t("columns.direction")}</Table.Th>
            <Table.Th>{t("columns.platinum")}</Table.Th>
            <Table.Th>{t("columns.items")}</Table.Th>
            <Table.Th>{t("columns.status")}</Table.Th>
            <Table.Th>{t("columns.reason")}</Table.Th>
            <Table.Th />
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {(data?.results ?? []).map((event) => {
            const direction = event.resolution?.direction;
            return (
              <Table.Tr key={event.event_id}>
                <Table.Td>{event.detected_at}</Table.Td>
                <Table.Td>{event.payload.player_name}</Table.Td>
                <Table.Td>{direction ? t(`direction.${direction}`) : "—"}</Table.Td>
                <Table.Td>{direction ? event.resolution?.platinum : "—"}</Table.Td>
                <Table.Td>{itemsSummary(event)}</Table.Td>
                <Table.Td>
                  <Badge color={STATUS_COLORS[event.status]}>{t(`status.${event.status}`)}</Badge>
                </Table.Td>
                <Table.Td>{event.reason ?? "—"}</Table.Td>
                <Table.Td>
                  {event.status === "needs_review" && (
                    <Group gap="xs" wrap="nowrap">
                      <Button size="xs" disabled={!direction} onClick={() => setReviewing(event)}>
                        {t("review")}
                      </Button>
                      <Button size="xs" variant="light" color="gray" onClick={() => confirmIgnore(event)}>
                        {t("ignore")}
                      </Button>
                    </Group>
                  )}
                </Table.Td>
              </Table.Tr>
            );
          })}
        </Table.Tbody>
      </Table>
      <Pagination total={pages} value={page} onChange={setPage} />
      {reviewing && (
        <ReviewTradeModal
          event={reviewing}
          onClose={() => setReviewing(null)}
          onApplied={() => {
            setReviewing(null);
            refresh();
          }}
        />
      )}
    </Stack>
  );
}
```

In `web/src/pages/live_scraper/Tabs/index.ts`, add `export * from "./Trades";` after `export * from "./Item";`.

In `web/src/pages/live_scraper/index.tsx`, replace:

```tsx
import { DryRunLogPanel, HelperDevicesPanel, ItemPanel, WishListPanel } from "./Tabs";
```

with:

```tsx
import { DryRunLogPanel, HelperDevicesPanel, ItemPanel, TradesPanel, WishListPanel } from "./Tabs";
```

and replace:

```tsx
      id: "wish_list",
    },
```

with:

```tsx
      id: "wish_list",
    },
    {
      label: useTranslateForm("trades.title"),
      component: (isActive: boolean) => <TradesPanel isActive={isActive} />,
      id: "trades",
    },
```

- [ ] **Step 4: Remove the `ee_log_path` field (E10)**

In `web/src/components/Forms/Settings/Tabs/Advanced/Tabs/Log/index.tsx`, replace:

```tsx
import { TauriTypes } from "$types";
import { TooltipIcon } from "@components/Shared/TooltipIcon";
import { useTranslateForms } from "@hooks/useTranslate.hook";
import { Box, Button, Grid, Group, TextInput } from "@mantine/core";
```

with:

```tsx
import { TauriTypes } from "$types";
import { useTranslateForms } from "@hooks/useTranslate.hook";
import { Box, Button, Grid, Group } from "@mantine/core";
```

and replace:

```tsx
          <Group gap="xs" grow>
            <TextInput
              w={350}
              label={useTranslateFormFields("ee_log_path.label")}
              placeholder={useTranslateFormFields("ee_log_path.placeholder")}
              rightSection={<TooltipIcon label={useTranslateFormFields("ee_log_path.tooltip")} />}
              radius="md"
              {...form.getInputProps(getFieldPath("ee_log_path"))}
            />
          </Group>
          <Group>
```

with:

```tsx
          <Group>
```

`form`, `getFieldPath` and `useTranslateFormFields` may now be unused. If `pnpm build` reports them (the tsconfig decides), delete `getFieldPath` and `useTranslateFormFields`, and destructure the prop as `({ form: _form }: LogPanelProps)`. Keep `LogPanelProps` unchanged so the parent still compiles. `TauriTypes.Settings.log_settings.ee_log_path` stays in the type, so saved settings keep round-tripping.

- [ ] **Step 5: Strings**

In `web/public/lang/en.json`, replace:

```json
          "message": "<blue>{{item_name}}</blue> X<blue>{{quantity}}</blue> <blue>{{trade_type}}</blue> For {{platinum}}p"
        }
      },
      "warframe_gdpr_data_loaded": {
```

with:

```json
          "message": "<blue>{{item_name}}</blue> X<blue>{{quantity}}</blue> <blue>{{trade_type}}</blue> For {{platinum}}p"
        },
        "applied": {
          "title": "Trade with {{player_name}} recorded",
          "message": "<blue>{{direction}}</blue> of {{items}} item(s) for <blue>{{platinum}}p</blue>"
        },
        "needs_review": {
          "title": "Trade with {{player_name}} needs review",
          "message": "{{reason}}. Open Live Scraper → Trades to finish it."
        }
      },
      "warframe_gdpr_data_loaded": {
```

and replace:

```json
        "created_config": "Put this in ~/.config/qf-helper/qf-helper.toml on the gaming PC:",
        "done": "Done"
      },
```

with:

```json
        "created_config": "Put this in ~/.config/qf-helper/qf-helper.toml on the gaming PC:",
        "done": "Done"
      },
      "trades": {
        "title": "Trades",
        "total": "{{total}} trades",
        "status": {
          "needs_review": "Needs review",
          "applied": "Applied",
          "ignored": "Ignored",
          "all": "All"
        },
        "direction": {
          "purchase": "Purchase",
          "sale": "Sale"
        },
        "columns": {
          "detected_at": "Detected",
          "player": "Player",
          "direction": "Direction",
          "platinum": "Platinum",
          "items": "Items",
          "status": "Status",
          "reason": "Reason"
        },
        "review": "Review",
        "ignore": "Ignore",
        "cancel": "Cancel",
        "ignore_title": "Ignore trade",
        "ignore_message": "The trade with {{player}} stays in the list as ignored. Stock, wish list and transactions don't change.",
        "review_modal": {
          "title": "Review trade with {{player}}",
          "summary": "{{direction}} for {{platinum}} platinum",
          "game_sent": "The game reported:",
          "in_game": "In game: {{name}}",
          "columns": {
            "item": "Item",
            "sub_type": "Rank / variant",
            "quantity": "Quantity",
            "price": "Line price"
          },
          "add_item": "Add item",
          "remove": "Remove",
          "split_equally": "Split platinum equally",
          "sum_mismatch": "Line prices add up to {{sum}}p; the trade was {{platinum}}p.",
          "cancel": "Cancel",
          "apply": "Apply"
        }
      },
```

- [ ] **Step 6: Check and build**

```bash
python3 -c "import json;d=json.load(open('web/public/lang/en.json'));t=d['pages']['live_scraper']['trades'];print(t['title'], t['review_modal']['apply'], d['common']['notifications']['on_trade_event']['needs_review']['title'])"
python3 scripts/check-rpc-commands.py
(cd web && pnpm build)
grep -rn "ee_log_path" web/src
```

Expected:
- The JSON line prints `Trades Apply Trade with {{player_name}} needs review`.
- The script reports 0 missing.
- `pnpm build` (tsc and vite) is clean.
- `grep` shows `ee_log_path` only in `tauri.type.ts`.

- [ ] **Step 7: Commit**

```bash
git add web/src web/public/lang/en.json
git commit -m "feat(web): add the trades tab with review and trade toasts

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 10: Deploy, install the helper and accept

**Files:**
- Create: `docs/PHASE-4B-ACCEPTANCE.md`

**Interfaces:**
- Consumes: everything above, deployed on ockohome and installed on the gaming PC.
- Produces: an acceptance record. Merge only after the user's go-ahead.

Global dry-run stays on throughout. Applied trades still change stock, transactions and **real** WFM orders (E8), so the real in-game checks use trades the user was making anyway.

- [ ] **Step 1: Run the local gate**

```bash
cargo test -p qf_log_parser && cargo test -p qf-helper && cargo test -p wf-market --lib && cargo test -p qf_core --lib && cargo test -p qf-server
python3 scripts/check-rpc-commands.py && (cd web && pnpm build)
```

Expected: all green. Record the test counts for the acceptance record.

- [ ] **Step 2: Sync and rebuild on the server**

```bash
rsync -a --delete --dry-run --itemize-changes \
  --exclude .git --exclude target --exclude web/node_modules --exclude web/dist --exclude secrets --exclude .env \
  ./ christopher@ockohome:~/stacks/quantframe-server/ | grep deleting
```

If only expected paths would be deleted, run the same `rsync` without `--dry-run --itemize-changes`, then:

```bash
ssh christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose up -d --build && docker compose ps'
ssh christopher@ockohome 'cd ~/stacks/quantframe-server && docker compose logs --since 10m | sed "s/\x1b\[[0-9;]*m//g" | grep -E "Trader|Collector|panic|CRITICAL|Migrat|Db:|HelperLink" | grep -v WarframeMarket:API'
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://ockohome:8080/helper/trade -H 'Content-Type: application/json' -d '{}'
```

Expected: the container is healthy, the migrations apply (`Database ready`), there's no panic and no `Trader started`, and the unauthenticated `curl` prints `401`.

- [ ] **Step 3: Install the new helper (gaming PC)**

```bash
cargo build --release -p qf-helper
install -Dm755 "$CARGO_TARGET_DIR/release/qf-helper" ~/.local/bin/qf-helper
grep -E '^\s*ee_log_path' ~/.config/qf-helper/qf-helper.toml || echo "ee_log_path not set (the default under \$HOME is used)"
systemctl --user restart qf-helper
journalctl --user -u qf-helper --since "2 min ago" --no-pager
```

That `grep` prints only the `ee_log_path` line, never the key. Expected in the journal: `watching <EE.log path> from byte <n> (0 queued trade(s))` and `heartbeat accepted`. If the path is wrong or the file is missing, fix `ee_log_path` with the user before going on. Under Proton, EE.log lives under `steamapps/compatdata/230410/pfx/drive_c/users/steamuser/AppData/Local/Warframe/EE.log`. Then run `~/.local/bin/qf-helper --parse "<that path>" | head -40`; it prints a JSON array, empty if no trade happened this session.

- [ ] **Step 4: Acceptance checks (with the user)**

On `http://ockohome:8080/live_scraper` → **Trades**, with global dry-run on and `auto_trade` on:

1. **Real sale in game** (E12): the journal shows `trade detected: sale <n>p with <player>, <k> items; server: applied`, and a green toast appears. The stock row goes down or disappears, a Sale transaction appears with the player and the game time, and the real WFM sell order is closed or its quantity lowered. **Applied** lists the event.
2. **Real purchase in game** (E12): `server: applied`. The item appears in stock, or the matching wish-list row is bought. There is exactly **one** Purchase transaction for it (the wish-list path must not add a second one).
3. **Unresolved name, applied from the modal, delivered from the queue** (E12): a real unresolved trade is unlikely, so use a queued event with a misspelled name.
   - Stop the helper: `systemctl --user stop qf-helper`.
   - Take an id from `python3 -c "import hashlib;print(hashlib.sha256(b'phase-4b-acceptance-3').hexdigest())"`, and append one line to `~/.local/state/qf-helper/trade-queue.jsonl`, using a cheap item the user agrees to, misspelled:
     ```json
     {"event_id":"<id>","detected_at":"2026-09-15T12:00:00Z","trade":{"player_name":"AcceptanceTest","ee_timestamp":"0.000","offered":[{"name":"Paryy","quantity":1,"rank":0}],"received":[{"name":"Platinum","quantity":1}]}}
     ```
   - Start the helper. The journal shows `... server: needs_review`, and a yellow toast says `unresolved: Paryy`.
   - In **Trades**, choose Review, pick the real item, and Apply. The event moves to **Applied** with `matched_by: review`.
   - Afterwards, with the user's agreement, delete the test transaction, fix the stock row from the UI, and put back any WFM order this changed.
4. **Replay returns `duplicate`** (E12): stop the helper, append the same line again, and start it. The journal shows `server: duplicate`, and **All** lists the event once.
5. **No platinum side is ignore-only:** queue a line with a new id (`b'phase-4b-acceptance-5'`) and `"received":[{"name":"Adaptation","quantity":1,"rank":10}]` instead of platinum. It lands as `no_platinum_side` with Review disabled. Ignore it; it moves to **Ignored** with reason `reviewed`, and stock and transactions don't change.
6. **Kill switch:** turn `auto_trade` off, queue check 3's line with a new id (`b'phase-4b-acceptance-6'`) and the correct name `Parry`, and start the helper. It lands as `auto_trade_off`. Ignore it, then turn `auto_trade` back on.
7. **Settings → Advanced → Log** no longer shows the EE.log path. Settings still save, and the Helper devices and Dry-run log tabs still load.
8. **`docker compose restart`:** the Trades tab still lists every event, and the helper's journal shows no dropped lines.

Evidence to record: the journal lines, the event ids (hex, never keys), and optionally `SELECT status, COUNT(*) FROM helper_events GROUP BY status` run inside the container.

- [ ] **Step 5: Write the acceptance record and commit**

`docs/PHASE-4B-ACCEPTANCE.md`, in the same layout as `docs/PHASE-4A-ACCEPTANCE.md`:
- A header with the server, branch and commit, the gaming PC, and the local gate counts.
- A table of checks 1–8 with Result and Notes, using observed values and never the device key.
- A `## Follow-ups` section that carries forward phase 4a follow-ups 2–6 and 8, closes follow-up 7 (`ee_log_path`, removed in Task 9), and adds the known limitations below.

```bash
git add docs/PHASE-4B-ACCEPTANCE.md
git commit -m "docs: record phase 4b trade events acceptance

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

Ask the user whether to merge `phase-4b-trade-events` into `main`, and update the project memory note.

---

## Self-review notes

- **§18 coverage:**
  - E1 (WFM names, normalisation, `overrides.toml`): Task 5 `resolve`, plus the README.
  - E2 (byte-stream scanner, markers, raw items, `event_id`): Task 1, and `--parse` in Task 2.
  - E3 (1 s tail, truncation, queue, status handling, log line): Tasks 2 and 3.
  - E4 (route, body, 200/400/401, duplicate): Task 6 `validate` and `handle_incoming`, and Task 8.
  - E5 (`helper_events`, resolution shape, 90-day retention): Task 4. Task 6 stores the resolution for needs_review too.
  - E6 (classification, extras): Task 5 `classify`, and end to end in Task 6.
  - E7 (rank capping, lazy set folding with `sets.json`, `unresolved: <name>`): Tasks 5 and 6.
  - E8 (kill switch, split, handler order and flags, `SetDate`, real orders under dry-run, `apply_failed`, refresh events, toasts, `on_new_trade`): Task 5 `split`, Task 6 `apply` and the pipeline, Task 7 `LiveEnv`.
  - E9 (three RPCs, refusal rules): Tasks 6 and 7.
  - E10 (Trades tab, needs_review default filter, columns, Review/Ignore, modal prefill and equal split, toasts, `ee_log_path` removed): Task 9.
  - E11 (fixtures in four chunk sizes, server unit tests, route integration test, web checks): Tasks 1 and 4–9.
  - E12: Task 10 checks 1–4.
  - E13: nothing is built for riven trades, item-for-item prices, Russian strings, WFCD or extras review.
- **Decisions this plan adds inside the spec's room:**
  - **Purchase order:** `handle_item` runs only when `handle_wish_list` reports `WishListItemBought_NotFound`. When a wish-list row matches, that handler already wrote the transaction.
  - **Extra reasons:** `applying` (the row exists before any handler runs), `no_goods` (platinum only) and `auto_trade_off` (the kill switch).
  - **Modal prices:** the modal starts from an equal split, as E10 says, not from the server's weighted prices. The weighted prices are used only for auto-apply.
  - **Unresolved rows:** the modal adds one blank row per name after `unresolved: ` in the reason.
- **Type consistency:**
  - `HelperEvent`, `Resolution` and `ResolvedItem` field names match between Rust (Task 4) and `TauriTypes` (Task 9).
  - `ReviewItem { slug, sub_type, quantity, price }` matches `HelperReviewItem`.
  - The toast values `{player_name, direction, platinum, items, reason}` match the en.json placeholders.
  - `resolve::tests::items()` (Task 5) is what Task 6's fake returns.
  - `TradeEnv` has the same seven methods in Task 6, `LiveEnv` (Task 7) and `FakeTrades` (Task 8).
- **Known limitations** (carry into the acceptance record):
  - `handle_wish_list_by_entity` passes `operations` rather than `flags` to `handle_transaction`, so a purchase that matched a wish-list row is dated at server time, not game time. This is upstream behaviour, left unchanged.
  - A partial `apply_failed` followed by a review apply re-applies the items that had already succeeded. The warning log names them so stock can be fixed by hand.
  - Presence and trades assume one gaming PC; the phase 4a second-helper caveat stands.
