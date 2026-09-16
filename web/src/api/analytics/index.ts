import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class AnalyticsModule {
  constructor(private readonly client: TauriClient) {}

  items(from: string, to: string) {
    return this.client.sendInvoke<TauriTypes.AnalyticsItemRow[]>("analytics_items", { from, to });
  }
  stock() {
    return this.client.sendInvoke<TauriTypes.AnalyticsStockRow[]>("analytics_stock");
  }
  partners(from: string, to: string) {
    return this.client.sendInvoke<TauriTypes.AnalyticsPartnerRow[]>("analytics_partners", { from, to });
  }
  timeline(from: string, to: string, bucket: TauriTypes.AnalyticsBucket) {
    return this.client.sendInvoke<TauriTypes.AnalyticsTimelineRow[]>("analytics_timeline", { from, to, bucket });
  }
}
