import { useState } from "react";
import { saveTopology } from "../lib/backend";
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
const BOX_H = 94;
const CELL_H = 120;
const TOP = 210;

/** 상자 안에 들어가게 이름을 줄인다. */
const short = (name: string, max = 19) => (name.length > max ? `${name.slice(0, max - 1)}…` : name);

/** 공유기를 뺀 장비들. 내 PC를 맨 앞에 둔다. */
const devicesOf = (t: Topology) => t.nodes.filter((n) => n.role !== "gateway").sort((a, b) => Number(b.role === "me") - Number(a.role === "me"));

const esc = (v: string) => v.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

/**
 * 파일로 저장할 독립 SVG. 화면용 그림과 달리 테일윈드 클래스 없이 색을 직접 적는다.
 * 스크립트, 이벤트 속성, 링크는 넣지 않는다(백엔드 save_topology가 다시 검사한다).
 */
export function toStandaloneSvg(topology: Topology): string {
  const BG = "#0b1020";
  const PANEL = "#141a2e";
  const LINE = "#2a3350";
  const INK = "#e6e9f2";
  const DIM = "#8b93a7";
  const ACCENT = "#2dd4bf";
  const SANS = "'Malgun Gothic','Segoe UI','Noto Sans KR',sans-serif";
  const MONO = "Consolas,'D2Coding',monospace";
  const gateway = topology.nodes.find((n) => n.role === "gateway") ?? null;
  const devices = devicesOf(topology);
  const rows = Math.max(1, Math.ceil(devices.length / COLS));
  const height = TOP + rows * CELL_H + 34;
  const cx = W / 2;
  const text = (x: number, y: number, body: string, o: { size: number; fill?: string; anchor?: string; mono?: boolean; bold?: boolean }) =>
    `<text x="${x}" y="${y}" font-family="${o.mono ? MONO : SANS}" font-size="${o.size}" fill="${o.fill ?? INK}"${o.anchor ? ` text-anchor="${o.anchor}"` : ""}${o.bold ? ' font-weight="600"' : ""}>${esc(body)}</text>`;
  const line = (x1: number, y1: number, x2: number, y2: number, stroke: string, w: number) => `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${stroke}" stroke-width="${w}"/>`;
  const gwLine = [topology.gateway ?? "주소 모름", gateway?.name ? short(gateway.name) : "", gateway?.vendor ?? ""].filter(Boolean).join(" · ");

  const parts: string[] = [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${height}" width="${W * 2}" height="${height * 2}">`,
    `<rect x="0" y="0" width="${W}" height="${height}" fill="${BG}"/>`,
    `<rect x="${cx - 70}" y="6" width="140" height="40" rx="20" fill="${PANEL}" stroke="${LINE}"/>`,
    text(cx, 31, "☁ 인터넷", { size: 13, anchor: "middle" }),
    line(cx, 46, cx, 76, ACCENT, 2),
    `<rect x="${cx - 110}" y="76" width="220" height="58" rx="8" fill="${PANEL}" stroke="${LINE}" stroke-width="1.5"/>`,
    text(cx - 92, 112, "📡", { size: 22 }),
    text(cx - 58, 100, "공유기(게이트웨이)", { size: 12, bold: true }),
    text(cx - 58, 118, gwLine, { size: 11, fill: DIM, mono: true }),
    line(cx, 134, cx, 170, ACCENT, 2),
    line(CELL_W / 2, 170, W - CELL_W / 2, 170, ACCENT, 2),
    text(W - 8, 162, `내부망 ${topology.cidr}`, { size: 10, fill: DIM, anchor: "end" }),
  ];
  devices.forEach((n, i) => {
    const row = Math.floor(i / COLS);
    const x = (i % COLS) * CELL_W + CELL_W / 2;
    const y = TOP + row * CELL_H;
    const me = n.role === "me";
    parts.push(
      line(x, row === 0 ? 170 : y - CELL_H + BOX_H, x, y, LINE, 1.5),
      `<rect x="${x - CELL_W / 2 + 8}" y="${y}" width="${CELL_W - 16}" height="${BOX_H}" rx="8" fill="${me ? "#12303a" : PANEL}" stroke="${me ? ACCENT : LINE}" stroke-width="1.5"/>`,
      text(x, y + 26, ICON[n.kind] ?? "❔", { size: 20, anchor: "middle" }),
      text(x, y + 46, n.ip, { size: 11, anchor: "middle", mono: true }),
    );
    if (n.name) parts.push(text(x, y + 62, short(n.name), { size: 10, anchor: "middle", mono: true }));
    parts.push(text(x, y + (n.name ? 78 : 62), n.label, { size: 10, fill: me ? ACCENT : DIM, anchor: "middle" }));
  });
  parts.push(text(8, height - 10, `NetX 네트워크 구성도 · ${topology.cidr} · 장비 ${topology.nodes.length}대`, { size: 10, fill: DIM }), "</svg>");
  return parts.join("\n");
}

