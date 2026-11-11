import { useState } from "react";
import type { Door, Flow, Highway, RadarHost, RouteHop } from "../lib/types";

/**
 * 시각화 4종: 라우팅 노선도, 소나 레이더, 요새 도면, 패킷 하이웨이 (docs/DESIGN.md 6장).
 * 끝난 단계의 자료로 그린다. 외부 라이브러리 없이 SVG로 그리고, 움직임은 SVG 자체 애니메이션을 쓴다
 * (NetX의 CSP는 스크립트가 만든 <style>을 막으므로 CSS를 끼워 넣는 방식은 쓰지 않는다).
 * 진단 결과 화면이므로 말씀 표시는 넣지 않는다.
 */

const SAFE = "var(--color-safe)";
const ACCENT = "var(--color-accent)";
const CAUTION = "var(--color-caution)";
const DANGER = "var(--color-danger)";
const DIM = "var(--color-dim)";
const INK = "var(--color-ink)";
const LINE = "var(--color-line)";
const BLUE = "#60a5fa";

const clip = (text: string, max: number) => (text.length > max ? `${text.slice(0, max - 1)}…` : text);
const ms = (v: number) => (v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2));

/** 맥박처럼 뛰는 효과(문제 구간) */
function Pulse({ attr, from, to }: { attr: string; from: string; to: string }) {
  return <animate attributeName={attr} values={`${from};${to};${from}`} dur="1.2s" repeatCount="indefinite" />;
}

// ---------------------------------------------------------------- 라우팅 노선도

type HopState = "ok" | "slow" | "quiet" | "loss" | "dead";

function hopState(h: RouteHop, from: number): HopState {
  if (h.loss >= 100) return from > 0 && h.n >= from ? "dead" : "quiet";
  if (h.loss > 0) return from > 0 && h.n >= from ? "loss" : "quiet";
  return h.avg > 100 ? "slow" : "ok";
}

function hopText(h: RouteHop, state: HopState): string {
  switch (state) {
    case "dead":
      return `${h.n}번 홉(${h.host})부터 응답이 없습니다. 이 구간에서 끊긴 것으로 보입니다.`;
    case "loss":
      return `이 라우터(${h.host})에서 ${h.loss.toFixed(0)}%가 유실되고 있고, 목적지까지 손실이 이어집니다. 이 구간이 원인으로 보입니다.`;
    case "quiet":
      return `${h.n}번 홉(${h.host})은 손실 ${h.loss.toFixed(0)}%로 보이지만 그 뒤 구간에는 영향이 없습니다. 라우터가 응답을 아끼는 것으로 보입니다.`;
    case "slow":
      return `${h.n}번 홉(${h.host})의 지연이 큽니다(평균 ${ms(h.avg)}ms, 가장 느릴 때 ${ms(h.worst)}ms).`;
    default:
      return `${h.n}번 홉(${h.host})은 정상입니다(평균 ${ms(h.avg)}ms, 가장 느릴 때 ${ms(h.worst)}ms, 손실 없음).`;
  }
}

const HOP_COLOR: Record<HopState, string> = { ok: ACCENT, slow: DANGER, quiet: CAUTION, loss: DANGER, dead: DANGER };

