# Phase 4d: Live Log Tab — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task with Opus implementers and a reviewer between tasks. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A **Log** tab on the Live Scraper page that shows the server's log lines live over the existing websocket, with scrollback, auto-scroll and a level filter.

**Architecture:** One function-pointer sink in `utils::core::dolog` (the single logging funnel, which already keeps a 10 000-line ring buffer) is installed by `qf_core::startup::start` and calls `qf_core::events::emit("log", …)` directly. A `log_tail` RPC reads the ring for scrollback. The web tab fetches the tail once, then appends frames from the raw `log` channel.

**Tech Stack:** Rust (tokio broadcast, serde_json), React 19 + Mantine 9 + TanStack Query 5, pnpm 11.3.0.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` — read **§20 (L1–L8)** first; §7.1 and §18 E10 for the tab conventions. §20 takes precedence. Research notes with file:line evidence (may not survive the session): `/tmp/claude-1000/-home-ocko-Projects-Personal-quantframe-server/b41025b5-777e-45e6-b6c5-5e8993344628/scratchpad/live-log-research.md`.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-phase-4d`, branch `phase-4d-live-log` (from `main` at `21ffaa4`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)`. Output pristine.
- **Never emit the log event through `send_event!` / `emit_event!`** (they log on every emit → infinite recursion). Only `crate::events::emit` directly (L2).
- **Exact strings:** channel `log`; payload `{ "level": <LogLevel prefix, e.g. "INFO">, "line": <ANSI-free line> }`; RPC `log_tail { limit: i64 }` returning `[{ level, line }]`, `limit` clamped to `1..=2000`; tab key `log`; en.json keys under `pages.live_scraper.log`; client cap **2000** lines; initial tail **500**.
- **`en.json`** by targeted insertion only; confirm it parses afterwards.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; `git push` after each task. Never push `main`.
- Tasks 1–3 do not touch ockohome; Task 4 does (deploy + acceptance) and asks the user before deploying.

## File Structure (end of phase 4d)

```
crates/utils/src/core.rs                         MOD  SINK, set_sink, tail, tests
crates/qf_core/src/startup.rs                    MOD  log_sink; install it after init_logger()
crates/qf_core/src/events.rs                     MOD  test for the "log" frame shape
crates/qf_core/src/commands/logs.rs              MOD  LogLine, log_tail
crates/qf_core/src/commands/rpc.rs               MOD  log_tail row + test
web/src/api/log/index.ts                         MOD  tail(limit)
web/src/types/tauri.type.ts                      MOD  LogLine
web/src/pages/live_scraper/Tabs/Log/index.tsx    NEW  LogPanel
web/src/pages/live_scraper/Tabs/index.ts         MOD  export
web/src/pages/live_scraper/index.tsx             MOD  Log tab
web/public/lang/en.json                          MOD  pages.live_scraper.tabs.log, pages.live_scraper.log.*
docs/PHASE-4D-ACCEPTANCE.md                      NEW
```

---

### Task 1: Logger sink and `tail` (utils)

**Files:**
- Modify: `crates/utils/src/core.rs` (`CACHED_LOGS` ~lines 10-18, `cache_log_entry` ~64-73, `dolog` ~101-221 with `cache_log_entry(level, clean_message)` at ~204, `export_cached_logs` ~346)

**Interfaces:**
- Consumes: `CACHED_LOGS: Mutex<Vec<CachedLogEntry>>` (`{ level, message }`), `LogLevel` (`prefix()`), `remove_ansi_codes`.
- Produces: `pub fn set_sink(sink: fn(&LogLevel, &str))` (first call wins; later calls ignored); `pub fn tail(limit: usize) -> Vec<(LogLevel, String)>` oldest-first, at most `limit`, `limit == 0` → empty. The sink is invoked in `dolog` right after `cache_log_entry`, with the same `level` and `clean_message`, only for lines that passed all existing filters.

- [ ] **Step 1: Failing tests**

Add at the end of `crates/utils/src/core.rs` (no tests module exists there today):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    static SEEN: StdMutex<Vec<(String, String)>> = StdMutex::new(Vec::new());

    fn test_sink(level: &LogLevel, line: &str) {
        SEEN.lock().unwrap().push((level.prefix().to_string(), line.to_string()));
    }

    #[test]
    fn tail_returns_the_newest_lines_oldest_first_and_the_sink_sees_clean_lines() {
        set_sink(test_sink);
        let marker = format!("marker-{}", std::process::id());
        info("SinkTest", format!("\x1b[1;32m{marker}-one\x1b[0m"), &LoggerOptions::default());
        warning("SinkTest", format!("{marker}-two"), &LoggerOptions::default());

        let mine: Vec<(LogLevel, String)> = tail(10_000).into_iter().filter(|(_, l)| l.contains(&marker)).collect();
        assert_eq!(mine.len(), 2);
        assert!(mine[0].1.contains(&format!("{marker}-one")), "oldest first");
        assert!(!mine[0].1.contains("\x1b["), "ANSI codes are stripped");
        assert!(matches!(mine[1].0, LogLevel::Warning));
        assert!(tail(0).is_empty());
        assert!(tail(1).len() <= 1);

        let seen = SEEN.lock().unwrap();
        assert!(seen.iter().any(|(lvl, l)| lvl == "INFO" && l.contains(&format!("{marker}-one")) && !l.contains("\x1b[")));
        assert!(seen.iter().any(|(lvl, l)| lvl == "WARNING" && l.contains(&format!("{marker}-two"))));
    }
}
```

