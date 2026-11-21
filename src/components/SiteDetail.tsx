// 거점 상세: 한 거점의 접속 내역, 전송 내역, 7일 시간표, 건강(빠진 구간과 경고).
// 여기 나오는 글자 가운데 제목과 종류는 거점이 보낸 자료다. 글자로만 그린다.
import { TONE, hidden } from "../lib/ui";
import { useDialog } from "../lib/dialog";
import { useCallback, useEffect, useState } from "react";
import { CONN_LABEL, VERDICT_LABEL, collectSite, sizeText, timeText, type Site, type Verdict } from "../lib/collect";

type Tab = "summary" | "conns" | "transfers" | "health";

const GOOD = new Set(["ok", "closed", "pair_ok"]);

function Stat({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className="rounded border border-line px-3 py-2">
      <div className="text-[11px] text-dim">{label}</div>
      <div className={`text-sm font-semibold ${tone ?? ""}`}>{value}</div>
    </div>
  );
}

/** 7일 시간표: 한 칸이 한 시간. 받은 건수만큼 진하고, 위험이 있으면 붉고, 접속 실패만 있으면 노랗다. */
function Timeline({ hours }: { hours: Site["timeline"] }) {
  const peak = Math.max(1, ...hours.map((h) => h.items));
  const days: Site["timeline"][] = [];
  for (let i = 0; i < hours.length; i += 24) days.push(hours.slice(i, i + 24));
  return (
    <div className="space-y-0.5">
      {days.map((day, d) => (
        <div key={d} className="flex items-center gap-1">
          <span className="w-12 shrink-0 text-[10px] text-dim">{new Date(day[0].at).toLocaleDateString("ko-KR", { month: "numeric", day: "numeric" })}</span>
          <div className="flex flex-1 gap-px">
            {day.map((h) => {
              // 빈 칸은 테두리만(바탕과 3:1 넘게), 채운 칸은 가장 옅어도 3:1을 넘긴다. 위험과 실패는 색 말고 표식으로도 알린다.
              const empty = h.items === 0 && h.failed === 0 && h.conns === 0;
              const tone = h.danger > 0 ? "text-danger" : h.items > 0 ? "text-safe" : h.failed > 0 ? "text-caution" : "text-dim";
              const opacity = h.danger > 0 ? 1 : h.items > 0 ? 0.6 + 0.4 * (h.items / peak) : 0.6;
              const mark = h.danger > 0 ? "!" : h.failed > 0 ? "×" : "";
              const label = `${timeText(h.at)} — 받은 결과 ${h.items}건${h.danger ? `(위험 ${h.danger})` : ""}, 접속 ${h.conns}번${h.failed ? `(실패 ${h.failed})` : ""}`;
              return empty ? (
                <span key={h.at} className="h-4 flex-1 rounded-sm border border-dim/60" title={label} />
              ) : (
                <span key={h.at} className={`flex h-4 flex-1 items-center justify-center rounded-sm bg-current ${tone}`} style={{ opacity }} title={label}>
                  {mark && <span className="text-[10px] leading-none font-bold text-void">{mark}</span>}
                </span>
              );
            })}
          </div>
        </div>
      ))}
      <p className="pt-1 text-[11px] text-dim">한 줄이 24시간, 한 칸이 한 시간이고 맨 아래 줄의 오른쪽 끝이 지금입니다. 초록은 결과를 받은 시간, 빨강(!)은 위험 판정이 있던 시간, 노랑(×)은 접속이 실패한 시간, 테두리만 있는 칸은 소식이 없던 시간입니다.</p>
    </div>
  );
}

