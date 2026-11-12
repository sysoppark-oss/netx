// 최근 진단 기록 목록. 한 건을 고르면 그때의 계획과 결과를 본 화면에 다시 연다.
// 기록은 이 PC에만 있다. 결과 목록이므로 말씀 표시를 넣지 않는다.
import { useEffect, useState } from "react";
import { historyDelete, historyList, historyLoad, type HistoryEntry, type HistoryItem } from "../lib/history";

const LIGHT: Record<string, string> = { green: "🟢", yellow: "🟡", red: "🔴", info: "⚪" };

function whenText(ms: number): string {
  const d = new Date(ms);
  const two = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

export function HistoryView({ onOpen, onClose }: { onOpen: (entry: HistoryEntry, time: number) => void; onClose: () => void }) {
  const [items, setItems] = useState<HistoryItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmAll, setConfirmAll] = useState(false);

  const refresh = () =>
    historyList()
      .then(setItems)
      .catch((e) => setError(String(e)));
  useEffect(() => {
    void refresh();
  }, []);

  function open(item: HistoryItem) {
    historyLoad(item.id)
      .then((entry) => (entry ? onOpen(entry, item.time) : setError("이 기록은 읽을 수 없습니다(형식이 다릅니다).")))
      .catch((e) => setError(String(e)));
  }

  function remove(id?: string) {
    historyDelete(id)
      .then(refresh)
      .catch((e) => setError(String(e)));
    setConfirmAll(false);
  }

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6" onClick={onClose}>
      <div className="flex max-h-full w-full max-w-3xl flex-col rounded-lg border border-line bg-panel" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center gap-3 border-b border-line px-4 py-3">
          <h1 className="text-base font-semibold">🕘 최근 진단</h1>
          <span className="text-xs text-dim">끝난 진단을 최근 30건까지 이 PC에만 남깁니다.</span>
          <span className="flex-1" />
          {items && items.length > 0 &&
            (confirmAll ? (
              <>
                <span className="text-xs text-caution">기록을 모두 지울까요?</span>
                <button onClick={() => remove()} className="rounded border border-danger/60 px-2.5 py-1 text-xs text-danger hover:bg-raised">
                  모두 지우기
                </button>
                <button onClick={() => setConfirmAll(false)} className="rounded border border-line px-2.5 py-1 text-xs hover:bg-raised">
                  그만두기
                </button>
              </>
            ) : (
              <button onClick={() => setConfirmAll(true)} className="rounded border border-line px-2.5 py-1 text-xs hover:bg-raised">
                기록 모두 지우기
              </button>
            ))}
          <button onClick={onClose} className="rounded border border-line px-3 py-1 text-xs hover:bg-raised">
            닫기
          </button>
        </header>
        {error && <p className="border-b border-danger/40 bg-danger/10 px-4 py-2 text-sm text-danger">{error}</p>}
        <div className="min-h-40 overflow-y-auto">
          {items === null ? (
            <p className="p-8 text-center text-sm text-dim">기록을 읽는 중입니다…</p>
          ) : items.length === 0 ? (
            <p className="p-8 text-center text-sm text-dim">아직 기록이 없습니다. 진단을 실행하면 끝난 뒤 여기에 남습니다.</p>
          ) : (
            <ul>
              {items.map((item) => (
                <li key={item.id} className="flex items-center gap-2 border-b border-line/50 px-2 hover:bg-raised">
                  <button onClick={() => open(item)} className="flex min-w-0 flex-1 items-center gap-3 px-2 py-2.5 text-left text-sm">
                    <span title={item.meta.light}>{LIGHT[item.meta.light] ?? "⚪"}</span>
                    <span className="w-36 shrink-0 font-mono text-xs text-dim">{whenText(item.time)}</span>
                    <span className="w-40 shrink-0 truncate">{item.meta.title}</span>
                    <span className="min-w-0 flex-1 truncate font-mono text-xs text-accent">{item.meta.target}</span>
                    <span className="shrink-0 text-xs text-dim">{item.meta.steps}단계</span>
                  </button>
                  <button onClick={() => remove(item.id)} title="이 기록을 지웁니다" className="shrink-0 rounded border border-line px-2 py-0.5 text-xs text-dim hover:text-ink">
                    지우기
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}