export function RouteMap({ hops, from }: { hops: RouteHop[]; from: number }) {
  const [picked, setPicked] = useState<number | null>(null);
  const PER_ROW = 6;
  const GAP = 100;
  // 줄 끝 역의 글자를 바깥쪽에 적을 자리를 양옆에 둔다.
  const X0 = 110;
  const ROW = 78;
  // 0번은 내 PC. 줄이 바뀔 때마다 방향을 뒤집어 한 줄로 이어지게 한다.
  const at = (i: number) => {
    const row = Math.floor(i / PER_ROW);
    const col = i % PER_ROW;
    return { x: X0 + (row % 2 === 0 ? col : PER_ROW - 1 - col) * GAP, y: 34 + row * ROW };
  };
  const rows = Math.ceil((hops.length + 1) / PER_ROW);
  const states = hops.map((h) => hopState(h, from));
  const chosen = picked !== null ? hops.findIndex((h) => h.n === picked) : -1;

  return (
    <div>
      <svg viewBox={`0 0 ${X0 * 2 + (PER_ROW - 1) * GAP} ${rows * ROW + 6}`} className="w-full" style={{ maxHeight: rows * 110 }}>
        {hops.map((h, i) => {
          const a = at(i);
          const b = at(i + 1);
          const bad = states[i] === "loss" || states[i] === "dead" || states[i] === "slow";
          return (
            <line key={`l${h.n}`} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={HOP_COLOR[states[i]]} strokeWidth={bad ? 6 : 2.5} strokeLinecap="round" strokeDasharray={states[i] === "dead" ? "2 9" : undefined}>
              {bad && <Pulse attr="stroke-opacity" from="1" to="0.35" />}
            </line>
          );
        })}
        <g>
          <circle cx={at(0).x} cy={at(0).y} r="10" fill={INK} />
          <text x={at(0).x} y={at(0).y + 26} fontSize="10" fill={DIM} textAnchor="middle">
            내 PC
          </text>
        </g>
        {hops.map((h, i) => {
          const p = at(i + 1);
          const bad = states[i] !== "ok";
          // 줄 끝의 역은 아래로 내려가는 선과 글자가 겹치지 않게 글자를 바깥쪽 옆에 적는다.
          const turn = (i + 1) % PER_ROW === PER_ROW - 1 && i + 1 < hops.length;
          const leftward = Math.floor((i + 1) / PER_ROW) % 2 === 1;
          const tx = turn ? p.x + (leftward ? -16 : 16) : p.x;
          const anchor = turn ? (leftward ? "end" : "start") : "middle";
          const ty = turn ? p.y - 22 : p.y;
          return (
            <g key={h.n} onClick={() => setPicked(picked === h.n ? null : h.n)} style={{ cursor: "pointer" }}>
              <title>{hopText(h, states[i])}</title>
              <circle cx={p.x} cy={p.y} r={picked === h.n ? 12 : 9} fill="var(--color-void)" stroke={HOP_COLOR[states[i]]} strokeWidth={bad ? 4 : 2.5}>
                {(states[i] === "loss" || states[i] === "dead") && <Pulse attr="r" from="9" to="12" />}
              </circle>
              <text x={p.x} y={p.y + 3.5} fontSize="9" fill={INK} textAnchor="middle">
                {h.n}
              </text>
              <text x={tx} y={ty + 25} fontSize="9" fill={bad ? HOP_COLOR[states[i]] : DIM} textAnchor={anchor}>
                {clip(h.host, turn ? 15 : 17)}
              </text>
              <text x={tx} y={ty + 36} fontSize="8.5" fill={DIM} textAnchor={anchor}>
                {h.loss >= 100 ? "응답 없음" : `${ms(h.avg)}ms${h.loss > 0 ? ` · 손실 ${h.loss.toFixed(0)}%` : ""}`}
              </text>
            </g>
          );
        })}
      </svg>
      <p className="mt-1 min-h-5 text-xs text-ink/90">{chosen >= 0 ? hopText(hops[chosen], states[chosen]) : <span className="text-dim">청록색은 정상, 노란색은 그 홉만의 손실, 붉은색은 손실이 이어지거나 지연이 큰 구간입니다.</span>}</p>
    </div>
  );
}

// ---------------------------------------------------------------- 소나 레이더

