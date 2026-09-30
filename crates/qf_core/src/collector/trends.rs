//! Long-term price trends and a conservative hold projection (spec §25 P28).

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::closed::median_f64;
use super::{db_err, stmt, ts};

pub const TREND_WINDOW_DAYS: i64 = 90;
pub const MIN_WEEKS: usize = 11;
pub const QUOTE_MAX_AGE_H: i64 = 24;

/// One closed day with trades; only rows with `median > 0` become points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailyPoint {
    pub day: NaiveDate,
    pub volume: i64,
    pub median: f64,
}

/// Least squares of `ln(weekly price)` on the week index: `ln p ≈ a + b·w` with residual sd `s`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    pub a: f64,
    pub b: f64,
    pub r2: f64,
    pub s: f64,
    pub last_week: usize,
    pub weeks: usize,
    pub price_now: f64,
    pub volume: f64,
    pub trend_intact: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quote {
    pub ask: f64,
    pub bid: f64,
    /// `"sweep"` or `"closed"`.
    pub source: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    pub exit_low: f64,
    pub exit_mid: f64,
    pub profit_low: f64,
    pub profit_low_pct: f64,
    pub profit_mid: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldThresholds {
    pub horizon_weeks: i64,
    pub min_volume: f64,
    pub min_margin_pct: f64,
    pub min_steadiness: f64,
    pub min_weeks: usize,
}

/// Points with trades inside `latest − TREND_WINDOW_DAYS ..= latest`, with their week index.
fn in_window(points: &[DailyPoint], latest: NaiveDate) -> impl Iterator<Item = (usize, &DailyPoint)> {
    let first = latest - Duration::days(TREND_WINDOW_DAYS);
    points.iter().filter(move |p| p.median > 0.0 && p.day >= first && p.day <= latest).map(move |p| (((p.day - first).num_days() / 7) as usize, p))
}

/// `(week index, median of that week's daily medians)`, ascending by week.
pub fn weekly_prices(points: &[DailyPoint], latest: NaiveDate) -> Vec<(usize, f64)> {
    let mut weeks: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for (w, p) in in_window(points, latest) {
        weeks.entry(w).or_default().push(p.median);
    }
    weeks.into_iter().filter_map(|(w, medians)| Some((w, median_f64(&medians)?))).collect()
}

/// `None` below `MIN_WEEKS` scored weeks or when any output is not finite.
pub fn fit(points: &[DailyPoint], latest: NaiveDate) -> Option<Fit> {
    let weekly = weekly_prices(points, latest);
    let n = weekly.len();
    if n < MIN_WEEKS {
        return None;
    }
    let xs: Vec<f64> = weekly.iter().map(|(w, _)| *w as f64).collect();
    let ys: Vec<f64> = weekly.iter().map(|(_, p)| p.ln()).collect();
    let nf = n as f64;
    let (mx, my) = (xs.iter().sum::<f64>() / nf, ys.iter().sum::<f64>() / nf);
    let sxx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let b = sxy / sxx;
    let a = my - b * mx;
    let sse: f64 = xs.iter().zip(&ys).map(|(x, y)| (y - a - b * x).powi(2)).sum();
    let sst: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    let r2 = if sst == 0.0 { 0.0 } else { 1.0 - sse / sst };
    let s = (sse / (nf - 2.0)).sqrt();
    // The epsilon keeps a perfect series from failing its own band on rounding.
    let trend_intact = (n - 2..n).all(|i| ys[i] >= a + b * xs[i] - 2.0 * s - 1e-9);
    let volumes: Vec<i64> = in_window(points, latest).map(|(_, p)| p.volume).collect();
    let f = Fit {
        a,
        b,
        r2,
        s,
        last_week: weekly[n - 1].0,
        weeks: n,
        price_now: ((ys[n - 2] + ys[n - 1]) / 2.0).exp(),
        volume: volumes.iter().sum::<i64>() as f64 / volumes.len() as f64,
        trend_intact,
    };
    [f.a, f.b, f.r2, f.s, f.price_now, f.volume].iter().all(|v| v.is_finite()).then_some(f)
}

pub fn trend_pct_month(b: f64) -> f64 {
    ((b * 30.0 / 7.0).exp() - 1.0) * 100.0
}

/// The latest sweep's `(min_sell, max_buy)`; without both sides, both are `price_now`.
pub fn quote_or_closed(sweep: Option<(Option<i64>, Option<i64>)>, price_now: f64) -> Quote {
    match sweep {
        Some((Some(ask), Some(bid))) => Quote { ask: ask as f64, bid: bid as f64, source: "sweep" },
        _ => Quote { ask: price_now, bid: price_now, source: "closed" },
    }
}

pub fn project(fit: &Fit, quote: &Quote, horizon_weeks: i64) -> Projection {
    let t = (fit.last_week as i64 + horizon_weeks) as f64;
    let spread = (fit.price_now - quote.bid).max(0.0);
    let exit_mid = (fit.a + fit.b * t).exp() - spread;
    let exit_low = (fit.a + fit.b * t - 2.0 * fit.s).exp() - spread;
    let profit_low = exit_low - quote.ask;
    Projection {
        exit_low,
        exit_mid,
        profit_low,
        profit_low_pct: if quote.ask > 0.0 { profit_low / quote.ask * 100.0 } else { 0.0 },
        profit_mid: exit_mid - quote.ask,
    }
}

pub fn qualifies(fit: &Fit, quote: &Quote, p: &Projection, t: &HoldThresholds) -> bool {
    fit.r2 >= t.min_steadiness
        && fit.weeks >= t.min_weeks
        && fit.volume >= t.min_volume
        && fit.trend_intact
        && quote.source == "sweep"
        && p.profit_low_pct >= t.min_margin_pct
}

pub fn category_of(tags: &[String]) -> &'static str {
    let has = |tag: &str| tags.iter().any(|t| t == tag);
    if has("arcane_enhancement") {
        "arcane"
    } else if has("set") && has("prime") {
        "prime set"
    } else if has("prime") {
        "prime part"
    } else if has("mod") {
        "mod"
    } else if has("relic") {
        "relic"
    } else {
        "other"
    }
}

