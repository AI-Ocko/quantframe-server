# Phase 1 acceptance — 2026-09-15

- **Server:** `ockohome` (Debian 13, Docker 29.8, Compose v5.5), stack at `~/stacks/quantframe-server`
- **Origin:** `http://ockohome:8080`

| # | Check | Result | Notes |
|---|---|---|---|
| 1 | Container becomes healthy | Pass | Healthy within 6 s |
| 2 | Startup logs load item list, no startup failure | Pass | `Loaded 3840 tradable items` |
| 3 | `/` redirects to `/login`; wrong password rejected; right password loads app | Pass | Redirect and wrong-password checks via curl from the desktop; login checked by the user in the browser |
| 4 | warframe.market sign-in shows in-game name | Pass | Checked by the user |
| 5 | Status Online/Invisible over v2 websocket with v1 sign-in token (spec §13.3) | Pass | Checked by the user on the warframe.market profile |
| 6 | After restart: still signed in, status forced invisible | Pass | `auth.json` has `anonymous:false`; log shows `User status set to invisible` |
| 7 | Stock create, sell (transaction created), delete; stock JSON export downloads | Pass | Checked by the user; transaction export also restored and checked |
| 8 | Custom sound upload and playback | Pass | Checked by the user |
| 9 | No token in `/data/auth.json` | Pass | `grep -c wfm_token` prints `0` |
| 10 | `docker compose down` keeps the `qf-data` volume; `up` restores the session | Pass | Volume `quantframe-server_qf-data` retained; healthy and signed in after `up` |

## Also verified

- **Requests:** the server makes only allowed warframe.market calls. At startup it makes exactly `GET /v2/me` and `GET /v2/orders/my`; sign-in adds `POST /v1/auth/signin`.
- **v1 fix:** the first deploy showed `wf-market` also calling v1 `/profile/{name}/auctions` and `/im/chats` on sign-in and startup. That was fixed by vendoring the crate (`crates/wf-market/PATCHES.md`) and re-verified after redeploy.
- **Origin check:** requests with a foreign `Origin` get 403, and `/rpc` without a session gets 401.

## Follow-ups (not blocking)

- **Log path:** log files land in `/data/logs/logs/<date>/`. The utils logger appends its own `logs` folder to the base path; the base path should be the data directory.
- **Doc tests:** `cargo test --workspace` doc-tests fail in the unmodified upstream `utils` and `wf-market` doc examples. The library, binary and integration tests all pass.
