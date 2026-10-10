import { invoke } from "@tauri-apps/api/core";

export interface LocalModel {
  spec: { id: string; name: string; size: number; license: string; licenseUrl: string };
  downloaded: boolean;
  filePresent?: boolean;
  downloadActive?: boolean;
  licenseText?: string;
  partialBytes: number;
}
export interface LocalStatus { phase: string; model?: string; device?: string; error?: string }
export interface LocalDownload { model: string; phase: string; received: number; total: number; error?: string }

export const localLlm = {
  models: () => invoke<LocalModel[]>("local_llm_models"),
  status: () => invoke<LocalStatus>("local_llm_status"),
  download: (model: string) => invoke<void>("local_llm_download", { model }),
  cancelDownload: (model: string) => invoke<void>("local_llm_cancel_download", { model }),
  delete: (model: string) => invoke<void>("local_llm_delete", { model }),
  load: () => invoke<void>("local_llm_load"),
  release: () => invoke<void>("local_llm_release"),
  cancel: () => invoke<void>("local_llm_cancel"),
  configure: (model: string, device: string) => invoke<void>("local_llm_configure", { model, device }),
  switchAll: (restore = false) => invoke<void>("local_llm_switch_all", { restore }),
};
