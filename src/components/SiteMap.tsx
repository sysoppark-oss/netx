// 거점 지도(관제실 화면): 관제 PC를 가운데 두고 짝 맺은 거점을 둘레에 그린다.
// 선 색 = 상태, 선 굵기 = 최근 24시간에 받은 양, 점선 = 중계 서버를 거침. 색만으로 말하지 않도록 글자를 함께 적는다.
// 이름과 주소는 거점이 보낸 자료다. SVG의 글자로만 그린다. 움직임은 SVG 자체 애니메이션만 쓴다(CSP).
// 움직이는 것(레이더, 흐르는 신호)에는 motion-dot 을 붙여 "움직임 줄이기"에서 감춘다.
import { useCallback, useEffect, useState } from "react";
import { Zone } from "./Zone";
import { collectMap, sizeText, timeText, type MapSite, type SiteMapData } from "../lib/collect";
import { hidden } from "../lib/ui";

const W = 800;
const CX = W / 2;

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

/** 받은 건수에 따른 선 굵기(1.5~5). 건수가 열 배가 되면 한 단계쯤 굵어진다. */
function widthOf(items: number): number {
  return Math.min(5, 1.5 + Math.log10(1 + items) * 1.3);
}

function short(name: string, max = 18): string {
  const chars = [...name];
  return chars.length > max ? `${chars.slice(0, max - 1).join("")}…` : name;
}

/** 꼭짓점이 위를 향한 육각형 */
function hex(cx: number, cy: number, r: number): string {
  return Array.from({ length: 6 }, (_, i) => {
    const a = (Math.PI / 3) * i - Math.PI / 2;
    return `${(cx + Math.cos(a) * r).toFixed(1)},${(cy + Math.sin(a) * r).toFixed(1)}`;
  }).join(" ");
}

function Tile({ label, value, unit, tone }: { label: string; value: string; unit?: string; tone?: string }) {
  return (
    <div className="hud px-3 py-2">
      <div className="text-[10px] tracking-[0.18em] text-dim">{label}</div>
      <div className={`font-mono text-xl leading-tight font-semibold ${tone ?? "text-ink"}`}>
        {value}
        {unit && <span className="ml-1 text-xs font-normal text-dim">{unit}</span>}
      </div>
    </div>
  );
}

/** 지도 자료를 20초마다 읽는다. 숫자판과 지도가 같이 쓴다. */
export function useSiteMap(onError: (m: string) => void): SiteMapData | null {
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

  return map;
}

/** 숫자판 네 개 */
export function MapTiles({ map }: { map: SiteMapData }) {
  const n = map.sites.length;
  const live = map.sites.filter((s) => stateOf(s).live).length;
  const items = map.sites.reduce((a, s) => a + s.items, 0);
  const bytes = map.sites.reduce((a, s) => a + s.bytes, 0);
  const danger = map.sites.reduce((a, s) => a + s.danger, 0);
  const direct = map.sites.filter((s) => s.path?.kind === "direct").length;
  const relay = map.sites.filter((s) => s.path?.kind === "relay").length;

  return (
    <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Tile label="연결된 거점" value={`${live}/${n}`} unit="곳" tone={n > 0 && live === n ? "text-safe" : live > 0 ? "text-caution" : "text-dim"} />
        <Tile label="24시간 수신" value={String(items)} unit={`건 · ${sizeText(bytes)}`} tone="text-accent" />
        <Tile label="24시간 위험 판정" value={String(danger)} unit="건" tone={danger > 0 ? "text-danger" : "text-ink"} />
        <Tile label="경로" value={map.paths ? `${direct}` : "?"} unit={map.paths ? `직접 · 중계 ${relay}` : "테일스케일에서 읽지 못함"} />
    </div>
  );
}

