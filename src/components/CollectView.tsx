// 거점 관제 화면: 받은 결과(목록·필터), 이 PC의 결과 보내기, 이 PC를 관제 PC로 쓰기.
// 받은 글자(대상, 제목, 해석 문장)는 다른 PC가 보낸 자료다. 글자로만 그리고 링크나 서식으로 만들지 않는다.
import { useCallback, useEffect, useState } from "react";
import {
  CONN_LABEL,
  NOT_SENT,
  SENT_FIELDS,
  STEP_STATE_LABEL,
  VERDICT_LABEL,
  collectAnalysis,
  collectBoard,
  collectForget,
  collectItem,
  collectJoin,
  collectJoinCancel,
  collectList,
  collectOverview,
  collectPairClose,
  collectPairDecide,
  collectPairOpen,
  collectPcSet,
  collectSent,
  collectSet,
  dayBound,
  timeText,
  trend,
  type Analysis,
  type Board,
  type Finished,
  type Overview,
  type Pc,
  type Row,
  type SentRow,
  type Verdict,
} from "../lib/collect";
import { SiteDetail } from "./SiteDetail";

type Tab = "inbox" | "analysis" | "send" | "receive";

const TONE: Record<Verdict, string> = { ok: "text-safe", caution: "text-caution", danger: "text-danger", unknown: "text-dim" };
const BTN = "rounded border border-line px-2.5 py-1 text-xs hover:bg-raised disabled:opacity-50";
const BTN_MAIN = "rounded border border-accent/60 px-2.5 py-1 text-xs text-accent hover:bg-raised disabled:opacity-50";
const FIELD = "rounded border border-line bg-void px-2 py-1 text-xs";

const SENT_STATE: Record<SentRow["state"], string> = { queued: "보낼 함에서 기다림", sent: "보냄", dropped: "버림(보내지 않음)", local: "이 PC의 거점 관제에만" };

/** 글자를 클립보드에 넣는 단추. 넣었으면 잠깐 "복사됨"으로 바뀐다. */
function CopyButton({ text, label }: { text: string; label: string }) {
  const [state, setState] = useState<"idle" | "done" | "failed">("idle");
  function copy() {
    const settle = (next: "done" | "failed") => {
      setState(next);
      setTimeout(() => setState("idle"), 1500);
    };
    const writer = navigator.clipboard?.writeText(text);
    if (!writer) return settle("failed");
    void writer.then(() => settle("done")).catch(() => settle("failed"));
  }
  return (
    <button type="button" onClick={copy} className={BTN}>
      {state === "done" ? "복사됨" : state === "failed" ? "복사 못 함" : label}
    </button>
  );
}

function Badge({ children, tone = "text-dim" }: { children: React.ReactNode; tone?: string }) {
  return <span className={`ml-1 rounded border border-line px-1 text-[11px] ${tone}`}>{children}</span>;
}

function Detail({ row, onClose }: { row: Row; onClose: () => void }) {
  const [item, setItem] = useState<Finished | null | undefined>(undefined);
  useEffect(() => {
    void collectItem(row.pc, row.seq)
      .then(setItem)
      .catch(() => setItem(null));
  }, [row.pc, row.seq]);
  return (
    <div className="border-t border-line bg-void px-4 py-3 text-xs">
      <div className="mb-2 flex items-center gap-2">
        <span className="font-semibold">{row.title || row.kind}</span>
        <span className={TONE[row.verdict]}>{VERDICT_LABEL[row.verdict]}</span>
        <span className="text-dim">
          보낸 PC: {row.pc_name || "이름 없음"} · 순번 {row.seq} · 진단 {timeText(row.at)} · 받음 {timeText(row.got)}
        </span>
        <span className="flex-1" />
        <button onClick={onClose} className={BTN}>
          접기
        </button>
      </div>
      {item === undefined && <p className="text-dim">읽는 중…</p>}
      {item === null && <p className="text-dim">내용을 읽지 못했습니다.</p>}
      {item && (
        <>
          {item.summary.length > 0 && (
            <ul className="mb-2 list-disc pl-5">
              {item.summary.map((s, i) => (
                <li key={i}>{s}</li>
              ))}
            </ul>
          )}
          <ol className="space-y-1.5">
            {item.steps.map((s) => (
              <li key={s.order} className="rounded border border-line px-2 py-1.5">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-semibold">
                    {s.order}. {s.title}
                  </span>
                  <span className={TONE[s.verdict]}>{VERDICT_LABEL[s.verdict]}</span>
                  <span className="text-dim">
                    {STEP_STATE_LABEL[s.state] ?? s.state}
                    {s.exit !== null && ` · 종료 코드 ${s.exit}`}
                    {s.secs !== null && ` · ${s.secs}초`}
                  </span>
                </div>
                {s.command && <div className="mt-1 break-all font-mono text-dim">{s.command}</div>}
                {s.notes.map((n, i) => (
                  <div key={i} className="mt-0.5 break-words">
                    {n}
                  </div>
                ))}
              </li>
            ))}
          </ol>
          <p className="mt-2 text-dim">원본 출력은 보낸 PC에만 있습니다(관제 PC로 오지 않습니다).</p>
        </>
      )}
    </div>
  );
}

