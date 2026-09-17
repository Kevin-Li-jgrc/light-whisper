export interface SubtitleTiming {
  hold_ms: number;
  fade_ms: number;
  hide_ms: number;
}

export const DEFAULT_SUBTITLE_TIMING: SubtitleTiming = { hold_ms: 2000, fade_ms: 300, hide_ms: 2500 };

export function isValidSubtitleTiming(value: unknown): value is SubtitleTiming {
  if (!value || typeof value !== "object") return false;
  const timing = value as SubtitleTiming;
  return [timing.hold_ms, timing.fade_ms, timing.hide_ms].every((ms) => Number.isSafeInteger(ms) && ms >= 0)
    && timing.hold_ms <= 30000 && timing.fade_ms <= 5000 && timing.hide_ms <= 60000
    && timing.hide_ms >= timing.hold_ms + timing.fade_ms + 200;
}

export function resolveSubtitleTiming(value: unknown): SubtitleTiming {
  return isValidSubtitleTiming(value) ? value : DEFAULT_SUBTITLE_TIMING;
}
