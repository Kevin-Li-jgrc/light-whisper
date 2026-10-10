import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";
import { localLlm, type LocalDownload, type LocalModel, type LocalStatus } from "@/api/localLlm";
import type { UserProfile } from "@/types";
import { loadLocalAiTranslations } from "@/i18n/localAi";

export default function LocalModelsSettingsSection({ profile, onSaved, speechEngine, speechLocal }: { profile: UserProfile | null; onSaved: () => void; speechEngine?: string; speechLocal?: boolean }) {
  const { t } = useTranslation();
  const [models, setModels] = useState<LocalModel[]>([]);
  const [status, setStatus] = useState<LocalStatus>({ phase: "unloaded" });
  const [progress, setProgress] = useState<Record<string, LocalDownload>>({});
  const [busy, setBusy] = useState(false);
  const [translated, setTranslated] = useState(false);
  const [deleting, setDeleting] = useState<string | null>(null);
  const config = profile?.llm_provider.local ?? { model: "qwen3.5-0.8b", device: "auto" };
  const refresh = useCallback(async () => {
    const [models, status] = await Promise.all([localLlm.models(), localLlm.status()]);
    setModels(models); setStatus(status);
  }, []);
  useEffect(() => {
    void loadLocalAiTranslations().then(() => setTranslated(true)).catch((e) => toast.error(String(e)));
    void refresh().catch((e) => toast.error(String(e)));
    const statusListener = listen<LocalStatus>("local-llm-status", (e) => setStatus(e.payload));
    const downloadListener = listen<LocalDownload>("local-llm-download", (e) => {
      setProgress((p) => ({ ...p, [e.payload.model]: e.payload }));
      if (["ready", "error"].includes(e.payload.phase)) void refresh().catch(() => undefined);
    });
    return () => { void statusListener.then((off) => off()); void downloadListener.then((off) => off()); };
  }, [refresh]);
  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    try { await work(); onSaved(); await refresh(); }
    catch (e) { toast.error(String(e)); }
    finally { setBusy(false); }
  };
  if (!translated) return null;
  return <section className="settings-section" aria-label={t("localAi.title")}>
    <h2 className="settings-section-title">{t("localAi.title")}</h2>
    <p className="settings-hint">{t("localAi.boundary")}</p>
    {speechEngine && <p className="settings-hint">{t("localAi.speech", { engine: speechEngine, mode: t(speechLocal ? "localAi.speechLocal" : "localAi.speechOnline") })}</p>}
    <p role="status">{t(`localAi.${status.phase}`, { defaultValue: status.phase })}{status.device ? ` · ${status.device.toUpperCase()}` : ""}</p>
    {status.error && <p role="alert">{status.error}</p>}
    <label className="settings-row">{t("localAi.device")}
      <select className="settings-input" value={config.device} disabled={busy} onChange={(e) => void run(() => localLlm.configure(config.model, e.target.value))}>
        <option value="auto">{t("localAi.auto")}</option><option value="cpu">CPU</option>
      </select>
    </label>
    {models.map(({ spec, downloaded, downloadActive, filePresent, licenseText, partialBytes }) => {
      const p = progress[spec.id];
      const downloading = downloadActive || (p && ["downloading", "verifying"].includes(p.phase));
      return <div key={spec.id} className="settings-card" style={{ padding: 12, marginTop: 12 }}>
        <strong>{spec.name}</strong>
        <p className="settings-hint">{(spec.size / 1e9).toFixed(2)} GB · {spec.license}</p>
        <p className="settings-hint">{spec.id.startsWith("lfm") ? t("localAi.lfmLicense") : t("localAi.qwenLicense")}</p>
        <details><summary>{spec.license}</summary><pre style={{ whiteSpace: "pre-wrap", maxHeight: 180, overflow: "auto", fontSize: 11 }}>{licenseText}</pre></details>
        {config.model === spec.id && <span>{t("localAi.selected")}</span>}
        {p && <p role="status">{t(`localAi.${p.phase}`, { defaultValue: p.phase })} · {Math.round(p.received / p.total * 100)}% {p.error}</p>}
        <div className="settings-row" style={{ flexWrap: "wrap", gap: 8 }}>
          {!downloaded && !downloading && <button className="btn-ghost" onClick={() => {
            setProgress((all) => ({ ...all, [spec.id]: { model: spec.id, phase: "downloading", received: partialBytes, total: spec.size } }));
            void localLlm.download(spec.id).then(refresh).catch((e) => { toast.error(String(e)); setProgress((all) => ({ ...all, [spec.id]: { ...all[spec.id], phase: "error" } })); });
          }}>{t(partialBytes ? "localAi.resume" : "localAi.download")}</button>}
          {downloading && <button className="btn-ghost" onClick={() => void localLlm.cancelDownload(spec.id)}>{t("localAi.cancel")}</button>}
          {downloaded && <button className="btn-ghost" disabled={busy} onClick={() => void run(() => localLlm.configure(spec.id, config.device))}>{t("localAi.enable")}</button>}
          {(downloaded || filePresent || partialBytes > 0) && !downloading && <button className="btn-ghost" disabled={busy} onClick={() => setDeleting(spec.id)}>{t("localAi.delete")}</button>}
        </div>
        {deleting === spec.id && <div role="alertdialog" aria-label={t("localAi.deleteConfirm")}>
          <p>{t("localAi.deleteConfirm")}</p>
          <button className="btn-ghost" disabled={busy} onClick={() => void run(async () => { await localLlm.delete(spec.id); setDeleting(null); })}>{t("localAi.confirm")}</button>
          <button className="btn-ghost" onClick={() => setDeleting(null)}>{t("localAi.cancel")}</button>
        </div>}
      </div>;
    })}
    <div className="settings-row" style={{ flexWrap: "wrap", gap: 8, marginTop: 12 }}>
      <button className="btn-ghost" disabled={busy} onClick={() => void run(async () => { await localLlm.release(); await localLlm.load(); })}>{t("localAi.reload")}</button>
      <button className="btn-ghost" disabled={busy} onClick={() => void run(localLlm.release)}>{t("localAi.release")}</button>
      <button className="btn-ghost" onClick={() => void localLlm.cancel()}>{t("localAi.cancelRequest")}</button>
      <button className="btn-ghost" disabled={busy} onClick={() => void run(() => localLlm.switchAll())}>{t("localAi.switchAll")}</button>
      {profile?.llm_provider.local_cloud_backup && <button className="btn-ghost" disabled={busy} onClick={() => void run(() => localLlm.switchAll(true))}>{t("localAi.restore")}</button>}
    </div>
  </section>;
}
