import { TauriClient } from "..";
import { TauriTypes } from "$types";
import { fileToBase64 } from "@utils/pickFile";

export class SoundModule {
    constructor(private readonly client: TauriClient) { }

    getCustomSounds(): Promise<TauriTypes.CustomSound[]> {
        return this.client.sendInvoke<TauriTypes.CustomSound[]>("sound_get_custom_sounds");
    }

    async addCustomSound(name: string, file: File): Promise<TauriTypes.CustomSound[]> {
        return this.client.sendInvoke<TauriTypes.CustomSound[]>("sound_add_custom_sound", {
            name,
            file_name: file.name,
            data_base64: await fileToBase64(file),
        });
    }

    deleteCustomSound(fileName: string): Promise<TauriTypes.CustomSound[]> {
        return this.client.sendInvoke<TauriTypes.CustomSound[]>("sound_delete_custom_sound", { fileName });
    }

}
