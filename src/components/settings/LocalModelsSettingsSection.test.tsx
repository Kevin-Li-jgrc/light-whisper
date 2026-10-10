import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import LocalModelsSettingsSection from "./LocalModelsSettingsSection";
import { localLlm } from "@/api/localLlm";
vi.mock("@/i18n/localAi", () => ({ loadLocalAiTranslations: vi.fn().mockResolvedValue(undefined) }));

vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => undefined) }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
vi.mock("@/api/localLlm", () => ({ localLlm: {
  models: vi.fn(), status: vi.fn(), download: vi.fn(), delete: vi.fn(), configure: vi.fn(), switchAll: vi.fn(),
} }));

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(localLlm.models).mockResolvedValue([{ spec: { id: "qwen3.5-0.8b", name: "Qwen Q8", size: 811843840, license: "Apache-2.0", licenseUrl: "" }, downloaded: true, partialBytes: 0 }]);
  vi.mocked(localLlm.status).mockResolvedValue({ phase: "unloaded" });
  vi.mocked(localLlm.delete).mockResolvedValue();
  vi.mocked(localLlm.switchAll).mockResolvedValue();
});

describe("local text model controls", () => {
  it("does not download, load, or change providers on opening settings", async () => {
    render(<LocalModelsSettingsSection profile={null} onSaved={() => undefined} />);
    await screen.findByText("Qwen Q8");
    expect(localLlm.download).not.toHaveBeenCalled();
    expect(localLlm.configure).not.toHaveBeenCalled();
    expect(localLlm.switchAll).not.toHaveBeenCalled();
  });
  it("deletes only after explicit confirmation", async () => {
    render(<LocalModelsSettingsSection profile={null} onSaved={() => undefined} />);
    fireEvent.click(await screen.findByText("localAi.delete"));
    expect(localLlm.delete).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("localAi.confirm"));
    await waitFor(() => expect(localLlm.delete).toHaveBeenCalledWith("qwen3.5-0.8b"));
  });
  it("requires a user action to switch every text role", async () => {
    const saved = vi.fn();
    render(<LocalModelsSettingsSection profile={null} onSaved={saved} />);
    fireEvent.click(await screen.findByText("localAi.switchAll"));
    await waitFor(() => expect(localLlm.switchAll).toHaveBeenCalledOnce());
    await waitFor(() => expect(saved).toHaveBeenCalledOnce());
  });
});
