# Phase 2 acceptance — in progress (deployed 2026-09-15 04:59 UTC)

- **Server:** `ockohome`, stack at `~/stacks/quantframe-server`, branch `phase-2-data`
- **Origin:** `http://ockohome:8080`

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Local gate: wf-market, qf_core and qf-server tests, RPC check, web build | Pass | wf-market 2, qf_core 79, qf-server 9 tests; 65/65 RPC commands; `pnpm build` clean |
| 2 | Container healthy; collector started with the item count in the logs | Pass | Healthy in 12 s; `Loaded 3840 tradable items`, `Collector Started: 3840 items, 0 hot` |
| 3 | Hot and cold lanes both sweeping; combined rate ≈ 3 req/s | Pending | Cold confirmed indirectly (DB 5.5 → 15 MB in ~90 s). Hot set was empty at start (0 stock/wish-list items), so the hot lane is untested live |
| 4 | No sustained 429 pauses in 30 min | Pending | No 429s in the startup logs |
| 5 | Cold pass completes; duration recorded | Pending | Expected 20–30 min |
| 6 | Stock, wish list and order refresh still work (Trader lane) | Partial | Startup made only `GET /v2/me`, `GET /v2/orders/my`, then set status to invisible, all through the gate. The UI check needs the user |
| 7 | Hourly price history shows after 2 h | Pending | |
| 8 | Pending vanishes resolve to trade/relist/bulk after 2 h | Pending | |
| 9 | DB size after 24 h, extrapolated to 30 d | Pending | Local smoke run: ~330 live orders per item, so ~1.2 M `last_seen_orders` rows at steady state |
| 10 | Restart: collector resumes, trading state untouched, stats kept | Pending | |

## Follow-ups
