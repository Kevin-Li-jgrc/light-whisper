export interface SubtitleRect { x: number; y: number; width: number; height: number }
export interface SubtitleViewport { width: number; height: number }
export interface SubtitleLayout { x: number; y: number; width?: number; height?: number }

const MARGIN = 8;

export function clampSubtitleRect(rect: SubtitleRect, viewport: SubtitleViewport): SubtitleRect {
  const width = Math.min(rect.width, Math.max(1, viewport.width - MARGIN * 2));
  const height = Math.min(rect.height, Math.max(1, viewport.height - MARGIN * 2));
  return {
    x: Math.max(MARGIN, Math.min(rect.x, viewport.width - width - MARGIN)),
    y: Math.max(MARGIN, Math.min(rect.y, viewport.height - height - MARGIN)),
    width,
    height,
  };
}

export function resizeSubtitleRect(rect: SubtitleRect, dx: number, dy: number, viewport: SubtitleViewport): SubtitleRect {
  const minimum = clampSubtitleRect({ ...rect, width: 240, height: 80 }, viewport);
  return {
    x: minimum.x,
    y: minimum.y,
    width: Math.min(viewport.width - minimum.x - MARGIN, Math.max(minimum.width, rect.width + dx)),
    height: Math.min(viewport.height - minimum.y - MARGIN, Math.max(minimum.height, rect.height + dy)),
  };
}

export function parseSubtitleLayout(stored: string | null): SubtitleLayout | null {
  try {
    const value: unknown = JSON.parse(stored ?? "null");
    if (!value || typeof value !== "object" || Array.isArray(value)) return null;
    const { x, y, width, height } = value as Partial<SubtitleRect>;
    if (typeof x !== "number" || !Number.isFinite(x) || typeof y !== "number" || !Number.isFinite(y)) return null;
    if (width === undefined && height === undefined) return { x, y };
    if (typeof width !== "number" || !Number.isFinite(width) || width <= 0
      || typeof height !== "number" || !Number.isFinite(height) || height <= 0) return null;
    return { x, y, width, height };
  } catch {
    return null;
  }
}
