// 보고서 만들기: 구역, 기간, 가리기를 고르고 보고서 파일 하나를 만든다.
// 보고서는 사용자가 고른 자리에 파일로만 저장한다(보내는 단추가 없다). 내보내는 파일에는 말씀 표시를 넣지 않는다.
import { useState } from "react";
import { REPORT_ZONES, periodOf, reportCancel, reportMake, reportOpen, type PeriodKind, type ReportSaved } from "../lib/report";

const PERIODS: { id: PeriodKind; label: string }[] = [
  { id: "today", label: "오늘" },
  { id: "7d", label: "최근 7일" },
  { id: "30d", label: "최근 30일" },
  { id: "custom", label: "직접" },
];

function dayText(ms: number): string {
  const d = new Date(ms);
  const two = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
}

/** 저장한 뒤의 한 줄: 경로와 [열기] [폴더 열기] */
export function ReportSavedBar({ saved, onError }: { saved: ReportSaved; onError: (message: string) => void }) {
  const open = (folder: boolean) => void reportOpen(folder).catch((e) => onError(String(e)));
  return (
    <p className="flex items-center gap-3 border-b border-safe/40 bg-safe/10 px-4 py-2 text-xs">
      <span className="min-w-0 flex-1 break-all">
        저장했습니다: <span className="font-mono">{saved.path}</span>
      </span>
      <button onClick={() => open(false)} className="shrink-0 rounded border border-accent/60 px-2.5 py-1 text-accent hover:bg-raised">
        열기
      </button>
      <button onClick={() => open(true)} className="shrink-0 rounded border border-line px-2.5 py-1 hover:bg-raised">
        폴더 열기
      </button>
    </p>
  );
}

