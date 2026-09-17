import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import StatusIndicator from "@/components/StatusIndicator";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key }),
}));

describe("StatusIndicator device label", () => {
  it.each([
    ["vulkan", "Intel Arc Graphics", "Intel Arc Graphics"],
    ["vulkan", null, "GPU (Vulkan)"],
    ["cuda", "NVIDIA RTX A4000 Laptop GPU", "NVIDIA RTX A4000 Laptop GPU"],
    ["cuda", null, "GPU"],
    ["cpu", null, "CPU"],
    ["cloud", null, "status.online"],
  ])("labels %s with name %s as %s", (device, gpuName, expected) => {
    render(
      <StatusIndicator
        stage="ready"
        isReady
        isStarting={false}
        isRecording={false}
        isProcessing={false}
        device={device}
        gpuName={gpuName}
        downloadProgress={0}
        downloadMessage={null}
        isDownloading={false}
        downloadModels={vi.fn()}
        cancelDownload={vi.fn()}
      />,
    );

    expect(screen.getByText(expected)).toBeInTheDocument();
    if (device !== "cpu") {
      expect(screen.queryByText("CPU")).not.toBeInTheDocument();
    }
  });
});
