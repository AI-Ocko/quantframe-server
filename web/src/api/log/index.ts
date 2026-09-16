import { TauriClient } from "..";
import { TauriTypes } from "$types";

// Log export is not available in the server version (spec §14 A3).
export class LogModule {
  constructor(private readonly client: TauriClient) {}

  tail(limit: number) {
    return this.client.sendInvoke<TauriTypes.LogLine[]>("log_tail", { limit });
  }
}
