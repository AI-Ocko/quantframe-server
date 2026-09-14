import { TauriClient } from "..";

// Log export is not available in the server version (spec §14 A3).
export class LogModule {
  constructor(_client: TauriClient) {}
}