export function RadarView({ hosts }: { hosts: RadarHost[] }) {
  const [picked, setPicked] = useState<string | null>(null);
  const C = 150;
  const known = hosts.map((h) => h.ms).filter((v): v is number => v !== null);
  const slowest = Math.max(...known, 1);
  const dots = hosts.map((h) => {
    const last = Number(h.host.split(".").pop() ?? 0) || 0;
    const angle = (last / 256) * Math.PI * 2 - Math.PI / 2;
    // 응답이 빠를수록 가운데에 가깝고, 크고 밝다. 시간을 모르면 바깥 고리에 둔다.
    const far = h.ms === null ? 1 : Math.log10(1 + h.ms) / Math.log10(1 + slowest);
    const r = 28 + far * 100;
    return { ...h, last, x: C + r * Math.cos(angle), y: C + r * Math.sin(angle), size: 6 - far * 3.2, glow: 1 - far * 0.55 };
  });
  const chosen = dots.find((d) => d.host === picked);

  return (
    <div className="flex flex-wrap items-center gap-5">
      <svg viewBox="0 0 300 300" className="h-72 w-72 shrink-0">
        <circle cx={C} cy={C} r="140" fill="#04130f" stroke={SAFE} strokeOpacity="0.5" />
        {[35, 70, 105].map((r) => (
          <circle key={r} cx={C} cy={C} r={r} fill="none" stroke={SAFE} strokeOpacity="0.22" />
        ))}
        <path d={`M${C},10 V290 M10,${C} H290`} stroke={SAFE} strokeOpacity="0.18" />
        {/* 도는 레이더 줄기 */}
        <g>
          <path d={`M${C},${C} L${C},10 A140,140 0 0 1 ${C + 140 * Math.sin(0.5)},${C - 140 * Math.cos(0.5)} Z`} fill={SAFE} fillOpacity="0.16" />
          <line x1={C} y1={C} x2={C} y2="10" stroke={SAFE} strokeWidth="1.5" strokeOpacity="0.8" />
          <animateTransform attributeName="transform" type="rotate" from={`0 ${C} ${C}`} to={`360 ${C} ${C}`} dur="4s" repeatCount="indefinite" />
        </g>
        <circle cx={C} cy={C} r="3" fill={SAFE} />
        {dots.map((d) => (
          <g key={d.host} onClick={() => setPicked(picked === d.host ? null : d.host)} style={{ cursor: "pointer" }}>
            <title>{d.host}{d.ms !== null ? ` · ${ms(d.ms)}ms` : ""}</title>
            <circle cx={d.x} cy={d.y} r={d.size + 5} fill={SAFE} fillOpacity={picked === d.host ? 0.35 : 0.12} />
            <circle cx={d.x} cy={d.y} r={d.size} fill={SAFE} fillOpacity={d.glow}>
              <animate attributeName="fill-opacity" values={`${d.glow};${d.glow * 0.45};${d.glow}`} dur="4s" begin={`${(d.last / 256) * 4}s`} repeatCount="indefinite" />
            </circle>
            {hosts.length <= 40 && (
              <text x={d.x + d.size + 4} y={d.y + 3} fontSize="8.5" fill={SAFE} fillOpacity="0.85">
                .{d.last}
              </text>
            )}
          </g>
        ))}
      </svg>
      <div className="min-w-40 flex-1 text-xs">
        <p className="font-mono text-2xl font-semibold text-safe">{hosts.length}대</p>
        <p className="mt-1 text-dim">응답이 빠를수록 가운데에 가깝고 크게 보입니다. 각도는 주소 끝자리입니다.</p>
        <p className="mt-3 min-h-10 leading-relaxed">
          {chosen ? (
            <>
              <span className="font-mono text-ink">{chosen.host}</span>
              <br />
              <span className="text-dim">{chosen.ms !== null ? `응답 ${ms(chosen.ms)}ms` : "응답함 (시간 기록 없음)"}</span>
            </>
          ) : (
            <span className="text-dim">점을 누르면 주소와 응답 시간이 나옵니다.</span>
          )}
        </p>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------- 요새 도면

export function FortressView({ doors, more }: { doors: Door[]; more: number }) {
  const [picked, setPicked] = useState<number | null>(null);
  const W = 560;
  const H = 320;
  const cx = W / 2;
  const cy = H / 2;
  const RX = 150;
  const RY = 100;
  const wall = Array.from({ length: 8 }, (_, i) => {
    const a = (i / 8) * Math.PI * 2 + Math.PI / 8;
    return `${(cx + RX * Math.cos(a)).toFixed(1)},${(cy + RY * Math.sin(a)).toFixed(1)}`;
  }).join(" ");
  const unknown = (d: Door) => d.process.includes("알 수 없음") || d.process === "";
  const chosen = picked !== null ? doors[picked] : null;

  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ maxHeight: 360 }}>
        <polygon points={wall} fill="var(--color-panel)" stroke={LINE} strokeWidth="10" strokeLinejoin="round" />
        <polygon points={wall} fill="none" stroke={DIM} strokeOpacity="0.5" strokeWidth="1.5" strokeDasharray="5 5" />
        <text x={cx} y={cy - 2} fontSize="14" fill={INK} textAnchor="middle" fontWeight="600">
          내 PC
        </text>
        <text x={cx} y={cy + 16} fontSize="10" fill={DIM} textAnchor="middle">
          열린 문 {doors.length + more}개{more > 0 ? ` (그림에는 ${doors.length}개)` : ""}
        </text>
        {doors.map((d, i) => {
          const a = (i / doors.length) * Math.PI * 2 - Math.PI / 2;
          const x = cx + RX * 0.96 * Math.cos(a);
          const y = cy + RY * 0.96 * Math.sin(a);
          const lx = cx + (RX + 46) * Math.cos(a);
          const ly = cy + (RY + 30) * Math.sin(a);
          const color = unknown(d) ? DANGER : d.listening ? BLUE : DIM;
          const on = picked === i;
          return (
            <g key={i} onClick={() => setPicked(on ? null : i)} style={{ cursor: "pointer" }}>
              <title>{`${d.proto} ${d.port} · ${d.process} (PID ${d.pid})`}</title>
              <rect x={x - 7} y={y - 7} width="14" height="14" rx="2" fill="var(--color-void)" stroke={color} strokeWidth={on ? 3.5 : 2}>
                {unknown(d) && <Pulse attr="stroke-opacity" from="1" to="0.2" />}
              </rect>
              <line x1={x} y1={y} x2={lx} y2={ly} stroke={color} strokeOpacity="0.35" />
              <text x={lx} y={ly - 1} fontSize="9.5" fill={color} textAnchor="middle" fontWeight={on ? 700 : 400}>
                {d.port}
              </text>
              {(doors.length <= 18 || on) && (
                <text x={lx} y={ly + 10} fontSize="8" fill={DIM} textAnchor="middle">
                  {clip(d.process, 14)}
                </text>
              )}
            </g>
          );
        })}
      </svg>
      <p className="mt-1 min-h-9 text-xs leading-relaxed">
        {chosen ? (
          <>
            <span className="font-mono text-ink">
              {chosen.proto} {chosen.port}
            </span>{" "}
            문은 <span className="font-semibold">{chosen.process}</span> (PID {chosen.pid})이(가) 지키고 있습니다. {chosen.listening ? "들어오는 연결을 기다리는 문입니다." : "밖으로 맺은 연결입니다."}
            {chosen.connections > 0 && ` 지금 연결 ${chosen.connections}개.`}
            {unknown(chosen) && <span className="text-danger"> 어떤 프로그램인지 알 수 없습니다. 확인이 필요합니다.</span>}
            <span className="text-dim"> 종료는 아래 "포트를 쓰고 있는 프로그램"에서 합니다.</span>
          </>
        ) : (
          <span className="text-dim">파란 문은 들어오는 연결을 기다리는 포트, 회색 문은 맺어진 연결, 붉게 깜박이는 문은 프로그램을 알 수 없는 포트입니다. 문을 누르면 지키는 프로그램이 나옵니다.</span>
        )}
      </p>
    </div>
  );
}

