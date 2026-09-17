import type { SubtitleTiming } from "@/lib/subtitleTiming";
import { invoke } from "@tauri-apps/api/core";

export const openSubtitleLayoutEditor = () => invoke<void>("open_subtitle_layout_editor");
export const showSubtitleLayoutEditor = () => invoke<void>("show_subtitle_layout_editor");
export const closeSubtitleLayoutEditor = () => invoke<void>("close_subtitle_layout_editor");

export const getSubtitleTiming = () => invoke<SubtitleTiming>("get_subtitle_timing");
export const setSubtitleTiming = (timing: SubtitleTiming) => invoke<void>("set_subtitle_timing", { timing });
