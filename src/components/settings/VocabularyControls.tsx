import { lazy, Suspense, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { HotWord } from "@/types";

const VocabularyManager = lazy(() => import("./VocabularyManager"));

export default function VocabularyControls({ words, loaded, onSaved }: {
  words: HotWord[]; loaded: boolean; onSaved: () => Promise<unknown>;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const preview = useMemo(() => [...words]
    .sort((a, b) => b.weight - a.weight || b.use_count - a.use_count)
    .slice(0, 8), [words]);
  return <>
    <div style={{ display: "flex", flexWrap: "wrap", gap: 6, overflowWrap: "anywhere" }}>
      {preview.map((word) => <span key={word.text} style={{
        padding: "3px 8px", borderRadius: 8, fontSize: 12,
        background: "var(--color-bg-secondary)", border: "1px solid var(--color-border)",
      }}>{word.text}</span>)}
    </div>
    <div className="settings-row">
      <p className="settings-hint" style={{ flex: 1, margin: 0 }}>{t("vocabulary.summary", { count: words.length })}</p>
      <button type="button" className="test-btn" disabled={!loaded} onClick={() => setOpen(true)}>{t("vocabulary.manage")}</button>
    </div>
    {open && <Suspense fallback={<p role="status">{t("common.loading")}</p>}>
      <VocabularyManager words={words} onSaved={onSaved} onClose={() => setOpen(false)} />
    </Suspense>}
  </>;
}
