import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import SubtitleFloatingPanel from "./SubtitleFloatingPanel";

const native = vi.hoisted(() => ({ sync: vi.fn(async () => undefined) }));
vi.mock("@/api/subtitleWindow", () => ({ updateSubtitleHitRegion: native.sync }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));

const storageKey = "light-whisper-subtitle-layout-v1";
beforeEach(() => {
  localStorage.clear();
  native.sync.mockClear();
  vi.stubGlobal("PointerEvent", MouseEvent);
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(100, 100, 400, 160));
  HTMLElement.prototype.setPointerCapture = vi.fn();
  HTMLElement.prototype.hasPointerCapture = vi.fn(() => false);
  HTMLElement.prototype.releasePointerCapture = vi.fn();
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function panel() {
  return render(<SubtitleFloatingPanel active editable><div className="subtitle-capsule">Text</div></SubtitleFloatingPanel>);
}

describe("subtitle direct manipulation", () => {
  it("moves with the handle and saves the position only after release", () => {
    const { container } = panel();
    const handle = screen.getByRole("button", { name: "subtitle.moveWindow" });
    fireEvent.pointerDown(handle, { button: 0, clientX: 110, clientY: 110 });
    fireEvent.pointerMove(handle, { clientX: 190, clientY: 160 });
    expect(localStorage.getItem(storageKey)).toBeNull();
    expect(container.querySelector(".subtitle-floating-panel")).toHaveStyle({ left: "180px", top: "150px" });
    fireEvent.pointerUp(handle);
    expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual({ x: 180, y: 150 });
  });

  it("resizes from the bottom-right and restores saved dimensions on remount", () => {
    const first = panel();
    const handle = screen.getByRole("button", { name: "subtitle.resizeWindow" });
    fireEvent.pointerDown(handle, { button: 0, clientX: 500, clientY: 260 });
    fireEvent.pointerMove(handle, { clientX: 600, clientY: 320 });
    fireEvent.pointerUp(handle);
    expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual({ x: 100, y: 100, width: 500, height: 220 });
    first.unmount();
    const second = panel();
    expect(second.container.querySelector(".subtitle-floating-panel")).toHaveStyle({ width: "500px", height: "220px" });
  });

  it("resets saved geometry and restores the automatic layout", () => {
    localStorage.setItem(storageKey, '{"x":100,"y":100,"width":500,"height":220}');
    const { container } = panel();
    fireEvent.click(screen.getByRole("button", { name: "subtitle.resetWindow" }));
    expect(localStorage.getItem(storageKey)).toBe("null");
    expect(container.querySelector(".subtitle-floating-panel")).not.toHaveStyle({ position: "absolute" });
  });

  it("locks the ordinary overlay and never changes native regions through the recording lifecycle", async () => {
    const view = render(<SubtitleFloatingPanel active><div>Text</div></SubtitleFloatingPanel>);
    expect(screen.queryByRole("button")).toBeNull();
    view.rerender(<SubtitleFloatingPanel active={false}><div>Text</div></SubtitleFloatingPanel>);
    await act(async () => {});
    view.unmount();
    expect(native.sync).not.toHaveBeenCalled();
  });

  it("refreshes the locked overlay when the separate editor saves a layout", () => {
    const view = render(<SubtitleFloatingPanel active><div>Text</div></SubtitleFloatingPanel>);
    localStorage.setItem(storageKey, '{"x":180,"y":140,"width":500,"height":220}');
    fireEvent(window, new StorageEvent("storage", { key: storageKey }));
    expect(view.container.querySelector(".subtitle-floating-panel")).toHaveStyle({ left: "180px", top: "140px", width: "500px", height: "220px" });
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("finishes a cancelled gesture and persists a usable layout", () => {
    panel();
    const handle = screen.getByRole("button", { name: "subtitle.moveWindow" });
    fireEvent.pointerDown(handle, { button: 0, clientX: 110, clientY: 110 });
    fireEvent.pointerMove(handle, { clientX: 130, clientY: 120 });
    fireEvent.pointerCancel(handle);
    expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual({ x: 120, y: 110 });
  });
});
