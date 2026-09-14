import { TauriClient } from "..";
import { TauriTypes } from "$types";
// Live trading returns in phase 3; these stubs keep the stock and wish list screens working.
export class LiveScraperModule {
  constructor(_client: TauriClient) {}
  async toggle(): Promise<void> {}
  async get_interesting_wtb_items(_settings: TauriTypes.ItemSettings): Promise<TauriTypes.ItemPriceInfo[]> {
    return [];
  }
  async get_state(): Promise<{ is_running: boolean }> {
    return { is_running: false };
  }
}
