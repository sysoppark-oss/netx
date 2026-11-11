import type { Chart, Light } from "../lib/types";
import { FortressView, HighwayView, RadarView, RouteMap } from "./Viz";

// 결과 그래프. 외부 라이브러리 없이 SVG와 CSS로 그린다.

const LIGHT_COLOR: Record<Light, string> = {
  green: "var(--color-safe)",
  yellow: "var(--color-caution)",
  red: "var(--color-danger)",
  info: "var(--color-accent)",
};

function fmt(v: number): string {
  if (v >= 1000) return v.toFixed(0);
  if (v >= 100) return v.toFixed(0);
  if (v >= 10) return v.toFixed(1);
  return v.toFixed(2);
}

export function ChartView({ chart }: { chart: Chart }) {
  return (
    <figure className="rounded-md border border-line bg-void px-3 py-3">
      <figcaption className="mb-2 text-xs text-dim">
        {chart.title} <span className="opacity-70">({chart.unit})</span>
      </figcaption>
      {chart.kind === "bars" && <Bars chart={chart} />}
      {chart.kind === "line" && <Line chart={chart} />}
      {chart.kind === "gauge" && <Gauge chart={chart} />}
      {chart.kind === "route" && <RouteMap hops={chart.hops} from={chart.from} />}
      {chart.kind === "radar" && <RadarView hosts={chart.hosts} />}
      {chart.kind === "fortress" && <FortressView doors={chart.doors} more={chart.more} />}
      {chart.kind === "highway" && <HighwayView road={chart.road} />}
    </figure>
  );
}

/** 항목별 가로 막대. mtr 홉별 지연, 장비별 응답 시간. */
function Bars({ chart }: { chart: Extract<Chart, { kind: "bars" }> }) {
  const max = Math.max(...chart.bars.map((b) => b.value), 0.001);
  return (
    <ul className="space-y-1">
      {chart.bars.map((b, i) => (
        <li key={i} className="flex items-center gap-2 text-xs">
          <span className="w-56 shrink-0 truncate font-mono text-dim" title={b.label}>
            {b.label}
          </span>
          <span className="relative h-3 flex-1 overflow-hidden rounded-sm bg-raised">
            <span
              className="absolute inset-y-0 left-0 rounded-sm"
              style={{ width: `${Math.max(1, (b.value / max) * 100)}%`, background: LIGHT_COLOR[b.light] }}
            />
          </span>
          <span className="w-16 shrink-0 text-right font-mono">{fmt(b.value)}</span>
          <span className="w-16 shrink-0 text-caution">{b.note}</span>
        </li>
      ))}
    </ul>
  );
}

/** 시간 순서 선 그래프. ping 응답 시간, iperf3 초당 속도. */
function Line({ chart }: { chart: Extract<Chart, { kind: "line" }> }) {
  const W = 600;
  const H = 120;
  const pad = 6;
  const pts = chart.points;
  const max = Math.max(...pts) * 1.1 || 1;
  const min = Math.min(0, ...pts);
  const x = (i: number) => pad + (i / Math.max(1, pts.length - 1)) * (W - pad * 2);
  const y = (v: number) => H - pad - ((v - min) / (max - min)) * (H - pad * 2);
  const path = pts.map((v, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join(" ");
  const avg = pts.reduce((a, b) => a + b, 0) / pts.length;

  return (
    <div>
      <svg viewBox={`0 0 ${W} ${H}`} className="h-28 w-full" preserveAspectRatio="none">
        <line x1={pad} x2={W - pad} y1={y(avg)} y2={y(avg)} stroke="var(--color-line)" strokeDasharray="4 4" />
        <path d={`${path} L${x(pts.length - 1)},${H - pad} L${x(0)},${H - pad} Z`} fill="var(--color-accent)" opacity="0.12" />
        <path d={path} fill="none" stroke="var(--color-accent)" strokeWidth="2" vectorEffect="non-scaling-stroke" />
        {pts.length <= 60 &&
          pts.map((v, i) => <circle key={i} cx={x(i)} cy={y(v)} r="2.5" fill="var(--color-accent)" />)}
      </svg>
      <p className="mt-1 flex justify-between font-mono text-xs text-dim">
        <span>최저 {fmt(Math.min(...pts))}</span>
        <span>평균 {fmt(avg)}</span>
        <span>최고 {fmt(Math.max(...pts))}</span>
      </p>
    </div>
  );
}

/** 계기판. iperf3 전송 속도. */
function Gauge({ chart }: { chart: Extract<Chart, { kind: "gauge" }> }) {
  const ratio = Math.min(1, Math.max(0, chart.value / chart.max));
  const R = 80;
  const cx = 100;
  const cy = 95;
  // 왼쪽(180°)에서 오른쪽(0°)으로 도는 반원
  const end = Math.PI * (1 - ratio);
  const px = cx + R * Math.cos(end);
  const py = cy - R * Math.sin(end);
  const arc = `M${cx - R},${cy} A${R},${R} 0 0 1 ${px.toFixed(1)},${py.toFixed(1)}`;

  return (
    <div className="flex items-center gap-6">
      <svg viewBox="0 0 200 110" className="h-28 w-52 shrink-0">
        <path d={`M${cx - R},${cy} A${R},${R} 0 0 1 ${cx + R},${cy}`} fill="none" stroke="var(--color-raised)" strokeWidth="14" strokeLinecap="round" />
        <path d={arc} fill="none" stroke="var(--color-accent)" strokeWidth="14" strokeLinecap="round" />
        <line x1={cx} y1={cy} x2={cx + (R - 22) * Math.cos(end)} y2={cy - (R - 22) * Math.sin(end)} stroke="var(--color-ink)" strokeWidth="2.5" strokeLinecap="round" />
        <circle cx={cx} cy={cy} r="4" fill="var(--color-ink)" />
        <text x={cx - R} y="108" fontSize="9" fill="var(--color-dim)" textAnchor="middle">0</text>
        <text x={cx + R} y="108" fontSize="9" fill="var(--color-dim)" textAnchor="middle">{chart.max >= 1000 ? `${chart.max / 1000}G` : chart.max}</text>
      </svg>
      <p>
        <span className="font-mono text-3xl font-semibold">{chart.value >= 1000 ? (chart.value / 1000).toFixed(2) : fmt(chart.value)}</span>
        <span className="ml-2 text-sm text-dim">{chart.value >= 1000 ? "Gbps" : chart.unit}</span>
      </p>
    </div>
  );
}
