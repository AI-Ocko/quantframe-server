//! Trader lifecycle: checklist, start and stop sequences, and the monitor tick (spec §5.7, amendments C7–C10, D5).

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use service::sea_orm::DatabaseConnection;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use utils::{error, get_location, info, Error, LoggerOptions};

use super::engine::EngineExit;
use super::lifecycle::{stop_trigger, Checklist, LifecycleState, StopReason, TriggerInput};
use super::session::SessionSnapshot;
use super::store::{self, TraderOptions};
use crate::collector::ts;
use crate::helper_link::presence::HelperSnapshot;

pub const MONITOR_EVERY: StdDuration = StdDuration::from_secs(5);
const PRUNE_EVERY: StdDuration = StdDuration::from_secs(60 * 60);

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Everything the controller does outside itself. Faked in tests, `LivePlatform` in production.
pub trait Platform: Send + Sync {
    fn session(&self, now: DateTime<Utc>) -> SessionSnapshot;
    fn helper(&self, now: DateTime<Utc>) -> HelperSnapshot;
    fn game_data_loaded(&self) -> bool;
    /// `live_scraper.general.auto_delete` (amendment H1).
    fn auto_delete(&self) -> bool;
    fn spawn_engine(&self, dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit>;
    fn set_status(&self, status: &'static str) -> BoxFuture<'_, Result<(), Error>>;
    fn delete_live_buy_orders(&self) -> BoxFuture<'_, Result<usize, Error>>;
    fn notify_stopped(&self, reason: &StopReason, dry_run: bool, at: DateTime<Utc>);
    fn token_expiry_alert_due(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>>;
    fn notify_token_expiring(&self, expires_at: DateTime<Utc>, now: DateTime<Utc>);
    fn broadcast(&self, status: &TraderStatus);
}

#[derive(Debug, Clone, Serialize)]
pub struct TraderStatus {
    pub state: LifecycleState,
    pub checklist: Checklist,
    pub options: TraderOptions,
    pub session: SessionSnapshot,
    pub helper: HelperSnapshot,
    pub running_since: Option<String>,
    pub running_dry_run: Option<bool>,
}

struct Running {
    flag: Arc<AtomicBool>,
    handle: JoinHandle<EngineExit>,
    started_at: DateTime<Utc>,
    dry_run: bool,
}

struct Inner {
    state: LifecycleState,
    options: TraderOptions,
    running: Option<Running>,
}

pub struct TraderController {
    conn: DatabaseConnection,
    platform: Arc<dyn Platform>,
    inner: Mutex<Inner>,
}

impl TraderController {
    /// Loads the saved options and starts `Offline`. It never resumes trading (spec §5.7).
    pub async fn new(conn: DatabaseConnection, platform: Arc<dyn Platform>) -> Result<Self, Error> {
        let options = store::load_options(&conn).await?;
        Ok(Self { conn, platform, inner: Mutex::new(Inner { state: LifecycleState::Offline, options, running: None }) })
    }

    pub fn conn(&self) -> &DatabaseConnection {
        &self.conn
    }

    fn checklist(&self, session: &SessionSnapshot, helper: &HelperSnapshot) -> Checklist {
        Checklist {
            token_valid: session.token_valid,
            ws_connected: session.ws_connected,
            game_data_loaded: self.platform.game_data_loaded(),
            helper_connected: helper.connected,
            warframe_running: helper.warframe_running,
            auto_delete_off: !self.platform.auto_delete(),
        }
    }

    fn status_of(&self, inner: &Inner, now: DateTime<Utc>) -> TraderStatus {
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        TraderStatus {
            state: inner.state,
            checklist: self.checklist(&session, &helper),
            options: inner.options.clone(),
            session,
            helper,
            running_since: inner.running.as_ref().map(|r| ts(r.started_at)),
            running_dry_run: inner.running.as_ref().map(|r| r.dry_run),
        }
    }

    fn idle_state(&self, dry_run: bool, now: DateTime<Utc>) -> LifecycleState {
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        if self.checklist(&session, &helper).ready_for(dry_run) { LifecycleState::Ready } else { LifecycleState::Offline }
    }

    pub async fn status(&self, now: DateTime<Utc>) -> TraderStatus {
        let inner = self.inner.lock().await;
        self.status_of(&inner, now)
    }

    /// `Ready → Trading` (spec §5.7): status `ingame` unless dry-run, start the loop, broadcast.
    pub async fn start(&self, now: DateTime<Utc>) -> Result<TraderStatus, Error> {
        let mut inner = self.inner.lock().await;
        if inner.running.is_some() {
            return Ok(self.status_of(&inner, now));
        }
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        let dry_run = inner.options.dry_run;
        if !self.checklist(&session, &helper).ready_for(dry_run) {
            return Err(Error::new("Trader:Start", "The trader is not ready; see the start checklist", get_location!()));
        }
        if !dry_run {
            self.platform.set_status("ingame").await?;
        }
        let flag = Arc::new(AtomicBool::new(true));
        let handle = self.platform.spawn_engine(dry_run, flag.clone());
        inner.running = Some(Running { flag, handle, started_at: now, dry_run });
        inner.state = LifecycleState::Trading;
        info("Trader:Start", format!("Trader started ({})", if dry_run { "dry-run" } else { "live" }), &LoggerOptions::default());
        let status = self.status_of(&inner, now);
        self.platform.broadcast(&status);
        Ok(status)
    }

    /// `Trading → Stopping → Ready/Offline`. When `reason` is `None`, it comes from how the engine exited.
    async fn finish(&self, inner: &mut Inner, reason: Option<StopReason>, now: DateTime<Utc>) -> Result<Option<StopReason>, Error> {
        let Some(running) = inner.running.take() else { return Ok(None) };
        inner.state = LifecycleState::Stopping;
        self.platform.broadcast(&self.status_of(inner, now));

        running.flag.store(false, Ordering::SeqCst);
        let exit = running.handle.await;
        let reason = reason.unwrap_or_else(|| match exit {
            Ok(EngineExit::Critical(message)) => StopReason::TraderCritical(message),
            Ok(EngineExit::OrderFailures(count)) => StopReason::OrderFailures(count),
            Ok(EngineExit::Stopped) => StopReason::UserStop,
            Err(join_error) => StopReason::TraderPanic(join_error.to_string()),
        });

        if let Err(e) = self.platform.set_status("invisible").await {
            error("Trader:Stop", format!("Could not set status invisible: {}", e.message), &LoggerOptions::default());
        }
        if inner.options.delete_buy_orders_on_stop && !running.dry_run {
            match self.platform.delete_live_buy_orders().await {
                Ok(count) => info("Trader:Stop", format!("Deleted {} buy orders", count), &LoggerOptions::default()),
                Err(e) => error("Trader:Stop", format!("Could not delete buy orders: {}", e.message), &LoggerOptions::default()),
            }
        }
        let description = reason.describe();
        if let Err(e) = store::record_stop(&self.conn, &description, now).await {
            let _ = e.log("trader.log");
        }
        inner.options.last_stop_reason = Some(description.clone());
        inner.options.last_stop_at = Some(ts(now));
        self.platform.notify_stopped(&reason, running.dry_run, now);
        inner.state = self.idle_state(inner.options.dry_run, now);
        info("Trader:Stop", format!("Trader stopped: {}", description), &LoggerOptions::default());
        self.platform.broadcast(&self.status_of(inner, now));
        Ok(Some(reason))
    }

    pub async fn stop(&self, reason: StopReason, now: DateTime<Utc>) -> Result<TraderStatus, Error> {
        let mut inner = self.inner.lock().await;
        self.finish(&mut inner, Some(reason), now).await?;
        Ok(self.status_of(&inner, now))
    }

    /// Runs every `MONITOR_EVERY`: expiry alerts, idle state, engine exits and stop triggers.
    pub async fn tick(&self, now: DateTime<Utc>) -> Result<Option<StopReason>, Error> {
        if let Some(expires_at) = self.platform.token_expiry_alert_due(now) {
            self.platform.notify_token_expiring(expires_at, now);
        }
        let mut inner = self.inner.lock().await;
        let session = self.platform.session(now);
        let helper = self.platform.helper(now);
        let checklist = self.checklist(&session, &helper);
        let Some(running) = inner.running.as_ref() else {
            let next = if checklist.ready_for(inner.options.dry_run) { LifecycleState::Ready } else { LifecycleState::Offline };
            if next != inner.state {
                inner.state = next;
                self.platform.broadcast(&self.status_of(&inner, now));
            }
            return Ok(None);
        };
        if running.handle.is_finished() {
            return self.finish(&mut inner, None, now).await;
        }
        let trigger = stop_trigger(&TriggerInput {
            signed_in: session.signed_in,
            unauthorized: session.unauthorized,
            ws_down_for_s: session.ws_down_for_s,
            helper_seconds_since: helper.seconds_since_heartbeat,
            warframe_running: helper.warframe_running,
        });
        match trigger {
            Some(reason) => self.finish(&mut inner, Some(reason), now).await,
            None => Ok(None),
        }
    }

    pub async fn set_options(
        &self,
        dry_run: Option<bool>,
        delete_buy_orders_on_stop: Option<bool>,
    ) -> Result<TraderOptions, Error> {
        let mut inner = self.inner.lock().await;
        let mut next = inner.options.clone();
        if let Some(value) = dry_run {
            if inner.running.is_some() && value != next.dry_run {
                return Err(Error::new("Trader:Options", "Stop the trader before changing dry-run", get_location!()));
            }
            next.dry_run = value;
        }
        if let Some(value) = delete_buy_orders_on_stop {
            next.delete_buy_orders_on_stop = value;
        }
        store::save_flags(&self.conn, next.dry_run, next.delete_buy_orders_on_stop).await?;
        inner.options = next.clone();
        // While idle the badge follows the mode, so flip it here instead of waiting for the next tick.
        if inner.running.is_none() {
            inner.state = self.idle_state(next.dry_run, Utc::now());
        }
        self.platform.broadcast(&self.status_of(&inner, Utc::now()));
        Ok(next)
    }
}

pub async fn monitor_loop(controller: Arc<TraderController>) {
    let mut last_prune: Option<Instant> = None;
    loop {
        let now = Utc::now();
        if let Err(e) = controller.tick(now).await {
            let _ = e.log("trader.log");
        }
        if last_prune.is_none_or(|t| t.elapsed() >= PRUNE_EVERY) {
            if let Err(e) = store::prune_dry_run(controller.conn(), now).await {
                let _ = e.log("trader.log");
            }
            last_prune = Some(Instant::now());
        }
        tokio::time::sleep(MONITOR_EVERY).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Fake {
        session: StdMutex<SessionSnapshot>,
        helper: StdMutex<HelperSnapshot>,
        loaded: AtomicBool,
        auto_delete: AtomicBool,
        exit: StdMutex<Option<EngineExit>>,
        statuses: StdMutex<Vec<&'static str>>,
        deletes: AtomicUsize,
        stopped: StdMutex<Vec<(StopReason, bool)>>,
        expiry: StdMutex<Option<DateTime<Utc>>>,
        expiry_alerts: AtomicUsize,
        broadcasts: AtomicUsize,
    }

    impl Platform for Fake {
        fn session(&self, _now: DateTime<Utc>) -> SessionSnapshot {
            self.session.lock().unwrap().clone()
        }
        fn helper(&self, _now: DateTime<Utc>) -> HelperSnapshot {
            self.helper.lock().unwrap().clone()
        }
        fn game_data_loaded(&self) -> bool {
            self.loaded.load(Ordering::SeqCst)
        }
        fn auto_delete(&self) -> bool {
            self.auto_delete.load(Ordering::SeqCst)
        }
        fn spawn_engine(&self, _dry_run: bool, running: Arc<AtomicBool>) -> JoinHandle<EngineExit> {
            let exit = self.exit.lock().unwrap().clone();
            tokio::spawn(async move {
                if let Some(exit) = exit {
                    return exit;
                }
                while running.load(Ordering::SeqCst) {
                    tokio::time::sleep(StdDuration::from_millis(5)).await;
                }
                EngineExit::Stopped
            })
        }
        fn set_status(&self, status: &'static str) -> BoxFuture<'_, Result<(), Error>> {
            self.statuses.lock().unwrap().push(status);
            Box::pin(async { Ok(()) })
        }
        fn delete_live_buy_orders(&self) -> BoxFuture<'_, Result<usize, Error>> {
            self.deletes.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(2) })
        }
        fn notify_stopped(&self, reason: &StopReason, dry_run: bool, _at: DateTime<Utc>) {
            self.stopped.lock().unwrap().push((reason.clone(), dry_run));
        }
        fn token_expiry_alert_due(&self, _now: DateTime<Utc>) -> Option<DateTime<Utc>> {
            self.expiry.lock().unwrap().take()
        }
        fn notify_token_expiring(&self, _expires_at: DateTime<Utc>, _now: DateTime<Utc>) {
            self.expiry_alerts.fetch_add(1, Ordering::SeqCst);
        }
        fn broadcast(&self, _status: &TraderStatus) {
            self.broadcasts.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-16T12:00:00Z").unwrap()
    }

    fn healthy_session() -> SessionSnapshot {
        SessionSnapshot { signed_in: true, token_valid: true, ws_connected: true, ..Default::default() }
    }

    fn healthy_helper() -> HelperSnapshot {
        HelperSnapshot { connected: true, warframe_running: true, seconds_since_heartbeat: Some(3), ..Default::default() }
    }

    /// Ready in dry-run with a fresh heartbeat from a running game.
    async fn ready_controller() -> (tempfile::TempDir, Arc<Fake>, TraderController) {
        let (dir, conn) = crate::trader::store::tests::db().await;
        store::save_flags(&conn, true, true).await.unwrap();
        let fake = Arc::new(Fake::default());
        *fake.session.lock().unwrap() = healthy_session();
        *fake.helper.lock().unwrap() = healthy_helper();
        fake.loaded.store(true, Ordering::SeqCst);
        let controller = TraderController::new(conn, fake.clone()).await.unwrap();
        (dir, fake, controller)
    }

    #[tokio::test]
    async fn starts_offline_then_ticks_to_ready_and_never_trades_by_itself() {
        let (_dir, _fake, controller) = ready_controller().await;
        assert_eq!(controller.status(now()).await.state, LifecycleState::Offline);
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Ready);
    }

    #[tokio::test]
    async fn without_a_heartbeat_the_trader_stays_offline() {
        let (_dir, fake, controller) = ready_controller().await;
        *fake.helper.lock().unwrap() = HelperSnapshot::default();
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        let status = controller.status(now()).await;
        assert_eq!(status.state, LifecycleState::Offline);
        assert!(!status.checklist.helper_connected && !status.checklist.warframe_running);
        assert!(controller.start(now()).await.is_err());
    }

    #[tokio::test]
    async fn a_live_start_is_refused_while_auto_delete_is_on_but_a_dry_run_start_is_not() {
        let (_dir, fake, controller) = ready_controller().await;
        fake.auto_delete.store(true, Ordering::SeqCst);
        // Dry-run (the default from ready_controller): the item is informational.
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Ready);
        assert!(!controller.status(now()).await.checklist.auto_delete_off);
        let started = controller.start(now()).await.unwrap();
        assert_eq!(started.state, LifecycleState::Trading);
        controller.stop(StopReason::UserStop, now()).await.unwrap();

        // Live: the badge goes Offline at once, without waiting for a tick, and start() refuses.
        controller.set_options(Some(false), None).await.unwrap();
        assert_eq!(controller.status(now()).await.state, LifecycleState::Offline, "set_options flips the badge itself");
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Offline);
        let err = controller.start(now()).await.unwrap_err();
        assert_eq!(err.component, "Trader:Start");

        // Turning auto_delete off makes the same live start possible.
        fake.auto_delete.store(false, Ordering::SeqCst);
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Ready);
        assert_eq!(controller.start(now()).await.unwrap().state, LifecycleState::Trading);
        controller.stop(StopReason::UserStop, now()).await.unwrap();
    }

    #[tokio::test]
    async fn start_is_refused_until_the_checklist_passes() {
        let (_dir, fake, controller) = ready_controller().await;
        fake.session.lock().unwrap().ws_connected = false;
        assert!(controller.start(now()).await.is_err());
        fake.session.lock().unwrap().ws_connected = true;
        fake.helper.lock().unwrap().warframe_running = false;
        let status = controller.status(now()).await;
        assert!(status.checklist.helper_connected && !status.checklist.warframe_running);
        assert!(controller.start(now()).await.is_err());
        fake.helper.lock().unwrap().warframe_running = true;
        assert_eq!(controller.start(now()).await.unwrap().state, LifecycleState::Trading);
    }

    #[tokio::test]
    async fn dry_run_start_and_stop_follow_the_sequence() {
        let (_dir, fake, controller) = ready_controller().await;
        let status = controller.start(now()).await.unwrap();
        assert_eq!(status.state, LifecycleState::Trading);
        assert_eq!(status.running_dry_run, Some(true));
        assert_eq!(status.helper, healthy_helper());
        assert!(fake.statuses.lock().unwrap().is_empty(), "no ingame status in dry-run");

        let status = controller.stop(StopReason::UserStop, now()).await.unwrap();
        assert_eq!(status.state, LifecycleState::Ready);
        assert_eq!(*fake.statuses.lock().unwrap(), vec!["invisible"]);
        assert_eq!(fake.deletes.load(Ordering::SeqCst), 0, "dry-run never deletes real buy orders");
        assert_eq!(*fake.stopped.lock().unwrap(), vec![(StopReason::UserStop, true)]);
        assert_eq!(status.options.last_stop_reason.as_deref(), Some("Stop button"));
        let saved = store::load_options(controller.conn()).await.unwrap();
        assert_eq!(saved.last_stop_reason.as_deref(), Some("Stop button"));
    }

    #[tokio::test]
    async fn session_problems_stop_trading() {
        for (edit, expected) in [
            (Box::new(|s: &mut SessionSnapshot| s.unauthorized = true) as Box<dyn Fn(&mut SessionSnapshot)>, StopReason::Unauthorized),
            (Box::new(|s: &mut SessionSnapshot| { s.ws_connected = false; s.ws_down_for_s = Some(61) }), StopReason::WebsocketDown),
            (Box::new(|s: &mut SessionSnapshot| s.signed_in = false), StopReason::SignedOut),
        ] {
            let (_dir, fake, controller) = ready_controller().await;
            controller.start(now()).await.unwrap();
            edit(&mut fake.session.lock().unwrap());
            assert_eq!(controller.tick(now()).await.unwrap(), Some(expected.clone()));
            assert_ne!(controller.status(now()).await.state, LifecycleState::Trading);
        }
    }

    #[tokio::test]
    async fn helper_problems_stop_trading() {
        for (edit, expected) in [
            (Box::new(|h: &mut HelperSnapshot| h.warframe_running = false) as Box<dyn Fn(&mut HelperSnapshot)>, StopReason::WarframeClosed),
            (Box::new(|h: &mut HelperSnapshot| { h.connected = false; h.seconds_since_heartbeat = Some(61) }), StopReason::HelperSilent),
        ] {
            let (_dir, fake, controller) = ready_controller().await;
            controller.start(now()).await.unwrap();
            edit(&mut fake.helper.lock().unwrap());
            assert_eq!(controller.tick(now()).await.unwrap(), Some(expected.clone()));
            assert_ne!(controller.status(now()).await.state, LifecycleState::Trading);
        }
    }

    #[tokio::test]
    async fn a_heartbeat_under_a_minute_old_keeps_trading() {
        let (_dir, fake, controller) = ready_controller().await;
        controller.start(now()).await.unwrap();
        {
            let mut helper = fake.helper.lock().unwrap();
            helper.connected = false;
            helper.seconds_since_heartbeat = Some(45);
        }
        assert_eq!(controller.tick(now()).await.unwrap(), None);
        assert_eq!(controller.status(now()).await.state, LifecycleState::Trading);
    }

    #[tokio::test]
    async fn engine_exit_becomes_the_stop_reason() {
        let (_dir, fake, controller) = ready_controller().await;
        *fake.exit.lock().unwrap() = Some(EngineExit::Critical("Trader:Item: bad request".into()));
        controller.start(now()).await.unwrap();
        tokio::time::sleep(StdDuration::from_millis(20)).await;
        assert_eq!(
            controller.tick(now()).await.unwrap(),
            Some(StopReason::TraderCritical("Trader:Item: bad request".into()))
        );
    }

    #[tokio::test]
    async fn dry_run_cannot_change_while_trading() {
        let (_dir, _fake, controller) = ready_controller().await;
        controller.start(now()).await.unwrap();
        assert!(controller.set_options(Some(false), None).await.is_err());
        assert!(controller.set_options(None, Some(false)).await.is_ok());
    }

    #[tokio::test]
    async fn tick_sends_token_expiry_alerts() {
        let (_dir, fake, controller) = ready_controller().await;
        *fake.expiry.lock().unwrap() = Some(now());
        controller.tick(now()).await.unwrap();
        controller.tick(now()).await.unwrap();
        assert_eq!(fake.expiry_alerts.load(Ordering::SeqCst), 1);
    }
}
