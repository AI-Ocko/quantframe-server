# Backup On Its Own Connection — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan with an Opus implementer and a reviewer. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The daily `VACUUM INTO` backup no longer holds the app's only database connection, so heartbeats, trader cycles and RPCs keep running while the copy is written.

**Architecture:** `backup::write` opens a second, read-only connection to the database file, runs `VACUUM INTO` on it and closes it. The two paths the backup needs travel as one named struct, `backup::Job { source, dir }`, from `startup::start` through `housekeeping::start` and `tick` to `backup::run`. Nothing else about the backup changes.

**Tech Stack:** Rust; sea-orm 0.12 over bundled SQLite; the existing file-backed test database helper `crate::trader::store::tests::db()`.

**Spec:** `docs/superpowers/specs/2026-09-14-quantframe-server-design.md` §25 **P29** (read it first); §19 **H5/H6** for the housekeeping loop and the backup's original rules.

## Global Constraints

- **Worktree:** `~/Projects/Personal/quantframe-server-backup-conn`, branch `fix-backup-own-connection` (from `main` f078397). Build with `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target`.
- **Tests:** `cargo test -p utils --lib`, `cargo test -p qf_core --lib`, `cargo test -p qf-server`; never bare `--workspace`. Web: `python3 scripts/check-rpc-commands.py` and `(cd web && pnpm build)` (no web change is expected; the gate still runs). Output pristine; no new warnings.
- **Only these files may change:** `crates/qf_core/src/housekeeping/backup.rs`, `crates/qf_core/src/housekeeping/mod.rs`, `crates/qf_core/src/startup.rs`.
- **Source connection URL:** `sqlite://<source>?mode=ro`. Never `rwc`: a missing source must fail, not be created.
- **Unchanged behaviour:** file name `quantframe-<YYYY-MM-DD>.sqlite`, `.tmp` → verify → rename, retention 7 dailies + 4 Sundays, the once-per-UTC-day gate, `alert_backup_failed`, the quote check on the target path.
- **No network:** nothing in this change talks to warframe.market; do not run any binary, example or test outside the gate.
- **Commits:** conventional commits ending with exactly `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`; do not push; never touch `main`.

## Review Focus

Conditions P29 implies that are most likely to bite, each pinned by a test in Task 1:

1. The app's connection is busy, or holds an uncommitted write, when the backup starts → the backup completes and the copy excludes the uncommitted row (Step 1 test).
2. Committed rows that are still only in the WAL → they are in the copy (Step 1 test).
3. The source path is wrong or the file is missing → the backup fails, writes no dated file and creates nothing at the source path (Step 5 test).
4. The backup directory is not a directory or not writable → the existing failure path and once-a-day alert still hold (existing `a_failed_backup_alerts_once_per_day`, new signature).
5. A stale `.tmp` from a crashed run, and a bad copy → still replaced / removed and never renamed (existing `a_bad_copy_is_removed_and_not_renamed`, new signature).

---

### Task 1: The backup opens its own read-only connection

**Files:**
- Modify: `crates/qf_core/src/housekeeping/backup.rs` (`Job`, `write`, `run`, module doc, tests)
- Modify: `crates/qf_core/src/housekeeping/mod.rs` (`tick`, `run_loop`, `start`, one test)
- Modify: `crates/qf_core/src/startup.rs:98` (the `housekeeping::start` call)

**Interfaces:**
- Consumes: `crate::db::DB_FILE` (`"quantframe.sqlite"`), `paths::get().data_dir`, `paths::get().backups_dir()`, `crate::collector::stmt`, `crate::trader::store::tests::db() -> (tempfile::TempDir, DatabaseConnection)`.
- Produces:
  - `pub struct backup::Job { pub source: PathBuf, pub dir: PathBuf }` (`Debug, Clone`)
  - `pub async fn backup::write(job: &Job, date: NaiveDate) -> Result<PathBuf, Error>`
  - `pub async fn backup::run(job: &Job, today: NaiveDate) -> Result<Option<PathBuf>, Error>`
  - `pub async fn housekeeping::tick(conn: &DatabaseConnection, env: &dyn TradeEnv, now: DateTime<Utc>, gates: &mut Gates, backup: Option<&backup::Job>) -> Result<TickReport, Error>`
  - `pub fn housekeeping::start(conn: DatabaseConnection, job: backup::Job)`

- [ ] **Step 1: Write the failing test** in the `tests` module of `backup.rs`, against today's signature (`run(&conn, &backups, today)`), so the stall itself is what fails:

