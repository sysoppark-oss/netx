import { useState } from "react";
import type { TopoNode, Topology } from "../lib/types";

const ICON: Record<string, string> = {
  router: "📡",
  pc: "💻",
  server: "🖥️",
  printer: "🖨️",
  camera: "📷",
  phone: "📱",
  network: "🔀",
  unknown: "❔",
};

const COLS = 5;
const W = 760;
const CELL_W = W / COLS;
const CELL_H = 104;
const TOP = 210;

/** 네트워크 구성도: 인터넷 → 공유기(게이트웨이) → 내부망 → 장비들. 장비를 누르면 자세한 내용이 나온다. */
export function TopologyView({ topology }: { topology: Topology }) {
  const [picked, setPicked] = useState<TopoNode | null>(null);
  const gateway = topology.nodes.find((n) => n.role === "gateway") ?? null;
  // 내 PC를 맨 앞에 둔다.
  const devices = topology.nodes.filter((n) => n.role !== "gateway").sort((a, b) => Number(b.role === "me") - Number(a.role === "me"));
  const rows = Math.max(1, Math.ceil(devices.length / COLS));
  const height = TOP + rows * CELL_H + 10;
  const cx = W / 2;

  return (
    <div className="mt-3 rounded-md border border-line bg-void p-3">
      <div className="flex items-baseline justify-between">
        <p className="text-xs font-semibold">네트워크 구성도</p>
        <p className="font-mono text-xs text-dim">
          {topology.cidr} · 장비 {topology.nodes.length}대
        </p>
      </div>
      <svg viewBox={`0 0 ${W} ${height}`} className="mt-2 w-full" role="img" aria-label="네트워크 구성도">
        {/* 인터넷 */}
        <rect x={cx - 70} y={6} width={140} height={40} rx={20} className="fill-raised stroke-line" />
        <text x={cx} y={31} textAnchor="middle" className="fill-ink text-[13px]">
          ☁ 인터넷
        </text>
        <line x1={cx} y1={46} x2={cx} y2={76} className="stroke-accent" strokeWidth={2} />

        {/* 공유기 */}
        <g onClick={() => gateway && setPicked(gateway)} className={gateway ? "cursor-pointer" : ""}>
          <rect x={cx - 110} y={76} width={220} height={58} rx={8} className={`fill-panel ${picked === gateway && gateway ? "stroke-accent" : "stroke-line"}`} strokeWidth={1.5} />
          <text x={cx - 92} y={112} className="text-[22px]">
            📡
          </text>
          <text x={cx - 58} y={100} className="fill-ink text-[12px] font-semibold">
            공유기(게이트웨이)
          </text>
          <text x={cx - 58} y={118} className="fill-dim font-mono text-[11px]">
            {topology.gateway ?? "주소 모름"}
            {gateway?.vendor ? ` · ${gateway.vendor}` : ""}
          </text>
        </g>
        <line x1={cx} y1={134} x2={cx} y2={170} className="stroke-accent" strokeWidth={2} />

        {/* 내부망 선 */}
        <line x1={CELL_W / 2} y1={170} x2={W - CELL_W / 2} y2={170} className="stroke-accent" strokeWidth={2} />
        <text x={W - 8} y={162} textAnchor="end" className="fill-dim text-[10px]">
          내부망 {topology.cidr}
        </text>

        {devices.map((n, i) => {
          const col = i % COLS;
          const row = Math.floor(i / COLS);
          const x = col * CELL_W + CELL_W / 2;
          const y = TOP + row * CELL_H;
          const me = n.role === "me";
          return (
            <g key={n.ip} onClick={() => setPicked(n)} className="cursor-pointer">
              {/* 첫 줄은 내부망 선에서, 다음 줄은 윗줄 장비 아래에서 내려온다 */}
              <line x1={x} y1={row === 0 ? 170 : y - CELL_H + 78} x2={x} y2={y} className="stroke-line" strokeWidth={1.5} />
              <rect
                x={x - CELL_W / 2 + 8}
                y={y}
                width={CELL_W - 16}
                height={78}
                rx={8}
                className={`${me ? "fill-accent/10" : "fill-panel"} ${picked === n ? "stroke-accent" : me ? "stroke-accent/60" : "stroke-line"}`}
                strokeWidth={1.5}
              />
              <text x={x} y={y + 26} textAnchor="middle" className="text-[20px]">
                {ICON[n.kind] ?? "❔"}
              </text>
              <text x={x} y={y + 46} textAnchor="middle" className="fill-ink font-mono text-[11px]">
                {n.ip}
              </text>
              <text x={x} y={y + 62} textAnchor="middle" className={`text-[10px] ${me ? "fill-accent" : "fill-dim"}`}>
                {n.label}
              </text>
            </g>
          );
        })}
      </svg>

      {picked ? (
        <dl className="mt-2 flex flex-wrap gap-x-5 gap-y-1 rounded border border-line px-3 py-2 text-xs">
          {[
            ["주소", picked.ip],
            ["추정 종류", picked.label],
            ["MAC", picked.mac ?? "알 수 없음"],
            ["제조사", picked.vendor || "알 수 없음"],
            ["열린 포트", picked.ports.length > 0 ? picked.ports.join(", ") : "찾지 못함"],
          ].map(([k, v]) => (
            <div key={k} className="flex gap-1.5">
              <dt className="text-dim">{k}</dt>
              <dd className="font-mono">{v}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="mt-2 text-xs text-dim">장비를 누르면 MAC, 제조사, 열린 포트가 나옵니다. 종류는 열린 포트와 MAC으로 짐작한 것이라 틀릴 수 있습니다.</p>
      )}
    </div>
  );
}
