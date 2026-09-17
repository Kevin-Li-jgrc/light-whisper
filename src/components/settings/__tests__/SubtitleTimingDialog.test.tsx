import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({
  get: vi.fn(), set: vi.fn(), close: vi.fn(), t: (key: string) => key,
}));
vi.mock("@/api/subtitleWindow", () => ({ getSubtitleTiming: api.get, setSubtitleTiming: api.set }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: api.t }) }));
import SubtitleTimingDialog from "../SubtitleTimingDialog";

beforeEach(() => {
  vi.clearAllMocks();
  api.get.mockResolvedValue({ hold_ms: 2000, fade_ms: 300, hide_ms: 2500 });
  api.set.mockResolvedValue(undefined);
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
});
afterEach(() => vi.restoreAllMocks());
const input = (name: string) => screen.getByRole("spinbutton", { name: new RegExp(name) });
const change = (name: string, value: string) => fireEvent.change(input(name), { target: { value } });
async function open() {
  render(<SubtitleTimingDialog onClose={api.close} />);
  await waitFor(() => expect(input("subtitle.timingHold")).not.toBeDisabled());
}

describe("subtitle timing settings dialog", () => {
  it("loads seconds and saves all three values together in milliseconds", async () => {
    await open();
    expect(input("subtitle.timingHold")).toHaveValue(2);
    expect(input("subtitle.timingFade")).toHaveValue(0.3);
    expect(input("subtitle.timingHide")).toHaveValue(2.5);
    change("subtitle.timingHold", "5");
    change("subtitle.timingFade", "0.8");
    change("subtitle.timingHide", "6.2");
    fireEvent.click(screen.getByRole("button", { name: "subtitle.timingSave" }));
    await waitFor(() => expect(api.set).toHaveBeenCalledWith({ hold_ms: 5000, fade_ms: 800, hide_ms: 6200 }));
    expect(api.close).toHaveBeenCalledOnce();
  });
  it("blocks a premature hide and empty inputs", async () => {
    await open();
    change("subtitle.timingHold", "5");
    expect(screen.getByRole("button", { name: "subtitle.timingSave" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("subtitle.timingInvalid");
    change("subtitle.timingHold", "");
    expect(screen.getByRole("button", { name: "subtitle.timingSave" })).toBeDisabled();
    expect(api.set).not.toHaveBeenCalled();
  });
  it("resets the draft but does not save when cancelled", async () => {
    api.get.mockResolvedValue({ hold_ms: 5000, fade_ms: 800, hide_ms: 6200 });
    await open();
    fireEvent.click(screen.getByRole("button", { name: "subtitle.timingReset" }));
    expect(input("subtitle.timingHide")).toHaveValue(2.5);
    fireEvent.click(screen.getByRole("button", { name: "common.cancel" }));
    expect(api.set).not.toHaveBeenCalled();
    expect(api.close).toHaveBeenCalledOnce();
  });
  it("preserves the draft and shows save failures", async () => {
    api.set.mockRejectedValueOnce(new Error("save failed"));
    await open();
    fireEvent.click(screen.getByRole("button", { name: "subtitle.timingSave" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("save failed"));
    expect(api.close).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "subtitle.timingSave" })).not.toBeDisabled();
  });
  it("does not overwrite settings when loading fails", async () => {
    api.get.mockRejectedValueOnce(new Error("load failed"));
    render(<SubtitleTimingDialog onClose={api.close} />);
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("load failed"));
    expect(screen.getByRole("button", { name: "subtitle.timingSave" })).toBeDisabled();
    expect(api.set).not.toHaveBeenCalled();
  });
});
