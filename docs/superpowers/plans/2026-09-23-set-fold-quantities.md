# Set Folding With Part Quantities — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan with an Opus implementer and a reviewer. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fold a traded set of parts into the set item using each part's quantity in the set, so a purchase of a set that needs two of a part no longer leaves phantom part rows in stock.

**Architecture:** `PartsMap` gains a quantity per part; the set cache fetches each part's `quantityInSet` once when it first caches a root and stores it in a new `sets_v2.json`; `fold_sets` divides by the needed quantity. One file plus its tests; the `SetSource` trait and the caller in `mod.rs` are unchanged apart from the type.

**Tech Stack:** Rust (serde, reqwest, tokio), existing test fakes in `helper_link/trades`.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` §25 **P19** (read it first); §18 E7 for the original set-folding rule.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-fold-fix`, branch `fix-set-fold-quantities` (from `main` at `8fbcf95`). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)` (no web change is expected; the gate still runs). Output pristine; no new warnings (base: qf_core 32, utils 1, entity 5, service 3, migration 3).
- **Exact values:** `SetPart { slug: String, quantity: i64 }`; `PartsMap = HashMap<String, Vec<SetPart>>`; `quantityInSet` absent or `< 1` → 1; `SETS_FILE = "sets_v2.json"`; `sets = min(available / needed)` with integer division; a failed part fetch leaves the root uncached.
- Only `crates/qf_core/src/helper_link/trades/sets.rs` and the `PartsMap` literals in `crates/qf_core/src/helper_link/trades/mod.rs`'s tests may change. No change to `resolve.rs`, `apply_and_record`, the trader, or the collector.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; do not push; never touch `main`.

---

### Task 1: Part quantities in the set cache and the fold

**Files:**
- Modify: `crates/qf_core/src/helper_link/trades/sets.rs` (types, `parse_set_parts`, a new `parse_quantity_in_set`, `SetCache::fetch`, `remember`, `fold_sets`, tests), `crates/qf_core/src/helper_link/trades/mod.rs` (only the `PartsMap::from(...)` literal in the test `fake()`)

**Interfaces (produces):**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetPart { pub slug: String, pub quantity: i64 }
pub type PartsMap = HashMap<String, Vec<SetPart>>;
pub const SETS_FILE: &str = "sets_v2.json";
pub fn parse_set_parts(json: &str, index: &ItemIndex) -> Result<Vec<String>, String>;      // unchanged: the root's part slugs
pub fn parse_quantity_in_set(json: &str) -> Result<i64, String>;                            // data.quantityInSet, 1 when absent or < 1, error when the body is not JSON or has no data
pub fn fold_sets(items: Vec<ResolvedItem>, candidates: &[String], parts: &PartsMap, index: &ItemIndex) -> Vec<ResolvedItem>;   // unchanged signature
```

- [ ] **Step 1: Failing fold tests.** In `sets.rs`'s test module add a Kogake-shaped index entry set (a root `Kogake Prime Set`/`kogake_prime_set` tagged `set`, parts `kogake_prime_blueprint` ×1, `kogake_prime_gauntlet` ×2, `kogake_prime_boot` ×2; extend `resolve::tests::index()` or build a local index the same way that helper does) and a `kogake_parts()` `PartsMap`. Tests: blueprint 1 + gauntlet 2 + boot 2 → exactly one item, `kogake_prime_set` quantity 1, `matched_by == "set"`; gauntlet 3 + boot 2 + blueprint 1 → the set plus `kogake_prime_gauntlet` quantity 1; blueprint 1 + gauntlet 1 + boot 2 → unchanged input (nothing folds); blueprint 2 + gauntlet 4 + boot 4 → set quantity 2. Update the existing `wolf_parts()` literal and `mod.rs`'s `fake()` literal to `SetPart { .., quantity: 1 }` so the Wolf Sledge tests keep proving what they proved. Run `cargo test -p qf_core --lib helper_link::trades` — expected: compile failure on the new type, then, once the type is landed with the old one-per-set fold, the Kogake assertions fail (behavioural RED).

- [ ] **Step 2: Implement the type and the fold.** `sets = part_slugs.iter().map(|p| available(p.slug) / p.quantity.max(1)).min()`; subtract `p.quantity × sets` from that part's items (across duplicates, as the current loop does); keep the leftover retention and the pushed set item. Run the fold tests — expected: PASS, including the Wolf Sledge ones.

- [ ] **Step 3: Failing parse and cache tests.** `parse_quantity_in_set`: `{"data":{"quantityInSet":2}}` → 2; `{"data":{"slug":"x"}}` → 1; `{"data":{"quantityInSet":0}}` → 1; `"nope"` and `{}` → error. Cache: extend the existing `an_empty_parts_list_is_neither_cached_nor_saved` pattern with a scripted HTTP source or, if `SetCache::fetch` cannot be faked without a real server, split the fetch into a pure `assemble(root_parts: Vec<String>, quantities: HashMap<String, Result<i64, String>>) -> Result<Vec<SetPart>, String>` that returns an error when any part's quantity is an error, and test that instead. RED first.

- [ ] **Step 4: Implement the cache side.** In `SetCache::fetch(root)`: fetch the root's detail (as today) → part slugs; for each part slug other than the root, fetch `{WFM_ITEM_URL}/{part}` through the same limiter lane and `parse_quantity_in_set`; any error → `Err` for the root (not cached, retried on the next trade); build `Vec<SetPart>`. `remember` stores it; `save`/`new` use `sets_v2.json`. `parts_for` returns the new map type. Log one `info` per newly cached root naming the parts and quantities, so the first Kogake fold after deploy is visible in the Log tab.

- [ ] **Step 5: Full gate.** `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)` — green, no new warnings.

- [ ] **Step 6: Commit** `fix(trades): fold a set with each part's quantity in the set`. Do not push.

---

## Self-Review

- Spec coverage: P19 parts-carry-a-quantity → Steps 3–4; folding → Steps 1–2; cache file → Step 4; tests → Steps 1 and 3.
- Type consistency: `SetPart`, `PartsMap`, `parse_quantity_in_set` named identically in every step; `fold_sets`' signature unchanged so `mod.rs:174` compiles without edit.
- The first live Kogake purchase after deploy is the acceptance check: one stock row, the set, at the trade's platinum, and an `info` line listing gauntlet ×2 and boot ×2.
