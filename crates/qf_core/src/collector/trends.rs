//! Long-term price trends and a conservative hold projection (spec §25 P28).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use service::sea_orm::{ConnectionTrait, DatabaseConnection};
use utils::Error;

use super::closed::median_f64;
use super::{db_err, stmt, ts};

/// `latest − 89 ..= latest`: the 90 days `closed_stats_daily` keeps.
pub const TREND_WINDOW_DAYS: i64 = 90;
const _: () = assert!(TREND_WINDOW_DAYS <= super::closed::CLOSED_RETENTION_DAYS);
/// The index of the week holding the latest day; weeks count back from it in whole weeks.
const LAST_WEEK: i64 = (TREND_WINDOW_DAYS - 1) / 7;
pub const MIN_WEEKS: usize = 11;
// The holdout fit drops two weeks and needs a residual degree of freedom.
const _: () = assert!(MIN_WEEKS >= 5);
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
    /// Mean scored week index and `Σ(w − mean_w)²`, for the prediction interval.
    pub mean_w: f64,
    pub sxx: f64,
    pub price_now: f64,
    /// Trades per calendar day over the `TREND_WINDOW_DAYS` days of the window.
    pub volume: f64,
    /// Each of the last two weeks is at or above the `2s` band of a fit over the weeks before them.
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

fn first_day(latest: NaiveDate) -> NaiveDate {
    latest - Duration::days(TREND_WINDOW_DAYS - 1)
}

/// Points with trades inside the window, with their week index: `LAST_WEEK` is the seven most recent days,
/// and week 0 is the oldest, partial one.
fn in_window(points: &[DailyPoint], latest: NaiveDate) -> impl Iterator<Item = (usize, &DailyPoint)> {
    let first = first_day(latest);
    points
        .iter()
        .filter(move |p| p.median > 0.0 && p.day >= first && p.day <= latest)
        .map(move |p| ((LAST_WEEK - (latest - p.day).num_days() / 7) as usize, p))
}

/// `(week index, median of that week's daily medians)`, ascending by week.
pub fn weekly_prices(points: &[DailyPoint], latest: NaiveDate) -> Vec<(usize, f64)> {
    let mut weeks: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for (w, p) in in_window(points, latest) {
        weeks.entry(w).or_default().push(p.median);
    }
    weeks.into_iter().filter_map(|(w, medians)| Some((w, median_f64(&medians)?))).collect()
}

/// Ordinary least squares `y ≈ a + b·x`.
struct Ols {
    a: f64,
    b: f64,
    sse: f64,
    sst: f64,
    mean_x: f64,
    sxx: f64,
}

fn ols(xs: &[f64], ys: &[f64]) -> Ols {
    let n = xs.len() as f64;
    let (mean_x, my) = (xs.iter().sum::<f64>() / n, ys.iter().sum::<f64>() / n);
    let sxx: f64 = xs.iter().map(|x| (x - mean_x).powi(2)).sum();
    let sxy: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mean_x) * (y - my)).sum();
    let b = sxy / sxx;
    let a = my - b * mean_x;
    let sse: f64 = xs.iter().zip(ys).map(|(x, y)| (y - a - b * x).powi(2)).sum();
    let sst: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    Ols { a, b, sse, sst, mean_x, sxx }
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
    let full = ols(&xs, &ys);
    let (a, b) = (full.a, full.b);
    let r2 = if full.sst == 0.0 { 0.0 } else { 1.0 - full.sse / full.sst };
    let s = (full.sse / (n as f64 - 2.0)).sqrt();
    // Out of sample: a break in the last two weeks would pull an in-sample fit down and widen its band enough
    // to hide itself, so they are judged against the band of the weeks before them.
    let h = n - 2;
    let hold = ols(&xs[..h], &ys[..h]);
    let s_h = (hold.sse / (h as f64 - 2.0)).sqrt();
    // A plain 2·s_h band, narrower than a prediction interval: the strict side. The epsilon keeps a perfect
    // series from failing its own band on rounding.
    let trend_intact = (h..n).all(|i| ys[i] >= hold.a + hold.b * xs[i] - 2.0 * s_h - 1e-9);
    let trades: i64 = in_window(points, latest).map(|(_, p)| p.volume).sum();
    let f = Fit {
        a,
        b,
        r2,
        s,
        last_week: weekly[n - 1].0,
        weeks: n,
        mean_w: full.mean_x,
        sxx: full.sxx,
        price_now: ((ys[n - 2] + ys[n - 1]) / 2.0).exp(),
        volume: trades as f64 / TREND_WINDOW_DAYS as f64,
        trend_intact,
    };
    [f.a, f.b, f.r2, f.s, f.mean_w, f.sxx, f.price_now, f.volume].iter().all(|v| v.is_finite()).then_some(f)
}

