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
}