export function SiteDetail({ id, onClose }: { id: string; onClose: () => void }) {
  const dialog = useDialog(onClose);
  const [site, setSite] = useState<Site | null>(null);
  const [error, setError] = useState("");
  const [tab, setTab] = useState<Tab>("summary");

  const load = useCallback(() => {
    if (hidden()) return;
    collectSite(id)
      .then((s) => {
        setSite(s);
        setError("");
      })
      .catch((e) => setError(String(e)));
  }, [id]);

  useEffect(() => {
    load();
    const t = window.setInterval(load, 10_000);
    return () => window.clearInterval(t);
  }, [load]);

  const tabs: { id: Tab; label: string }[] = [
    { id: "summary", label: "개요" },
    { id: "conns", label: `접속 내역${site ? ` ${site.conns.length}` : ""}` },
    { id: "transfers", label: `전송 내역${site ? ` ${site.transfers.length}` : ""}` },
    { id: "health", label: `건강${site && site.gaps.length + site.alerts.length > 0 ? ` ${site.gaps.length + site.alerts.length}` : ""}` },
  ];
  const pc = site?.pc;
  const week = site ? site.timeline.reduce((a, h) => ({ items: a.items + h.items, danger: a.danger + h.danger, conns: a.conns + h.conns, failed: a.failed + h.failed }), { items: 0, danger: 0, conns: 0, failed: 0 }) : null;
  const state = !pc ? "" : pc.state === "stopped" ? "받기 멈춤" : pc.state === "removed" ? "짝 끊음" : pc.own ? "이 PC" : pc.quiet ? "미확인(소식 없음)" : "연결됨";
  const stateTone = !pc ? "" : pc.state !== "active" ? "text-danger" : pc.quiet ? "text-caution" : "text-safe";

  return (
    <div {...dialog} aria-label="거점 상세" className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-6" onClick={onClose}>
      <div className="flex h-full max-h-full w-full max-w-4xl flex-col rounded-lg border border-line bg-panel" onClick={(e) => e.stopPropagation()}>
        <header className="flex flex-wrap items-center gap-3 border-b border-line px-4 py-3">
          <h1 className="text-base font-semibold">🏠 {pc?.name || "거점"}</h1>
          {pc && !pc.own && <span className="font-mono text-xs text-dim">{pc.addr}</span>}
          {pc && <span className={`text-xs font-semibold ${stateTone}`}>{state}</span>}
          <span className="flex-1" />
          <button onClick={onClose} className="rounded border border-line px-3 py-1 text-xs hover:bg-raised">
            닫기
          </button>
        </header>
        <nav className="flex gap-1 border-b border-line px-3 pt-2">
          {tabs.map((t) => (
            <button key={t.id} onClick={() => setTab(t.id)} className={`rounded-t px-3 py-1.5 text-xs ${tab === t.id ? "bg-raised font-semibold" : "text-dim hover:bg-raised"}`}>
              {t.label}
            </button>
          ))}
        </nav>
        <div className="min-h-0 flex-1 overflow-auto p-4 text-xs">
          {error && <p className="text-danger">{error}</p>}
          {!site && !error && <p className="text-dim">불러오는 중…</p>}

          {site && pc && week && tab === "summary" && (
            <div className="space-y-4">
              <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
                <Stat label="상태" value={state} tone={stateTone} />
                <Stat label="마지막 소식" value={pc.seen ? timeText(pc.seen) : "없음"} />
                <Stat label="보관 중인 결과" value={`${pc.items}건 · ${sizeText(pc.bytes)}`} />
                <Stat label="짝 맺은 때" value={timeText(pc.paired)} />
                <Stat label="7일 동안 받은 결과" value={`${week.items}건`} />
                <Stat label="7일 동안 위험 판정" value={`${week.danger}건`} tone={week.danger ? "text-danger" : undefined} />
                <Stat label="7일 동안 접속" value={`${week.conns}번`} />
                <Stat label="7일 동안 접속 실패" value={`${week.failed}번`} tone={week.failed ? "text-caution" : undefined} />
              </div>
              <section>
                <h2 className="mb-1.5 font-semibold text-dim">최근 7일</h2>
                <Timeline hours={site.timeline} />
              </section>
              <p className="break-all font-mono text-[11px] text-dim">공개 키 지문: {pc.id}</p>
            </div>
          )}

          {site && tab === "conns" && (
            <>
              {site.conns.length === 0 && <p className="text-dim">아직 접속 기록이 없습니다. 기록은 이 판부터 남습니다.</p>}
              {site.conns.length > 0 && (
                <table className="w-full border-collapse">
                  <thead className="text-left text-dim">
                    <tr>
                      <th scope="col" className="py-1 pr-3 font-normal">때</th>
                      <th scope="col" className="py-1 pr-3 font-normal">끝난 까닭</th>
                      <th scope="col" className="py-1 pr-3 font-normal">걸린 시간</th>
                      <th scope="col" className="py-1 pr-3 font-normal">받음</th>
                      <th scope="col" className="py-1 pr-3 font-normal">크기</th>
                      <th scope="col" className="py-1 pr-3 font-normal">순번</th>
                      <th scope="col" className="py-1 font-normal">주소</th>
                    </tr>
                  </thead>
                  <tbody>
                    {site.conns.map((c, i) => (
                      <tr key={i} className="border-t border-line">
                        <td className="py-1 pr-3 whitespace-nowrap">{timeText(c.at)}</td>
                        <td className={`py-1 pr-3 ${GOOD.has(c.outcome) ? "" : "text-caution"}`}>{CONN_LABEL[c.outcome] ?? "알 수 없음"}</td>
                        <td className="py-1 pr-3 whitespace-nowrap">{spanText(c.ms)}</td>
                        <td className="py-1 pr-3 whitespace-nowrap">
                          {c.stored}건{c.again > 0 && ` · 다시 ${c.again}`}
                          {c.refused > 0 && <span className="text-caution"> · 거절 {c.refused}</span>}
                        </td>
                        <td className="py-1 pr-3 whitespace-nowrap">{c.bytes ? sizeText(c.bytes) : "-"}</td>
                        <td className="py-1 pr-3 whitespace-nowrap">{c.first_seq ? (c.first_seq === c.last_seq ? c.first_seq : `${c.first_seq}–${c.last_seq}`) : "-"}</td>
                        <td className="py-1 font-mono text-dim">{c.addr}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </>
          )}

          {site && tab === "transfers" && (
            <>
              {site.transfers.length === 0 && <p className="text-dim">이 거점에서 받은 결과가 없습니다.</p>}
              {site.transfers.length > 0 && (
                <table className="w-full border-collapse">
                  <thead className="text-left text-dim">
                    <tr>
                      <th scope="col" className="py-1 pr-3 font-normal">순번</th>
                      <th scope="col" className="py-1 pr-3 font-normal">받은 때</th>
                      <th scope="col" className="py-1 pr-3 font-normal">진단</th>
                      <th scope="col" className="py-1 pr-3 font-normal">판정</th>
                      <th scope="col" className="py-1 pr-3 font-normal">크기</th>
                      <th scope="col" className="py-1 font-normal">늦게 온 정도</th>
                    </tr>
                  </thead>
                  <tbody>
                    {site.transfers.map((t) => (
                      <tr key={t.seq} className="border-t border-line">
                        <td className="py-1 pr-3">{t.seq}</td>
                        <td className="py-1 pr-3 whitespace-nowrap">{timeText(t.got)}</td>
                        <td className="py-1 pr-3">{t.title}</td>
                        <td className={`py-1 pr-3 ${TONE[t.verdict as Verdict] ?? "text-dim"}`}>{VERDICT_LABEL[t.verdict as Verdict] ?? "판정 없음"}</td>
                        <td className="py-1 pr-3 whitespace-nowrap">{sizeText(t.size)}</td>
                        <td className="py-1 whitespace-nowrap">
                          {spanText(Math.max(0, t.got - t.at))}
                          {t.stale && <span className="text-caution"> · 밀려서 옴</span>}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              <p className="mt-2 text-[11px] text-dim">최근 300건까지 보입니다. 결과의 내용은 "받은 결과" 탭에서 이 거점을 골라 봅니다.</p>
            </>
          )}

          {site && tab === "health" && (
            <div className="space-y-3">
              {site.gaps.length + site.alerts.length === 0 && <p className="text-dim">빠진 구간과 경고가 없습니다.</p>}
              {site.alerts.length > 0 && (
                <section>
                  <h2 className="mb-1 font-semibold text-dim">경고</h2>
                  <ul className="space-y-0.5">
                    {site.alerts.map((a, i) => (
                      <li key={i}>
                        <span className="text-dim">{timeText(a.at)}</span> <span className="text-caution">{a.note}</span>
                      </li>
                    ))}
                  </ul>
                </section>
              )}
              {site.gaps.length > 0 && (
                <section>
                  <h2 className="mb-1 font-semibold text-dim">빠진 구간</h2>
                  <ul className="space-y-0.5">
                    {site.gaps.map((g, i) => (
                      <li key={i}>
                        <span className="text-dim">{timeText(g.got)}</span> 순번 {g.first}–{g.last} {g.dropped ? "(거점이 버렸다고 알림)" : "(건너뜀)"}
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function spanText(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}초`;
  if (s < 3600) return `${Math.floor(s / 60)}분 ${s % 60}초`;
  if (s < 86400) return `${Math.floor(s / 3600)}시간 ${Math.floor((s % 3600) / 60)}분`;
  return `${Math.floor(s / 86400)}일`;
}
