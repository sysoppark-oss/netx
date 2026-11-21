// 대시보드 배치: 구역의 순서, 너비(12칸 가운데 몇 칸), 높이(0 = 내용에 맞춤).
// 저장된 값은 이 PC의 localStorage에서 온다. 모양이 맞는 것만 되살리고 나머지는 처음 배치로 채운다.

export type Slot = { id: string; span: number; h: number };
/** 구역의 처음 배치와 가장 좁은 너비 */
export type Spec = { id: string; span: number; min: number };

export const COLS = 12;
export const H_MIN = 120;
export const H_MAX = 1600;
export const H_STEP = 40;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

export function defaults(specs: Spec[]): Slot[] {
  return specs.map((s) => ({ id: s.id, span: s.span, h: 0 }));
}

/** 저장된 글자를 배치로 되살린다. 모르는 구역은 버리고, 빠진 구역은 처음 배치대로 뒤에 붙인다. */
export function parseLayout(raw: string | null, specs: Spec[]): Slot[] {
  const out: Slot[] = [];
  try {
    const list: unknown = raw ? JSON.parse(raw) : null;
    if (Array.isArray(list)) {
      for (const item of list.slice(0, 32)) {
        if (typeof item !== "object" || item === null) continue;
        const { id, span, h } = item as Record<string, unknown>;
        const spec = specs.find((s) => s.id === id);
        if (!spec || out.some((o) => o.id === spec.id)) continue;
        const width = typeof span === "number" && Number.isFinite(span) ? clamp(Math.round(span), spec.min, COLS) : spec.span;
        const height = typeof h === "number" && Number.isFinite(h) && h > 0 ? clamp(Math.round(h), H_MIN, H_MAX) : 0;
        out.push({ id: spec.id, span: width, h: height });
      }
    }
  } catch {
    // 깨진 값이면 처음 배치를 쓴다.
  }
  for (const s of specs) if (!out.some((o) => o.id === s.id)) out.push({ id: s.id, span: s.span, h: 0 });
  return out;
}

/** id 구역을 to 구역의 자리로 옮긴다. */
export function moveTo(layout: Slot[], id: string, to: string): Slot[] {
  const from = layout.findIndex((s) => s.id === id);
  const at = layout.findIndex((s) => s.id === to);
  if (from < 0 || at < 0 || from === at) return layout;
  const next = [...layout];
  const [item] = next.splice(from, 1);
  next.splice(at, 0, item);
  return next;
}

/** 한 칸 앞(-1)이나 뒤(+1)로 */
export function nudge(layout: Slot[], id: string, by: -1 | 1): Slot[] {
  const from = layout.findIndex((s) => s.id === id);
  const other = layout[from + by];
  return from < 0 || !other ? layout : moveTo(layout, id, other.id);
}

export function resize(layout: Slot[], specs: Spec[], id: string, span: number, h: number): Slot[] {
  const min = specs.find((s) => s.id === id)?.min ?? 3;
  const width = clamp(Math.round(span), min, COLS);
  const height = h > 0 ? clamp(Math.round(h / (H_STEP / 2)) * (H_STEP / 2), H_MIN, H_MAX) : 0;
  return layout.map((s) => (s.id === id && (s.span !== width || s.h !== height) ? { ...s, span: width, h: height } : s));
}