If `LogLevel::prefix()` returns a different type or different strings (`INFO`, `WARNING`), adapt the assertions to what `prefix()` really returns and note it; the spec's payload `level` is `prefix()`'s value whatever it is. If `info`/`warning` take `&str` rather than `String`, adjust the calls.

- [ ] **Step 2: Run, expect a compile failure**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p utils --lib core::`
Expected: FAIL — `set_sink`/`tail` not found.

- [ ] **Step 3: Implement**

Near `CACHED_LOGS`:

```rust
/// Optional tee for every line that reaches the cache (spec §20 L1). Installed once by qf_core.
static SINK: OnceLock<fn(&LogLevel, &str)> = OnceLock::new();

pub fn set_sink(sink: fn(&LogLevel, &str)) {
    let _ = SINK.set(sink);
}

/// The newest `limit` cached lines, oldest first (spec §20 L3).
pub fn tail(limit: usize) -> Vec<(LogLevel, String)> {
    let cache = CACHED_LOGS.lock().unwrap();
    let start = cache.len().saturating_sub(limit);
    cache[start..].iter().map(|e| (e.level.clone(), e.message.clone())).collect()
}
```

(`use std::sync::OnceLock;` if missing; use `CachedLogEntry`'s real field names; derive `Clone` on `LogLevel` if it lacks it.) In `dolog`, immediately after `cache_log_entry(level, clean_message)`:

```rust
    if let Some(sink) = SINK.get() {
        sink(level, clean_message);
    }
```

with the same variables and reference forms `cache_log_entry` receives.

- [ ] **Step 4: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p utils --lib` — whole crate green, no new warnings.

- [ ] **Step 5: Commit and push**

```bash
git add crates/utils/src/core.rs
git commit -m "feat(utils): add a log sink hook and a tail over the cached log lines

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 2: Emit `log` frames and the `log_tail` RPC (qf_core)

**Files:**
- Modify: `crates/qf_core/src/startup.rs` (~line 29, after `init_logger()`), `crates/qf_core/src/events.rs` (tests), `crates/qf_core/src/commands/logs.rs`, `crates/qf_core/src/commands/rpc.rs`

**Interfaces:**
- Consumes: Task 1 (`utils::set_sink`, `utils::tail`), `crate::events::emit(channel, payload) -> usize`, the `rpc_table!` macro, the existing `log` command in `commands/logs.rs`.
- Produces: `startup::log_sink(level: &LogLevel, line: &str)` (a plain `fn`, installed once); RPC `log_tail { limit: i64 } -> Vec<LogLine>` with `pub struct LogLine { pub level: String, pub line: String }` (Serialize).

- [ ] **Step 1: Failing tests**

In `crates/qf_core/src/events.rs` tests, next to `emitted_frames_reach_subscribers` (copy its runtime style):

```rust
    #[test]
    fn a_log_frame_carries_level_and_line() {
        let mut rx = subscribe();
        crate::startup::log_sink(&utils::LogLevel::Warning, "[2026-09-16 05:49:33] [3.3] [WARNING] [Test] hello");
        let frame = rx.try_recv().expect("one frame");
        assert_eq!(frame["channel"], "log");
        assert_eq!(frame["payload"]["level"], "WARNING");
        assert!(frame["payload"]["line"].as_str().unwrap().ends_with("hello"));
    }