/** 연결 지도 구역 */
export function MapZone({ map, onPick, className }: { map: SiteMapData; onPick: (id: string) => void; className?: string }) {
  const n = map.sites.length;
  // 거점이 한두 곳이면 한 줄로 놓으므로 낮게 그린다.
  const H = n <= 2 ? 250 : 380;
  const CY = n <= 2 ? 104 : H / 2 - 8;
  const spots = map.sites.map((s, i) => {
    // 셋 넘으면 반 칸 돌려 놓아, 관제 PC 바로 위아래(이름 적는 자리)에 거점이 오지 않게 한다.
    const angle = Math.PI + (2 * Math.PI * i) / n + (n > 2 ? Math.PI / n : 0);
    return { s, x: CX + Math.cos(angle) * 300, y: CY + Math.sin(angle) * (n <= 2 ? 0 : 100) };
  });
  const live = map.sites.filter((s) => stateOf(s).live).length;
  return (
      <Zone className={className} code="01" title="연결 지도" note="선 굵기 = 24시간에 받은 양 · 점선 = 중계 · 거점을 누르면 상세" flush>
        <div className="overflow-x-auto">
        <svg viewBox={`0 0 ${W} ${H}`} className="mx-auto block max-h-[26rem] w-full min-w-[30rem]" role="group" aria-label={`거점 지도: 거점 ${n}곳 중 ${live}곳 연결됨`}>
          <defs>
            <pattern id="map-grid" width="32" height="32" patternUnits="userSpaceOnUse">
              <path d="M32 0H0V32" fill="none" className="stroke-accent" strokeWidth="0.5" opacity="0.16" />
            </pattern>
            <radialGradient id="map-core" cx="50%" cy="50%" r="50%">
              <stop offset="0%" className="[stop-color:var(--color-accent)]" stopOpacity="0.22" />
              <stop offset="60%" className="[stop-color:var(--color-accent)]" stopOpacity="0.04" />
              <stop offset="100%" className="[stop-color:var(--color-accent)]" stopOpacity="0" />
            </radialGradient>
            <linearGradient id="map-sweep" x1="0" y1="0" x2="1" y2="0">
              <stop offset="0%" className="[stop-color:var(--color-accent)]" stopOpacity="0" />
              <stop offset="100%" className="[stop-color:var(--color-accent)]" stopOpacity="0.35" />
            </linearGradient>
            <filter id="map-glow" x="-50%" y="-50%" width="200%" height="200%">
              <feGaussianBlur stdDeviation="3.5" result="b" />
              <feMerge>
                <feMergeNode in="b" />
                <feMergeNode in="SourceGraphic" />
              </feMerge>
            </filter>
          </defs>

          {/* 바탕: 격자와 가운데의 빛 */}
          <rect width={W} height={H} fill="url(#map-grid)" />
          <circle cx={CX} cy={CY} r="230" fill="url(#map-core)" />

          {/* 레이더: 동심원과 도는 부채꼴 */}
          <g className="stroke-accent" fill="none">
            {[52, 88, 124].map((r, i) => (
              <circle key={r} cx={CX} cy={CY} r={r} strokeWidth="0.8" opacity={0.42 - i * 0.11} strokeDasharray={i === 1 ? "3 7" : undefined} />
            ))}
            <path d={`M${CX - 132},${CY}H${CX + 132}M${CX},${CY - 132}V${CY + 132}`} strokeWidth="0.5" opacity="0.2" />
          </g>
          <g className="motion-dot">
            <path d={`M${CX},${CY} L${CX + 124},${CY} A124,124 0 0 0 ${(CX + 124 * Math.cos(-0.7)).toFixed(1)},${(CY + 124 * Math.sin(-0.7)).toFixed(1)} Z`} fill="url(#map-sweep)" />
            <animateTransform attributeName="transform" type="rotate" from={`0 ${CX} ${CY}`} to={`360 ${CX} ${CY}`} dur="7s" repeatCount="indefinite" />
          </g>

          {/* 선: 번지는 빛 + 본선 + 흐르는 신호 */}
          {spots.map(({ s, x, y }) => {
            const st = stateOf(s);
            const relayed = s.path?.kind === "relay";
            const w = widthOf(s.items);
            return (
              <g key={`l${s.pc.id}`} className={st.tone} stroke="currentColor" strokeLinecap="round">
                <line x1={x} y1={y} x2={CX} y2={CY} strokeWidth={w + 7} opacity={st.live ? 0.16 : 0.08} />
                <line x1={x} y1={y} x2={CX} y2={CY} strokeWidth={w} strokeDasharray={relayed ? "8 7" : undefined} opacity={st.live ? 0.95 : 0.6} />
                {st.live && (
                  <line x1={x} y1={y} x2={CX} y2={CY} strokeWidth={Math.max(1.2, w - 1.5)} strokeDasharray="3 21" className="motion-dot stroke-ink" opacity="0.9">
                    <animate attributeName="stroke-dashoffset" from="24" to="0" dur={relayed ? "1.6s" : "0.9s"} repeatCount="indefinite" />
                  </line>
                )}
              </g>
            );
          })}

          {/* 관제 PC */}
          <g>
            <polygon points={hex(CX, CY, 34)} className="fill-void stroke-accent" strokeWidth="2" filter="url(#map-glow)" />
            <polygon points={hex(CX, CY, 24)} fill="none" className="stroke-accent" strokeWidth="1" opacity="0.6" />
            <circle cx={CX} cy={CY} r="6" className="fill-accent">
              <animate attributeName="opacity" values="1;0.35;1" dur="2.4s" repeatCount="indefinite" />
            </circle>
            {/* 이름은 도형 밖에 적는다(긴 PC 이름이 넘치지 않게). */}
            <text x={CX} y={CY + 56} textAnchor="middle" className="fill-accent font-mono text-[12px] font-semibold tracking-wider">
              {short(map.name || "관제 PC", 20)}
            </text>
            <text x={CX} y={CY + 71} textAnchor="middle" className="fill-dim text-[10px] tracking-[0.2em]">
              관제 PC
            </text>
            <text x={CX} y={CY + 85} textAnchor="middle" className={`font-mono text-[10px] ${map.listening ? "fill-dim" : "fill-danger"}`}>
              {map.listening ? map.listening : "듣지 못함"}
            </text>
            {n === 0 && (
              <text x={CX} y={CY + 108} textAnchor="middle" className="fill-dim text-[12px]">
                아직 짝 맺은 거점이 없습니다
              </text>
            )}
          </g>

          {/* 거점 */}
          {spots.map(({ s, x, y }) => {
            const st = stateOf(s);
            const label = `${s.pc.name || "이름 없는 거점"}: ${st.text}, ${pathText(s, map.paths)}, 최근 24시간 ${s.items}건${s.danger ? `, 위험 ${s.danger}건` : ""}. 누르면 상세를 엽니다.`;
            // 글자는 지도 밖으로 나가지 않게 안쪽으로 맞춘다.
            const anchor = x < 140 ? "start" : x > W - 140 ? "end" : "middle";
            const tx = anchor === "start" ? x - 22 : anchor === "end" ? x + 22 : x;
            const up = y < CY - 20;
            const base = up ? y - 62 : y + 40;
            return (
              <g
                key={s.pc.id}
                role="button"
                tabIndex={0}
                aria-label={label}
                className={`cursor-pointer outline-none ${st.tone} [&:focus-visible>polygon]:stroke-accent [&:hover>polygon]:fill-raised`}
                onClick={() => onPick(s.pc.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onPick(s.pc.id);
                  }
                }}
              >
                <title>{`${s.pc.name} · ${s.pc.addr}\n마지막 소식 ${s.pc.seen ? timeText(s.pc.seen) : "없음"}\n${s.path?.kind === "direct" ? `직접 연결: ${s.path.addr}` : pathText(s, map.paths)}\n최근 24시간 ${s.items}건 · ${sizeText(s.bytes)}`}</title>
                <polygon points={hex(x, y, 22)} className="fill-void" stroke="currentColor" strokeWidth="2" filter={st.live ? "url(#map-glow)" : undefined} />
                <polygon points={hex(x, y, 13)} fill="currentColor" opacity={st.live ? 0.9 : 0.45} />
                {st.live && (
                  <circle cx={x} cy={y} r="22" fill="none" stroke="currentColor" strokeWidth="1" className="motion-dot">
                    <animate attributeName="r" values="22;38" dur="2.2s" repeatCount="indefinite" />
                    <animate attributeName="opacity" values="0.6;0" dur="2.2s" repeatCount="indefinite" />
                  </circle>
                )}
                {s.danger > 0 && (
                  <g aria-hidden>
                    <circle cx={x + 19} cy={y - 19} r="9" className="fill-danger stroke-void" strokeWidth="2" />
                    <text x={x + 19} y={y - 15} textAnchor="middle" className="fill-void text-[11px] font-bold">
                      !
                    </text>
                  </g>
                )}
                <text x={tx} y={base} textAnchor={anchor} className="fill-ink text-[12px] font-semibold">
                  {short(s.pc.name || "이름 없음")}
                </text>
                <text x={tx} y={base + 14} textAnchor={anchor} className="fill-current text-[11px]">
                  ● {st.text} · {pathText(s, map.paths)}
                </text>
                <text x={tx} y={base + 27} textAnchor={anchor} className="fill-dim font-mono text-[10px]">
                  {s.pc.addr} · 24H {s.items}건{s.danger > 0 ? ` · 위험 ${s.danger}` : ""}
                </text>
              </g>
            );
          })}
        </svg>
        </div>
      </Zone>
  );
}