function Inbox({ board, onError, reload }: { board: Board; onError: (m: string) => void; reload: () => void }) {
  const [pc, setPc] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [target, setTarget] = useState("");
  const [kind, setKind] = useState("");
  const [verdict, setVerdict] = useState("");
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<{ rows: Row[]; total: number; page: number }>({ rows: [], total: 0, page: 200 });
  const [open, setOpen] = useState<string | null>(null);
  const [site, setSite] = useState<string | null>(null);

  const load = useCallback(() => {
    void collectList({ pc, from: dayBound(from, false), to: dayBound(to, true), target, kind, verdict, offset })
      .then(setPage)
      .catch((e) => onError(String(e)));
  }, [pc, from, to, target, kind, verdict, offset, onError]);
  useEffect(load, [load]);
  useEffect(() => {
    const t = setInterval(load, 15_000);
    return () => clearInterval(t);
  }, [load]);

  const name = (id: string) => board.pcs.find((p) => p.id === id)?.name || id.slice(0, 8);
  const pcState = (p: Pc, state: Pc["state"]) =>
    void collectPcSet(p.id, state)
      .then(reload)
      .catch((e) => onError(String(e)));
  const reset = () => setOffset(0);

  return (
    <div className="space-y-3">
      <section>
        <h2 className="mb-1 text-xs font-semibold text-dim">거점</h2>
        {board.pcs.length === 0 && <p className="text-xs text-dim">아직 짝 맺은 거점이 없습니다. "이 PC를 관제 PC로" 탭에서 [새 PC 짝 맺기]를 누르세요.</p>}
        <ul className="space-y-1">
          {board.pcs.map((p) => (
            <li key={p.id} className="flex flex-wrap items-center gap-2 rounded border border-line px-2 py-1.5 text-xs">
              <span className="font-semibold">{p.name || "이름 없음"}</span>
              {p.own ? <Badge>이 PC</Badge> : <span className="font-mono text-dim">{p.addr}</span>}
              {p.state === "stopped" && <Badge tone="text-danger">멈춤 — 다시 켜야 받습니다</Badge>}
              {p.state === "removed" && <Badge>짝 끊음</Badge>}
              {p.quiet && (
                <span title="일정 시간 보고가 없습니다. PC가 꺼져 있거나 NetX를 닫은 것일 수 있습니다(이상으로 단정하지 않습니다).">
                  <Badge tone="text-caution">미확인</Badge>
                </span>
              )}
              <span className="text-dim">
                {p.items}건 · 마지막 순번 {p.last_seq}
                {!p.own && ` · 마지막 보고 ${timeText(p.seen)}`}
              </span>
              <span className="flex-1" />
              <span className="font-mono text-[11px] text-dim" title={`공개 키 지문: ${p.id}`}>
                {p.id.slice(0, 12)}…
              </span>
              <button onClick={() => setSite(p.id)} className={BTN_MAIN} title="이 거점의 접속 내역, 전송 내역, 최근 7일을 봅니다.">
                상세
              </button>
              {!p.own && p.state === "active" && (
                <button onClick={() => pcState(p, "stopped")} className={BTN}>
                  받기 멈춤
                </button>
              )}
              {!p.own && p.state !== "active" && (
                <button onClick={() => pcState(p, "active")} className={BTN_MAIN}>
                  다시 받기
                </button>
              )}
              {!p.own && p.state !== "removed" && (
                <button onClick={() => pcState(p, "removed")} className={BTN} title="이 PC가 보내는 것을 더 받지 않습니다. 이미 받은 결과는 보관 기한까지 남습니다.">
                  짝 끊기
                </button>
              )}
            </li>
          ))}
        </ul>
        {site && <SiteDetail id={site} onClose={() => setSite(null)} />}
      </section>

      {board.strangers.length > 0 && (
        <details className="rounded border border-line px-2 py-1.5 text-xs">
          <summary className="cursor-pointer">짝이 아닌 곳에서 온 접속 {board.strangers.length}건 (모두 받지 않았습니다)</summary>
          <ul className="mt-1.5 space-y-0.5">
            {board.strangers.map((c, i) => (
              <li key={i}>
                <span className="text-dim">{timeText(c.at)}</span> <span className="font-mono">{c.addr}</span> · {CONN_LABEL[c.outcome] ?? "알 수 없음"}
                {c.pc && <span className="font-mono text-dim"> · {c.pc.slice(0, 12)}…</span>}
              </li>
            ))}
          </ul>
        </details>
      )}

      {(board.alerts.length > 0 || board.gaps.length > 0 || board.downtime.length > 0) && (
        <details className="rounded border border-line px-2 py-1.5 text-xs">
          <summary className="cursor-pointer">
            경고 {board.alerts.length}건 · 빠진 구간 {board.gaps.length}건 · 받지 않던 구간 {board.downtime.length}건
          </summary>
          <ul className="mt-1.5 space-y-0.5">
            {board.alerts.map((a, i) => (
              <li key={`a${i}`} className="text-caution">
                {timeText(a.at)} · {name(a.pc)} · {a.note}
              </li>
            ))}
            {board.gaps.map((g, i) => (
              <li key={`g${i}`}>
                {name(g.pc)} · 순번 {g.first}
                {g.last > g.first && `~${g.last}`} 빠짐 {g.dropped ? "(보낸 PC의 보낼 함이 넘치거나 기한이 지나 버린 것)" : "(오지 않음)"}
              </li>
            ))}
            {board.downtime.map(([a, b], i) => (
              <li key={`d${i}`} className="text-dim">
                받지 않던 구간: {timeText(a)} ~ {timeText(b)} (관제 PC가 꺼져 있었습니다)
              </li>
            ))}
          </ul>
        </details>
      )}

      <section>
        <div className="mb-1.5 flex flex-wrap items-center gap-2 text-xs">
          <select value={pc} onChange={(e) => (setPc(e.target.value), reset())} className={FIELD}>
            <option value="">모든 PC</option>
            {board.pcs.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name || p.id.slice(0, 8)}
              </option>
            ))}
          </select>
          <input type="date" value={from} max={to || undefined} onChange={(e) => (setFrom(e.target.value), reset())} className={FIELD} />
          ~
          <input type="date" value={to} min={from || undefined} onChange={(e) => (setTo(e.target.value), reset())} className={FIELD} />
          <input value={target} onChange={(e) => (setTarget(e.target.value), reset())} placeholder="대상(주소·이름)" maxLength={120} className={`${FIELD} w-44`} />
          <select value={kind} onChange={(e) => (setKind(e.target.value), reset())} className={FIELD}>
            <option value="">모든 진단</option>
            {board.kinds.map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </select>
          <select value={verdict} onChange={(e) => (setVerdict(e.target.value), reset())} className={FIELD}>
            <option value="">모든 판정</option>
            {(Object.keys(VERDICT_LABEL) as Verdict[]).map((v) => (
              <option key={v} value={v}>
                {VERDICT_LABEL[v]}
              </option>
            ))}
          </select>
          <span className="flex-1" />
          <span className="text-dim">{page.total}건</span>
        </div>
        {page.rows.length === 0 ? (
          <p className="rounded border border-line px-3 py-6 text-center text-xs text-dim">조건에 맞는 결과가 없습니다.</p>
        ) : (
          <ul className="rounded border border-line">
            {page.rows.map((r) => {
              const key = `${r.pc}:${r.seq}`;
              return (
                <li key={key} className="border-b border-line last:border-b-0">
                  <button onClick={() => setOpen(open === key ? null : key)} className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-xs hover:bg-raised">
                    <span className="w-28 shrink-0 text-dim">{timeText(r.at)}</span>
                    <span className="w-24 shrink-0 truncate" title={r.pc_name}>
                      {r.pc_name || r.pc.slice(0, 8)}
                    </span>
                    <span className={`w-14 shrink-0 ${TONE[r.verdict]}`}>{VERDICT_LABEL[r.verdict]}</span>
                    <span className="w-40 shrink-0 truncate">{r.title || r.kind}</span>
                    <span className="min-w-0 flex-1 truncate font-mono">{r.target}</span>
                    {r.masked && (
                      <span title="보낸 PC가 내부 주소·PC 이름을 가려서 보냈습니다. '내부-…'는 그 PC 안에서만 같은 주소를 가리키는 가명입니다.">
                        <Badge>가림</Badge>
                      </span>
                    )}
                    {r.skew && (
                      <span title="보낸 PC의 시계가 5분 넘게 앞서 있어, 진단 시각을 받은 시각으로 바꿔 적었습니다.">
                        <Badge tone="text-caution">시계 어긋남</Badge>
                      </span>
                    )}
                    {r.stale && (
                      <span title="보낼 함의 보관 기한보다 오래된 진단 시각입니다.">
                        <Badge tone="text-caution">오래된 시각</Badge>
                      </span>
                    )}
                  </button>
                  {open === key && <Detail row={r} onClose={() => setOpen(null)} />}
                </li>
              );
            })}
          </ul>
        )}
        {page.total > page.page && (
          <div className="mt-1.5 flex items-center gap-2 text-xs">
            <button disabled={offset === 0} onClick={() => setOffset(Math.max(0, offset - page.page))} className={BTN}>
              이전
            </button>
            <span className="text-dim">
              {offset + 1}~{Math.min(offset + page.page, page.total)}
            </span>
            <button disabled={offset + page.page >= page.total} onClick={() => setOffset(offset + page.page)} className={BTN}>
              다음
            </button>
          </div>
        )}
      </section>
    </div>
  );
}

