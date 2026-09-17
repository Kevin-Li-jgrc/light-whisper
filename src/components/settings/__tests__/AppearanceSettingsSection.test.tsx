import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
const api = vi.hoisted(() => ({ open: vi.fn(), error: vi.fn() }));
vi.mock("@/api/subtitleWindow", () => ({ openSubtitleLayoutEditor: api.open }));
vi.mock("sonner", () => ({ toast: { error: api.error } }));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => ({ isDark: false, theme: "light", setTheme: vi.fn() }) }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key, i18n: { language: "zh" } }) }));
import AppearanceSettingsSection from "../AppearanceSettingsSection";

const picker = { isOpen: false, isExpanded: false, toggle: vi.fn(), close: vi.fn(), setRef: vi.fn(), popoverClass: "" };
beforeEach(() => { vi.clearAllMocks(); api.open.mockResolvedValue(undefined); });
describe("overlay layout settings entry", () => {
  it("opens the dedicated editor from appearance settings", async () => {
    render(<AppearanceSettingsSection picker={picker} />);
    fireEvent.click(screen.getByRole("button", { name: "subtitle.layoutAdjust" }));
    await waitFor(() => expect(api.open).toHaveBeenCalledTimes(1));
  });
  it("reports opening failures and allows retry", async () => {
    api.open.mockRejectedValueOnce(new Error("open failed"));
    render(<AppearanceSettingsSection picker={picker} />);
    const button = screen.getByRole("button", { name: "subtitle.layoutAdjust" });
    fireEvent.click(button);
    await waitFor(() => expect(api.error).toHaveBeenCalledWith("Error: open failed"));
    expect(button).not.toBeDisabled();
  });
});
