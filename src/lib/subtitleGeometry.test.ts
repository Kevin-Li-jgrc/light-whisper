import { describe, expect, it } from "vitest";
import { clampSubtitleRect, parseSubtitleLayout, resizeSubtitleRect } from "./subtitleGeometry";

describe("subtitle window geometry", () => {
  it("keeps a moved panel inside a smaller monitor", () => {
    expect(clampSubtitleRect({ x: 1700, y: -40, width: 500, height: 200 }, { width: 800, height: 600 }))
      .toEqual({ x: 292, y: 8, width: 500, height: 200 });
  });

  it("shrinks saved dimensions to fit a smaller viewport", () => {
    expect(clampSubtitleRect({ x: 100, y: 100, width: 1200, height: 900 }, { width: 640, height: 480 }))
      .toEqual({ x: 8, y: 8, width: 624, height: 464 });
  });

  it("limits resizing at the screen edge and enforces a usable minimum", () => {
    const initial = { x: 200, y: 150, width: 400, height: 200 };
    expect(resizeSubtitleRect(initial, 1000, 1000, { width: 800, height: 600 }))
      .toEqual({ x: 200, y: 150, width: 592, height: 442 });
    expect(resizeSubtitleRect(initial, -1000, -1000, { width: 800, height: 600 }))
      .toEqual({ x: 200, y: 150, width: 240, height: 80 });
  });

  it("restores a position without forcing status-hint dimensions on future text", () => {
    expect(parseSubtitleLayout('{"x":25,"y":30}')).toEqual({ x: 25, y: 30 });
    expect(parseSubtitleLayout('{"x":25,"y":30,"width":500,"height":240}'))
      .toEqual({ x: 25, y: 30, width: 500, height: 240 });
  });

  it.each([null, "broken", "null", "[]", '{"x":"25","y":30}', '{"x":1,"y":2,"width":-1,"height":80}', '{"x":1,"y":2,"width":400}'])("ignores invalid stored geometry: %s", (stored) => {
      expect(parseSubtitleLayout(stored)).toBeNull();
    });
});
