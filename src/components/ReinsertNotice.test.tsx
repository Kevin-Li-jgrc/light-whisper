import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const events = vi.hoisted(() => new Map<string, (event: { payload: unknown }) => void>());
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    events.set(name, handler);
    return () => { events.delete(name); };
  }),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import ReinsertNotice from "./ReinsertNotice";
beforeEach(() => { events.clear(); vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });
function emit(name: string, payload: unknown) {
  act(() => { events.get(name)?.({ payload }); });
}
it("displays feedback without the transcription panel and expires it", () => {
  render(<ReinsertNotice />);
  emit("reinsert-status", "empty");
  expect(screen.getByRole("status")).toHaveTextContent("reinsert.empty");
  act(() => { vi.advanceTimersByTime(2400); });
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
it("replaces earlier feedback and clears it for a new recording session", () => {
  const { rerender } = render(<ReinsertNotice key={1} />);
  emit("reinsert-status", "empty");
  act(() => { vi.advanceTimersByTime(2000); });
  emit("reinsert-status", "sent");
  act(() => { vi.advanceTimersByTime(500); });
  expect(screen.getByRole("status")).toHaveTextContent("reinsert.sent");
  rerender(<ReinsertNotice key={2} />);
  expect(screen.queryByRole("status")).not.toBeInTheDocument();
});
