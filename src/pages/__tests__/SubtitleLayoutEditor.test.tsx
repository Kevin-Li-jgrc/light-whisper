import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ show: vi.fn(), close: vi.fn() }));
vi.mock("@/api/subtitleWindow", () => ({
  showSubtitleLayoutEditor: api.show, closeSubtitleLayoutEditor: api.close,
}));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => ({}) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import SubtitleLayoutEditor from "../SubtitleLayoutEditor";

beforeEach(() => {
  vi.clearAllMocks();
  api.show.mockResolvedValue(undefined);
  api.close.mockResolvedValue(undefined);
});

describe("subtitle layout settings preview", () => {
  it("shows adjustment controls and opens the native editor after rendering", async () => {
    render(<SubtitleLayoutEditor />);
    expect(screen.getByRole("button", { name: "subtitle.moveWindow" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "subtitle.resizeWindow" })).toBeInTheDocument();
    await waitFor(() => expect(api.show).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "subtitle.layoutDone" }));
    expect(api.close).toHaveBeenCalledTimes(1);
  });
  it("closes with Escape and removes the listener on unmount", () => {
    const view = render(<SubtitleLayoutEditor />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(api.close).toHaveBeenCalledTimes(1);
    view.unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(api.close).toHaveBeenCalledTimes(1);
  });
  it("keeps a failed close visible so it can be retried", async () => {
    api.close.mockRejectedValueOnce(new Error("close failed"));
    render(<SubtitleLayoutEditor />);
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "subtitle.layoutDone" })));
    expect(screen.getByRole("alert")).toHaveTextContent("close failed");
    fireEvent.click(screen.getByRole("button", { name: "subtitle.layoutDone" }));
    expect(api.close).toHaveBeenCalledTimes(2);
  });
});
