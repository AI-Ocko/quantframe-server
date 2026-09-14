import { TauriTypes } from "$types";
import { useQuery } from "@tanstack/react-query";
import { TauriClient } from "..";
enum CacheType {
  TradableItems = "tradable_items",
}
export class CacheModule {
  private readonly _cache: Map<string, any> = new Map();
  constructor(private readonly client: TauriClient) {}

  async getTradableItems(forceRefresh = false): Promise<TauriTypes.CacheTradableItem[]> {
    if (!forceRefresh && this._cache.has(CacheType.TradableItems)) return this._cache.get(CacheType.TradableItems);
    const items = await this.client.sendInvoke<TauriTypes.CacheTradableItem[]>("cache_get_tradable_items");
    this._cache.set(CacheType.TradableItems, items);
    return items;
  }
  async getTradableItemById(id: string): Promise<TauriTypes.CacheTradableItem | undefined> {
    let items = await this.getTradableItems();
    return items.find((i) => i.wfmId === id);
  }
  getThemePresets() {
    return useQuery({
      queryKey: ["cache_get_theme_presets"],
      queryFn: () => this.client.sendInvoke<TauriTypes.CacheTheme[]>("cache_get_theme_presets"),
      retry: false,
    });
  }
  clearCache() {
    this._cache.clear();
  }
}