```

In `crates/qf_core/src/commands/rpc.rs` tests, next to `trader_commands_are_routable_and_validate_args` (use the same helper names it uses):

```rust
    #[tokio::test]
    async fn log_tail_is_routable_and_validates_args() {
        assert!(COMMANDS.contains(&"log_tail"));
        let bad = dispatch("log_tail", serde_json::json!({"limit": "many"})).await.unwrap();
        assert!(bad.is_err(), "a non-numeric limit is rejected");
        let ok = dispatch("log_tail", serde_json::json!({"limit": 5})).await.unwrap().unwrap();
        assert!(ok.as_array().unwrap().len() <= 5);
    }
```

- [ ] **Step 2: Run, expect failures**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib events:: commands::rpc::` — FAIL (`log_sink`, `log_tail` missing).

- [ ] **Step 3: Implement**

`startup.rs`:

```rust
/// Tee every log line to the browser as a raw `log` frame (spec §20 L2).
/// Calls `events::emit` directly: `send_event!` logs on emit and would recurse.
pub fn log_sink(level: &utils::LogLevel, line: &str) {
    let _ = crate::events::emit("log", serde_json::json!({"level": level.prefix(), "line": line}));
}
```

and in `start`, right after `init_logger();`: `utils::set_sink(log_sink);`.

`commands/logs.rs`:

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogLine {
    pub level: String,
    pub line: String,
}

/// The newest `limit` cached log lines, oldest first (spec §20 L3). `limit` is clamped to 1..=2000.
pub async fn log_tail(limit: i64) -> Result<Vec<LogLine>, Error> {
    let limit = limit.clamp(1, 2000) as usize;
    Ok(utils::tail(limit).into_iter().map(|(level, line)| LogLine { level: level.prefix().to_string(), line }).collect())
}
```

`rpc.rs`: add `log_tail => logs::log_tail { limit: i64 },` next to the `log` row; return whatever type the macro expects from neighbours.

- [ ] **Step 4: Run, expect PASS**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib` and `cargo test -p qf-server` — green, no new warnings; `python3 scripts/check-rpc-commands.py` still passes.

- [ ] **Step 5: Commit and push**

```bash
git add crates/qf_core
git commit -m "feat(core): stream log lines to the browser and add the log_tail rpc

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 3: The Log tab (web)

**Files:**
- Create: `web/src/pages/live_scraper/Tabs/Log/index.tsx`
- Modify: `web/src/api/log/index.ts`, `web/src/types/tauri.type.ts`, `web/src/pages/live_scraper/Tabs/index.ts`, `web/src/pages/live_scraper/index.tsx`, `web/public/lang/en.json`

**Interfaces:**
- Consumes: RPC `log_tail { limit }` (Task 2); the raw `log` channel via `OnTauriEvent`/`OffTauriEvent` (`web/src/api/index.ts` ~193-197) or `useTauriEvent` (`web/src/hooks/useTauriEvent.hook.ts`) — see how `play_sound` is consumed in `web/src/contexts/app.context.tsx` ~120-125 for a raw channel; the handler receives the `data` payload only.
- Produces: `TauriTypes.LogLine { level: string; line: string }`; `api.log.tail(limit)`; `LogPanel`.

- [ ] **Step 1: API and type**

`tauri.type.ts`: `export interface LogLine { level: string; line: string; }`. `web/src/api/log/index.ts` (a stub today): add `tail(limit: number): Promise<TauriTypes.LogLine[]> { return this.client.sendInvoke<TauriTypes.LogLine[]>("log_tail", { limit }); }` following `web/src/api/live_scraper/index.ts`'s pattern.

- [ ] **Step 2: The panel**

`web/src/pages/live_scraper/Tabs/Log/index.tsx`, modelled on `Tabs/DryRunLog/index.tsx` for props (`isActive`) and hooks:

- State: `lines: LogLine[]` (cap 2000: `setLines(prev => [...prev, l].slice(-2000))`), `paused: boolean` (true when the user is more than ~40 px above the bottom), `levels: Set<string>` (default `INFO`, `WARNING`, `ERROR`, `CRITICAL`; `DEBUG`/`TRACE` off).
- On `isActive` true: `api.log.tail(500)` → `setLines`; subscribe to the raw `log` channel; cleanup unsubscribes when `isActive` turns false or the component unmounts.
- Render: a `Paper` with a scrollable `div` `ref` of height `calc(100vh - 320px)` (min 300 px), `fontFamily: monospace`, `whiteSpace: "pre-wrap"`, `fontSize: 12`; one `div` per visible line (filtered by `levels.has(l.level)`); colour `WARNING` → `var(--mantine-color-yellow-6)`, `ERROR`/`CRITICAL` → `var(--mantine-color-red-6)`, else inherit. Above it a `Group`: level toggles (`Chip.Group multiple`), a **Clear** button (`setLines([])`), and a **Jump to latest** button shown only while `paused` (scroll to bottom, clear `paused`). `useEffect` on `lines`: if `!paused`, scroll to bottom. `onScroll`: `paused = scrollHeight - scrollTop - clientHeight > 40`. Empty state text when no lines.
- Strings: `t("title")`, `t("levels")`, `t("clear")`, `t("jump")`, `t("empty")` via `useTranslatePages("live_scraper.log.…")`.

- [ ] **Step 3: Register the tab**

`Tabs/index.ts`: `export { LogPanel } from "./Log";`. `web/src/pages/live_scraper/index.tsx` (tabs array ~14-63): add an entry with id `log`, label `t("tabs.log")` and panel `<LogPanel isActive={active === "log"} />` after Trades, in the exact shape of the existing entries.

- [ ] **Step 4: Strings**

`en.json`, targeted insertions: under `pages.live_scraper.tabs` add `"log": "Log"`; under `pages.live_scraper` add
```json
      "log": {
        "title": "Server log",
        "levels": "Levels",
        "clear": "Clear",
        "jump": "Jump to latest",
        "empty": "No log lines yet."
      }