export function ReportView({ onClose }: { onClose: () => void }) {
  const [picked, setPicked] = useState<Record<string, boolean>>(() => Object.fromEntries(REPORT_ZONES.map((z) => [z.id, !z.later && !z.off])));
  const [kind, setKind] = useState<PeriodKind>("7d");
  const [from, setFrom] = useState(() => dayText(Date.now() - 7 * 86_400_000));
  const [to, setTo] = useState(() => dayText(Date.now()));
  const [mask, setMask] = useState(true);
  const [askPlain, setAskPlain] = useState(false);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<ReportSaved | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const sections = REPORT_ZONES.filter((z) => !z.later && picked[z.id]).map((z) => z.id);
  const period = periodOf(kind, from, to);
  const ready = sections.length > 0 && period !== null && !busy;

  async function make() {
    if (!period) return;
    setAskPlain(false);
    setError(null);
    setNote(null);
    setSaved(null);
    setBusy(true);
    try {
      const result = await reportMake({ sections, ...period, mask });
      if (result) setSaved(result);
      else setNote("저장하지 않았습니다. 만들던 보고서는 남기지 않았습니다.");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6" onClick={busy ? undefined : onClose}>
      <div className="flex max-h-full w-full max-w-2xl flex-col rounded-lg border border-line bg-panel" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center gap-3 border-b border-line px-4 py-3">
          <h1 className="text-base font-semibold">📄 보고서 만들기</h1>
          <span className="text-xs text-dim">브라우저로 열리는 파일 하나로 저장합니다. 어디로도 보내지 않습니다.</span>
          <span className="flex-1" />
          <button onClick={onClose} disabled={busy} className="rounded border border-line px-3 py-1 text-xs hover:bg-raised disabled:opacity-50">
            닫기
          </button>
        </header>
        {saved && <ReportSavedBar saved={saved} onError={setError} />}
        {note && <p className="border-b border-line bg-raised px-4 py-2 text-xs">{note}</p>}
        {error && <p className="border-b border-danger/40 bg-danger/10 px-4 py-2 text-sm text-danger">{error}</p>}
        <div className="space-y-4 overflow-y-auto px-4 py-3 text-sm">
          <fieldset disabled={busy}>
            <legend className="mb-1.5 text-xs font-semibold text-dim">넣을 구역</legend>
            <ul className="space-y-1">
              {REPORT_ZONES.map((z) => (
                <li key={z.id}>
                  <label className={`flex items-start gap-2 ${z.later ? "opacity-50" : ""}`}>
                    <input type="checkbox" className="mt-1" disabled={z.later} checked={!z.later && !!picked[z.id]} onChange={(e) => setPicked({ ...picked, [z.id]: e.target.checked })} />
                    <span>
                      {z.title}
                      {z.later && <span className="ml-1.5 rounded border border-line px-1 text-[11px] text-dim">다음 판</span>}
                      <span className="block text-xs text-dim">{z.hint}</span>
                    </span>
                  </label>
                </li>
              ))}
            </ul>
          </fieldset>
          <fieldset disabled={busy}>
            <legend className="mb-1.5 text-xs font-semibold text-dim">기간</legend>
            <div className="flex flex-wrap items-center gap-3">
              {PERIODS.map((p) => (
                <label key={p.id} className="flex items-center gap-1.5">
                  <input type="radio" name="report-period" checked={kind === p.id} onChange={() => setKind(p.id)} />
                  {p.label}
                </label>
              ))}
              {kind === "custom" && (
                <span className="flex items-center gap-1.5 text-xs">
                  <input type="date" value={from} max={to} onChange={(e) => setFrom(e.target.value)} className="rounded border border-line bg-void px-1.5 py-0.5" />
                  ~
                  <input type="date" value={to} min={from} onChange={(e) => setTo(e.target.value)} className="rounded border border-line bg-void px-1.5 py-0.5" />
                </span>
              )}
            </div>
            {kind === "custom" && period === null && <p className="mt-1 text-xs text-caution">시작 날짜가 끝 날짜보다 늦거나 날짜가 비어 있습니다.</p>}
            <p className="mt-1 text-xs text-dim">내 네트워크처럼 기간이 없는 구역은 만든 시각 기준으로 적습니다.</p>
          </fieldset>
          <fieldset disabled={busy}>
            <legend className="mb-1.5 text-xs font-semibold text-dim">가리기</legend>
            <label className="flex items-start gap-2">
              <input type="checkbox" className="mt-1" checked={mask} onChange={(e) => setMask(e.target.checked)} />
              <span>
                내부 주소, MAC 뒷부분, PC 이름, 사용자 이름, 이 PC의 공인 주소를 가립니다
                <span className="block text-xs text-dim">남에게 보여 줄 때를 위한 것입니다. 바깥 목적지 주소와 도메인은 가리지 않습니다. 암호와 커뮤니티 문자열은 어느 쪽이든 싣지 않습니다.</span>
              </span>
            </label>
          </fieldset>
        </div>
        <footer className="flex items-center gap-3 border-t border-line px-4 py-3 text-xs">
          {busy ? (
            <>
              <span className="flex-1 animate-pulse">보고서를 만들고 있습니다. 저장 창이 뜨면 저장할 자리를 고르세요…</span>
              <button onClick={() => void reportCancel()} className="rounded border border-line px-3 py-1.5 hover:bg-raised">
                취소
              </button>
            </>
          ) : askPlain ? (
            <>
              <span className="flex-1 text-caution">내부 주소, PC 이름이 그대로 들어갑니다.</span>
              <button onClick={() => void make()} className="rounded border border-caution/60 px-3 py-1.5 text-caution hover:bg-raised">
                그대로 만들기
              </button>
              <button onClick={() => setAskPlain(false)} className="rounded border border-line px-3 py-1.5 hover:bg-raised">
                그만두기
              </button>
            </>
          ) : (
            <>
              <span className="flex-1 text-dim">{sections.length === 0 ? "구역을 하나 이상 고르세요." : `구역 ${sections.length}개 · 구역마다 표는 2,000줄까지 싣습니다.`}</span>
              <button
                onClick={() => (mask ? void make() : setAskPlain(true))}
                disabled={!ready}
                className="rounded bg-accent px-4 py-1.5 text-sm font-semibold text-void hover:opacity-90 disabled:opacity-40"
              >
                만들기
              </button>
            </>
          )}
        </footer>
      </div>
    </div>
  );
}
