import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class LiveScraperModule {
  constructor(private readonly client: TauriClient) {}

  status() {
    return this.client.sendInvoke<TauriTypes.TraderStatus>("trader_status");
  }
  start() {
    return this.client.sendInvoke<TauriTypes.TraderStatus>("trader_start");
  }
  stop() {
    return this.client.sendInvoke<TauriTypes.TraderStatus>("trader_stop");
  }
  setOptions(options: { dryRun?: boolean; deleteBuyOrdersOnStop?: boolean }) {
    return this.client.sendInvoke<TauriTypes.TraderOptions>("trader_set_options", options);
  }
  dryRunLog(page: number, limit: number) {
    return this.client.sendInvoke<TauriTypes.DryRunPage>("trader_dry_run_log", { page, limit });
  }
  dryRunSummary(days: number) {
    return this.client.sendInvoke<TauriTypes.DryRunSummary>("trader_dry_run_summary", { days });
  }

  async toggle(): Promise<TauriTypes.TraderStatus> {
    const status = await this.status();
    return status.state === "trading" ? this.stop() : this.start();
  }
  async get_state(): Promise<{ is_running: boolean }> {
    const status = await this.status();
    return { is_running: status.state === "trading" };
  }
  get_interesting_wtb_items(settings: TauriTypes.ItemSettings): Promise<TauriTypes.ItemPriceInfo[]> {
    return this.client.sendInvoke<TauriTypes.ItemPriceInfo[]>("trader_interesting_items", { settings });
  }
}
