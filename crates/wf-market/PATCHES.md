# Vendored wf-market

- **Source:** https://github.com/KibbeWater/wf-market at commit `aba1d268a7a0f76d54ba3dcd862d2f3a4f7e3496` (v0.1.11), by KibbeWater.
- **License:** GPL-3.0-only, the same as this repository.

## Local changes

1. **`Client::<Authenticated>::refresh()` no longer fetches riven auctions or chats.** Those calls went to the v1 endpoints `/profile/{name}/auctions` and `/im/chats` on every sign-in and startup. quantframe-server allows v1 only for `POST /v1/auth/signin`, and it has no auction or chat features. The regression test `quantframe_server_patch_tests` in `src/client.rs` guards this.
2. **Upstream `src/tests/` was removed.** Those tests call the live warframe.market API with real credentials from a `.env` file.
3. **`publish = false` was added,** and the `dotenv` dev-dependency was removed.
4. **A process-wide request gate (`src/gate.rs`).** `call_api` awaits the installed gate before each request and passes it every response status. quantframe-server installs a gate that shares one 3 req/s budget between the trader, the collector and item refresh. With no gate installed, the crate behaves as upstream. The test `gate::tests::every_api_call_passes_the_installed_gate` guards this.
5. **The gate can refuse a request and hears about challenges and transport errors (`src/gate.rs`, `call_api`).** The trait is now `Gate`: `acquire` returns `Result<(), String>`, and on `Err(text)` `call_api` returns `ApiError::RequestError` with `text` as its content without sending anything; every response is passed to `on_response(status, is_challenge(headers))`, where `is_challenge(status, headers)` is true for `cf-mitigated: challenge` on any status ≥ 400 or a `text/html` content type on a 403 or 503 (the same rule as quantframe-server's `outcome_of`); a request that gets no response calls `on_transport_error()`. quantframe-server's gate refuses while its circuit breaker is open ("warframe.market unreachable: breaker open until HH:MM UTC"). The crate's own per-route 429 handling (Retry-After, quota tightening) is unchanged. The tests `gate::tests::a_closed_gate_returns_request_error_without_sending`, `challenge_responses_are_flagged`, `an_html_403_reaches_the_gate_flagged` and `transport_error_is_reported` guard this.

When updating from upstream, copy the new source over this folder and re-apply the changes above.