pub async fn latest_day(conn: &DatabaseConnection) -> Result<Option<NaiveDate>, Error> {
    const C: &str = "Trends:LatestDay";
    let row = conn.query_one(stmt("SELECT MAX(day) AS day FROM closed_stats_daily", vec![])).await.map_err(|e| db_err(C, e))?;
    let day: Option<String> = match row {
        Some(r) => r.try_get("", "day").map_err(|e| db_err(C, e))?,
        None => None,
    };
    day.map(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(|e| db_err(C, e))).transpose()
}

/// Every key's days with trades in `latest − TREND_WINDOW_DAYS ..= latest`.
pub async fn load_points(conn: &DatabaseConnection, latest: NaiveDate) -> Result<HashMap<(String, String), Vec<DailyPoint>>, Error> {
    const C: &str = "Trends:Points";
    let rows = conn
        .query_all(stmt(
            "SELECT item_id, sub_type, day, volume, median FROM closed_stats_daily WHERE day >= ? AND day <= ? AND median > 0",
            vec![(latest - Duration::days(TREND_WINDOW_DAYS)).to_string().into(), latest.to_string().into()],
        ))
        .await
        .map_err(|e| db_err(C, e))?;
    let mut points: HashMap<(String, String), Vec<DailyPoint>> = HashMap::new();
    for r in rows.iter() {
        let day: String = r.try_get("", "day").map_err(|e| db_err(C, e))?;
        points
            .entry((r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "sub_type").map_err(|e| db_err(C, e))?))
            .or_default()
            .push(DailyPoint {
                day: NaiveDate::parse_from_str(&day, "%Y-%m-%d").map_err(|e| db_err(C, e))?,
                volume: r.try_get("", "volume").map_err(|e| db_err(C, e))?,
                median: r.try_get("", "median").map_err(|e| db_err(C, e))?,
            });
    }
    Ok(points)
}