function AnalysisTab({ onError }: { onError: (m: string) => void }) {
  const [a, setA] = useState<Analysis | null>(null);
  const [only, setOnly] = useState<"all" | "multi" | "differs" | "single">("all");
  useEffect(() => {
    const load = () =>
      void collectAnalysis()
        .then(setA)
        .catch((e) => onError(String(e)));
    load();
    const t = setInterval(load, 30_000);
    return () => clearInterval(t);
  }, [onError]);
  if (!a) return <p className="text-sm text-dim">읽는 중…</p>;
  if (!a.made) return <p className="rounded border border-line px-3 py-8 text-center text-sm text-dim">요약을 아직 만들지 않았습니다. 관제 PC로 쓰는 동안 5분마다 만듭니다.</p>;
  const { days, byHour } = trend(a.hours);
  const top = Math.max(1, ...days.map((d) => d.ok + d.caution + d.danger + d.unknown));
  const topHour = Math.max(1, ...byHour);
  const targets = a.targets.filter((t) => (only === "multi" ? !t.single : only === "differs" ? t.differs : only === "single" ? t.single : true));
  return (
    <div className="space-y-4 text-xs">
      <p className="text-dim">
        요약을 만든 시각: {timeText(a.made)} (5분마다 다시 만듭니다). 모인 결과는 보낸 PC가 알려 온 것입니다. 한 PC에서만 나온 것은 다른 지점이 확인해 준 적이 없는 것이니 따로 표시합니다.
      </p>
      <section>
        <h2 className="mb-1 font-semibold text-dim">날짜별 추이 (최근 30일)</h2>
        {days.length === 0 ? (
          <p className="text-dim">최근 30일에 모인 결과가 없습니다.</p>
        ) : (
          <div className="flex h-28 items-end gap-0.5 rounded border border-line px-2 pt-2">
            {days.map((d) => {
              const part = (n: number) => `${(n / top) * 100}%`;
              return (
                <div key={d.day} className="flex h-full min-w-0 flex-1 flex-col justify-end" title={`${d.day} — 정상 ${d.ok}, 주의 ${d.caution}, 위험 ${d.danger}, 판정 없음 ${d.unknown}`}>
                  <div className="bg-danger" style={{ height: part(d.danger) }} />
                  <div className="bg-caution" style={{ height: part(d.caution) }} />
                  <div className="bg-safe" style={{ height: part(d.ok) }} />
                  <div className="bg-line" style={{ height: part(d.unknown) }} />
                </div>
              );
            })}
          </div>
        )}
        {days.length > 0 && (
          <p className="mt-0.5 flex justify-between text-dim">
            <span>{days[0].day}</span>
            <span>
              <span className="text-safe">■</span> 정상 <span className="text-caution">■</span> 주의 <span className="text-danger">■</span> 위험
            </span>
            <span>{days[days.length - 1].day}</span>
          </p>
        )}
      </section>
      <section>
        <h2 className="mb-1 font-semibold text-dim">주의·위험이 나온 시각대 (0~23시, 이 PC의 시각)</h2>
        <div className="flex h-14 items-end gap-0.5 rounded border border-line px-2 pt-2">
          {byHour.map((n, h) => (
            <div key={h} className="flex h-full flex-1 flex-col justify-end" title={`${h}시 — ${n}건`}>
              <div className="bg-caution" style={{ height: `${(n / topHour) * 100}%` }} />
            </div>
          ))}
        </div>
      </section>
      <section>
        <div className="mb-1 flex items-center gap-2">
          <h2 className="font-semibold text-dim">대상별 — 지점마다 어떻게 보였나</h2>
          <span className="flex-1" />
          <select value={only} onChange={(e) => setOnly(e.target.value as typeof only)} className={FIELD}>
            <option value="all">모두</option>
            <option value="multi">여러 PC가 본 대상</option>
            <option value="differs">PC마다 판정이 다른 대상</option>
            <option value="single">한 PC에서만 나온 대상</option>
          </select>
        </div>
        {targets.length === 0 ? (
          <p className="rounded border border-line px-3 py-6 text-center text-dim">해당하는 대상이 없습니다.</p>
        ) : (
          <ul className="space-y-1">
            {targets.map((t) => (
              <li key={`${t.target}|${t.seen[0]?.pc}`} className="rounded border border-line px-2 py-1.5">
                <div className="flex flex-wrap items-center gap-1">
                  <span className="break-all font-mono">{t.target}</span>
                  <span className="text-dim">· 모두 {t.total}건</span>
                  {t.alias && (
                    <span title="보낸 PC가 가린 내부 주소입니다. 그 PC 안에서만 같은 대상을 가리킵니다.">
                      <Badge>가명</Badge>
                    </span>
                  )}
                  {t.single ? <Badge>한 PC에서만</Badge> : <Badge tone="text-accent">{t.seen.length}대의 PC</Badge>}
                  {t.differs && <Badge tone="text-caution">PC마다 판정이 다름</Badge>}
                </div>
                <ul className="mt-1 space-y-0.5">
                  {t.seen.map((s) => (
                    <li key={s.pc} className="flex flex-wrap gap-2">
                      <span className="w-28 shrink-0 truncate">{s.pc_name || s.pc.slice(0, 8)}</span>
                      <span className={`w-16 shrink-0 ${TONE[s.last_verdict] ?? "text-dim"}`}>{VERDICT_LABEL[s.last_verdict] ?? "판정 없음"}</span>
                      <span className="text-dim">
                        {s.n}건 (정상 {s.ok}, 주의 {s.caution}, 위험 {s.danger}) · 마지막 {timeText(s.last_at)}
                      </span>
                    </li>
                  ))}
                </ul>
              </li>
            ))}
          </ul>
        )}
        {a.targets.length >= 100 && <p className="mt-1 text-dim">여러 PC가 본 대상, 건수가 많은 대상 순으로 100개까지 보입니다.</p>}
      </section>
    </div>
  );
}