```rust
    #[tokio::test]
    async fn the_backup_does_not_wait_for_the_apps_connection() {
        use service::sea_orm::TransactionTrait;
        let (dir, conn) = db().await;
        let backups = dir.path().join("backups");
        conn.execute_unprepared("CREATE TABLE backup_probe (v INTEGER)").await.unwrap();
        conn.execute_unprepared("INSERT INTO backup_probe VALUES (42)").await.unwrap();
        // The app's only pooled connection is held for the whole backup, with a write still uncommitted.
        let txn = conn.begin().await.unwrap();
        txn.execute_unprepared("INSERT INTO backup_probe VALUES (43)").await.unwrap();
        let written = tokio::time::timeout(std::time::Duration::from_secs(10), run(&conn, &backups, d("2026-09-16")))
            .await
            .expect("the backup must not wait for the app's connection")
            .unwrap()
            .expect("the first run writes");
        txn.rollback().await.unwrap();

        let copy = Database::connect(format!("sqlite://{}?mode=ro", written.display())).await.unwrap();
        let row = copy.query_one(stmt("SELECT COUNT(*) AS n, MAX(v) AS v FROM backup_probe", vec![])).await.unwrap().unwrap();
        assert_eq!(row.try_get::<i64>("", "n").unwrap(), 1, "the uncommitted row is not in the copy");
        assert_eq!(row.try_get::<i64>("", "v").unwrap(), 42, "the committed row, still in the WAL, is");
    }
```

- [ ] **Step 2: Run it and see it fail.**

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping::backup::tests::the_backup_does_not_wait_for_the_apps_connection`
Expected: FAIL after about 10 s with `the backup must not wait for the app's connection` (the `VACUUM INTO` is queued behind the open transaction on the pool's single connection).

- [ ] **Step 3: Implement in `backup.rs`.** Add `Job`, and change `write` and `run` to take it. The body of `write` up to the quote check is unchanged apart from `dir` → `job.dir`.

```rust
/// Where the daily backup reads from and writes to (spec §25 P29).
#[derive(Debug, Clone)]
pub struct Job {
    /// The live database file.
    pub source: PathBuf,
    /// The backup directory.
    pub dir: PathBuf,
}

/// `VACUUM INTO` a fresh `.tmp` file beside the final name, so an unverified copy never wears it.
/// SQLite refuses to overwrite, so a stale temp left by a crashed run is deleted first.
/// The copy is made on its own read-only connection (spec §25 P29): the app's single pooled
/// connection stays free for the whole copy, and a missing source fails instead of being created.
pub async fn write(job: &Job, date: NaiveDate) -> Result<PathBuf, Error> {
    std::fs::create_dir_all(&job.dir)
        .map_err(|e| Error::new("Housekeeping:Backup", format!("cannot create {}: {e}", job.dir.display()), get_location!()))?;
    let path = job.dir.join(format!("{}{TMP}", file_name(date)));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| Error::new("Housekeeping:Backup", format!("cannot remove stale {}: {e}", path.display()), get_location!()))?;
    }
    let target = path.to_string_lossy();
    if target.contains('\'') {
        return Err(Error::new("Housekeeping:Backup", format!("backup path must not contain a quote: {target}"), get_location!()));
    }
    let source = Database::connect(format!("sqlite://{}?mode=ro", job.source.display()))
        .await
        .map_err(|e| Error::new("Housekeeping:Backup", format!("cannot open {}: {e}", job.source.display()), get_location!()))?;
    let copied = source.execute_unprepared(&format!("VACUUM INTO '{target}'")).await;
    // Closed before the result is looked at: the read snapshot must not outlive the copy.
    let _ = source.close().await;
    copied.map_err(|e| Error::new("Housekeeping:Backup", format!("VACUUM INTO {} failed: {e}", path.display()), get_location!()))?;
    Ok(path)
}
```

```rust
/// The daily job: skip if today's file exists; else write a temp copy, verify and rename it, prune.
pub async fn run(job: &Job, today: NaiveDate) -> Result<Option<PathBuf>, Error> {
    let final_path = job.dir.join(file_name(today));
    if final_path.exists() {
        return Ok(None);
    }
    let temp = write(job, today).await?;
    promote(&temp, &final_path).await?;
    let removed = prune(&job.dir)?;
    info(
        "Housekeeping:Backup",
        format!("Wrote {} and pruned {} old backup(s)", final_path.display(), removed.len()),
        &LoggerOptions::default(),
    );
    Ok(Some(final_path))
}
```

Also: change the module doc's first line to say the copy is made on the backup's own read-only connection (spec §19 H6, §25 P29), and drop any import the change leaves unused.

If, and only if, the bundled SQLite refuses `VACUUM INTO` on a `mode=ro` connection in Step 4, use `mode=rw` for the source URL instead (it also fails on a missing file), keep everything else, and say so in the report.

- [ ] **Step 4: Move the tests in `backup.rs` to the new signature and run them.** Add this helper to the `tests` module and use it wherever a test called `run(&conn, &backups, …)`:

```rust
    fn job(dir: &tempfile::TempDir) -> Job {
        Job { source: dir.path().join(crate::db::DB_FILE), dir: dir.path().join("backups") }
    }
```

