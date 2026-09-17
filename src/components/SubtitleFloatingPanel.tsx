import { useCallback, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type PointerEvent, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { GripHorizontal, MoveDiagonal2, RotateCcw } from "lucide-react";
import { clampSubtitleRect, parseSubtitleLayout, resizeSubtitleRect, type SubtitleLayout, type SubtitleRect } from "@/lib/subtitleGeometry";
import { readLocalStorage, writeLocalStorage } from "@/lib/storage";

const STORAGE_KEY = "light-whisper-subtitle-layout-v1";
const viewport = () => ({ width: window.innerWidth, height: window.innerHeight });
type Gesture = { kind: "move" | "resize"; pointerId: number; startX: number; startY: number; rect: SubtitleRect };

export default function SubtitleFloatingPanel({ active, editable = false, children }: { active: boolean; editable?: boolean; children: ReactNode }) {
  const { t } = useTranslation();
  const [layout, setLayout] = useState(() => parseSubtitleLayout(readLocalStorage(STORAGE_KEY)));
  const layoutRef = useRef(layout);
  const panelRef = useRef<HTMLDivElement>(null);
  const gestureRef = useRef<Gesture | null>(null);

  const applyLayout = useCallback((next: SubtitleLayout | null, persist = false) => {
    layoutRef.current = next;
    setLayout(next);
    if (persist) writeLocalStorage(STORAGE_KEY, JSON.stringify(next));
  }, []);

  const measure = useCallback(() => {
    const element = panelRef.current;
    if (!active || !element) return;
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return;
    const current = layoutRef.current;
    if (current) {
      const bounded = clampSubtitleRect({ x: current.x, y: current.y, width: current.width ?? rect.width, height: current.height ?? rect.height }, viewport());
      const sized = current.width !== undefined;
      if (bounded.x !== current.x || bounded.y !== current.y
        || (sized && (bounded.width !== current.width || bounded.height !== current.height))) {
        applyLayout({ x: bounded.x, y: bounded.y, ...(sized ? { width: bounded.width, height: bounded.height } : {}) });
        return;
      }
    }
  }, [active, applyLayout]);

  useLayoutEffect(() => {
    const reload = () => applyLayout(parseSubtitleLayout(readLocalStorage(STORAGE_KEY)));
    const onStorage = (event: StorageEvent) => {
      if (event.key === STORAGE_KEY || event.key === null) reload();
    };
    if (active) reload();
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  }, [active, applyLayout]);

  useLayoutEffect(() => { measure(); }, [layout, measure]);
  useLayoutEffect(() => {
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(measure);
    if (panelRef.current) observer?.observe(panelRef.current);
    window.addEventListener("resize", measure);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [measure]);

  const changeRect = (rect: SubtitleRect, kind: Gesture["kind"], persist = false) => {
    const sized = kind === "resize" || layoutRef.current?.width !== undefined;
    applyLayout({ x: rect.x, y: rect.y, ...(sized ? { width: rect.width, height: rect.height } : {}) }, persist);
  };

  const start = (event: PointerEvent<HTMLButtonElement>, kind: Gesture["kind"]) => {
    if (!editable || event.button !== 0 || event.isPrimary === false || !panelRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    const { x, y, width, height } = panelRef.current.getBoundingClientRect();
    gestureRef.current = { kind, pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, rect: { x, y, width, height } };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const move = (event: PointerEvent<HTMLButtonElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || event.pointerId !== gesture.pointerId) return;
    const dx = event.clientX - gesture.startX;
    const dy = event.clientY - gesture.startY;
    const next = gesture.kind === "resize"
      ? resizeSubtitleRect(gesture.rect, dx, dy, viewport())
      : clampSubtitleRect({ ...gesture.rect, x: gesture.rect.x + dx, y: gesture.rect.y + dy }, viewport());
    changeRect(next, gesture.kind);
  };

  const finish = (event: PointerEvent<HTMLButtonElement>) => {
    if (!gestureRef.current || event.pointerId !== gestureRef.current.pointerId) return;
    gestureRef.current = null;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    writeLocalStorage(STORAGE_KEY, JSON.stringify(layoutRef.current));
  };

  const keyboard = (event: KeyboardEvent<HTMLButtonElement>, kind: Gesture["kind"]) => {
    const directions: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };
    const direction = directions[event.key];
    if (!editable || !direction || !panelRef.current) return;
    event.preventDefault();
    event.stopPropagation();
    const { x, y, width, height } = panelRef.current.getBoundingClientRect();
    const step = event.shiftKey ? 10 : 1;
    const [dx, dy] = direction.map((value) => value * step);
    const next = kind === "resize" ? resizeSubtitleRect({ x, y, width, height }, dx, dy, viewport())
      : clampSubtitleRect({ x: x + dx, y: y + dy, width, height }, viewport());
    changeRect(next, kind, true);
  };

  const handlers = (kind: Gesture["kind"]) => ({
    onPointerDown: (event: PointerEvent<HTMLButtonElement>) => start(event, kind),
    onPointerMove: move, onPointerUp: finish, onPointerCancel: finish, onLostPointerCapture: finish,
    onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => keyboard(event, kind),
  });
  const style: CSSProperties = layout ? { position: "absolute", left: layout.x, top: layout.y, width: layout.width, height: layout.height } : {};

  return (
    <div ref={panelRef} className={`subtitle-floating-panel${layout?.width !== undefined ? " is-sized" : ""}${active ? "" : " is-idle"}`}
      style={style} role="presentation" onClick={(event) => event.stopPropagation()}>
      {editable && <div className="subtitle-layout-toolbar">
        <button type="button" className="subtitle-move-handle" aria-label={t("subtitle.moveWindow")} title={t("subtitle.moveWindow")} {...handlers("move")}>
          <GripHorizontal size={16} aria-hidden="true" />
        </button>
        <button type="button" className="subtitle-reset-layout" aria-label={t("subtitle.resetWindow")} title={t("subtitle.resetWindow")} onClick={() => applyLayout(null, true)}>
          <RotateCcw size={12} aria-hidden="true" />
        </button>
      </div>}
      {children}
      {editable && <button type="button" className="subtitle-resize-handle" aria-label={t("subtitle.resizeWindow")} title={t("subtitle.resizeWindow")} {...handlers("resize")}>
        <MoveDiagonal2 size={14} aria-hidden="true" />
      </button>}
    </div>
  );
}
