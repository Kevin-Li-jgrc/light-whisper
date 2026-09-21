import { invoke } from "@tauri-apps/api/core";
import type { HotWord } from "@/types";

export interface WeightEvent { at: number; before: number; after: number; reason: string }
export interface RankedWord {
  text: string; source: HotWord["source"]; rank: number; base_weight: number; effective_weight: number;
  in_asr: boolean; score: number; uses: number; corrections: number; last_used: number;
  next_threshold: number | null; events: WeightEvent[];
}
export interface PrioritySnapshot { enabled: boolean; words: RankedWord[] }
export const getHotwordPriority = () => invoke<PrioritySnapshot>("get_hotword_priority");
export const setHotwordLearning = (enabled: boolean) => invoke<void>("set_hotword_learning", { enabled });
export const resetHotwordLearning = (text: string) => invoke<void>("reset_hotword_learning", { text });
