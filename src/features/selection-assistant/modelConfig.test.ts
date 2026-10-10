import { describe, expect, it } from "vitest";
import { resolveSelectionModelConfig } from "./modelConfig";

describe("local selection routing", () => {
  it("stays local when polish returns to cloud and no separate model name is set", () => {
    expect(resolveSelectionModelConfig({ active: "openai", selection_use_separate_model: true, selection_provider: "local" })).toMatchObject({ provider: "local", model: "local-selected", followsPolish: false });
  });
});
