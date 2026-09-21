import { invoke } from "@tauri-apps/api/core";
import type { CorrectionPattern } from "@/types";

export interface RuleKey { original: string; corrected: string }
export interface AuditContext { provider: string; model: string; api_url: string; api_format: string; reasoning: string; version: number }
export interface AuditRow {
  rule: CorrectionPattern; verdict: "valid" | "invalid" | "uncertain" | null;
  reason: string; error: string | null; previous: boolean; reviewed_at: number | null;
}
export interface AuditReport {
  id: string; at: number; context: AuditContext; status: "complete" | "partial" | "failed" | "empty";
  total: number; checked: number; reused: number; suggested: number; uncertain: number; failed: number; rows: AuditRow[];
}
export interface DeletedCorrection { rule: CorrectionPattern; at: number; reason: string }
export interface AuditView { report: AuditReport | null; current_rules: CorrectionPattern[]; deleted: DeletedCorrection[]; running: boolean; context_current: boolean }
export interface AuditMutation { changed: number; skipped: { key: RuleKey; reason: string }[] }
export const getCorrectionAudit = () => invoke<AuditView>("get_correction_audit");
export const validateCorrections = (force = false) => invoke<AuditReport>("validate_corrections", { force });
export const confirmCorrectionDeletions = (reportId: string, selected: RuleKey[]) => invoke<AuditMutation>("confirm_correction_deletions", { reportId, selected });
export const restoreAuditedCorrection = (key: RuleKey) => invoke<void>("restore_audited_correction", { key });
