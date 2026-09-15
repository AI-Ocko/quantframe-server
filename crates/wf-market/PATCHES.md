# Vendored wf-market

- **Source:** https://github.com/KibbeWater/wf-market at commit `aba1d268a7a0f76d54ba3dcd862d2f3a4f7e3496` (v0.1.11), by KibbeWater.
- **License:** GPL-3.0-only, the same as this repository.

## Local changes

1. **`Client::<Authenticated>::refresh()` no longer fetches riven auctions or chats.** Those calls went to the v1 endpoints `/profile/{name}/auctions` and `/im/chats` on every sign-in and startup. quantframe-server allows v1 only for `POST /v1/auth/signin`, and it has no auction or chat features. The regression test `quantframe_server_patch_tests` in `src/client.rs` guards this.
2. **Upstream `src/tests/` was removed.** Those tests call the live warframe.market API with real credentials from a `.env` file.
3. **`publish = false` was added,** and the `dotenv` dev-dependency was removed.
4. **A process-wide `RequestGate` (`src/gate.rs`).** `call_api` awaits the installed gate before each request and passes it every response status. quantframe-server installs a gate that shares one 3 req/s budget between the trader, the collector and item refresh, and pauses everything after a 429. With no gate installed, the crate behaves as upstream. The test `gate::tests::every_api_call_passes_the_installed_gate` guards this.

When updating from upstream, copy the new source over this folder and re-apply the changes above.