// ---------------------------------------------------------------- 패킷 하이웨이

const FLOW_COLOR: Record<Flow["class"], string> = { secure: SAFE, dns: BLUE, plain: DIM, trouble: DANGER };
const FLOW_NAME: Record<Flow["class"], string> = { secure: "암호화 통신", dns: "DNS", plain: "그 밖", trouble: "재전송·거부" };

const kbps = (v: number) => (v >= 1000 ? `${(v / 1000).toFixed(1)} Mbps` : `${v.toFixed(v >= 10 ? 0 : 1)} Kbps`);

export function HighwayView({ road }: { road: Highway }) {
  const ROW = 34;
  const W = 640;
  const H = Math.max(120, road.flows.length * ROW + 30);
  const me = { x: 70, y: H / 2 };
  const px = 430;

  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H}`} className="w-full" style={{ maxHeight: H * 1.4 }}>
        {road.flows.map((f, i) => {
          const y = 22 + i * ROW + (H - 30 - road.flows.length * ROW) / 2 + 6;
          const path = `M${me.x},${me.y} C${me.x + 150},${me.y} ${px - 150},${y} ${px},${y}`;
          const color = FLOW_COLOR[f.class];
          const total = f.sent + f.received;
          const cars = (n: number) => (n === 0 ? 0 : Math.min(5, Math.max(1, Math.round(Math.log2(n + 1)))));
          const tip = `${f.peer}:${f.port} (${f.proto.toUpperCase()}) · ${kbps(f.kbps)} · 보냄 ${f.sent} · 받음 ${f.received}${f.trouble > 0 ? ` · 재전송·거부 ${f.trouble}` : ""}`;
          return (
            <g key={`${f.peer}:${f.port}:${f.proto}`}>
              <title>{tip}</title>
              {/* 마우스를 올리기 쉽게 넓고 투명한 선을 겹친다 */}
              <path d={path} fill="none" stroke={color} strokeOpacity="0" strokeWidth="14" />
              <path d={path} fill="none" stroke={color} strokeOpacity="0.35" strokeWidth={Math.min(5, 1 + Math.log10(total + 1))} />
              {Array.from({ length: cars(f.sent) }, (_, k) => (
                <circle key={`o${k}`} r="3" fill={color}>
                  <animateMotion path={path} dur="2.4s" begin={`${(k * 2.4) / cars(f.sent)}s`} repeatCount="indefinite" />
                </circle>
              ))}
              {Array.from({ length: cars(f.received) }, (_, k) => (
                <circle key={`i${k}`} r="2.2" fill={color} fillOpacity="0.7">
                  <animateMotion path={path} dur="2.4s" begin={`${(k * 2.4) / cars(f.received) + 0.6}s`} repeatCount="indefinite" keyPoints="1;0" keyTimes="0;1" calcMode="linear" />
                </circle>
              ))}
              <circle cx={px} cy={y} r="6" fill="var(--color-void)" stroke={color} strokeWidth="2" />
              <text x={px + 13} y={y - 1} fontSize="10" fill={INK}>
                {clip(f.peer, 28)}:{f.port}
              </text>
              <text x={px + 13} y={y + 10} fontSize="8.5" fill={color}>
                {FLOW_NAME[f.class]} · {kbps(f.kbps)} · {total}개
              </text>
            </g>
          );
        })}
        <circle cx={me.x} cy={me.y} r="16" fill="var(--color-panel)" stroke={ACCENT} strokeWidth="2.5" />
        <text x={me.x} y={me.y + 3.5} fontSize="9" fill={INK} textAnchor="middle">
          내 PC
        </text>
        <text x={me.x} y={me.y + 31} fontSize="9" fill={DIM} textAnchor="middle">
          {clip(road.me, 22)}
        </text>
      </svg>
      <p className="mt-1 flex flex-wrap gap-x-4 gap-y-1 text-xs text-dim">
        {(Object.keys(FLOW_NAME) as Flow["class"][]).map((c) => (
          <span key={c} className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-full" style={{ background: FLOW_COLOR[c] }} />
            {FLOW_NAME[c]}
          </span>
        ))}
        <span>
          패킷 {road.packets}개 · {road.seconds.toFixed(1)}초
          {road.more > 0 && ` · 상대 ${road.more}곳은 그림에서 뺐습니다`}
        </span>
      </p>
    </div>
  );
}
