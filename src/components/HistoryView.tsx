// 최근 진단 기록 목록. 한 건을 고르면 그때의 계획과 결과를 본 화면에 다시 연다.
// 기록은 이 PC에만 있다. 결과 목록이므로 말씀 표시를 넣지 않는다.
import { useEffect, useRef, useState } from "react";
import { captureFolder } from "../lib/capture";
import { historyDelete, historyExport, historyImport, historyList, historyLoad, type HistoryEntry, type HistoryItem } from "../lib/history";

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
  const [mask, setMask] = useState(false);
  const [saved, setSaved] = useState<string | null>(null);

  const [info, setInfo] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  /** 사용자가 고른 보고서 파일(다른 PC의 NetX가 내보낸 것)을 읽어 기록에 더한다. */
  async function importFile(file: File | undefined) {
    setError(null);
    setInfo(null);
    if (!file) return;
    if (file.size > 64 * 1024 * 1024) {
      setError("파일이 너무 큽니다 (64MB까지).");
      return;
    }
    try {
      const added = await historyImport(await file.text());
      setInfo(`${added}건을 가져왔습니다. 목록에 "가져옴"으로 표시됩니다.`);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  function exportTo(id?: string) {
    setError(null);
    historyExport(id, mask)
      .then(setSaved)
      .catch((e) => setError(String(e)));
  }

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
        <div className="flex flex-wrap items-center gap-3 border-b border-line px-4 py-2 text-xs">
          {items && items.length > 0 && (
            <>
              <button onClick={() => exportTo()} className="rounded border border-accent/60 px-2.5 py-1 text-accent hover:bg-raised">
                모두 내보내기
              </button>
              <label className="flex items-center gap-1.5" title="192.168.x.x 같은 내부 주소를 [내부IP-번호]로 바꿔서 내보냅니다. 밖으로 보낼 때 켜세요.">
                <input type="checkbox" checked={mask} onChange={(e) => setMask(e.target.checked)} />
                내부 주소 가리기
              </label>
            </>
          )}
          <button
            onClick={() => fileRef.current?.click()}
            title="다른 PC의 NetX가 내보낸 보고서 파일(netx-report-….html)을 골라 이 목록에 더합니다."
            className="rounded border border-line px-2.5 py-1 hover:bg-raised"
          >
            가져오기
          </button>
          <input
            ref={fileRef}
            type="file"
            accept=".html,.htm,text/html"
            className="hidden"
            onChange={(e) => {
              void importFile(e.target.files?.[0]);
              e.target.value = "";
            }}
          />
          <span className="min-w-0 flex-1 text-dim">내보내면 브라우저로 열리는 보고서 파일 하나가 됩니다(그림 포함). 받은 사람은 그냥 열어 보거나 NetX에서 [가져오기]로 불러올 수 있습니다.</span>
        </div>
        {info && <p className="border-b border-safe/40 bg-safe/10 px-4 py-2 text-xs">{info}</p>}
        {saved && (
          <p className="flex items-center gap-3 border-b border-safe/40 bg-safe/10 px-4 py-2 text-xs">
            <span className="min-w-0 flex-1 break-all">
              저장했습니다: <span className="font-mono">{saved}</span>
            </span>
            <button onClick={() => void captureFolder().catch(() => {})} className="shrink-0 rounded border border-line px-2.5 py-1 hover:bg-raised">
              폴더 열기
            </button>
          </p>
        )}
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
                    <span className="w-40 shrink-0 truncate">
                      {item.meta.title}
                      {item.meta.imported && <span className="ml-1.5 rounded border border-line px-1 text-[11px] text-dim">가져옴</span>}
                    </span>
                    <span className="min-w-0 flex-1 truncate font-mono text-xs text-accent">{item.meta.target}</span>
                    <span className="shrink-0 text-xs text-dim">{item.meta.steps}단계</span>
                  </button>
                  <button onClick={() => exportTo(item.id)} title="이 기록만 보고서 파일로 내보냅니다" className="shrink-0 rounded border border-line px-2 py-0.5 text-xs hover:bg-raised">
                    내보내기
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