/// Each key's latest `(min_sell, max_buy)` swept within `QUOTE_MAX_AGE_H`.
pub async fn load_sweep_quotes(conn: &DatabaseConnection, now: DateTime<Utc>) -> Result<HashMap<(String, String), (Option<i64>, Option<i64>)>, Error> {
    const C: &str = "Trends:Quotes";
    // Without INDEXED BY, SQLite scans all of idx_sweep_summary_item to skip the GROUP BY sort; the window
    // is ~1/30 of the table. SQLite takes the bare columns from the MAX(swept_at) row.
    conn.query_all(stmt(
        "SELECT item_id, sub_type, min_sell, max_buy, MAX(swept_at) AS swept_at
         FROM sweep_summary INDEXED BY idx_sweep_summary_swept_at WHERE swept_at >= ? GROUP BY item_id, sub_type",
        vec![ts(now - Duration::hours(QUOTE_MAX_AGE_H)).into()],
    ))
    .await
    .map_err(|e| db_err(C, e))?
    .iter()
    .map(|r| {
        let key = (r.try_get("", "item_id").map_err(|e| db_err(C, e))?, r.try_get("", "sub_type").map_err(|e| db_err(C, e))?);
        Ok((key, (r.try_get("", "min_sell").map_err(|e| db_err(C, e))?, r.try_get("", "max_buy").map_err(|e| db_err(C, e))?)))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::collector::store::{exec, tests::setup};

    fn latest() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
    }
    fn first() -> NaiveDate {
        latest() - Duration::days(TREND_WINDOW_DAYS)
    }
    fn point(day: NaiveDate, volume: i64, median: f64) -> DailyPoint {
        DailyPoint { day, volume, median }
    }
    /// One day per week, the first day of week `w`, so each weekly price is the given price.
    fn weekly(prices: &[f64]) -> Vec<DailyPoint> {
        prices.iter().enumerate().map(|(w, p)| point(first() + Duration::days(7 * w as i64), 20, *p)).collect()
    }
    fn riser() -> Vec<f64> {
        (0..13).map(|w| 100.0 * 1.05f64.powi(w)).collect()
    }
    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn fit_recovers_a_clean_exponential_riser() {
        let f = fit(&weekly(&riser()), latest()).unwrap();
        assert!(close(f.b, 1.05f64.ln(), 1e-9), "b = {}", f.b);
        assert!(close(f.a, 100f64.ln(), 1e-9), "a = {}", f.a);
        assert!(f.r2 > 0.999, "r2 = {}", f.r2);
        assert!(f.s < 1e-6, "s = {}", f.s);
        let month = trend_pct_month(f.b);
        assert!(close(month, (1.05f64.powf(30.0 / 7.0) - 1.0) * 100.0, 1e-6) && close(month, 23.26, 0.01), "{month}");
        assert!(f.trend_intact, "a perfect series sits on its own fit");
        assert_eq!((f.weeks, f.last_week), (13, 12));
        assert!(close(f.price_now, 100.0 * 1.05f64.powf(11.5), 1e-6), "geometric mean of weeks 11 and 12: {}", f.price_now);
        assert!(close(f.volume, 20.0, 1e-9));
    }

    #[test]
    fn fit_flat_noisy_series_has_low_r2() {
        let noisy = [100.0, 104.0, 97.0, 103.0, 98.0, 102.0, 99.0, 104.0, 96.0, 101.0, 103.0, 97.0, 100.0];
        let f = fit(&weekly(&noisy), latest()).unwrap();
        assert!(f.r2 < 0.1, "r2 = {}", f.r2);
        assert!(f.b.abs() < 0.005 && trend_pct_month(f.b).abs() < 1.0, "b = {}", f.b);
        assert!(close(f.s, 0.028689687736545302, 1e-9), "s = {}", f.s);

        let flat = fit(&weekly(&[50.0; 13]), latest()).unwrap();
        assert_eq!(flat.r2, 0.0, "prices that never vary have no steadiness");
        assert!(flat.b.abs() < 1e-12 && flat.s < 1e-9 && flat.trend_intact);
        assert!(close(flat.price_now, 50.0, 1e-9));
    }

    #[test]
    fn fit_faller_has_negative_slope() {
        let faller: Vec<f64> = (0..13).map(|w| 200.0 * 0.97f64.powi(w)).collect();
        let f = fit(&weekly(&faller), latest()).unwrap();
        assert!(close(f.b, 0.97f64.ln(), 1e-9), "b = {}", f.b);
        assert!(f.r2 > 0.999);
        assert!(close(trend_pct_month(f.b), -12.2378, 1e-3), "{}", trend_pct_month(f.b));
    }

    #[test]
    fn fewer_than_11_weeks_is_none() {
        let prices = riser();
        assert_eq!(fit(&weekly(&prices[..10]), latest()), None);
        let eleven = fit(&weekly(&prices[..11]), latest()).unwrap();
        assert_eq!((eleven.weeks, eleven.last_week), (11, 10));
    }

    #[test]
    fn zero_median_days_are_ignored() {
        let clean = fit(&weekly(&riser()), latest()).unwrap();
        // Every week also carries a zero-median day with a huge volume, one day later.
        let mut points = weekly(&riser());
        points.extend(weekly(&[0.0; 13]).into_iter().map(|p| point(p.day + Duration::days(1), 1000, 0.0)));
        assert_eq!(fit(&points, latest()), Some(clean), "neither the weekly price nor the volume sees them");

        // Two weeks with only zero-median days are not scored.
        let mut sparse = weekly(&riser());
        sparse[3].median = 0.0;
        sparse[7].median = 0.0;
        let f = fit(&sparse, latest()).unwrap();
        assert_eq!(f.weeks, 11);
        assert!(close(f.b, 1.05f64.ln(), 1e-9));
    }

    #[test]
    fn weekly_bucketing_at_the_window_edge() {
        let points = vec![
            point(latest() - Duration::days(91), 1, 999.0), // outside the window
            point(latest() - Duration::days(90), 1, 10.0),  // day 0: week 0
            point(latest() - Duration::days(84), 1, 20.0),  // day 6: still week 0
            point(latest() - Duration::days(83), 1, 5.0),   // day 7: week 1
            point(latest() - Duration::days(82), 1, 7.0),
            point(latest() - Duration::days(81), 1, 100.0),
            point(latest(), 1, 42.0),                       // day 90: week 12
            point(latest() + Duration::days(1), 1, 999.0),  // after the latest day
        ];
        assert_eq!(weekly_prices(&points, latest()), vec![(0, 15.0), (1, 7.0), (12, 42.0)], "an even count averages the middle two");
    }

    #[test]
    fn trend_intact_false_when_the_last_two_weeks_break_the_band() {
        // A riser with ±2 % alternating noise (s ≈ 0.02) sits inside its band.
        let base: Vec<f64> = (0..13).map(|w| 100.0 * 1.05f64.powi(w) * (1.0 + 0.02 * (-1f64).powi(w))).collect();
        assert!(fit(&weekly(&base), latest()).unwrap().trend_intact);
        for broken in [11, 12] {
            let mut dropped = base.clone();
            dropped[broken] *= 0.7;
            assert!(!fit(&weekly(&dropped), latest()).unwrap().trend_intact, "week {broken} fell 30 %");
        }
        let mut jumped = base.clone();
        jumped[11] *= 1.3;
        jumped[12] *= 1.3;
        assert!(fit(&weekly(&jumped), latest()).unwrap().trend_intact, "only the downside breaks the band");
    }

    fn sample_fit() -> Fit {
        Fit { a: 100f64.ln(), b: 0.05, r2: 0.95, s: 0.1, last_week: 12, weeks: 13, price_now: 180.0, volume: 25.0, trend_intact: true }
    }

    #[test]
    fn project_uses_the_lower_band_and_the_spread() {
        // t = 12 + 4 = 16, so a + b·t = ln 100 + 0.8; spread = 180 − 170 = 10.
        let quote = Quote { ask: 160.0, bid: 170.0, source: "sweep" };
        let p = project(&sample_fit(), &quote, 4);
        assert!(close(p.exit_mid, 222.55409284924679 - 10.0, 1e-9), "100·e^0.8 − 10: {}", p.exit_mid);
        assert!(close(p.exit_low, 182.2118800390509 - 10.0, 1e-9), "100·e^(0.8 − 2·0.1) − 10: {}", p.exit_low);
        assert!(close(p.profit_low, 12.211880039050897, 1e-9));
        assert!(close(p.profit_low_pct, 7.632425024406811, 1e-9));
        assert!(close(p.profit_mid, 52.554092849246786, 1e-9));

        let above = project(&sample_fit(), &Quote { ask: 160.0, bid: 190.0, source: "sweep" }, 4);
        assert!(close(above.exit_mid, 222.55409284924679, 1e-9), "a bid above price_now is no negative spread");
        let free = project(&sample_fit(), &Quote { ask: 0.0, bid: 170.0, source: "sweep" }, 4);
        assert_eq!(free.profit_low_pct, 0.0);
    }

    #[test]
    fn quote_falls_back_to_closed_when_a_side_is_missing() {
        assert_eq!(quote_or_closed(Some((Some(12), Some(9))), 11.0), Quote { ask: 12.0, bid: 9.0, source: "sweep" });
        let closed = Quote { ask: 11.0, bid: 11.0, source: "closed" };
        for sweep in [None, Some((None, Some(9))), Some((Some(12), None)), Some((None, None))] {
            assert_eq!(quote_or_closed(sweep, 11.0), closed, "{sweep:?}");
        }
    }

    #[test]
    fn each_threshold_flips_qualifies() {
        let t = HoldThresholds { horizon_weeks: 4, min_volume: 10.0, min_margin_pct: 10.0, min_steadiness: 0.85, min_weeks: 12 };
        let f = Fit { r2: 0.85, weeks: 12, volume: 10.0, ..sample_fit() };
        let q = Quote { ask: 100.0, bid: 95.0, source: "sweep" };
        let p = Projection { exit_low: 110.0, exit_mid: 130.0, profit_low: 10.0, profit_low_pct: 10.0, profit_mid: 30.0 };
        assert!(qualifies(&f, &q, &p, &t), "every threshold met exactly");
        assert!(!qualifies(&Fit { r2: 0.849, ..f }, &q, &p, &t), "steadiness");
        assert!(!qualifies(&Fit { weeks: 11, ..f }, &q, &p, &t), "weeks");
        assert!(!qualifies(&Fit { volume: 9.9, ..f }, &q, &p, &t), "volume");
        assert!(!qualifies(&Fit { trend_intact: false, ..f }, &q, &p, &t), "trend intact");
        assert!(!qualifies(&f, &Quote { source: "closed", ..q }, &p, &t), "quote source");
        assert!(!qualifies(&f, &q, &Projection { profit_low_pct: 9.99, ..p }, &t), "margin");
    }

    #[test]
    fn category_mapping() {
        let of = |tags: &[&str]| category_of(&tags.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        assert_eq!(of(&["arcane_enhancement", "legendary"]), "arcane");
        assert_eq!(of(&["prime", "set", "warframe"]), "prime set");
        assert_eq!(of(&["prime", "blueprint", "warframe"]), "prime part");
        assert_eq!(of(&["mod", "rare", "prime"]), "prime part", "prime is checked before mod");
        assert_eq!(of(&["mod", "rare"]), "mod");
        assert_eq!(of(&["relic", "lith"]), "relic");
        assert_eq!(of(&["set", "weapon"]), "other");
        assert_eq!(of(&[]), "other");
    }

    async fn closed_row(conn: &DatabaseConnection, item: &str, sub_type: &str, day: NaiveDate, volume: i64, median: Option<f64>) {
        exec(
            conn,
            "Test:Closed",
            "INSERT INTO closed_stats_daily (item_id, sub_type, day, volume, median, min_price, max_price, avg_price, wa_price)
             VALUES (?, ?, ?, ?, ?, NULL, NULL, NULL, NULL)",
            vec![item.into(), sub_type.into(), day.to_string().into(), volume.into(), median.into()],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn load_points_reads_the_window_only() {
        let (_dir, conn) = setup().await;
        assert_eq!(latest_day(&conn).await.unwrap(), None, "no closed rows yet");
        let l = latest();
        closed_row(&conn, "item1", "", l - Duration::days(91), 5, Some(10.0)).await; // before the window
        closed_row(&conn, "item1", "", l - Duration::days(90), 6, Some(11.0)).await;
        closed_row(&conn, "item1", "", l - Duration::days(3), 7, Some(0.0)).await; // no trades
        closed_row(&conn, "item1", "", l - Duration::days(2), 0, None).await;
        closed_row(&conn, "item1", "", l, 8, Some(12.5)).await;
        closed_row(&conn, "item1", "", l + Duration::days(1), 9, Some(13.0)).await; // after the given latest day
        closed_row(&conn, "item2", "rank=5", l - Duration::days(10), 3, Some(40.0)).await;
        assert_eq!(latest_day(&conn).await.unwrap(), Some(l + Duration::days(1)));

        let mut points = load_points(&conn, l).await.unwrap();
        assert_eq!(points.len(), 2);
        let item1 = points.get_mut(&("item1".to_string(), String::new())).unwrap();
        item1.sort_by_key(|p| p.day);
        assert_eq!(*item1, vec![point(l - Duration::days(90), 6, 11.0), point(l, 8, 12.5)]);
        assert_eq!(points[&("item2".to_string(), "rank=5".to_string())], vec![point(l - Duration::days(10), 3, 40.0)]);
    }

    async fn sweep_row(conn: &DatabaseConnection, item: &str, sub_type: &str, at: DateTime<Utc>, min_sell: Option<i64>, max_buy: Option<i64>) {
        exec(
            conn,
            "Test:Sweep",
            "INSERT INTO sweep_summary (item_id, sub_type, swept_at, lane, min_sell, max_buy, sell_count, buy_count, sell_ingame, buy_ingame, top_sells, top_buys)
             VALUES (?, ?, ?, 'cold', ?, ?, 1, 1, 1, 1, '[]', '[]')",
            vec![item.into(), sub_type.into(), ts(at).into(), min_sell.into(), max_buy.into()],
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn load_sweep_quotes_ignores_rows_older_than_24h() {
        let (_dir, conn) = setup().await;
        let now = parse_ts("2026-09-29T12:00:00Z").unwrap();
        let h = Duration::hours;
        sweep_row(&conn, "item1", "", now - h(25), Some(1), Some(1)).await;
        sweep_row(&conn, "item1", "", now - h(2), Some(10), Some(8)).await;
        sweep_row(&conn, "item1", "", now - h(1), Some(12), None).await; // the latest row wins, missing side and all
        sweep_row(&conn, "item2", "rank=5", now - h(30), Some(50), Some(40)).await;
        sweep_row(&conn, "item2", "", now - h(QUOTE_MAX_AGE_H), Some(7), Some(6)).await; // exactly 24 h old still counts

        let quotes = load_sweep_quotes(&conn, now).await.unwrap();
        assert_eq!(
            quotes,
            HashMap::from([(("item1".to_string(), String::new()), (Some(12), None)), (("item2".to_string(), String::new()), (Some(7), Some(6)))])
        );
    }
}
