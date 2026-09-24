# Helper Trade Default Variant — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan with an Opus implementer and a reviewer. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A helper-detected trade of an item whose warframe.market listing has variants resolves to the item's first listed variant, so the stock row matches live orders instead of being stamped `NoSellers`.

**Architecture:** One function, `resolve::resolved`, consults `item.sub_type.variants` when building the sub-type. Nothing else changes.

**Tech Stack:** Rust; existing test helpers in `helper_link/trades/resolve.rs`.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` §25 **P20** (read it first); §18 E6/E8 for the resolver's original rules.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-variant-fix`, branch `fix-helper-trade-variant` (from `main`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)` (no web change is expected; the gate still runs). Output pristine; no new warnings.
- **Exact rule:** `variant = item.sub_type.as_ref().and_then(|s| s.variants.as_ref()).and_then(|v| v.first()).cloned()`; sub-type is `Some` when the trade has a rank **or** a variant exists; rank stays capped at `max_rank` as today; `SubType { rank, variant, ..Default::default() }`.
- Only `crates/qf_core/src/helper_link/trades/resolve.rs` may change (the `item(..)` test helper may gain a variants parameter or a sibling helper; keep the other test modules that reuse `tests::items()`/`index()` compiling unchanged).
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; do not push; never touch `main`.

---

### Task 1: Default variant in the resolver

**Files:**
- Modify: `crates/qf_core/src/helper_link/trades/resolve.rs` (`resolved`, tests)

- [ ] **Step 1: Failing tests.** Add an index entry `Archon Vitality`/`archon_vitality`, max rank 10, tags `mod`, `archon`, variants `["regular","atragraph"]`, and `Lith A1 Relic`/`lith_a1_relic`, no rank, tags `relic`, variants `["intact","exceptional","flawless","radiant"]`. Assert `resolve_item(raw("Archon Vitality", 1, Some(10)))` gives `SubType { rank: Some(10), variant: Some("regular".into()), ..Default::default() }`; `raw("Archon Vitality", 1, Some(12))` caps to 10 and keeps the variant; `raw("Lith A1 Relic", 3, None)` gives `SubType { variant: Some("intact".into()), ..Default::default() }`; `Adaptation` rank 10 (no variants) still gives `SubType::rank(10)` and `wolf sledge HANDLE` still gives `None`. Run `cargo test -p qf_core --lib helper_link::trades::resolve` — expected: the new assertions fail (RED).

- [ ] **Step 2: Implement.** Change `resolved` per the exact rule above. Run the resolve tests — expected: PASS; run `cargo test -p qf_core --lib helper_link::trades` — all existing trades tests still pass.

- [ ] **Step 3: Full gate.** `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)` — green, no new warnings.

- [ ] **Step 4: Commit** `fix(trades): resolve a helper trade to the item's default market variant`. Do not push.

---

## Self-Review

- Spec coverage: P20 rule → Step 2; tests → Step 1. Data repair is the controller's job after deploy, not part of this plan.
- The acceptance check after deploy: the next sell pass on Archon Vitality (row repaired by hand) logs non-zero sell orders and a `Create`, and the next helper-detected Archon Vitality trade stores `{"rank":10,"variant":"regular"}`.