- In `the_backup_does_not_wait_for_the_apps_connection`: `run(&job(&dir), d("2026-09-16"))`; the local `backups` is no longer needed.
- In `vacuum_into_writes_a_consistent_copy_once_per_day_and_prunes` and `a_bad_copy_is_removed_and_not_renamed`: bind `let (dir, _conn) = db().await;` (keep the connection alive, as in production), keep `let backups = dir.path().join("backups");` for the path assertions, and call `run(&job(&dir), …)`.

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping::backup`
Expected: PASS, including `the_backup_does_not_wait_for_the_apps_connection` in well under 10 s.

- [ ] **Step 5: Add the missing-source test** to `backup.rs` and run it:

```rust
    #[tokio::test]
    async fn a_missing_source_fails_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let job = Job { source: dir.path().join("nope.sqlite"), dir: dir.path().join("backups") };
        let today = d("2026-09-16");
        assert!(run(&job, today).await.is_err());
        assert!(!job.source.exists(), "the source is never created");
        assert!(!job.dir.join(file_name(today)).exists(), "no dated backup");
        assert!(!job.dir.join(format!("{}{TMP}", file_name(today))).exists(), "no temp file is left");
    }
```

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping::backup::tests::a_missing_source_fails_and_creates_nothing`
Expected: PASS.

- [ ] **Step 6: Thread `Job` through `housekeeping/mod.rs` and `startup.rs`.**

In `mod.rs`:
- `tick(…, backup: Option<&backup::Job>)`; the backup block becomes `if let Some(job) = backup { … match backup::run(job, today).await { … } }` with the gate and alert logic unchanged. Update the doc comment: "`backup = None` skips the backup job (tests)".
- `async fn run_loop(conn: DatabaseConnection, job: backup::Job)` calls `tick(&conn, &env, Utc::now(), &mut gates, Some(&job))`.
- `pub fn start(conn: DatabaseConnection, job: backup::Job)`:

```rust
pub fn start(conn: DatabaseConnection, job: backup::Job) {
    let dir = job.dir.clone();
    crate::collector::runner::supervise("Housekeeping", RESTART_DELAY, move || run_loop(conn.clone(), job.clone()));
    info("Housekeeping", format!("Started: tick {} s, backups in {}", TICK.as_secs(), dir.display()), &LoggerOptions::default());
}
```

- In the test `a_failed_backup_alerts_once_per_day`, build `let job = backup::Job { source: dir.path().join(crate::db::DB_FILE), dir: not_a_dir.clone() };` and pass `Some(&job)` in both `tick` calls. The `tick(…, None)` calls in the other tests stay as they are.
- Drop any import the change leaves unused (`Path`, `PathBuf`).

In `startup.rs`, replace line 98:

```rust
    crate::housekeeping::start(
        conn.clone(),
        crate::housekeeping::backup::Job { source: paths::get().data_dir.join(db::DB_FILE), dir: paths::get().backups_dir() },
    );
```

Run: `CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target cargo test -p qf_core --lib housekeeping`
Expected: PASS (all backup and housekeeping tests).

- [ ] **Step 7: Full gate.**

Run: `export CARGO_TARGET_DIR=~/Projects/Personal/quantframe-server/target && cargo test -p utils --lib && cargo test -p qf_core --lib && cargo test -p qf-server && python3 scripts/check-rpc-commands.py && (cd web && pnpm build)`
Expected: green; the same 44 compiler warnings as on `main`, no new one.

- [ ] **Step 8: Commit** `fix(housekeeping): run the daily backup on its own read-only connection`. Do not push.

---

## After the plan (controller and user)

1. Controller: review, re-run the gate on the branch tip.
2. User: deploy from `~/Projects/Personal/quantframe-server-backup-conn` (rsync + `docker compose up -d --build`), on the user's go-ahead only.
3. Acceptance at the next 00:00 UTC (spec P29): the server logs `Wrote /backups/quantframe-<date>.sqlite`; `journalctl --user -u qf-helper` shows no `heartbeat failed` around 17:00 Pacific; the new file answers `ok` to `PRAGMA integrity_check`.
4. Controller: append the record to `docs/PHASE-5-ACCEPTANCE.md`, gate, fast-forward `main`, push, remove the branch and worktree.

## Self-Review

- Spec coverage: P29 rule and `mode=ro` → Step 3; signatures → Steps 3 and 6; the three P29 tests → Steps 1 and 5 plus the migrated existing tests in Steps 4 and 6; acceptance → "After the plan". Out-of-scope items are not touched.
- Placeholders: none; every code step carries its code.
- Types: `backup::Job { source, dir }`, `run(job, today)`, `write(job, date)`, `tick(…, backup: Option<&backup::Job>)`, `start(conn, job)` are used with the same names and shapes in every step.
- Review Focus: all five lines map to a named test.
