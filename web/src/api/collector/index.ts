import { useQuery } from "@tanstack/react-query";
import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class CollectorModule {
  constructor(private readonly client: TauriClient) {}

  health() {
    return useQuery({
      queryKey: ["collector_health"],
      queryFn: () => this.client.sendInvoke<TauriTypes.CollectorHealth>("collector_health"),
      refetchInterval: 10_000,
      retry: false,
    });
  }

  itemHistory(wfmUrl: string | undefined, subType: string | undefined, days: number) {
    return useQuery({
      queryKey: ["market_item_history", wfmUrl, subType, days],
      queryFn: () =>
        this.client.sendInvoke<TauriTypes.MarketItemHistory>("market_item_history", { wfmUrl, subType, days }),
      enabled: !!wfmUrl,
      retry: false,
    });
  }
}
