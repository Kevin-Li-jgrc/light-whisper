import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import Kbd from "@/components/Kbd";
import { useHotkeyCapture } from "@/hooks/useHotkeyCapture";
import { formatHotkeyForDisplay } from "@/lib/hotkey";

export default function ReinsertHotkeySettings() {
  const { t } = useTranslation();
  const [shortcut, setShortcut] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void invoke<string | null>("get_reinsert_hotkey").then((value) => {
      if (active) { setShortcut(value); setLoaded(true); }
    }).catch((error: unknown) => { if (active) setError(String(error)); });
    return () => { active = false; };
  }, []);
  const save = async (value: string | null) => {
    setError("");
    try {
      await invoke("set_reinsert_hotkey", { shortcut: value });
      setShortcut(value);
    } catch (error) {
      setError(String(error));
      throw error;
    }
  };
  const capture = useHotkeyCapture({ save, label: t("settings.reinsertHotkeyLabel") });
  const disabled = !loaded || clearing || capture.saving;
  const clear = async () => {
    capture.cancelCapture();
    setClearing(true);
    try { await save(null); } catch { /* 错误已显示在设置项下方。 */ }
    finally { setClearing(false); }
  };
  return (
    <div className="settings-column" style={{ gap: 6, marginTop: 16 }}>
      <span className="settings-option-desc">{t("settings.reinsertHotkeyLabel")}</span>
      <div className="settings-row">
        <button className="theme-btn hotkey-capture-btn" aria-label={t("settings.reinsertHotkeyLabel")}
          disabled={disabled} data-capturing={capture.capturing} onClick={capture.startCapture}>
          {capture.capturing ? t("settings.pressReinsertHotkey") : shortcut
            ? <Kbd combo={formatHotkeyForDisplay(shortcut)} /> : t("settings.noReinsertHotkey")}
        </button>
        <button className="btn-ghost" disabled={disabled || !shortcut} onClick={() => { void clear(); }}>
          {t("common.clear")}
        </button>
      </div>
      <p className="settings-hint settings-hint-flush">{t("settings.reinsertHotkeyHint")}</p>
      {error && <p className="settings-error" role="alert">{error}</p>}
    </div>
  );
}
