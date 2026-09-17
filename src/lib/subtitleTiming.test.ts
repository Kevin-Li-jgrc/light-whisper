import { describe, expect, it } from "vitest";
import { DEFAULT_SUBTITLE_TIMING, isValidSubtitleTiming, resolveSubtitleTiming } from "./subtitleTiming";

describe("subtitle timing validation", () => {
  it("accepts custom timings and immediate results without a fade", () => {
    expect(isValidSubtitleTiming({ hold_ms: 5000, fade_ms: 800, hide_ms: 6200 })).toBe(true);
    expect(isValidSubtitleTiming({ hold_ms: 0, fade_ms: 0, hide_ms: 200 })).toBe(true);
  });
  it.each([
    undefined, {}, { hold_ms: NaN, fade_ms: 300, hide_ms: 2500 },
    { hold_ms: -1, fade_ms: 300, hide_ms: 2500 },
    { hold_ms: 5000, fade_ms: 800, hide_ms: 5900 },
    { hold_ms: 31000, fade_ms: 300, hide_ms: 35000 },
    { hold_ms: 2000, fade_ms: 6000, hide_ms: 10000 },
    { hold_ms: 2000, fade_ms: 300, hide_ms: 61000 },
  ])("falls back for invalid or missing settings: %j", (value) => {
    expect(isValidSubtitleTiming(value)).toBe(false);
    expect(resolveSubtitleTiming(value)).toEqual(DEFAULT_SUBTITLE_TIMING);
  });
});
