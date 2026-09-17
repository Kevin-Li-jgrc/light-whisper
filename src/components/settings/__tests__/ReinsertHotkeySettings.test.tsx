import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ invoke: vi.fn(), error: vi.fn(), success: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: api.invoke }));
vi.mock("sonner", () => ({ toast: { error: api.error, success: api.success } }));
vi.mock("@/i18n", () => ({ default: { t: (key: string) => key } }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import ReinsertHotkeySettings from "../ReinsertHotkeySettings";

beforeEach(() => {
  vi.clearAllMocks();
  api.invoke.mockImplementation(async (command: string) => command === "get_reinsert_hotkey" ? "Ctrl+Alt+V" : undefined);
});

describe("reinsert hotkey settings", () => {
  it("loads the saved shortcut and clears it without resetting other hotkeys", async () => {
    render(<ReinsertHotkeySettings />);
    await screen.findByText("Ctrl");
    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    await screen.findByText("settings.noReinsertHotkey");
    expect(api.invoke).toHaveBeenCalledWith("set_reinsert_hotkey", { shortcut: null });
  });

  it("captures a replacement shortcut and displays it after saving", async () => {
    render(<ReinsertHotkeySettings />);
    await screen.findByText("Ctrl");
    fireEvent.click(screen.getByRole("button", { name: "settings.reinsertHotkeyLabel" }));
    fireEvent.keyDown(window, { key: "F8", code: "F8" });
    await waitFor(() => expect(api.invoke).toHaveBeenCalledWith("set_reinsert_hotkey", { shortcut: "F8" }));
    await screen.findByText("F8");
  });

  it("keeps the previous shortcut when registration fails", async () => {
    render(<ReinsertHotkeySettings />);
    await screen.findByText("Ctrl");
    api.invoke.mockRejectedValueOnce(new Error("快捷键冲突"));
    fireEvent.click(screen.getByRole("button", { name: "common.clear" }));
    await screen.findByText("Error: 快捷键冲突");
    expect(screen.getByText("Ctrl")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "common.clear" })).not.toBeDisabled();
  });
});