/** 네트워크 구성도: 인터넷 → 공유기(게이트웨이) → 내부망 → 장비들. 장비를 누르면 자세한 내용이 나온다. */
export function TopologyView({ topology }: { topology: Topology }) {
  const [picked, setPicked] = useState<TopoNode | null>(null);
  const [saved, setSaved] = useState<{ ok: boolean; text: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const gateway = topology.nodes.find((n) => n.role === "gateway") ?? null;
  const devices = devicesOf(topology);

  async function save() {
    setSaving(true);
    try {
      setSaved({ ok: true, text: `저장했습니다: ${await saveTopology(toStandaloneSvg(topology))}` });
    } catch (e) {
      setSaved({ ok: false, text: `저장하지 못했습니다: ${String(e)}` });
    } finally {
      setSaving(false);
    }
  }
  const rows = Math.max(1, Math.ceil(devices.length / COLS));
  const height = TOP + rows * CELL_H + 10;
  const cx = W / 2;

  return (
    <div className="mt-3 rounded-md border border-line bg-void p-3">
      <div className="flex items-baseline justify-between">
        <p className="text-xs font-semibold">네트워크 구성도</p>
        <div className="flex items-center gap-3">
          <p className="font-mono text-xs text-dim">
            {topology.cidr} · 장비 {topology.nodes.length}대
          </p>
          <button type="button" onClick={save} disabled={saving} className="rounded border border-line px-2 py-0.5 text-xs hover:border-accent disabled:opacity-50">
            그림 저장
          </button>
        </div>
      </div>
      {saved && <p className={`mt-1 break-all text-xs ${saved.ok ? "text-dim" : "text-red-400"}`}>{saved.text}</p>}
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
            {gateway?.name ? ` · ${short(gateway.name)}` : ""}
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
              <line x1={x} y1={row === 0 ? 170 : y - CELL_H + BOX_H} x2={x} y2={y} className="stroke-line" strokeWidth={1.5} />
              <rect
                x={x - CELL_W / 2 + 8}
                y={y}
                width={CELL_W - 16}
                height={BOX_H}
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
              {n.name && (
                <text x={x} y={y + 62} textAnchor="middle" className="fill-ink font-mono text-[10px]">
                  {short(n.name)}
                </text>
              )}
              <text x={x} y={y + (n.name ? 78 : 62)} textAnchor="middle" className={`text-[10px] ${me ? "fill-accent" : "fill-dim"}`}>
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
            ["이름", picked.name ?? "알 수 없음"],
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
        <p className="mt-2 text-xs text-dim">장비를 누르면 이름, MAC, 제조사, 열린 포트가 나옵니다. 종류는 열린 포트와 MAC으로 짐작한 것이라 틀릴 수 있습니다.</p>
      )}
    </div>
  );
}