/// Widens `s` into the standard error of a new week's log price at week `t` off the fitted line.
pub fn prediction_factor(fit: &Fit, t: f64) -> f64 {
    (1.0 + 1.0 / fit.weeks as f64 + (t - fit.mean_w).powi(2) / fit.sxx).sqrt()
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
    let exit_low = (fit.a + fit.b * t - 2.0 * fit.s * prediction_factor(fit, t)).exp() - spread;
    let profit_low = exit_low - quote.ask;
    Projection {
        exit_low,
        exit_mid,
        profit_low,
        profit_low_pct: if quote.ask > 0.0 { profit_low / quote.ask * 100.0 } else { 0.0 },
        profit_mid: exit_mid - quote.ask,
    }
}

/// Every threshold met, a real price to buy at (1 p is warframe.market's minimum) and a finite projection.
pub fn qualifies(fit: &Fit, quote: &Quote, p: &Projection, t: &HoldThresholds) -> bool {
    [p.exit_low, p.exit_mid, p.profit_low, p.profit_low_pct, p.profit_mid].iter().all(|v| v.is_finite())
        && quote.ask >= 1.0
        && fit.r2 >= t.min_steadiness
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
    } else if has("mod") {
        "mod"
    } else if has("prime") {
        "prime part"
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

/// Every key's days with trades in the window ending on `latest`.
pub async fn load_points(conn: &DatabaseConnection, latest: NaiveDate) -> Result<HashMap<(String, String), Vec<DailyPoint>>, Error> {
    const C: &str = "Trends:Points";
    let rows = conn
        .query_all(stmt(
            "SELECT item_id, sub_type, day, volume, median FROM closed_stats_daily WHERE day >= ? AND day <= ? AND median > 0",
            vec![first_day(latest).to_string().into(), latest.to_string().into()],
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HoldRow {
    pub item_id: String,
    pub sub_type: String,
    pub name: String,
    pub slug: String,
    pub category: &'static str,
    pub ask_now: f64,
    pub bid_now: f64,
    pub quote_source: &'static str,
    pub exit_low: f64,
    pub exit_mid: f64,
    pub profit_low: f64,
    pub profit_low_pct: f64,
    pub profit_mid: f64,
    pub trend_pct_month: f64,
    pub steadiness: f64,
    pub volume: f64,
    pub weeks: usize,
    pub trend_intact: bool,
    pub qualified: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Holds {
    pub latest_day: String,
    /// Fitted keys the item cache knows: one row each.
    pub scored: usize,
    pub qualified: usize,
    pub rows: Vec<HoldRow>,
}

type Fits = HashMap<(String, String), Fit>;

/// The fits change only when a new closed day lands; quotes and thresholds apply per call.
static FITS: Mutex<Option<(NaiveDate, Arc<Fits>)>> = Mutex::new(None);
#[cfg(test)]
static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A panic while the lock was held leaves whole fits or none, so the value is taken back out.
fn fits_lock() -> MutexGuard<'static, Option<(NaiveDate, Arc<Fits>)>> {
    FITS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Every fitted key the item cache knows, projected and judged with `t`: qualified rows first, then the
/// best pessimistic margin.
pub async fn holds(
    conn: &DatabaseConnection,
    now: DateTime<Utc>,
    t: &HoldThresholds,
    name_of: impl Fn(&str) -> Option<(String, String, Vec<String>)>,
) -> Result<Holds, Error> {
    let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
    let t = HoldThresholds {
        // The spec's 1–12 weeks; `last_week + H` is plain arithmetic, and a huge H would overflow exp.
        horizon_weeks: t.horizon_weeks.clamp(1, 12),
        min_volume: finite(t.min_volume),
        min_margin_pct: finite(t.min_margin_pct),
        // `max` drops a NaN.
        min_steadiness: t.min_steadiness.max(0.0).min(1.0),
        min_weeks: t.min_weeks,
    };
    let Some(latest) = latest_day(conn).await? else {
        return Ok(Holds { latest_day: String::new(), scored: 0, qualified: 0, rows: vec![] });
    };
    // Each guard lives for its statement only, never across an await.
    let cached = fits_lock().as_ref().filter(|(day, _)| *day == latest).map(|(_, fits)| fits.clone());
    let fits = match cached {
        Some(fits) => fits,
        None => {
            #[cfg(test)]
            LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let points = load_points(conn, latest).await?;
            let fits: Arc<Fits> = Arc::new(points.into_iter().filter_map(|(key, points)| Some((key, fit(&points, latest)?))).collect());
            // Two calls racing on a new day both fit it; the last writer wins.
            *fits_lock() = Some((latest, fits.clone()));
            fits
        }
    };
    let quotes = load_sweep_quotes(conn, now).await?;
    let mut rows: Vec<HoldRow> = fits
        .iter()
        .filter_map(|(key, f)| {
            let (name, slug, tags) = name_of(&key.0)?;
            let quote = quote_or_closed(quotes.get(key).copied(), f.price_now);
            let p = project(f, &quote, t.horizon_weeks);
            Some(HoldRow {
                item_id: key.0.clone(),
                sub_type: key.1.clone(),
                name,
                slug,
                category: category_of(&tags),
                ask_now: quote.ask,
                bid_now: quote.bid,
                quote_source: quote.source,
                exit_low: p.exit_low,
                exit_mid: p.exit_mid,
                profit_low: p.profit_low,
                profit_low_pct: p.profit_low_pct,
                profit_mid: p.profit_mid,
                trend_pct_month: trend_pct_month(f.b),
                steadiness: f.r2,
                volume: f.volume,
                weeks: f.weeks,
                trend_intact: f.trend_intact,
                qualified: qualifies(f, &quote, &p, &t),
            })
        })
        .collect();
    rows.sort_by(|x, y| {
        y.qualified
            .cmp(&x.qualified)
            .then(y.profit_low_pct.total_cmp(&x.profit_low_pct))
            .then_with(|| (&x.item_id, &x.sub_type).cmp(&(&y.item_id, &y.sub_type)))
    });
    let qualified = rows.iter().filter(|r| r.qualified).count();
    Ok(Holds { latest_day: latest.format("%Y-%m-%d").to_string(), scored: rows.len(), qualified, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::parse_ts;
    use crate::collector::store::{exec, tests::setup};

    fn latest() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
    }
    fn point(day: NaiveDate, volume: i64, median: f64) -> DailyPoint {
        DailyPoint { day, volume, median }
    }
    /// One day per week, 7·(12 − w) days before the latest, so each weekly price is the given price.
    /// 70 trades a week: 13 weeks make 910 trades over the 90 calendar days.
    fn weekly(prices: &[f64]) -> Vec<DailyPoint> {
        prices.iter().enumerate().map(|(w, p)| point(latest() - Duration::days(7 * (12 - w as i64)), 70, *p)).collect()
    }
    fn defaults() -> HoldThresholds {
        HoldThresholds { horizon_weeks: 4, min_volume: 10.0, min_margin_pct: 10.0, min_steadiness: 0.85, min_weeks: 12 }
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
        assert!(close(f.volume, 910.0 / 90.0, 1e-9), "trades per calendar day: 910 / 90, not 910 / 13 trading days: {}", f.volume);
        assert_eq!((f.mean_w, f.sxx), (6.0, 182.0));
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
        assert!(close(f.volume, 11.0 * 70.0 / 90.0, 1e-9), "the divisor stays the 90 window days: {}", f.volume);
    }

    #[test]
    fn weekly_bucketing_at_the_window_edge() {
        let back = |d: i64| latest() - Duration::days(d);
        let points = vec![
            point(back(90), 1, 999.0), // retention keeps 90 days: outside the window
            point(back(89), 1, 10.0),  // oldest day: week 0, the partial week
            point(back(84), 1, 20.0),  // week 0
            point(back(83), 1, 5.0),   // week 1
            point(back(82), 1, 7.0),
            point(back(77), 1, 100.0), // week 1
            point(back(7), 1, 30.0),   // week 11
            point(back(6), 1, 40.0),   // week 12: the seven most recent days
            point(back(0), 1, 44.0),
            point(latest() + Duration::days(1), 1, 999.0), // after the latest day
        ];
        assert_eq!(
            weekly_prices(&points, latest()),
            vec![(0, 15.0), (1, 7.0), (11, 30.0), (12, 42.0)],
            "weeks count back from the latest day; an even count averages the middle two"
        );
    }

    #[test]
    fn trend_intact_false_when_the_last_two_weeks_break_the_band() {
        let noisy = |w: i32, k: f64| 100.0 * 1.05f64.powi(w) * (1.0 + k * (-1f64).powi(w));
        // A steady riser (±1 % noise) whose last two weeks sit 15 % below trend. In sample, the break pulls the
        // fit down and widens s enough to hide itself; the band of weeks 0..=10 alone does not.
        let mut broken: Vec<f64> = (0..13).map(|w| noisy(w, 0.01)).collect();
        broken[11] *= 0.85;
        broken[12] *= 0.85;
        let f = fit(&weekly(&broken), latest()).unwrap();
        assert!(!f.trend_intact, "both of the last two weeks broke the band of the weeks before them");
        let q = Quote { ask: f.price_now, bid: f.price_now, source: "sweep" };
        let p = project(&f, &q, 4);
        assert!(f.r2 >= 0.85 && f.weeks >= 12 && f.volume >= 10.0 && p.profit_low_pct >= 10.0, "every other threshold passes: r2 {} pct {}", f.r2, p.profit_low_pct);
        assert!(!qualifies(&f, &q, &p, &defaults()));
        assert!(qualifies(&Fit { trend_intact: true, ..f }, &q, &p, &defaults()), "the trend check alone blocks it");

        // A riser with ±2 % alternating noise whose last two weeks stay in the band.
        let base: Vec<f64> = (0..13).map(|w| noisy(w, 0.02)).collect();
        assert!(fit(&weekly(&base), latest()).unwrap().trend_intact);
        for week in [11, 12] {
            let mut dropped = base.clone();
            dropped[week] *= 0.7;
            assert!(!fit(&weekly(&dropped), latest()).unwrap().trend_intact, "week {week} fell 30 %");
        }
        let mut jumped = base.clone();
        jumped[11] *= 1.3;
        jumped[12] *= 1.3;
        assert!(fit(&weekly(&jumped), latest()).unwrap().trend_intact, "only the downside breaks the band");
    }

    #[test]
    fn holdout_band_is_two_sigma() {
        // Week 11 of the ±2 % riser sits 2 % under trend; the band of weeks 0..=10 has s_h ≈ 0.02202.
        // Its log margin over a − k·s_h: ×0.98 → +0.0020 at k = 2 but −0.0200 at k = 1;
        // ×0.97 → −0.0082 at k = 2 but +0.0138 at k = 3. Week 12's margin is ≥ +0.040 at every k.
        let base: Vec<f64> = (0..13).map(|w| 100.0 * 1.05f64.powi(w) * (1.0 + 0.02 * (-1f64).powi(w))).collect();
        let with_week_11 = |m: f64| {
            let mut prices = base.clone();
            prices[11] *= m;
            fit(&weekly(&prices), latest()).unwrap().trend_intact
        };
        assert!(with_week_11(0.98), "inside 2·s_h (would break at 1·s_h)");
        assert!(!with_week_11(0.97), "outside 2·s_h (would pass at 3·s_h)");
    }

    fn sample_fit() -> Fit {
        Fit {
            a: 100f64.ln(), b: 0.05, r2: 0.95, s: 0.1, last_week: 12, weeks: 13, mean_w: 6.0, sxx: 182.0,
            price_now: 180.0, volume: 25.0, trend_intact: true,
        }
    }

    #[test]
    fn prediction_factor_for_thirteen_weeks_four_ahead() {
        // Weeks 0..=12: n = 13, w̄ = 6, Sxx = 2·(1 + 4 + 9 + 16 + 25 + 36) = 182. t = 12 + 4 = 16, (t − w̄)² = 100.
        // 1 + 1/13 + 100/182 = (91 + 7 + 50)/91 = 148/91, so the factor is √(148/91) ≈ 1.2752935.
        let f = fit(&weekly(&riser()), latest()).unwrap();
        assert!(close(prediction_factor(&f, 16.0), (148.0f64 / 91.0).sqrt(), 1e-12), "{}", prediction_factor(&f, 16.0));
        assert!(close(prediction_factor(&f, 16.0), 1.2752935, 1e-7));
        assert!(close(prediction_factor(&f, 6.0), (1.0f64 + 1.0 / 13.0).sqrt(), 1e-12), "narrowest at the mean week");
        assert!(prediction_factor(&f, 24.0) > prediction_factor(&f, 16.0), "wider the further out");
    }

    #[test]
    fn project_uses_the_lower_band_and_the_spread() {
        // t = 12 + 4 = 16, so a + b·t = ln 100 + 0.8; spread = 180 − 170 = 10.
        // Lower band: 2·s·√(148/91) = 0.2 × 1.2752935 = 0.2550587, so exit_low = 100·e^(0.8 − 0.2550587) − 10.
        let quote = Quote { ask: 160.0, bid: 170.0, source: "sweep" };
        let p = project(&sample_fit(), &quote, 4);
        assert!(close(p.exit_mid, 222.55409284924679 - 10.0, 1e-9), "100·e^0.8 − 10: {}", p.exit_mid);
        assert!(close(p.exit_low, 172.4507135253479 - 10.0, 1e-9), "100·e^(0.8 − 2·0.1·1.2752935) − 10: {}", p.exit_low);
        assert!(close(p.profit_low, 2.4507135253479078, 1e-9));
        assert!(close(p.profit_low_pct, 1.5316959533424424, 1e-9));
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
        let t = defaults();
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
        assert!(qualifies(&f, &Quote { ask: 1.0, ..q }, &p, &t), "1 p is warframe.market's minimum price");
        assert!(!qualifies(&f, &Quote { ask: 0.99, ..q }, &p, &t), "ask below 1 p");
    }

    #[test]
    fn qualifies_is_false_for_a_non_finite_projection() {
        let q = Quote { ask: 100.0, bid: 95.0, source: "sweep" };
        // An absurd horizon overflows exp to infinity, which would clear any margin.
        let huge = project(&sample_fit(), &q, 100_000);
        assert!(huge.profit_low_pct.is_infinite(), "{huge:?}");
        let fields = |p: &Projection| [p.exit_low, p.exit_mid, p.profit_low, p.profit_low_pct, p.profit_mid];
        assert!(fields(&huge).iter().all(|v| !v.is_nan()), "a finite fit never projects NaN: {huge:?}");
        assert!(!qualifies(&sample_fit(), &q, &huge, &defaults()));
        let ok = Projection { exit_low: 120.0, exit_mid: 130.0, profit_low: 20.0, profit_low_pct: 20.0, profit_mid: 30.0 };
        assert!(qualifies(&sample_fit(), &q, &ok, &defaults()));
        assert!(!qualifies(&sample_fit(), &q, &Projection { exit_mid: f64::NAN, ..ok }, &defaults()), "any field counts");
    }

    #[test]
    fn category_mapping() {
        let of = |tags: &[&str]| category_of(&tags.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        assert_eq!(of(&["arcane_enhancement", "legendary"]), "arcane");
        assert_eq!(of(&["prime", "set", "warframe"]), "prime set");
        assert_eq!(of(&["prime", "blueprint", "warframe"]), "prime part");
        assert_eq!(of(&["mod", "prime"]), "mod", "mod is checked before prime: molecular_fission, gilded_truth");
        assert_eq!(of(&["mod", "legendary"]), "mod", "primed mods carry mod, not prime");
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
        closed_row(&conn, "item1", "", l - Duration::days(90), 5, Some(10.0)).await; // before the 90-day window
        closed_row(&conn, "item1", "", l - Duration::days(89), 6, Some(11.0)).await;
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
        assert_eq!(*item1, vec![point(l - Duration::days(89), 6, 11.0), point(l, 8, 12.5)]);
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

    fn clear_cache() {
        *fits_lock() = None;
        LOADS.store(0, std::sync::atomic::Ordering::SeqCst);
    }
    /// How many times `holds` loaded the closed points since the last `clear_cache`.
    fn loads() -> usize {
        LOADS.load(std::sync::atomic::Ordering::SeqCst)
    }
    /// The fit cache is process-wide, so the tests that serve holds run one at a time, each from an empty cache.
    fn cache_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        clear_cache();
        guard
    }
    fn now() -> DateTime<Utc> {
        parse_ts("2026-09-29T12:00:00Z").unwrap()
    }
    /// Every item but `gone` is known, as a mod.
    fn known(id: &str) -> Option<(String, String, Vec<String>)> {
        (id != "gone").then(|| (format!("Item {id}"), format!("{id}_slug"), vec!["mod".to_string()]))
    }
    /// 13 weekly closed days ending on the latest day, 70 trades each, growing by `growth` a week from 100 p.
    async fn series(conn: &DatabaseConnection, item: &str, sub_type: &str, growth: f64) {
        for p in weekly(&(0..13).map(|w| 100.0 * growth.powi(w)).collect::<Vec<_>>()) {
            closed_row(conn, item, sub_type, p.day, p.volume, Some(p.median)).await;
        }
    }
    async fn quote(conn: &DatabaseConnection, item: &str, ask: i64, bid: i64) {
        sweep_row(conn, item, "", now() - Duration::hours(1), Some(ask), Some(bid)).await;
    }
    async fn serve(conn: &DatabaseConnection, t: HoldThresholds) -> Holds {
        holds(conn, now(), &t, known).await.unwrap()
    }

    #[tokio::test]
    async fn cache_recomputes_only_when_the_latest_day_changes() {
        let _guard = cache_guard();
        let (_dir, conn) = setup().await;
        let empty = serve(&conn, defaults()).await;
        assert_eq!(empty, Holds { latest_day: String::new(), scored: 0, qualified: 0, rows: vec![] });
        assert_eq!(loads(), 0, "no closed day, nothing to load");

        series(&conn, "riser", "", 1.05).await;
        let first = serve(&conn, defaults()).await;
        assert_eq!((first.latest_day.as_str(), first.scored, loads()), ("2026-09-28", 1, 1));
        assert_eq!(first.rows[0].quote_source, "closed");

        // A key on days up to the same latest day stays unseen, but quotes are read on every call.
        series(&conn, "late", "", 1.02).await;
        quote(&conn, "riser", 175, 170).await;
        let second = serve(&conn, defaults()).await;
        assert_eq!((second.scored, loads()), (1, 1), "same latest day: the cached fits");
        assert_eq!(second.rows[0].quote_source, "sweep");

        closed_row(&conn, "riser", "", latest() + Duration::days(1), 70, Some(230.0)).await;
        let third = serve(&conn, defaults()).await;
        assert_eq!((third.latest_day.as_str(), third.scored, loads()), ("2026-09-29", 2, 2), "a newer day recomputes");
        serve(&conn, defaults()).await;
        assert_eq!(loads(), 2);
    }

    #[tokio::test]
    async fn thresholds_apply_at_serve_time() {
        let _guard = cache_guard();
        let (_dir, conn) = setup().await;
        // riser: exit ≈ 100·1.05^16 − (175.26 − 170) ≈ 213.0 against an ask of 175, +21.7 %.
        // slow: exit ≈ 100·1.02^16 − (125.57 − 125) ≈ 136.7 against 126, +8.5 %.
        series(&conn, "riser", "", 1.05).await;
        quote(&conn, "riser", 175, 170).await;
        series(&conn, "slow", "", 1.02).await;
        quote(&conn, "slow", 126, 125).await;
        let qualified = |h: &Holds| h.rows.iter().filter(|r| r.qualified).map(|r| r.item_id.clone()).collect::<Vec<_>>();

        let at = |min_margin_pct| HoldThresholds { min_margin_pct, ..defaults() };
        let default = serve(&conn, at(10.0)).await;
        assert_eq!((default.scored, default.qualified, qualified(&default)), (2, 1, vec!["riser".to_string()]));
        let riser = &default.rows[0];
        assert!((riser.profit_low_pct - 21.7).abs() < 0.1, "{}", riser.profit_low_pct);
        assert_eq!((riser.name.as_str(), riser.slug.as_str(), riser.category), ("Item riser", "riser_slug", "mod"));
        assert_eq!((riser.ask_now, riser.bid_now, riser.quote_source, riser.weeks, riser.trend_intact), (175.0, 170.0, "sweep", 13, true));
        assert!((riser.trend_pct_month - trend_pct_month(1.05f64.ln())).abs() < 1e-9 && riser.steadiness > 0.999);
        assert!((riser.volume - 910.0 / 90.0).abs() < 1e-9);

        assert_eq!(serve(&conn, at(25.0)).await.qualified, 0, "a stricter margin");
        assert_eq!(serve(&conn, at(5.0)).await.qualified, 2, "a looser margin");
        assert_eq!(serve(&conn, HoldThresholds { min_volume: 11.0, ..defaults() }).await.qualified, 0, "910 / 90 trades a day");
        assert_eq!(loads(), 1, "every call served from one fit");
    }

    #[tokio::test]
    async fn serve_time_inputs_are_clamped() {
        let _guard = cache_guard();
        let (_dir, conn) = setup().await;
        series(&conn, "riser", "", 1.05).await;
        quote(&conn, "riser", 175, 170).await;
        let at = |horizon_weeks| HoldThresholds { horizon_weeks, ..defaults() };
        let twelve = serve(&conn, at(12)).await;
        assert!(twelve.rows[0].exit_mid > serve(&conn, at(11)).await.rows[0].exit_mid);
        assert_eq!(serve(&conn, at(100_000)).await, twelve, "horizons past 12 weeks are 12 weeks");
        let one = serve(&conn, at(1)).await;
        assert_eq!(serve(&conn, at(0)).await, one, "horizons under a week are one week");
        assert_eq!(serve(&conn, at(-3)).await, one);

        let nonsense = HoldThresholds { min_volume: f64::INFINITY, min_margin_pct: f64::NAN, min_steadiness: f64::NAN, ..defaults() };
        assert_eq!(serve(&conn, nonsense).await.qualified, 1, "non-finite thresholds are 0");
        assert_eq!(serve(&conn, HoldThresholds { min_steadiness: -1.0, ..defaults() }).await.qualified, 1);
    }

    #[tokio::test]
    async fn unknown_items_are_skipped() {
        let _guard = cache_guard();
        let (_dir, conn) = setup().await;
        for item in ["riser", "gone"] {
            series(&conn, item, "", 1.05).await;
            quote(&conn, item, 175, 170).await;
        }
        let h = serve(&conn, defaults()).await;
        assert_eq!((h.scored, h.qualified), (1, 1));
        assert_eq!(h.rows.iter().map(|r| r.item_id.as_str()).collect::<Vec<_>>(), vec!["riser"]);
    }

    #[tokio::test]
    async fn rows_sort_qualified_first() {
        let _guard = cache_guard();
        let (_dir, conn) = setup().await;
        series(&conn, "riser", "", 1.05).await; // qualified, +21.7 %
        quote(&conn, "riser", 175, 170).await;
        series(&conn, "fast", "", 1.08).await; // closed quote, +41 %: never qualified
        series(&conn, "slow", "", 1.02).await; // +8.5 %
        quote(&conn, "slow", 126, 125).await;
        for (item, sub_type) in [("tie_b", ""), ("tie_a", "rank=5"), ("tie_a", "")] {
            series(&conn, item, sub_type, 1.03).await; // closed quote, +14.2 % each
        }
        let h = serve(&conn, defaults()).await;
        assert_eq!(
            h.rows.iter().map(|r| (r.item_id.as_str(), r.sub_type.as_str(), r.qualified)).collect::<Vec<_>>(),
            vec![("riser", "", true), ("fast", "", false), ("tie_a", "", false), ("tie_a", "rank=5", false), ("tie_b", "", false), ("slow", "", false)]
        );
        assert_eq!((h.scored, h.qualified), (6, 1));
    }
}
