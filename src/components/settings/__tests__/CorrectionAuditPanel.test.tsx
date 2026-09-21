import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { CorrectionPattern } from "@/types";
const api = vi.hoisted(() => ({ get: vi.fn(), run: vi.fn(), remove: vi.fn(), restore: vi.fn() }));
vi.mock("@/api/correctionAudit", () => ({ getCorrectionAudit: api.get, validateCorrections: api.run, confirmCorrectionDeletions: api.remove, restoreAuditedCorrection: api.restore }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string, args?: Record<string, unknown>) => `${key}${args ? ` ${JSON.stringify(args)}` : ""}` }) }));
import CorrectionAuditPanel from "../CorrectionAuditPanel";
const rule: CorrectionPattern = { original: "测试", corrected: "测试正", source: "ai", count: 3, last_seen: 100 };
const report = { id: "r1", at: 101, context: { provider: "test", model: "demo", api_url: "local", api_format: "OpenaiCompat", reasoning: "default", version: 1 }, status: "complete", total: 1, checked: 1, reused: 0, suggested: 1, uncertain: 0, failed: 0, rows: [{ rule, verdict: "invalid", reason: "替换关系不合理", error: null, previous: false, reviewed_at: 101 }] };
beforeEach(() => { vi.clearAllMocks(); api.get.mockResolvedValue({ report, current_rules: [rule], deleted: [], running: false, context_current: true }); api.run.mockResolvedValue(report); });
const open = () => render(<CorrectionAuditPanel configKey="test" onRefreshProfile={vi.fn()} />);
it("requires selecting suggestions and a separate confirmation before deleting", async () => {
  open(); await screen.findByText("替换关系不合理");
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  expect(screen.getByRole("button", { name: /correctionAudit.reviewSelection/ })).toBeDisabled();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: /correctionAudit.reviewSelection/ }));
  expect(api.remove).not.toHaveBeenCalled();
  expect(screen.getByRole("region", { name: /correctionAudit.confirmTitle/ })).toHaveTextContent("测试 → 测试正");
  api.remove.mockResolvedValue({ changed: 1, skipped: [] });
  api.get.mockResolvedValue({ report, current_rules: [], deleted: [{ rule, at: 110, reason: "替换关系不合理" }], running: false, context_current: true });
  fireEvent.click(screen.getByRole("button", { name: /correctionAudit.confirmDelete/ }));
  await waitFor(() => expect(api.remove).toHaveBeenCalledWith("r1", [{ original: "测试", corrected: "测试正" }]));
  expect(await screen.findByRole("button", { name: /correctionAudit.restore/ })).toBeInTheDocument();
});
it("shows failed review and old findings without enabling deletion", async () => {
  api.get.mockResolvedValue({ report: { ...report, status: "failed", failed: 1, suggested: 0, rows: [{ ...report.rows[0], previous: true, error: "network" }] }, current_rules: [rule], deleted: [], running: false, context_current: true });
  open(); await screen.findByText(/correctionAudit.status_failed/);
  expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
  expect(screen.getByText(/correctionAudit.previous/)).toBeInTheDocument();
});
it("explains full re-review before requesting the model again", async () => {
  open(); await screen.findByText("替换关系不合理");
  fireEvent.click(screen.getByRole("button", { name: "correctionAudit.force" }));
  expect(api.run).not.toHaveBeenCalled();
  expect(screen.getByText("correctionAudit.forceHint")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "correctionAudit.confirmForce" }));
  await waitFor(() => expect(api.run).toHaveBeenCalledWith(true));
});
it("does not report deletion success after a persistence error", async () => {
  open(); await screen.findByText("替换关系不合理");
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: /correctionAudit.reviewSelection/ }));
  api.remove.mockRejectedValue(new Error("disk full"));
  fireEvent.click(screen.getByRole("button", { name: /correctionAudit.confirmDelete/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
  expect(screen.getByRole("checkbox")).toBeChecked();
});

it("uses current rules returned with the report rather than an older parent profile", async () => {
  const updatedRule = { ...rule, count: 4, last_seen: 200 };
  api.get.mockResolvedValue({ report: { ...report, rows: [{ ...report.rows[0], rule: updatedRule }] }, current_rules: [updatedRule], deleted: [], running: false, context_current: true });
  open(); await screen.findByText("替换关系不合理");
  expect(screen.getByRole("checkbox")).not.toBeChecked();
});
