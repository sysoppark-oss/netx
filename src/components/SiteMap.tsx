// 거점 지도: 관제 PC를 가운데 두고 짝 맺은 거점을 둘레에 그린다.
// 선 색 = 상태, 선 굵기 = 최근 24시간에 받은 양, 점선 = 중계 서버를 거침. 색만으로 말하지 않도록 글자를 함께 적는다.
// 이름과 주소는 거점이 보낸 자료다. SVG의 글자로만 그린다. 움직임은 SVG 자체 애니메이션만 쓴다(CSP).
import { useCallback, useEffect, useState } from "react";
import { collectMap, sizeText, timeText, type MapSite, type SiteMapData } from "../lib/collect";
import { hidden } from "../lib/ui";

const W = 800;
const H = 330;
const CX = W / 2;
const CY = H / 2;

function stateOf(s: MapSite): { tone: string; text: string; live: boolean } {
  if (s.pc.state === "stopped") return { tone: "text-danger", text: "받기 멈춤", live: false };
  if (s.pc.quiet) return { tone: "text-caution", text: "소식 없음", live: false };
  return { tone: "text-safe", text: "연결됨", live: true };
}

function pathText(s: MapSite, known: boolean): string {
  if (!known) return "경로 모름";
  if (!s.path) return "테일스케일에 없음";
  if (s.path.kind === "direct") return "직접";
  if (s.path.kind === "relay") return `중계(${s.path.relay || "?"})`;
  return s.path.online ? "쉬는 중" : "꺼져 있음";
}

/** 받은 건수에 따른 선 굵기(1.5~6). 건수가 열 배가 되면 한 단계쯤 굵어진다. */
function widthOf(items: number): number {
  return Math.min(6, 1.5 + Math.log10(1 + items) * 1.6);
}

function short(name: string, max = 12): string {
  const chars = [...name];
  return chars.length > max ? `${chars.slice(0, max - 1).join("")}…` : name;
}

export function SiteMap({ onPick, onError }: { onPick: (id: string) => void; onError: (m: string) => void }) {
  const [map, setMap] = useState<SiteMapData | null>(null);
  const load = useCallback(() => {
    if (hidden()) return;
    collectMap()
      .then(setMap)
      .catch((e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const t = window.setInterval(load, 20_000);
    return () => window.clearInterval(t);
  }, [load]);

  if (!map || map.sites.length === 0) return null;
  const n = map.sites.length;
  // 둘레에 고르게 놓는다. 하나면 왼쪽, 둘이면 왼쪽과 오른쪽.
  const spots = map.sites.map((s, i) => {
    const angle = Math.PI + (2 * Math.PI * i) / n;
    return { s, x: CX + Math.cos(angle) * 300, y: CY + Math.sin(angle) * (n <= 2 ? 0 : 82) };
  });
  const live = map.sites.filter((s) => stateOf(s).live).length;

  return (
    <section>
      <div className="mb-1 flex flex-wrap items-baseline gap-x-3 text-xs">
        <h2 className="font-semibold text-dim">거점 지도</h2>
        <span className="text-dim">
          거점 {n}곳 중 {live}곳 연결됨 · 선 굵기는 최근 24시간에 받은 양 · 점선은 중계 서버를 거치는 길
          {!map.paths && " · 이 PC의 테일스케일에서 경로를 읽지 못했습니다"}
        </span>
      </div>
      <div className="overflow-x-auto rounded-lg border border-line">
        <svg viewBox={`0 0 ${W} ${H}`} className="block h-auto w-full min-w-[40rem]" role="group" aria-label={`거점 지도: 거점 ${n}곳 중 ${live}곳 연결됨`}>
          {spots.map(({ s, x, y }) => {
            const st = stateOf(s);
            const relay = s.path?.kind === "relay";
            return (
              <g key={`l${s.pc.id}`} className={st.tone}>
                <line x1={x} y1={y} x2={CX} y2={CY} stroke="currentColor" strokeWidth={widthOf(s.items)} strokeDasharray={relay ? "7 6" : undefined} strokeLinecap="round" opacity={st.live ? 0.9 : 0.7} />
                {st.live && s.items > 0 && (
                  <circle r="4" fill="currentColor" className="motion-dot">
                    <animateMotion dur={relay ? "3.2s" : "2s"} repeatCount="indefinite" path={`M${x},${y} L${CX},${CY}`} />
                  </circle>
                )}
              </g>
            );
          })}

          <g>
            <circle cx={CX} cy={CY} r="34" className="fill-panel stroke-accent" strokeWidth="2" />
            <text x={CX} y={CY - 3} textAnchor="middle" className="fill-accent text-[13px] font-semibold">
              {short(map.name || "관제 PC", 8)}
            </text>
            <text x={CX} y={CY + 13} textAnchor="middle" className="fill-dim text-[10px]">
              관제 PC
            </text>
            <text x={CX} y={CY + 52} textAnchor="middle" className={`text-[10px] ${map.listening ? "fill-dim" : "fill-danger"}`}>
              {map.listening ? map.listening : "듣지 못함"}
            </text>
          </g>

          {spots.map(({ s, x, y }) => {
            const st = stateOf(s);
            const label = `${s.pc.name || "이름 없는 거점"}: ${st.text}, ${pathText(s, map.paths)}, 최근 24시간 ${s.items}건${s.danger ? `, 위험 ${s.danger}건` : ""}. 누르면 상세를 엽니다.`;
            // 글자는 지도 밖으로 나가지 않게 안쪽으로 맞춘다.
            const anchor = x < 140 ? "start" : x > W - 140 ? "end" : "middle";
            const tx = anchor === "start" ? x - 22 : anchor === "end" ? x + 22 : x;
            const up = y < CY - 20;
            const base = up ? y - 58 : y + 34;
            return (
              <g
                key={s.pc.id}
                role="button"
                tabIndex={0}
                aria-label={label}
                className="cursor-pointer outline-none [&:focus-visible>circle]:stroke-accent"
                onClick={() => onPick(s.pc.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onPick(s.pc.id);
                  }
                }}
              >
                <title>{`${s.pc.name} · ${s.pc.addr}\n마지막 소식 ${s.pc.seen ? timeText(s.pc.seen) : "없음"}\n${s.path?.kind === "direct" ? `직접 연결: ${s.path.addr}` : pathText(s, map.paths)}\n최근 24시간 ${s.items}건 · ${sizeText(s.bytes)}`}</title>
                <circle cx={x} cy={y} r="20" className={`fill-raised ${st.tone}`} stroke="currentColor" strokeWidth="2" />
                <text x={x} y={y + 5} textAnchor="middle" className="text-[15px]" aria-hidden>
                  🏠
                </text>
                {s.danger > 0 && (
                  <g aria-hidden>
                    <circle cx={x + 16} cy={y - 16} r="8" className="fill-danger" />
                    <text x={x + 16} y={y - 12} textAnchor="middle" className="fill-void text-[10px] font-bold">
                      !
                    </text>
                  </g>
                )}
                <text x={tx} y={base} textAnchor={anchor} className="fill-ink text-[12px] font-semibold">
                  {short(s.pc.name || "이름 없음")}
                </text>
                <text x={tx} y={base + 14} textAnchor={anchor} className={`fill-current text-[11px] ${st.tone}`}>
                  ● {st.text} · {pathText(s, map.paths)}
                </text>
                <text x={tx} y={base + 27} textAnchor={anchor} className="fill-dim text-[10px]">
                  24시간 {s.items}건{s.danger > 0 ? ` · 위험 ${s.danger}` : ""}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    </section>
  );
}
