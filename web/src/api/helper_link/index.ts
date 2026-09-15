import { TauriClient } from "..";
import { TauriTypes } from "$types";

export class HelperLinkModule {
  constructor(private readonly client: TauriClient) {}

  devices() {
    return this.client.sendInvoke<TauriTypes.HelperDevice[]>("helper_devices");
  }
  create(name: string) {
    return this.client.sendInvoke<TauriTypes.HelperDeviceCreated>("helper_device_create", { name });
  }
  revoke(id: number) {
    return this.client.sendInvoke<boolean>("helper_device_revoke", { id });
  }
}
