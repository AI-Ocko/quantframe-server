//! Per-item trade tax from warframe.market's item detail (spec §25 P18).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use chrono::{DateTime, Utc};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::{get_location, warning, Error, LoggerOptions};

use super::backfill::{fetch_with_retries, StatisticsSource, StatsFuture};
use super::closed::pick;
use super::fetch::FetchError;
use super::{db_err, stmt, ts};
use crate::market::limiter::{Lane, Limiter};

pub const TAX_PACE_S: u64 = 10;
pub const TAX_IDLE_S: u64 = 600;
const TRANSIENT_RETRY_H: i64 = 1;

const C: &str = "TradeTax";

/// `data.tradingTax`; 0 when the field is absent, an error when the body is not a JSON object with `data`.
pub fn parse_trading_tax(body: &str) -> Result<i64, Error> {
    let body: serde_json::Value = serde_json::from_str(body).map_err(|e| Error::new(C, e.to_string(), get_location!()))?;
    let data = body.get("data").and_then(|d| d.as_object()).ok_or_else(|| Error::new(C, "no data object", get_location!()))?;
    Ok(match data.get("tradingTax") {
        None => 0,
        // A present but non-integer tax is still 0, so a schema change is visible instead of a silent universal pass.
        Some(value) => value.as_i64().unwrap_or_else(|| {
            warning(C, format!("tradingTax is not an integer: {value}"), &LoggerOptions::default());
            0
        }),
    })
}

/// Active `sweep_state` items with no tax row yet, by item id.
pub async fn missing_items(conn: &DatabaseConnection) -> Result<Vec<(String, String)>, Error> {
    conn.query_all(stmt(
        "SELECT s.item_id, s.slug FROM sweep_state s LEFT JOIN item_trade_tax t ON t.item_id = s.item_id
         WHERE s.active = 1 AND t.item_id IS NULL ORDER BY s.item_id",
        vec![],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|r| Ok((r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "slug").map_err(|e| db_err(C, e))?)))
    .collect()
}

pub async fn upsert_tax(conn: &DatabaseConnection, item_id: &str, trading_tax: i64, at: DateTime<Utc>) -> Result<(), Error> {
    conn.execute(stmt(
        "INSERT INTO item_trade_tax (item_id, trading_tax, fetched_at) VALUES (?, ?, ?)
         ON CONFLICT (item_id) DO UPDATE SET trading_tax = excluded.trading_tax, fetched_at = excluded.fetched_at",
        vec![item_id.into(), trading_tax.into(), ts(at).into()],
    ))
    .await
    .map_err(|e| db_err(C, e))?;
    Ok(())
}

pub async fn load_all(conn: &DatabaseConnection) -> Result<HashMap<String, i64>, Error> {
    conn.query_all(stmt("SELECT item_id, trading_tax FROM item_trade_tax", vec![]))
        .await
        .map_err(|e| db_err(C, e))?
        .iter()
        .map(|r| Ok((r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "trading_tax").map_err(|e| db_err(C, e))?)))
        .collect()
}

/// Unauthenticated client for the public v2 item detail, mirroring `backfill::HttpStatisticsSource`.
pub struct HttpItemDetailSource {
    http: reqwest::Client,
    base_url: String,
}

impl HttpItemDetailSource {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        Self { http, base_url: base_url.into() }
    }
}

impl StatisticsSource for HttpItemDetailSource {
    fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a> {
        Box::pin(async move {
            let url = format!("{}/item/{}", self.base_url, slug);
            let response = self
                .http
                .get(&url)
                .header("Platform", "pc")
                .header("Language", "en")
                .send()
                .await
                .map_err(|e| FetchError::Transient(e.to_string()))?;
            match response.status().as_u16() {
                200 => {}
                404 => return Err(FetchError::NotFound),
                429 => return Err(FetchError::RateLimited),
                code => return Err(FetchError::Transient(format!("HTTP {code} for {url}"))),
            }
            response.text().await.map_err(|e| FetchError::Transient(e.to_string()))
        })
    }
}

/// Items nothing was stored for, and when each may be picked again: without this the same item
/// would be picked every `TAX_PACE_S` and block the queue. A restart clears it.
static DEFERRED: OnceLock<Mutex<HashMap<String, DateTime<Utc>>>> = OnceLock::new();

/// A 404 or an unparseable body will not change, so that item is set aside for the whole process.
fn for_the_run() -> DateTime<Utc> {
    DateTime::<Utc>::MAX_UTC
}

pub async fn fetch_once(
    conn: &DatabaseConnection,
    source: &dyn StatisticsSource,
    limiter: &Limiter,
    hot: &HashSet<String>,
    now: DateTime<Utc>,
) -> Result<Option<usize>, Error> {
    fetch_once_with(DEFERRED.get_or_init(Mutex::default), conn, source, limiter, hot, now).await
}

