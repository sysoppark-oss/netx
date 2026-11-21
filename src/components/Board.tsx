// 대시보드 판: 구역을 12칸 격자에 놓는다. 제목 띠를 끌어 옮기고 오른쪽 아래 모서리를 끌어 크기를 바꾼다.
// [배치 편집]을 켜면 구역 어디를 잡아도 옮겨지고, 같은 일을 하는 단추가 보인다.
// 끌기는 포인터 이벤트로만 한다(라이브러리 없음). 키보드로도 되도록 같은 일을 하는 단추를 둔다.
// 배치는 이 PC의 localStorage에만 남는다.
import { useCallback, useRef, useState, type CSSProperties, type PointerEvent as RPointerEvent, type ReactNode } from "react";
import { BTN } from "../lib/ui";
import { COLS, H_MIN, H_STEP, defaults, moveTo, nudge, parseLayout, resize, type Slot, type Spec } from "../lib/board";

export type BoardItem = Spec & { title: string; node: ReactNode };

function read(key: string, specs: Spec[]): Slot[] {
  try {
    return parseLayout(localStorage.getItem(key), specs);
  } catch {
    return defaults(specs);
  }
}

const MINI = "rounded-sm border border-line px-1.5 py-px text-[11px] hover:border-accent hover:text-accent disabled:opacity-40";

