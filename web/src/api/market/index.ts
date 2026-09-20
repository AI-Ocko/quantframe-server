import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class MarketModule {
  constructor(private readonly client: TauriClient) {}

  overview() {
    return this.client.sendInvoke<TauriTypes.MarketOverviewRow[]>("market_overview");
  }
  movers(minVolume: number) {
    return this.client.sendInvoke<TauriTypes.MarketMovers>("market_movers", { minVolume });
  }
  warmup() {
    return this.client.sendInvoke<TauriTypes.MarketWarmup>("market_warmup");
  }
  backfillStart() {
    return this.client.sendInvoke<TauriTypes.MarketBackfillStatus>("market_backfill_start");
  }
  backfillStatus() {
    return this.client.sendInvoke<TauriTypes.MarketBackfillStatus>("market_backfill_status");
  }
  priceSources() {
    return this.client.sendInvoke<TauriTypes.MarketPriceSources>("market_price_sources");
  }
}