```
Confirm: `python3 -c "import json;json.load(open('web/public/lang/en.json'))"`.

- [ ] **Step 5: Checks**

`python3 scripts/check-rpc-commands.py && (cd web && pnpm build)` — clean.

- [ ] **Step 6: Commit and push**

```bash
git add web
git commit -m "feat(web): add a live server log tab to the live scraper page

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
git push
```

---

### Task 4: Deploy and accept (L8)

**Files:**
- Create: `docs/PHASE-4D-ACCEPTANCE.md`

- [ ] **Step 1: Local gate** — `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`.
- [ ] **Step 2: Deploy (ask the user first)** — rsync the worktree to `christopher@ockohome:~/stacks/quantframe-server/` with the usual excludes (`.git target web/node_modules web/dist secrets .env .superpowers backups`), `docker compose up -d --build`, wait healthy, confirm `Database ready` and `Housekeeping Started`. Use `ssh -o ClearAllForwardings=yes`.
- [ ] **Step 3: Acceptance with the user in the browser (L8):** the tab shows the last lines at once; a `Housekeeping` tick line arrives within 60 s without reloading; scrolling up pauses, "Jump to latest" resumes; toggling Info off hides the tick lines; after a few minutes the pane holds at most 2000 lines; `docker compose logs --since 5m | grep -c "Emit:SendEvent"` does not grow once per log line (no recursion).
- [ ] **Step 4: Record** — `docs/PHASE-4D-ACCEPTANCE.md` in the 4c layout (header, table of the L8 checks, follow-ups carrying forward 4c's open ones and adding the `RequestError.content` masking). Commit with the trailer, push, and ask the user about merging `phase-4d-live-log` into `main`.

---

## Self-review notes

- **§20 coverage:** L1 → Task 1; L2 → Task 2 (`log_sink`, direct `emit`); L3 → Tasks 1–2 (`tail`, `log_tail`); L4 → Task 3; L5 respected; L6 noted in Task 4's record; L7 → tests in Tasks 1–3; L8 → Task 4.
- **Type consistency:** `set_sink(fn(&LogLevel, &str))` matches `log_sink`'s signature; `tail(usize) -> Vec<(LogLevel, String)>` is what `log_tail` maps; `LogLine { level, line }` is the same shape in Rust, the frame payload and TypeScript.
- **Recursion guard:** the only emit path is `crate::events::emit` inside `log_sink`; `emit` itself does not log (verified in the research).
- **Known limitations:** `CACHED_LOGS` is process-global, so the utils test filters by a unique marker; `Debug`/`Trace` lines reach the tab only if the logger's own filters let them through.
