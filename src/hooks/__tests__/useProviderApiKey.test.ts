import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useProviderApiKey } from "../useProviderApiKey";

const api = vi.hoisted(() => ({ saveProviderApiKey: vi.fn(), deleteProviderApiKey: vi.fn() }));
const errors = vi.hoisted(() => ({ error: vi.fn() }));
const translate = (key: string) => key;
vi.mock("@/api/tauri", () => api);
vi.mock("sonner", () => ({ toast: errors }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: translate }) }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  api.saveProviderApiKey.mockResolvedValue(undefined);
  api.deleteProviderApiKey.mockResolvedValue(undefined);
});
afterEach(() => vi.useRealTimers());

describe("provider credential safety", () => {
  it("ignores late reads from the previous provider without writing either credential", async () => {
    const old = deferred<string>();
    const read = vi.fn((provider: string) => provider === "openai" ? old.promise : Promise.resolve("go-key"));
    const { result, rerender } = renderHook(({ provider }) => useProviderApiKey(provider, read), { initialProps: { provider: "openai" } });
    await act(async () => { rerender({ provider: "opencode-go" }); });
    expect(result.current.key).toBe("go-key");
    await act(async () => { old.resolve("old-key"); });
    expect(result.current.key).toBe("go-key");
    expect(api.saveProviderApiKey).not.toHaveBeenCalled();
    expect(api.deleteProviderApiKey).not.toHaveBeenCalled();
  });

  it("keeps the original provider attached to a pending save across a switch", async () => {
    const read = vi.fn().mockResolvedValue("saved-key");
    const { result, rerender } = renderHook(({ provider }) => useProviderApiKey(provider, read), { initialProps: { provider: "opencode-go" } });
    await act(async () => {});
    act(() => result.current.setKey("edited-key"));
    await act(async () => { rerender({ provider: "openai" }); });
    act(() => result.current.setKey("new-provider-key"));
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.saveProviderApiKey).toHaveBeenNthCalledWith(1, "opencode-go", "edited-key");
    expect(api.saveProviderApiKey).toHaveBeenNthCalledWith(2, "openai", "new-provider-key");
  });

  it("preserves the displayed key on a failed refresh and never writes it back", async () => {
    const read = vi.fn().mockResolvedValueOnce("saved-key").mockRejectedValue(new Error("unavailable"));
    const { result } = renderHook(() => useProviderApiKey("opencode-go", read));
    await act(async () => {});
    await act(async () => { await result.current.refresh(); });
    expect(result.current.key).toBe("saved-key");
    expect(errors.error).toHaveBeenCalledWith("toast.apiKeyReadFailed");
    expect(api.saveProviderApiKey).not.toHaveBeenCalled();
    expect(api.deleteProviderApiKey).not.toHaveBeenCalled();
  });

  it("does not delete when the user empties an input; explicit deletion removes the saved key", async () => {
    const read = vi.fn().mockResolvedValue("saved-key");
    const { result } = renderHook(() => useProviderApiKey("opencode-go", read));
    await act(async () => {});
    act(() => result.current.setKey(""));
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    expect(api.saveProviderApiKey).not.toHaveBeenCalled();
    expect(api.deleteProviderApiKey).not.toHaveBeenCalled();
    await act(async () => { await result.current.remove(); });
    expect(api.deleteProviderApiKey).toHaveBeenCalledExactlyOnceWith("opencode-go");
    expect(result.current.key).toBe("");
  });

  it("reports a failed save and preserves the draft for retry", async () => {
    const read = vi.fn().mockResolvedValue("saved-key");
    api.saveProviderApiKey.mockRejectedValue(new Error("unavailable"));
    const { result } = renderHook(() => useProviderApiKey("opencode-go", read));
    await act(async () => {});
    act(() => result.current.setKey("edited-key"));
    await act(async () => { await expect(result.current.flush()).rejects.toThrow("unavailable"); });
    expect(result.current.key).toBe("edited-key");
    expect(errors.error).toHaveBeenCalledWith("toast.apiKeySaveFailed");
  });

  it("waits for an in-flight save before deleting, so the key cannot be resurrected", async () => {
    const read = vi.fn().mockResolvedValue("saved-key");
    const saving = deferred<void>();
    api.saveProviderApiKey.mockReturnValue(saving.promise);
    const { result } = renderHook(() => useProviderApiKey("opencode-go", read));
    await act(async () => {});
    act(() => result.current.setKey("edited-key"));
    await act(async () => { await vi.advanceTimersByTimeAsync(600); });
    let removal!: Promise<void>;
    act(() => { removal = result.current.remove(); });
    expect(api.deleteProviderApiKey).not.toHaveBeenCalled();
    await act(async () => { saving.resolve(); await removal; });
    expect(api.deleteProviderApiKey).toHaveBeenCalledExactlyOnceWith("opencode-go");
    expect(result.current.key).toBe("");
  });
});
