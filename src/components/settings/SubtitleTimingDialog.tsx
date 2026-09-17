import { useEffect, useRef, useState, type FormEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { getSubtitleTiming, setSubtitleTiming } from "@/api/subtitleWindow";
import { DEFAULT_SUBTITLE_TIMING, isValidSubtitleTiming, type SubtitleTiming } from "@/lib/subtitleTiming";
import "./subtitleTiming.css";

const toSeconds = (timing: SubtitleTiming) => ({
  hold_ms: String(timing.hold_ms / 1000),
  fade_ms: String(timing.fade_ms / 1000),
  hide_ms: String(timing.hide_ms / 1000),
});
const fields = [
  { key: "hold_ms", label: "subtitle.timingHold", max: 30 },
  { key: "fade_ms", label: "subtitle.timingFade", max: 5 },
  { key: "hide_ms", label: "subtitle.timingHide", max: 60 },
] as const;

export default function SubtitleTimingDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [values, setValues] = useState(() => toSeconds(DEFAULT_SUBTITLE_TIMING));
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    let disposed = false;
    void getSubtitleTiming().then((timing) => {
      if (disposed) return;
      if (!isValidSubtitleTiming(timing)) throw new Error(t("subtitle.timingInvalid"));
      setValues(toSeconds(timing));
      setLoaded(true);
    }).catch((cause: unknown) => {
      if (!disposed) setError(String(cause));
    });
    return () => { disposed = true; dialog?.close(); };
  }, [t]);

  const timing: SubtitleTiming = {
    hold_ms: values.hold_ms.trim() ? Math.round(Number(values.hold_ms) * 1000) : NaN,
    fade_ms: values.fade_ms.trim() ? Math.round(Number(values.fade_ms) * 1000) : NaN,
    hide_ms: values.hide_ms.trim() ? Math.round(Number(values.hide_ms) * 1000) : NaN,
  };
  const valid = isValidSubtitleTiming(timing);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (!loaded || !valid || saving) return;
    setSaving(true);
    setError(null);
    try {
      await setSubtitleTiming(timing);
      onClose();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  };

  return createPortal(
    <dialog ref={dialogRef} className="subtitle-timing-dialog" aria-labelledby="subtitle-timing-title"
      onCancel={(event) => { event.preventDefault(); if (!saving) onClose(); }}>
      <form onSubmit={(event) => void save(event)}>
        <h2 id="subtitle-timing-title">{t("subtitle.timingTitle")}</h2>
        <p className="subtitle-timing-help">{t("subtitle.timingHelp")}</p>
        <fieldset disabled={!loaded || saving}>
          {fields.map(({ key, label, max }) => (
            <label className="subtitle-timing-field" key={key}>
              <span>{t(label)}</span>
              <input type="number" min={0} max={max} step="0.1" required value={values[key]}
                onChange={(event) => { setValues((current) => ({ ...current, [key]: event.target.value })); setError(null); }} />
              <span>{t("subtitle.timingSeconds")}</span>
            </label>
          ))}
        </fieldset>
        <p className="subtitle-timing-help">{t("subtitle.timingOrder")}</p>
        {loaded && !valid && <p className="subtitle-timing-error" role="alert">{t("subtitle.timingInvalid")}</p>}
        {error && <p className="subtitle-timing-error" role="alert">{error}</p>}
        {!loaded && !error && <p role="status">{t("common.loading")}</p>}
        <div className="subtitle-timing-actions">
          <button type="button" className="test-btn" disabled={!loaded || saving}
            onClick={() => { setValues(toSeconds(DEFAULT_SUBTITLE_TIMING)); setError(null); }}>{t("subtitle.timingReset")}</button>
          <button type="button" className="test-btn" disabled={saving} onClick={onClose}>{t("common.cancel")}</button>
          <button type="submit" className="test-btn" disabled={!loaded || !valid || saving}>
            {t(saving ? "subtitle.timingSaving" : "subtitle.timingSave")}
          </button>
        </div>
      </form>
    </dialog>,
    document.body,
  );
}