function Send({ ov, onError, reload }: { ov: Overview; onError: (m: string) => void; reload: () => void }) {
  const c = ov.config;
  const [addr, setAddr] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [agree, setAgree] = useState(false);
  const [sent, setSent] = useState<SentRow[]>([]);
  useEffect(() => {
    void collectSent()
      .then(setSent)
      .catch(() => {});
  }, [ov.waiting, ov.status.last_sent]);

  const save = (patch: Partial<typeof c>) =>
    void collectSet({ ...c, ...patch })
      .then(reload)
      .catch((e) => onError(String(e)));
  const join = () => {
    setBusy(true);
    // 붙여 넣은 코드에 빈칸이 끼었거나 소문자로 적었어도 받는다. 코드는 숫자와 대문자뿐이다.
    void collectJoin(addr.trim(), null, code.replace(/\s+/g, "").toUpperCase())
      .then(reload)
      .catch((e) => onError(String(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="space-y-4 text-sm">
      <section className="space-y-2">
        <h2 className="text-xs font-semibold text-dim">관제 PC와 짝 맺기</h2>
        {c.collector ? (
          <div className="flex flex-wrap items-center gap-2 rounded border border-line px-2 py-1.5 text-xs">
            <span className="font-semibold">{c.collector.name || "관제 PC"}</span>
            <span className="font-mono text-dim">
              {c.collector.addr}:{c.collector.port}
            </span>
            <span className="font-mono text-[11px] text-dim" title={`공개 키 지문: ${c.collector.id}`}>
              {c.collector.id.slice(0, 12)}…
            </span>
            <span className="flex-1" />
            <button
              onClick={() =>
                void collectForget()
                  .then(reload)
                  .catch((e) => onError(String(e)))
              }
              className={BTN}
              title="이 관제 PC로 더 보내지 않습니다. 보내기가 꺼지고 보낼 함에서 기다리던 것은 버립니다."
            >
              짝 끊기
            </button>
          </div>
        ) : ov.join.state === "waiting" ? (
          <div className="rounded border border-accent/60 px-3 py-2 text-xs">
            <p>
              코드가 맞았습니다. 관제 PC <b>{ov.join.name || ov.join.addr}</b> 화면에 아래와 <b>같은 확인 숫자</b>가 떠 있는지 보고, 같으면 관제 PC에서 [짝 맺기 승인]을 누르세요.
            </p>
            <p className="my-2 text-center font-mono text-2xl tracking-widest">{ov.join.number}</p>
            <p className="text-dim">숫자가 다르면 중간에 다른 기기가 끼어 있는 것입니다. 승인하지 말고 아래 단추로 그만두세요.</p>
            <button
              onClick={() =>
                void collectJoinCancel()
                  .then(reload)
                  .catch((e) => onError(String(e)))
              }
              className={`${BTN} mt-2`}
            >
              그만두기
            </button>
          </div>
        ) : (
          <div className="space-y-1.5 text-xs">
            <p className="text-dim">관제 PC PC의 NetX에서 [새 PC 짝 맺기]를 누르면 그 화면에 주소와 일회용 코드가 뜹니다(2분 동안). 그것을 여기에 적습니다.</p>
            <div className="flex flex-wrap items-center gap-2">
              <input value={addr} onChange={(e) => setAddr(e.target.value)} placeholder="관제 PC의 테일스케일 주소 (100.x.y.z)" maxLength={15} className={`${FIELD} w-64 font-mono`} />
              <input value={code} onChange={(e) => setCode(e.target.value)} placeholder="일회용 코드" maxLength={24} autoComplete="off" spellCheck={false} className={`${FIELD} w-40 font-mono uppercase`} />
              <button disabled={busy || !addr.trim() || code.replace(/\s+/g, "").length < 8} onClick={join} className={BTN_MAIN}>
                {busy ? "확인 중…" : "짝 맺기"}
              </button>
            </div>
            {ov.join.state === "failed" && <p className="text-danger">{ov.join.why}</p>}
            {ov.join.state === "done" && <p className="text-safe">짝을 맺었습니다.</p>}
          </div>
        )}
      </section>

      <section className="space-y-2">
        <h2 className="text-xs font-semibold text-dim">이 PC의 진단 결과 보내기</h2>
        {!c.send ? (
          <div className="rounded border border-line px-3 py-2 text-xs">
            <p className="mb-1">
              켜면 이 PC에서 끝난 진단마다 아래 항목이{" "}
              <b>{c.collector ? `${c.collector.name || "관제 PC"} (${c.collector.addr})` : "짝 맺은 관제 PC"}</b>(으)로 나갑니다. 지금은 <b>꺼져 있습니다.</b>
            </p>
            <ul className="list-disc pl-5">
              {SENT_FIELDS.map((f) => (
                <li key={f}>{f}</li>
              ))}
            </ul>
            <p className="mt-1 text-dim">{NOT_SENT}</p>
            <p className="mt-1 text-dim">스토리방화벽이 넘긴 의뢰를 진단한 결과도 같은 규칙으로 나갑니다.</p>
            <label className="mt-2 flex items-center gap-2">
              <input type="checkbox" checked={agree} disabled={!c.collector} onChange={(e) => setAgree(e.target.checked)} />위 항목이 이 PC 밖으로 나가는 것을 확인했습니다
            </label>
            <button disabled={!c.collector || !agree} onClick={() => (setAgree(false), save({ send: true }))} className={`${BTN_MAIN} mt-2`}>
              보내기 켜기
            </button>
            {!c.collector && <span className="ml-2 text-dim">먼저 관제 PC와 짝을 맺어야 합니다.</span>}
          </div>
        ) : (
          <div className="rounded border border-safe/40 px-3 py-2 text-xs">
            <p>
              <b>보내는 중</b> → {c.collector?.name || "관제 PC"} ({c.collector?.addr}) ·{" "}
              {ov.status.linked ? <span className="text-safe">관제 PC에 닿아 있음</span> : <span className="text-caution">닿지 않음 — 쌓아 두었다가 보냅니다</span>} · 기다리는 것 {ov.waiting}건
            </p>
            {ov.status.send_error && <p className="mt-1 text-caution">{ov.status.send_error}</p>}
            <p className="mt-1 text-dim">
              보낼 함은 {ov.limits.out_queue}건, {ov.limits.out_keep_days}일까지 쌓습니다. 넘으면 오래된 것부터 버리고, 버렸다는 사실을 관제 PC에 알립니다.
            </p>
            <button onClick={() => save({ send: false })} className={`${BTN} mt-2`} title="끄면 보낼 함에서 기다리던 것도 나가지 않습니다.">
              보내기 끄기
            </button>
          </div>
        )}
        <label className="flex items-start gap-2 text-xs">
          <input type="checkbox" className="mt-0.5" checked={c.mask} onChange={(e) => save({ mask: e.target.checked })} />
          <span>
            내부 주소·PC 이름 가리기 (기본 켬)
            <span className="block text-dim">
              내부 주소는 "내부-ab12cd" 같은 가명으로 바뀝니다. 같은 주소는 이 PC에서 언제나 같은 가명이 되므로 관제 PC에서 이 PC 안의 같은 대상끼리는 묶입니다. 다만 다른 PC가 본 같은 내부 주소와는 묶이지 않고, 관제 PC에서 원래 주소를 알 수 없습니다.
            </span>
          </span>
        </label>
      </section>

      <section>
        <h2 className="mb-1 text-xs font-semibold text-dim">나간 건의 목록 (최근 200건)</h2>
        {sent.length === 0 ? (
          <p className="text-xs text-dim">아직 없습니다.</p>
        ) : (
          <ul className="max-h-56 overflow-y-auto rounded border border-line text-xs">
            {sent.map((s) => (
              <li key={s.seq} className="flex items-center gap-2 border-b border-line px-2 py-1 last:border-b-0">
                <span className="w-10 shrink-0 text-dim">#{s.seq}</span>
                <span className="w-28 shrink-0 text-dim">{timeText(s.at)}</span>
                <span className="w-36 shrink-0 truncate">{s.title || s.kind}</span>
                <span className="min-w-0 flex-1 truncate font-mono">{s.target}</span>
                <span className={s.state === "sent" ? "text-safe" : s.state === "dropped" ? "text-caution" : "text-dim"}>
                  {SENT_STATE[s.state]}
                  {s.state === "sent" && ` ${timeText(s.sent)}`}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}

function Receive({ ov, onError, reload }: { ov: Overview; onError: (m: string) => void; reload: () => void }) {
  const c = ov.config;
  const [name, setName] = useState(c.name);
  const [days, setDays] = useState(String(c.keep_days));
  const w = ov.window;
  const act = (p: Promise<unknown>) => void p.then(reload).catch((e) => onError(String(e)));
  const save = (patch: Partial<typeof c>) => act(collectSet({ ...c, ...patch }));

  return (
    <div className="space-y-4 text-sm">
      <section className="space-y-2 text-xs">
        <h2 className="font-semibold text-dim">이 PC의 이름</h2>
        <div className="flex items-center gap-2">
          <input value={name} onChange={(e) => setName(e.target.value)} maxLength={40} className={`${FIELD} w-56`} />
          <button disabled={!name.trim() || name === c.name} onClick={() => save({ name })} className={BTN}>
            저장
          </button>
          <span className="text-dim">짝 맺을 때 상대 화면에 보이는 이름입니다.</span>
        </div>
      </section>

      <section className="space-y-2 text-xs">
        <h2 className="font-semibold text-dim">이 PC를 관제 PC로 쓰기</h2>
        <label className="flex items-start gap-2">
          <input type="checkbox" className="mt-0.5" checked={c.receive} onChange={(e) => save({ receive: e.target.checked })} />
          <span>
            다른 PC의 NetX가 보내는 진단 결과를 이 PC에 모읍니다 (기본 꺼짐)
            <span className="block text-dim">
              켜면 이 PC의 테일스케일 주소에서만 듣습니다(포트 {c.port}). 테일스케일 주소가 없으면 듣지 않습니다. 관제 PC는 받기만 하며, 보낸 PC로 명령이나 요청을 보내는 길이 없습니다. NetX를 닫으면 받지 못하고, 그동안은 "받지 않던 구간"으로 표시됩니다. 이 PC에서 한 진단도 관제 PC에 함께 들어갑니다.
            </span>
          </span>
        </label>
        {c.receive && (
          <p className={ov.status.listening ? "text-safe" : "text-caution"}>
            {ov.status.listening ? `듣는 중: ${ov.status.listening}` : (ov.status.listen_error ?? "듣기를 준비하는 중…")}
            {ov.status.listening && <span className="ml-2 text-dim">Windows 방화벽이 허용할지 물을 수 있습니다. NetX가 듣는 주소는 이 테일스케일 주소 하나뿐입니다.</span>}
          </p>
        )}
        <div className="flex items-center gap-2">
          보관 기한
          <input value={days} onChange={(e) => setDays(e.target.value.replace(/\D/g, "").slice(0, 3))} className={`${FIELD} w-16`} />일
          <button disabled={!Number(days) || Number(days) === c.keep_days || Number(days) > ov.limits.keep_days_max} onClick={() => save({ keep_days: Number(days) })} className={BTN}>
            저장
          </button>
          <span className="text-dim">받은 날부터 셉니다. 지나면 지웁니다. (1~{ov.limits.keep_days_max}일)</span>
        </div>
        <p className="text-dim">
          한 PC가 한 시간에 {ov.limits.hour_items}건, 모두 {ov.limits.pc_items.toLocaleString()}건까지 넣을 수 있습니다. 넘으면 연결을 끊고 경고하며, 하루에 세 번 넘으면 그 PC의 짝을 멈춥니다.
        </p>
      </section>

      {c.receive && (
        <section className="space-y-2 text-xs">
          <h2 className="font-semibold text-dim">새 PC 짝 맺기</h2>
          {!w.open ? (
            <button onClick={() => act(collectPairOpen())} className={BTN_MAIN}>
              새 PC 짝 맺기
            </button>
          ) : w.pending ? (
            <div className="rounded border border-accent/60 px-3 py-2">
              <p>
                <b>{w.pending.name || "이름 없는 PC"}</b> (<span className="font-mono">{w.pending.addr}</span>)가 맞는 코드를 넣었습니다. 그 PC의 화면에 아래와 <b>같은 확인 숫자</b>가 떠 있을 때만 승인하세요.
              </p>
              <p className="my-2 text-center font-mono text-2xl tracking-widest">{w.pending.number}</p>
              <div className="flex gap-2">
                <button onClick={() => act(collectPairDecide(true))} className={BTN_MAIN}>
                  짝 맺기 승인
                </button>
                <button onClick={() => act(collectPairDecide(false))} className={BTN}>
                  거절
                </button>
                <span className="text-dim">남은 시간 {w.secs_left}초</span>
              </div>
            </div>
          ) : (
            <div className="rounded border border-line px-3 py-2">
              <p>거점 PC의 NetX에서 "보내기" 탭에 아래 주소와 코드를 적으세요. 코드는 한 번만 쓰이고 {w.secs_left}초 뒤에 사라집니다.</p>
              <div className="my-2 flex flex-wrap items-center justify-center gap-x-6 gap-y-2">
                <span className="flex items-center gap-2">
                  <span className="select-all font-mono text-base">{ov.tailnet ?? "테일스케일 주소 없음"}</span>
                  {ov.tailnet && <CopyButton text={ov.tailnet} label="주소 복사" />}
                </span>
                <span className="flex items-center gap-2">
                  <span className="font-mono text-2xl tracking-widest">
                    {w.code.slice(0, 5)} {w.code.slice(5)}
                  </span>
                  <CopyButton text={w.code} label="코드 복사" />
                </span>
              </div>
              {w.wrong > 0 && <p className="text-caution">틀린 코드가 {w.wrong}번 들어왔습니다. 5번이 되면 이 코드를 버립니다.</p>}
              <button onClick={() => act(collectPairClose())} className={BTN}>
                닫기
              </button>
            </div>
          )}
        </section>
      )}
      <p className="font-mono text-[11px] text-dim">이 PC의 id(공개 키 지문): {ov.id}</p>
    </div>
  );
}

export function CollectView({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<Tab>("inbox");
  const [ov, setOv] = useState<Overview | null>(null);
  const [board, setBoard] = useState<Board>({ pcs: [], kinds: [], gaps: [], alerts: [], downtime: [], strangers: [] });
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    void collectOverview()
      .then((o) => (setOv(o), setError(null)))
      .catch((e) => setError(String(e)));
    void collectBoard()
      .then(setBoard)
      .catch(() => {});
  }, []);
  useEffect(reload, [reload]);
  // 짝 맺기 창이 열려 있거나 승인을 기다릴 때는 자주, 그 밖에는 가끔 다시 읽는다.
  const fast = ov?.window.open || ov?.join.state === "waiting";
  useEffect(() => {
    const t = setInterval(reload, fast ? 1000 : 5000);
    return () => clearInterval(t);
  }, [reload, fast]);

  const tabs: { id: Tab; label: string }[] = [
    { id: "inbox", label: "받은 결과" },
    { id: "analysis", label: "종합 분석" },
    { id: "send", label: `보내기${ov?.config.send ? " ●" : ""}` },
    { id: "receive", label: `이 PC를 관제 PC로${ov?.config.receive ? " ●" : ""}` },
  ];

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/60 p-6" onClick={onClose}>
      <div className="flex h-full max-h-full w-full max-w-5xl flex-col rounded-lg border border-line bg-panel" onClick={(e) => e.stopPropagation()}>
        <header className="flex items-center gap-3 border-b border-line px-4 py-3">
          <h1 className="text-base font-semibold">📡 거점 관제</h1>
          <span className="text-xs text-dim">테일스케일 안에서, 거점이 거는 연결로만 주고받습니다.</span>
          <span className="flex-1" />
          <button onClick={onClose} className="rounded border border-line px-3 py-1 text-xs hover:bg-raised">
            닫기
          </button>
        </header>
        <nav className="flex gap-1 border-b border-line px-3 pt-2 text-sm">
          {tabs.map((t) => (
            <button key={t.id} onClick={() => setTab(t.id)} className={`rounded-t px-3 py-1.5 ${tab === t.id ? "border border-b-0 border-line bg-raised" : "text-dim hover:bg-raised"}`}>
              {t.label}
            </button>
          ))}
        </nav>
        {error && <p className="border-b border-danger/40 bg-danger/10 px-4 py-2 text-sm text-danger">{error}</p>}
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
          {!ov ? (
            <p className="text-sm text-dim">읽는 중…</p>
          ) : tab === "inbox" ? (
            !ov.config.receive && board.pcs.length === 0 ? (
              <p className="rounded border border-line px-3 py-8 text-center text-sm text-dim">
                이 PC는 관제 PC로 쓰고 있지 않습니다. 다른 PC의 결과를 여기로 모으려면 "이 PC를 관제 PC로" 탭에서 켜세요.
              </p>
            ) : (
              <Inbox board={board} onError={setError} reload={reload} />
            )
          ) : tab === "analysis" ? (
            <AnalysisTab onError={setError} />
          ) : tab === "send" ? (
            <Send ov={ov} onError={setError} reload={reload} />
          ) : (
            <Receive ov={ov} onError={setError} reload={reload} />
          )}
        </div>
      </div>
    </div>
  );
}
