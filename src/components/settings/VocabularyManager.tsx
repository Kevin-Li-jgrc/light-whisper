import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { X } from "lucide-react";
import { addHotWords, previewHotWords, removeHotWords, type HotWordBatchPreview } from "@/api/vocabulary";
import type { HotWord } from "@/types";
import "./vocabulary.css";

export interface VocabularyManagerProps {
  words: HotWord[];
  onSaved: () => Promise<unknown>;
  onClose: () => void;
}

const PAGE_SIZE = 50;

export default function VocabularyManager({ words, onSaved, onClose }: VocabularyManagerProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [mode, setMode] = useState<"manage" | "add">("manage");
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [pendingDelete, setPendingDelete] = useState<string[]>([]);
  const [draft, setDraft] = useState("");
  const [preview, setPreview] = useState<HotWordBatchPreview | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [status, setStatus] = useState("");

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); };
  }, []);

  const filtered = useMemo(() => {
    const keyword = search.trim().toLowerCase();
    return words.filter((word) => word.text.toLowerCase().includes(keyword))
      .sort((a, b) => b.weight - a.weight || b.use_count - a.use_count || a.text.localeCompare(b.text));
  }, [words, search]);
  const pages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  const currentPage = Math.min(page, pages - 1);
  const visible = filtered.slice(currentPage * PAGE_SIZE, (currentPage + 1) * PAGE_SIZE);
  const selectedWords = visible.filter((word) => selected.has(word.text)).map((word) => word.text);
  const allSelected = visible.length > 0 && selectedWords.length === visible.length;
  const locked = busy || pendingDelete.length > 0;

  const run = async (operation: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    setError("");
    setStatus("");
    try { await operation(); }
    catch (cause) { setError(String(cause)); }
    finally { setBusy(false); }
  };
  const changePage = (next: number) => { setPage(next); setSelected(new Set()); };
  const confirmAdd = () => run(async () => {
    const result = await addHotWords(draft);
    setDraft("");
    setPreview(null);
    setStatus(t("vocabulary.addResult", { added: result.added, duplicates: result.duplicates, invalid: result.invalid.length }));
    await onSaved();
  });
  const confirmDelete = () => run(async () => {
    const removed = await removeHotWords(pendingDelete);
    setPendingDelete([]);
    setSelected(new Set());
    setStatus(t("vocabulary.deleteResult", { count: removed }));
    await onSaved();
  });

  return createPortal(
    <dialog ref={dialogRef} className="vocabulary-dialog" aria-labelledby="vocabulary-title"
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}>
      <header className="vocabulary-toolbar">
        <h2 id="vocabulary-title">{t(mode === "add" ? "vocabulary.batchAdd" : "vocabulary.manage")}</h2>
        <span className="vocabulary-muted">{t("vocabulary.total", { count: words.length })}</span>
        <button type="button" className="icon-btn" disabled={busy} aria-label={t("common.close")} onClick={onClose}><X size={16} /></button>
      </header>
      {error && <p className="settings-error" role="alert">{error}</p>}
      {status && <p className="vocabulary-message" role="status">{status}</p>}
      {mode === "manage" ? <>
        <div className="vocabulary-toolbar">
          <input type="search" className="settings-input" aria-label={t("vocabulary.search")}
            placeholder={t("vocabulary.search")} value={search} disabled={locked}
            onChange={(event) => { setSearch(event.target.value); changePage(0); }} />
          <button type="button" className="test-btn" disabled={locked}
            onClick={() => { setMode("add"); setStatus(""); setError(""); }}>{t("vocabulary.batchAdd")}</button>
        </div>
        <div className="vocabulary-toolbar">
          <label className="vocabulary-checkbox"><input type="checkbox" checked={allSelected} disabled={locked || visible.length === 0}
            onChange={(event) => setSelected(new Set(event.target.checked ? visible.map((word) => word.text) : []))} />{t("vocabulary.selectPage")}</label>
          <button type="button" className="btn-ghost" disabled={locked || selectedWords.length === 0}
            onClick={() => setPendingDelete(selectedWords)}>{t("vocabulary.deleteSelected", { count: selectedWords.length })}</button>
        </div>
        {pendingDelete.length > 0 && <div className="vocabulary-confirm" role="alert">
          <p>{t("vocabulary.deletePrompt", { count: pendingDelete.length })}</p>
          <div className="vocabulary-delete-preview">{pendingDelete.join("、")}</div>
          <div className="vocabulary-toolbar">
            <button type="button" className="test-btn" disabled={busy} onClick={() => setPendingDelete([])}>{t("common.cancel")}</button>
            <button type="button" className="test-btn" disabled={busy} onClick={() => void confirmDelete()}>{t("vocabulary.confirmDelete")}</button>
          </div>
        </div>}
        <div className="vocabulary-list">
          {visible.length === 0 && <p className="vocabulary-muted">{t("vocabulary.empty")}</p>}
          {visible.map((word) => <div className="vocabulary-row" key={word.text}>
            <label className="vocabulary-checkbox vocabulary-word">
              <input type="checkbox" checked={selected.has(word.text)} disabled={locked}
                onChange={(event) => setSelected((previous) => {
                  const next = new Set(previous);
                  if (event.target.checked) next.add(word.text); else next.delete(word.text);
                  return next;
                })} />
              <span>{word.text}</span>
            </label>
            <span className="vocabulary-muted">{t(word.source === "user" ? "settings.sourceManual" : "settings.sourceLearned")}</span>
            <button type="button" className="icon-btn" disabled={locked} aria-label={t("settings.removeHotWordLabel", { word: word.text })}
              onClick={() => setPendingDelete([word.text])}><X size={14} /></button>
          </div>)}
        </div>
        <footer className="vocabulary-toolbar vocabulary-pagination">
          <span>{t("vocabulary.page", { page: currentPage + 1, pages, count: filtered.length })}</span>
          <button type="button" className="test-btn" disabled={locked || currentPage === 0} onClick={() => changePage(currentPage - 1)}>{t("vocabulary.previous")}</button>
          <button type="button" className="test-btn" disabled={locked || currentPage + 1 >= pages} onClick={() => changePage(currentPage + 1)}>{t("vocabulary.next")}</button>
        </footer>
      </> : <>
        <p className="vocabulary-muted">{t("vocabulary.batchHint")}</p>
        <label className="vocabulary-draft-label">
          <span>{t("vocabulary.draft")}</span>
          <textarea className="settings-input vocabulary-draft" value={draft} disabled={busy}
            placeholder={"SPC 工作站\n狗窝检具\n自动测量机\nType 1"}
            onChange={(event) => { setDraft(event.target.value); setPreview(null); setError(""); setStatus(""); }} />
        </label>
        <div className="vocabulary-batch-preview" aria-live="polite">
          {preview && <>
            <p>{t("vocabulary.previewResult", { added: preview.words.length, duplicates: preview.duplicates, invalid: preview.invalid.length })}</p>
            <div className="vocabulary-preview-words">{preview.words.map((word) => <span key={word}>{word}</span>)}</div>
            {preview.invalid.length > 0 && <details>
              <summary>{t("vocabulary.invalidHint")}</summary>
              <ul>{preview.invalid.map((word, index) => <li key={index}>{word}</li>)}</ul>
            </details>}
          </>}
        </div>
        <footer className="vocabulary-toolbar vocabulary-pagination">
          <button type="button" className="btn-ghost" disabled={busy} onClick={() => { setMode("manage"); setError(""); }}>{t("vocabulary.back")}</button>
          <button type="button" className="test-btn" disabled={busy || !draft.trim()}
            onClick={() => void run(async () => { setPreview(await previewHotWords(draft)); })}>{t("vocabulary.preview")}</button>
          <button type="button" className="test-btn" disabled={busy || !preview?.words.length}
            onClick={() => void confirmAdd()}>{t("vocabulary.confirmAdd")}</button>
        </footer>
      </>}
      {busy && <span className="vocabulary-muted" role="status">{t("common.loading")}</span>}
    </dialog>, document.body,
  );
}