/// Fetches one item's tax through the Cold lane. `None` when nothing is missing; otherwise the missing count before this fetch.
pub(crate) async fn fetch_once_with(
    deferred: &Mutex<HashMap<String, DateTime<Utc>>>,
    conn: &DatabaseConnection,
    source: &dyn StatisticsSource,
    limiter: &Limiter,
    hot: &HashSet<String>,
    now: DateTime<Utc>,
) -> Result<Option<usize>, Error> {
    let all = missing_items(conn).await?;
    let missing: Vec<(String, String)> = {
        let set_aside = deferred.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        all.into_iter().filter(|(id, _)| !set_aside.get(id).is_some_and(|until| *until > now)).collect()
    };
    let Some((item_id, slug)) = pick(&missing, hot) else { return Ok(None) };
    // `None` when the tax was stored; otherwise when this item may be picked again.
    let set_aside_until = match fetch_with_retries(source, limiter, Lane::Cold, &slug).await {
        Ok(body) => match parse_trading_tax(&body) {
            Ok(tax) => {
                upsert_tax(conn, &item_id, tax, now).await?;
                None
            }
            Err(e) => {
                warning(C, format!("{slug}: {}", e.message), &LoggerOptions::default());
                Some(for_the_run())
            }
        },
        Err(FetchError::NotFound) => Some(for_the_run()),
        // An outage or a rate-limit storm must not strand items until a restart, so a transient
        // failure only costs an hour, as it does in the closed-statistics loop.
        Err(e) => {
            warning(C, format!("{slug}: {e}"), &LoggerOptions::default());
            Some(now + chrono::Duration::hours(TRANSIENT_RETRY_H))
        }
    };
    if let Some(until) = set_aside_until {
        deferred.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(item_id, until);
    }
    Ok(Some(missing.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::backfill::StatsFuture;
    use crate::collector::fetch::FetchError;
    use crate::collector::parse_ts;
    use crate::collector::store::{exec, tests::setup};
    use chrono::Duration;

    const SMALL: &str = include_str!("../../tests/fixtures/item_detail_small.json");

    struct Scripted(std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<Result<String, FetchError>>>>);
    impl StatisticsSource for Scripted {
        fn fetch<'a>(&'a self, slug: &'a str) -> StatsFuture<'a> {
            let next = self.0.lock().unwrap().get_mut(slug).and_then(|q| q.pop_front()).unwrap_or(Err(FetchError::Transient("unscripted".into())));
            Box::pin(async move { next })
        }
    }
    fn scripted(entries: Vec<(&str, Vec<Result<String, FetchError>>)>) -> Scripted {
        Scripted(std::sync::Mutex::new(entries.into_iter().map(|(k, v)| (k.to_string(), v.into())).collect()))
    }

    /// Each test owns its deferral map, so the process-wide one in `fetch_once` cannot leak between them.
    fn fresh_deferred() -> Mutex<HashMap<String, DateTime<Utc>>> {
        Mutex::new(HashMap::new())
    }

    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-22T12:00:00Z").unwrap()
    }

    #[test]
    fn parses_the_trading_tax_and_defaults_a_missing_field_to_zero() {
        assert_eq!(parse_trading_tax(SMALL).unwrap(), 1_000_000);
        assert_eq!(parse_trading_tax(r#"{"data":{"slug":"x"},"error":null}"#).unwrap(), 0, "no tradingTax means no tax");
        assert_eq!(parse_trading_tax(r#"{"data":{"tradingTax":"lots"}}"#).unwrap(), 0, "a non-integer tax is 0, with a warning");
        assert_eq!(parse_trading_tax(r#"{"data":{"tradingTax":1.5}}"#).unwrap(), 0, "a float tax is 0, with a warning");
        assert!(parse_trading_tax("not json").is_err());
        assert!(parse_trading_tax(r#"{"error":null}"#).is_err(), "a body without data is not an answer");
    }

    #[tokio::test]
    async fn missing_items_lists_active_items_without_a_row() {
        let (_dir, conn) = setup().await; // item1/slug1 and item2/slug2, both active
        assert_eq!(missing_items(&conn).await.unwrap(), vec![("item1".to_string(), "slug1".to_string()), ("item2".to_string(), "slug2".to_string())]);
        upsert_tax(&conn, "item1", 8_000, now()).await.unwrap();
        assert_eq!(missing_items(&conn).await.unwrap(), vec![("item2".to_string(), "slug2".to_string())]);
        exec(&conn, "Test", "UPDATE sweep_state SET active = 0 WHERE item_id = 'item2'", vec![]).await.unwrap();
        assert!(missing_items(&conn).await.unwrap().is_empty(), "inactive items are skipped");
    }

    #[tokio::test]
    async fn upsert_replaces_a_tax_and_load_all_returns_the_map() {
        let (_dir, conn) = setup().await;
        assert!(load_all(&conn).await.unwrap().is_empty());
        upsert_tax(&conn, "item1", 8_000, now()).await.unwrap();
        upsert_tax(&conn, "item2", 0, now()).await.unwrap();
        upsert_tax(&conn, "item1", 2_100_000, now()).await.unwrap();
        assert_eq!(load_all(&conn).await.unwrap(), HashMap::from([("item1".to_string(), 2_100_000), ("item2".to_string(), 0)]));
    }

    #[tokio::test]
    async fn fetch_once_stores_a_tax_stores_nothing_on_a_404_and_stops_when_nothing_is_missing() {
        let (_dir, conn) = setup().await;
        let deferred = fresh_deferred();
        let limiter = Limiter::new(1000);
        let source = scripted(vec![("slug1", vec![Ok(SMALL.to_string())]), ("slug2", vec![Err(FetchError::NotFound)])]);
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), Some(2));
        assert_eq!(load_all(&conn).await.unwrap(), HashMap::from([("item1".to_string(), 1_000_000)]));
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), Some(1));
        assert_eq!(load_all(&conn).await.unwrap(), HashMap::from([("item1".to_string(), 1_000_000)]), "the 404 stored nothing");
        assert_eq!(missing_items(&conn).await.unwrap(), vec![("item2".to_string(), "slug2".to_string())], "item2 still has no row");
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), None, "the 404 item is not picked again");
        upsert_tax(&conn, "item2", 0, now()).await.unwrap();
        assert_eq!(fetch_once_with(&fresh_deferred(), &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), None, "nothing is missing");
    }

    #[tokio::test]
    async fn fetch_once_prefers_a_hot_item() {
        let (_dir, conn) = setup().await;
        let source = scripted(vec![("slug2", vec![Ok(SMALL.to_string())])]);
        fetch_once_with(&fresh_deferred(), &conn, &source, &Limiter::new(1000), &HashSet::from(["item2".to_string()]), now()).await.unwrap();
        assert_eq!(load_all(&conn).await.unwrap(), HashMap::from([("item2".to_string(), 1_000_000)]));
    }

    #[tokio::test]
    async fn a_404_and_an_exhausted_retry_move_on_to_the_next_item() {
        let (_dir, conn) = setup().await;
        exec(&conn, "Test", "INSERT INTO sweep_state (item_id, slug, active) VALUES ('item3', 'slug3', 1)", vec![]).await.unwrap();
        let deferred = fresh_deferred();
        let limiter = Limiter::new(1000);
        let transient = || Err(FetchError::Transient("boom".into()));
        let source = scripted(vec![
            ("slug1", vec![Err(FetchError::NotFound)]),
            ("slug2", vec![transient(), transient(), transient(), Ok(SMALL.to_string())]),
            ("slug3", vec![Ok(SMALL.to_string())]),
        ]);
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), Some(3), "item1: 404");
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), Some(2), "item2, not item1 again");
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), Some(1), "item3, the last one left");
        assert_eq!(load_all(&conn).await.unwrap(), HashMap::from([("item3".to_string(), 1_000_000)]), "only the item that answered has a row");
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), None, "both failures are out of the queue");

        // spec §25 P18: a transient failure sets an item aside for an hour, not for the run.
        assert_eq!(
            fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now() + Duration::minutes(59)).await.unwrap(),
            None,
            "item2's hour is not up yet"
        );
        assert_eq!(
            fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now() + Duration::minutes(61)).await.unwrap(),
            Some(1),
            "item2 is eligible again after an hour; item1's 404 still is not"
        );
        assert_eq!(
            load_all(&conn).await.unwrap(),
            HashMap::from([("item2".to_string(), 1_000_000), ("item3".to_string(), 1_000_000)]),
            "the retry stored item2's tax"
        );
        assert_eq!(
            fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now() + Duration::hours(2)).await.unwrap(),
            None,
            "the 404 item is never picked again in this process"
        );
    }

    #[tokio::test]
    async fn an_unparseable_body_sets_the_item_aside_for_the_whole_process() {
        let (_dir, conn) = setup().await;
        let deferred = fresh_deferred();
        let limiter = Limiter::new(1000);
        let source = scripted(vec![("slug1", vec![Ok("not json".to_string()), Ok(SMALL.to_string())]), ("slug2", vec![Ok(SMALL.to_string())])]);
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), now()).await.unwrap(), Some(2));
        assert!(load_all(&conn).await.unwrap().is_empty(), "an unparseable body stores nothing");
        let later = now() + Duration::hours(2);
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), later).await.unwrap(), Some(1), "item2 is next, item1 is out");
        assert_eq!(load_all(&conn).await.unwrap(), HashMap::from([("item2".to_string(), 1_000_000)]));
        assert_eq!(fetch_once_with(&deferred, &conn, &source, &limiter, &HashSet::new(), later).await.unwrap(), None, "the unparseable item is not retried hours later");
    }
}
