import { invoke } from "@tauri-apps/api/core";

export interface HotWordBatchPreview {
  words: string[];
  duplicates: number;
  invalid: string[];
}

export interface HotWordBatchResult {
  added: number;
  duplicates: number;
  invalid: string[];
}

export const previewHotWords = (text: string) => invoke<HotWordBatchPreview>("preview_hot_words", { text });
export const addHotWords = (text: string) => invoke<HotWordBatchResult>("add_hot_words", { text });
export const removeHotWords = (texts: string[]) => invoke<number>("remove_hot_words", { texts });
