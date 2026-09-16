# quantframe-server — working rules

- The controller session writes specs and plans and hands tasks to Opus subagents; agents work only on feature branches (sibling worktrees `../quantframe-server-<phase>`, build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`).
- Only the controller merges into `main`, after the phase's acceptance record is committed and the gate is green on the branch tip. Agents never touch `main`.
- Deploying to ockohome needs the user's explicit go-ahead every time.
- Gate: `cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`; never bare `cargo test --workspace`.
- Conventional commits; `web/public/lang/en.json` by targeted insertion only.
