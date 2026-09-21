import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { confirmCorrectionDeletions, getCorrectionAudit, restoreAuditedCorrection, validateCorrections, type AuditView, type RuleKey } from "@/api/correctionAudit";
import "./correction-audit.css";

const keyOf = (rule: RuleKey) => JSON.stringify([rule.original, rule.corrected]);
const pairOf = (rule: RuleKey): RuleKey => ({ original: rule.original, corrected: rule.corrected });
const describeError = (error: unknown) => error instanceof Error ? error.message : String(error);

export default function CorrectionAuditPanel({ configKey, onRefreshProfile }: {
  configKey: string; onRefreshProfile: () => void;
}) {
  const { t } = useTranslation();
  const [view, setView] = useState<AuditView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirming, setConfirming] = useState(false);
  const [forcePrompt, setForcePrompt] = useState(false);
  const requestId = useRef(0);
  const reportId = useRef<string | null>(null);
  const load = useCallback(async () => {
    const id = ++requestId.current;
    try {
      const result = await getCorrectionAudit();
      if (id !== requestId.current) return;
      if (reportId.current !== (result.report?.id ?? null)) {
        reportId.current = result.report?.id ?? null;
        setSelected(new Set()); setConfirming(false);
      }
      setView(result);
    } catch (err) { if (id === requestId.current) setError(describeError(err)); }
  }, []);
  useEffect(() => {
    void load();
    const timer = window.setInterval(() => { if (!document.hidden) void load(); }, 10000);
    return () => { ++requestId.current; window.clearInterval(timer); };
  }, [load, configKey]);

  const disabled = busy || view?.running === true;
  const report = view?.report;
  const deletedKeys = new Set(view?.deleted.map(entry => keyOf(entry.rule)));
  const actionable = (report?.rows ?? []).filter(row => view?.context_current && !row.error && !row.previous
    && row.verdict === "invalid" && !deletedKeys.has(keyOf(row.rule))
    && view.current_rules.some(rule => keyOf(rule) === keyOf(row.rule) && rule.source === "ai" && rule.count === row.rule.count && rule.last_seen === row.rule.last_seen));
  const eligible = new Set(actionable.map(row => keyOf(row.rule)));
  const chosen = actionable.filter(row => selected.has(keyOf(row.rule)));
  const run = async (force: boolean) => {
    setBusy(true); setError(null); setNotice(null); setForcePrompt(false); setConfirming(false);
    ++requestId.current;
    try { await validateCorrections(force); await load(); onRefreshProfile(); }
    catch (err) { setError(describeError(err)); }
    finally { setBusy(false); }
  };
  const remove = async () => {
    if (!report || !chosen.length) return;
    setBusy(true); setError(null); setNotice(null);
    const id = report.id;
    try {
      const result = await confirmCorrectionDeletions(id, chosen.map(row => pairOf(row.rule)));
      setNotice([t("correctionAudit.deletedResult", { count: result.changed, skipped: result.skipped.length }), ...result.skipped.map(item => `${item.key.original} → ${item.key.corrected}：${item.reason}`)].join("\n"));
      setSelected(new Set()); setConfirming(false); await load(); onRefreshProfile();
    } catch (err) { setError(describeError(err)); }
    finally { setBusy(false); }
  };
  const restore = async (key: RuleKey) => {
    setBusy(true); setError(null); setNotice(null);
    try { await restoreAuditedCorrection(pairOf(key)); setNotice(t("correctionAudit.restored")); await load(); onRefreshProfile(); }
    catch (err) { setError(describeError(err)); }
    finally { setBusy(false); }
  };
  const time = (at: number) => new Date(at * 1000).toLocaleString();

  return <section className="correction-audit" aria-label={t("correctionAudit.title")}>
    <h3>{t("correctionAudit.title")}</h3>
    <p className="audit-hint">{t("correctionAudit.scopeHint")}</p>
    <div className="audit-actions">
      <button type="button" className="test-btn" disabled={disabled} onClick={() => void run(false)}>{disabled ? t("settings.correctionValidationRunning") : t("settings.correctionValidationRun")}</button>
      <button type="button" className="test-btn" disabled={disabled} onClick={() => { setForcePrompt(true); setConfirming(false); }}>{t("correctionAudit.force")}</button>
      <button type="button" className="test-btn" disabled={busy} onClick={() => void load()}>{t("correctionAudit.refresh")}</button>
    </div>
    <p className="audit-hint">{t("correctionAudit.cacheHint")}</p>
    {forcePrompt && <div className="audit-confirm">
      <p>{t("correctionAudit.forceHint")}</p>
      <div className="audit-actions"><button type="button" className="test-btn" disabled={disabled} onClick={() => void run(true)}>{t("correctionAudit.confirmForce")}</button><button type="button" className="test-btn" onClick={() => setForcePrompt(false)}>{t("correctionAudit.cancel")}</button></div>
    </div>}
    {error && <p role="alert" className="audit-error">{error}</p>}
    {notice && <p role="status" className="audit-notice">{notice}</p>}
    {report ? <>
      <p role="status"><strong>{t(`correctionAudit.status_${report.status}`)}</strong></p>
      <p className="audit-hint">{time(report.at)} · {report.context.provider} / {report.context.model}</p>
      <p>{t("correctionAudit.counts", { total: report.total, checked: report.checked, reused: report.reused, suggested: report.suggested, uncertain: report.uncertain, failed: report.failed })}</p>
      {!view?.context_current && <p className="audit-error">{t("correctionAudit.contextChanged")}</p>}
      <div className="audit-rows">
        {report.rows.map(row => {
          const id = keyOf(row.rule); const canDelete = eligible.has(id);
          return <article className="audit-row" key={id}>
            <div className="audit-rule">
              {canDelete && <input type="checkbox" aria-label={t("correctionAudit.select", { ...pairOf(row.rule) })} checked={selected.has(id)} disabled={disabled} onChange={() => { setConfirming(false); setSelected(current => { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next; }); }} />}
              <strong>{row.rule.original} → {row.rule.corrected}</strong>
            </div>
            <span className="audit-verdict">{deletedKeys.has(id) ? t("correctionAudit.alreadyDeleted") : row.error ? t("correctionAudit.itemFailed") : t(`correctionAudit.verdict_${row.verdict}`)}</span>
            {row.previous && <p>{t("correctionAudit.previous", { time: row.reviewed_at ? time(row.reviewed_at) : "" })}</p>}
            {row.reason && <p>{row.reason}</p>}
            {row.error && <p className="audit-error">{row.error}</p>}
            {!canDelete && !row.error && row.verdict === "invalid" && !deletedKeys.has(id) && <p className="audit-hint">{t("correctionAudit.stale")}</p>}
          </article>;
        })}
      </div>
      <button type="button" className="test-btn" disabled={disabled || chosen.length === 0} onClick={() => { setConfirming(true); setForcePrompt(false); }}>{t("correctionAudit.reviewSelection", { count: chosen.length })}</button>
      {confirming && chosen.length > 0 && <section className="audit-confirm" aria-label={t("correctionAudit.confirmTitle", { count: chosen.length })}>
        <h4>{t("correctionAudit.confirmTitle", { count: chosen.length })}</h4>
        <ul>{chosen.map(row => <li key={keyOf(row.rule)}>{row.rule.original} → {row.rule.corrected}</li>)}</ul>
        <p>{t("correctionAudit.deleteHint")}</p>
        <div className="audit-actions"><button type="button" className="test-btn" disabled={disabled} onClick={() => void remove()}>{t("correctionAudit.confirmDelete", { count: chosen.length })}</button><button type="button" className="test-btn" disabled={disabled} onClick={() => setConfirming(false)}>{t("correctionAudit.cancel")}</button></div>
      </section>}
    </> : <p className="audit-hint">{t("correctionAudit.noReport")}</p>}
    <details className="audit-deleted" open={view?.deleted.length ? true : undefined}>
      <summary>{t("correctionAudit.deletedTitle", { count: view?.deleted.length ?? 0 })}</summary>
      <p className="audit-hint">{t("correctionAudit.restoreHint")}</p>
      {view?.deleted.map(entry => <article className="audit-row" key={keyOf(entry.rule)}>
        <strong>{entry.rule.original} → {entry.rule.corrected}</strong>
        <p>{entry.reason}</p><p className="audit-hint">{time(entry.at)}</p>
        <button type="button" className="test-btn" disabled={disabled} onClick={() => void restore(entry.rule)}>{t("correctionAudit.restore", { ...pairOf(entry.rule) })}</button>
      </article>)}
    </details>
  </section>;
}
