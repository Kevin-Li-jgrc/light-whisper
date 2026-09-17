import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import SubtitleFloatingPanel from "@/components/SubtitleFloatingPanel";
import { closeSubtitleLayoutEditor, showSubtitleLayoutEditor } from "@/api/subtitleWindow";
import { useTheme } from "@/hooks/useTheme";
import "@/styles/subtitle.css";

export default function SubtitleLayoutEditor() {
  const { t } = useTranslation();
  const [error, setError] = useState<string | null>(null);
  useTheme();

  const finish = useCallback(() => {
    void closeSubtitleLayoutEditor().catch((cause: unknown) => setError(String(cause)));
  }, []);

  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      void showSubtitleLayoutEditor().catch((cause: unknown) => setError(String(cause)));
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") finish();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      cancelAnimationFrame(frame);
      window.removeEventListener("keydown", onKey);
    };
  }, [finish]);

  return (
    <div className="subtitle-root subtitle-layout-editor">
      <div className="subtitle-editor-instructions">
        <strong>{t("subtitle.layoutTitle")}</strong>
        <p>{t("subtitle.layoutHelp")}</p>
        <button type="button" onClick={finish}>{t("subtitle.layoutDone")}</button>
        {error && <p role="alert">{error}</p>}
      </div>
      <SubtitleFloatingPanel active editable>
        <div className="subtitle-capsule">
          <div className="subtitle-text">{t("subtitle.layoutPreview")}</div>
        </div>
      </SubtitleFloatingPanel>
    </div>
  );
}