export function Board({ storageKey, items }: { storageKey: string; items: BoardItem[] }) {
  const [layout, setLayout] = useState<Slot[]>(() => read(storageKey, items));
  const [edit, setEdit] = useState(false);
  const [held, setHeld] = useState<string | null>(null);
  const grid = useRef<HTMLDivElement>(null);
  const live = useRef(layout);
  live.current = layout;
  const sizing = useRef<{ id: string; x: number; y: number; w: number; h: number; tall: boolean } | null>(null);

  const keep = useCallback(
    (next: Slot[]) => {
      try {
        localStorage.setItem(storageKey, JSON.stringify(next));
      } catch {
        // 저장하지 못하면 이번에 켜져 있는 동안만 쓴다.
      }
    },
    [storageKey],
  );
  const change = (next: Slot[]) => {
    if (next === live.current) return;
    live.current = next;
    setLayout(next);
  };
  const commit = (next: Slot[]) => {
    change(next);
    keep(next);
  };

  // 옮기기: 구역의 제목 띠(data-grip)를 잡아 끌면 다른 구역 자리로 들어간다. [배치 편집]을 켜면 구역 어디를 잡아도 된다.
  // 6픽셀 넘게 움직여야 끌기로 본다(그냥 누른 것은 누르기로 남긴다).
  const press = useRef<{ id: string; x: number; y: number } | null>(null);
  const dragging = useRef<string | null>(null);
  const down = (id: string) => (e: RPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || sizing.current) return;
    const t = e.target as HTMLElement;
    if (t.closest("[data-board-tool]")) return;
    if (!edit) {
      const grip = t.closest("[data-grip]");
      const control = t.closest("button,input,select,a,textarea");
      // 제목 띠 안의 단추·입력칸을 누른 것은 끌기가 아니다. (카드처럼 단추가 띠를 감싼 경우는 끌 수 있다.)
      if (!grip || (control && grip.contains(control))) return;
    }
    press.current = { id, x: e.clientX, y: e.clientY };
  };
  const move = (id: string) => (e: RPointerEvent<HTMLDivElement>) => {
    const p = press.current;
    if (!p || p.id !== id) return;
    if (dragging.current !== id) {
      if (Math.hypot(e.clientX - p.x, e.clientY - p.y) < 6) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      dragging.current = id;
      setHeld(id);
    }
    const over = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-slot]")?.dataset.slot;
    if (over && over !== id) change(moveTo(live.current, id, over));
  };
  const up = () => {
    press.current = null;
    if (!dragging.current) return;
    dragging.current = null;
    setHeld(null);
    keep(live.current);
  };

  // 크기: 오른쪽 아래 모서리를 끈다. 너비는 격자 칸에, 높이는 20픽셀에 맞춘다.
  const sizeStart = (id: string) => (e: RPointerEvent<HTMLButtonElement>) => {
    if (e.button !== 0) return;
    const box = e.currentTarget.closest<HTMLElement>("[data-slot]");
    if (!box) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    sizing.current = { id, x: e.clientX, y: e.clientY, w: box.offsetWidth, h: box.offsetHeight, tall: false };
    setHeld(id);
  };
  const sizeMove = (e: RPointerEvent<HTMLButtonElement>) => {
    const s = sizing.current;
    const col = (grid.current?.clientWidth ?? 0) / COLS;
    if (!s || col <= 0) return;
    const dy = e.clientY - s.y;
    if (Math.abs(dy) > 12) s.tall = true;
    const now = live.current.find((x) => x.id === s.id);
    change(resize(live.current, items, s.id, (s.w + (e.clientX - s.x)) / col, s.tall ? s.h + dy : (now?.h ?? 0)));
  };
  const sizeEnd = () => {
    if (!sizing.current) return;
    sizing.current = null;
    setHeld(null);
    keep(live.current);
  };

  const step = (slot: Slot, dSpan: number, dH: number, box: HTMLElement | null) => {
    const h = dH === 0 ? slot.h : Math.max(H_MIN, (slot.h || box?.offsetHeight || 320) + dH);
    commit(resize(live.current, items, slot.id, slot.span + dSpan, h));
  };

  // 배치에 없는 구역이 나중에 생기면 맨 뒤에 붙여 그린다(사라지지 않게).
  const shown = [...layout, ...defaults(items.filter((x) => !layout.some((s) => s.id === x.id)))];

  return (
    <div>
      <div className="mb-2 flex flex-wrap items-center justify-end gap-2 text-xs">
        {edit && <span className="mr-auto text-dim">구역을 잡아 끌어 옮기고, 오른쪽 아래 모서리를 끌어 크기를 바꿉니다. ◀ ▶ − + 단추로도 됩니다.</span>}
        {edit && (
          <button onClick={() => commit(defaults(items))} className={BTN}>
            처음 배치로
          </button>
        )}
        <button onClick={() => setEdit(!edit)} aria-pressed={edit} className={`${BTN} ${edit ? "border-accent text-accent" : ""}`}>
          {edit ? "배치 편집 끝" : "배치 편집"}
        </button>
      </div>
      <div ref={grid} className={`grid grid-cols-12 gap-3 ${held ? "select-none" : ""}`}>
        {shown.map((slot, i) => {
          const item = items.find((x) => x.id === slot.id);
          if (!item) return null;
          const style = { "--span": slot.span, height: slot.h > 0 ? slot.h : undefined } as CSSProperties;
          return (
            <div
              key={slot.id}
              data-slot={slot.id}
              style={style}
              onPointerDown={down(slot.id)}
              onPointerMove={move(slot.id)}
              onPointerUp={up}
              onPointerCancel={up}
              className={`relative col-span-12 flex min-w-0 flex-col md:[grid-column:span_var(--span)] ${edit ? "cursor-grab touch-none" : ""} ${edit ? "rounded outline-1 outline-offset-4 outline-dashed" : ""} ${held === slot.id ? "opacity-80 outline-accent" : "outline-line"}`}
            >
              {edit && (
                <div data-board-tool className="mb-1 flex cursor-default flex-wrap items-center gap-1 text-[11px]">
                  <span className="rounded-sm border border-accent px-2 py-px font-semibold text-accent">⠿ {item.title}</span>
                  <button className={MINI} disabled={i === 0} onClick={() => commit(nudge(live.current, slot.id, -1))} aria-label={`${item.title} 앞으로`}>
                    ◀
                  </button>
                  <button className={MINI} disabled={i === shown.length - 1} onClick={() => commit(nudge(live.current, slot.id, 1))} aria-label={`${item.title} 뒤로`}>
                    ▶
                  </button>
                  <span className="flex-1" />
                  <span className="text-dim">너비</span>
                  <button className={MINI} disabled={slot.span <= item.min} onClick={(e) => step(slot, -1, 0, e.currentTarget.closest("[data-slot]"))} aria-label={`${item.title} 좁게`}>
                    −
                  </button>
                  <button className={MINI} disabled={slot.span >= COLS} onClick={(e) => step(slot, 1, 0, e.currentTarget.closest("[data-slot]"))} aria-label={`${item.title} 넓게`}>
                    +
                  </button>
                  <span className="ml-1 text-dim">높이</span>
                  <button className={MINI} onClick={(e) => step(slot, 0, -H_STEP, e.currentTarget.closest("[data-slot]"))} aria-label={`${item.title} 낮게`}>
                    −
                  </button>
                  <button className={MINI} onClick={(e) => step(slot, 0, H_STEP, e.currentTarget.closest("[data-slot]"))} aria-label={`${item.title} 높게`}>
                    +
                  </button>
                  <button className={MINI} disabled={slot.h === 0} onClick={() => commit(resize(live.current, items, slot.id, slot.span, 0))} title="높이를 내용에 맞춥니다">
                    자동
                  </button>
                  <span className="font-mono text-dim">
                    {slot.span}/{COLS}칸{slot.h > 0 ? ` · ${slot.h}px` : ""}
                  </span>
                </div>
              )}
              <div className={`flex min-h-0 min-w-0 flex-1 flex-col ${slot.h > 0 ? "overflow-auto" : ""} ${edit ? "pointer-events-none" : ""}`}>{item.node}</div>
              {(
                <button
                  data-board-tool
                  onPointerDown={sizeStart(slot.id)}
                  onPointerMove={sizeMove}
                  onPointerUp={sizeEnd}
                  onPointerCancel={sizeEnd}
                  tabIndex={-1}
                  aria-hidden
                  title="끌어서 크기 바꾸기"
                  className={`absolute -right-1 -bottom-1 h-5 w-5 cursor-nwse-resize touch-none rounded-sm border-r-4 border-b-4 border-accent ${edit ? "bg-accent/20" : "opacity-35 hover:opacity-100"}`}
                />
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
