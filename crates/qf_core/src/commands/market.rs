//! Market Data RPCs (spec §23 M1–M3).

use chrono::Utc;
use serde::Serialize;
use utils::{get_location, Error};

use crate::collector::backfill::{self, BackfillStatus};
use crate::collector::closed::{self, RefreshStatus};
use crate::collector::market::{self, Movers, OverviewRow, Warmup};
use crate::collector::stats::StatsConfig;
use crate::collector::trade_tax;
use crate::enums::{PriceSourceMode, ProfitBasis};
use crate::trader::compare::{self, CandidateCounts, ItemLookup, PriceSourceRow};
use crate::trader::price_source::{all_item_stats, effective_stats, source_settings};
use crate::utils::modules::states;
use crate::DATABASE;

#[derive(Serialize)]
pub struct PriceSources {
    pub mode: PriceSourceMode,
    pub guard_pct: i64,
    pub profit_basis: ProfitBasis,
    pub refresh: RefreshStatus,
    pub candidates: CandidateCounts,
    pub rows: Vec<PriceSourceRow>,
}

fn database() -> Result<&'static service::sea_orm::DatabaseConnection, Error> {
    DATABASE.get().ok_or_else(|| Error::new("Market:Rpc", "Database is not ready", get_location!()))
}

/// `(name, slug)` for a cache item id, or None when the item is no longer tradable.
fn name_of() -> Result<impl Fn(&str) -> Option<(String, String)>, Error> {
    let tradable = states::cache_client()?.tradable_item();
    Ok(move |id: &str| tradable.get_by(id).ok().map(|item| (item.name, item.wfm_url)))
}

pub async fn market_overview() -> Result<Vec<OverviewRow>, Error> {
    let stats: Vec<_> = effective_stats(database()?, source_settings(), Utc::now()).await?.into_iter().map(|e| e.stats).collect();
    Ok(market::overview(stats, name_of()?))
}

pub async fn market_movers(min_volume: f64) -> Result<Movers, Error> {
    market::movers(database()?, min_volume, name_of()?).await
}

/// Only keys the collector knows: `tracked`, the history histogram and the projection stay on its own universe (spec §25 P12).
pub async fn market_warmup() -> Result<Warmup, Error> {
    let stats: Vec<_> = effective_stats(database()?, source_settings(), Utc::now()).await?.into_iter().filter(|e| e.inferred).map(|e| e.stats).collect();
    Ok(market::warmup(&stats, Utc::now().date_naive()))
}

/// Starts the 90-day statistics import unless it is already running (spec §24 K4).
pub async fn market_backfill_start() -> Result<BackfillStatus, Error> {
    let conn = database()?.clone();
    let items = states::cache_client()?
        .tradable_item()
        .get_items()?
        .into_iter()
        .map(|item| (item.wfm_id, item.wfm_url))
        .collect();
    Ok(backfill::start(conn, items))
}

pub async fn market_backfill_status() -> Result<BackfillStatus, Error> {
    Ok(backfill::status())
}

/// Both price bases side by side, whatever the current mode (spec §25 P8).
pub async fn market_price_sources() -> Result<PriceSources, Error> {
    let conn = database()?;
    let now = Utc::now();
    let source = source_settings();
    let settings = states::app_state()?.settings.live_scraper.items.clone();
    let tradable = states::cache_client()?.tradable_item();
    // spec §25 P18: the tax comes from the collector's table; the cache's `trade_tax` is always 0.
    let taxes = trade_tax::load_all(conn).await?;
    let (rows, candidates) = compare::compare(
        all_item_stats(conn).await?,
        closed::load_fresh(conn, now, StatsConfig::default().warm_min_trades).await?,
        &settings,
        source.guard_pct,
        source.profit_basis,
        now,
        |id| {
            tradable.get_by(id).ok().map(|item| ItemLookup {
                max_rank: item.sub_type.as_ref().and_then(|s| s.max_rank),
                name: item.name,
                wfm_url: item.wfm_url,
                trade_tax: taxes.get(id).copied().unwrap_or(0),
            })
        },
        &closed::fetch_times(conn).await?,
    );
    Ok(PriceSources { mode: source.mode, guard_pct: source.guard_pct, profit_basis: source.profit_basis, refresh: closed::refresh_status(conn, now).await?, candidates, rows })
}
