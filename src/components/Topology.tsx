import { useState } from "react";
import { saveTopology } from "../lib/backend";
import type { TopoLink, TopoNode, Topology } from "../lib/types";

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
const BUS_Y = 170;
/** 가로선에서 그 아래 첫 줄 상자까지 */
const DROP = 40;
/** 앞 묶음의 마지막 상자에서 다음 스위치 가로선까지 */
const SECTION_GAP = 44;
/** 스위치마다 다른 색(화면용과 저장용이 같은 색을 쓴다) */
const SWITCH_COLORS = ["#f59e0b", "#a78bfa", "#38bdf8", "#f472b6"];

/** 상자 안에 들어가게 이름을 줄인다. */
const short = (name: string, max = 19) => (name.length > max ? `${name.slice(0, max - 1)}…` : name);

/** 공유기를 뺀 장비들. 내 PC를 맨 앞에 둔다. */
const devicesOf = (t: Topology) => t.nodes.filter((n) => n.role !== "gateway").sort((a, b) => Number(b.role === "me") - Number(a.role === "me"));

const esc = (v: string) => v.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#39;");

interface Placed {
  node: TopoNode;
  x: number;
  y: number;
  /** 위에서 내려오는 선의 시작 높이 */
  fromY: number;
  /** 물린 스위치의 색. 공유기에 바로 달린 장비는 null */
  color: string | null;
  /** 이 장비가 물린 연결(스위치와 포트). 없으면 null */
  link: TopoLink | null;
}

/** 스위치 한 대와 거기 물린 장비들의 묶음 */
interface Section {
  node: TopoNode;
  color: string;
  busY: number;
  /** 가로선의 왼쪽·오른쪽 끝 */
  x1: number;
  x2: number;
  /** 스위치 상자 아래에서 가로선까지 내려오는 꺾은선 */
  path: [number, number][];
  /** 공유기 쪽으로 올라가는 포트(알 때만) */
  uplink: string | null;
}

interface Layout {
  placed: Placed[];
  sections: Section[];
  height: number;
}

/**
 * 장비 상자와 선의 자리를 정한다. 화면용 그림과 저장용 그림이 함께 쓴다.
 * 맨 위 줄들은 공유기 아래 내부망 선에 달리고, SNMP로 연결을 알아낸 스위치는 아래에 자기 가로선을 따로 갖는다.
 */
export function layoutOf(t: Topology): Layout {
  const devices = devicesOf(t);
  const byIp = new Map(devices.map((n) => [n.ip, n]));
  const gatewayIp = t.nodes.find((n) => n.role === "gateway")?.ip ?? null;
  const links = t.links ?? [];

  // 장비 → 물린 스위치. 서로를 가리켜 고리가 생기는 연결은 버린다.
  const parent = new Map<string, TopoLink>();
  for (const l of links) {
    if (l.device === gatewayIp || l.switch === gatewayIp || !byIp.has(l.device) || !byIp.has(l.switch) || parent.has(l.device)) continue;
    let up: string | undefined = l.switch;
    let loops = false;
    for (let hops = 0; up && hops < 64; hops++) {
      if (up === l.device) loops = true;
      up = parent.get(up)?.switch;
    }
    if (!loops) parent.set(l.device, l);
  }
  /** 공유기에 바로 달린 장비의 포트(공유기가 SNMP에 답했을 때) */
  const onGateway = new Map(links.filter((l) => l.switch === gatewayIp && byIp.has(l.device)).map((l) => [l.device, l]));

  const placed: Placed[] = [];
  const sections: Section[] = [];
  const place = (nodes: TopoNode[], busY: number, color: string | null) => {
    nodes.forEach((node, i) => {
      const row = Math.floor(i / COLS);
      const y = busY + DROP + row * CELL_H;
      placed.push({ node, x: (i % COLS) * CELL_W + CELL_W / 2, y, fromY: row === 0 ? busY : y - CELL_H + BOX_H, color, link: parent.get(node.ip) ?? onGateway.get(node.ip) ?? null });
    });
    return busY + DROP + Math.max(1, Math.ceil(nodes.length / COLS)) * CELL_H - (CELL_H - BOX_H);
  };

  let bottom = place(
    devices.filter((n) => !parent.has(n.ip)),
    BUS_Y,
    null,
  );
  // 놓인 순서대로 스위치를 찾아 아래에 묶음을 만든다(스위치 아래의 스위치도 차례로 나온다).
  for (let i = 0; i < placed.length; i++) {
    const sw = placed[i];
    const children = devices.filter((n) => parent.get(n.ip)?.switch === sw.node.ip);
    if (children.length === 0) continue;
    const k = sections.length;
    const color = SWITCH_COLORS[k % SWITCH_COLORS.length];
    const busY = bottom + SECTION_GAP;
    // 스위치 상자 아래에서 왼쪽 칸 사이 틈으로 나와, 그 틈을 따라 가로선까지 내려간다.
    const col = Math.round((sw.x - CELL_W / 2) / CELL_W);
    const gx = Math.min(W - 3, Math.max(3, col * CELL_W - 4 + (k % 3) * 4));
    const turnY = sw.y + BOX_H + 10;
    const lastX = (Math.min(children.length, COLS) - 1) * CELL_W + CELL_W / 2;
    sections.push({
      node: sw.node,
      color,
      busY,
      x1: Math.min(gx, CELL_W / 2),
      x2: Math.max(gx, lastX),
      path: [
        [sw.x, sw.y + BOX_H],
        [sw.x, turnY],
        [gx, turnY],
        [gx, busY],
      ],
      uplink: links.find((l) => l.switch === sw.node.ip && l.device === gatewayIp)?.port ?? null,
    });
    bottom = place(children, busY, color);
  }
  return { placed, sections, height: bottom };
}

const portText = (l: TopoLink) => `포트 ${short(l.port, 12)}`;
const sectionTitle = (s: Section) => `🔀 ${s.node.name ? short(s.node.name) : s.node.ip} 에 물린 장비${s.uplink ? ` · 위로 가는 포트 ${short(s.uplink, 12)}` : ""}`;

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
  const { placed, sections, height: bottom } = layoutOf(topology);
  const height = bottom + 34;
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
    line(cx, 134, cx, BUS_Y, ACCENT, 2),
    line(CELL_W / 2, BUS_Y, W - CELL_W / 2, BUS_Y, ACCENT, 2),
    text(W - 8, BUS_Y - 8, `내부망 ${topology.cidr}`, { size: 10, fill: DIM, anchor: "end" }),
  ];
  for (const s of sections) {
    parts.push(
      `<polyline points="${s.path.map((p) => p.join(",")).join(" ")}" fill="none" stroke="${s.color}" stroke-width="1.5"/>`,
      line(s.x1, s.busY, s.x2, s.busY, s.color, 2),
      text(W - 8, s.busY - 8, sectionTitle(s), { size: 10, fill: s.color, anchor: "end" }),
    );
  }
  for (const p of placed) {
    const n = p.node;
    const me = n.role === "me";
    parts.push(
      line(p.x, p.fromY, p.x, p.y, p.color ?? LINE, 1.5),
      `<rect x="${p.x - CELL_W / 2 + 8}" y="${p.y}" width="${CELL_W - 16}" height="${BOX_H}" rx="8" fill="${me ? "#12303a" : PANEL}" stroke="${me ? ACCENT : LINE}" stroke-width="1.5"/>`,
      text(p.x, p.y + 26, ICON[n.kind] ?? "❔", { size: 20, anchor: "middle" }),
      text(p.x, p.y + 46, n.ip, { size: 11, anchor: "middle", mono: true }),
    );
    if (p.link) {
      parts.push(text(p.x + 5, p.y - 7, portText(p.link), { size: 9, fill: p.color ?? DIM, mono: true }));
      if (p.link.shared > 1) parts.push(text(p.x - 5, p.y - 7, `함께 ${p.link.shared}대`, { size: 9, fill: DIM, anchor: "end" }));
    }
    if (n.name) parts.push(text(p.x, p.y + 62, short(n.name), { size: 10, anchor: "middle", mono: true }));
    parts.push(text(p.x, p.y + (n.name ? 78 : 62), n.label, { size: 10, fill: me ? ACCENT : DIM, anchor: "middle" }));
  }
  parts.push(text(8, height - 10, `NetX 네트워크 구성도 · ${topology.cidr} · 장비 ${topology.nodes.length}대${sections.length > 0 ? ` · 스위치 연결 ${topology.links.length}개(SNMP)` : ""}`, { size: 10, fill: DIM }), "</svg>");
  return parts.join("\n");
}

/** 네트워크 구성도: 인터넷 → 공유기(게이트웨이) → 내부망 → 장비들. 스위치에 물린 장비는 그 스위치의 가로선 아래에 놓인다. 장비를 누르면 자세한 내용이 나온다. */
export function TopologyView({ topology }: { topology: Topology }) {
  const [picked, setPicked] = useState<TopoNode | null>(null);
  const [saved, setSaved] = useState<{ ok: boolean; text: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const gateway = topology.nodes.find((n) => n.role === "gateway") ?? null;
  const { placed, sections, height: bottom } = layoutOf(topology);
  const links = topology.links ?? [];

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
  const height = bottom + 10;
  const cx = W / 2;
  const nameOf = (ip: string) => topology.nodes.find((n) => n.ip === ip)?.name ?? ip;
  const pickedLink = picked ? links.find((l) => l.device === picked.ip) : undefined;
  const pickedPorts = picked ? links.filter((l) => l.switch === picked.ip) : [];

  return (
    <div className="mt-3 rounded-md border border-line bg-void p-3">
      <div className="flex items-baseline justify-between">
        <p className="text-xs font-semibold">네트워크 구성도</p>
        <div className="flex items-center gap-3">
          <p className="font-mono text-xs text-dim">
            {topology.cidr} · 장비 {topology.nodes.length}대{links.length > 0 ? ` · 스위치 연결 ${links.length}개` : ""}
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
        <line x1={cx} y1={134} x2={cx} y2={BUS_Y} className="stroke-accent" strokeWidth={2} />

        {/* 내부망 선 */}
        <line x1={CELL_W / 2} y1={BUS_Y} x2={W - CELL_W / 2} y2={BUS_Y} className="stroke-accent" strokeWidth={2} />
        <text x={W - 8} y={BUS_Y - 8} textAnchor="end" className="fill-dim text-[10px]">
          내부망 {topology.cidr}
        </text>

        {/* 스위치마다: 상자에서 내려오는 선, 가로선, 이름 */}
        {sections.map((s) => (
          <g key={s.node.ip}>
            <polyline points={s.path.map((p) => p.join(",")).join(" ")} fill="none" stroke={s.color} strokeWidth={1.5} />
            <line x1={s.x1} y1={s.busY} x2={s.x2} y2={s.busY} stroke={s.color} strokeWidth={2} />
            <text x={W - 8} y={s.busY - 8} textAnchor="end" fill={s.color} className="text-[10px]">
              {sectionTitle(s)}
            </text>
          </g>
        ))}

        {placed.map((p) => {
          const n = p.node;
          const me = n.role === "me";
          return (
            <g key={n.ip} onClick={() => setPicked(n)} className="cursor-pointer">
              {/* 첫 줄은 가로선에서, 다음 줄은 윗줄 장비 아래에서 내려온다 */}
              {p.color ? <line x1={p.x} y1={p.fromY} x2={p.x} y2={p.y} stroke={p.color} strokeWidth={1.5} /> : <line x1={p.x} y1={p.fromY} x2={p.x} y2={p.y} className="stroke-line" strokeWidth={1.5} />}
              {p.link && (
                <>
                  <text x={p.x + 5} y={p.y - 7} fill={p.color ?? undefined} className={`font-mono text-[9px] ${p.color ? "" : "fill-dim"}`}>
                    {portText(p.link)}
                  </text>
                  {p.link.shared > 1 && (
                    <text x={p.x - 5} y={p.y - 7} textAnchor="end" className="fill-dim text-[9px]">
                      함께 {p.link.shared}대
                    </text>
                  )}
                </>
              )}
              <rect
                x={p.x - CELL_W / 2 + 8}
                y={p.y}
                width={CELL_W - 16}
                height={BOX_H}
                rx={8}
                className={`${me ? "fill-accent/10" : "fill-panel"} ${picked === n ? "stroke-accent" : me ? "stroke-accent/60" : "stroke-line"}`}
                strokeWidth={1.5}
              />
              <text x={p.x} y={p.y + 26} textAnchor="middle" className="text-[20px]">
                {ICON[n.kind] ?? "❔"}
              </text>
              <text x={p.x} y={p.y + 46} textAnchor="middle" className="fill-ink font-mono text-[11px]">
                {n.ip}
              </text>
              {n.name && (
                <text x={p.x} y={p.y + 62} textAnchor="middle" className="fill-ink font-mono text-[10px]">
                  {short(n.name)}
                </text>
              )}
              <text x={p.x} y={p.y + (n.name ? 78 : 62)} textAnchor="middle" className={`text-[10px] ${me ? "fill-accent" : "fill-dim"}`}>
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
            ...(pickedLink
              ? [
                  [
                    "연결",
                    `${nameOf(pickedLink.switch)}의 포트 ${pickedLink.port}` +
                      (pickedLink.shared > 1 ? ` · 이 포트에 장비 ${pickedLink.shared}대(사이에 다른 스위치나 AP가 있음)` : "") +
                      (pickedLink.via === "lldp" ? " · 장비가 알려 줌(LLDP)" : " · MAC 표로 짐작"),
                  ],
                ]
              : []),
            ...(pickedPorts.length > 0 ? [["물린 장비", pickedPorts.map((l) => `${l.port}: ${nameOf(l.device)}`).join(", ")]] : []),
            ...(picked.descr ? [["장비 설명", picked.descr]] : []),
            ...(picked.ping_ms != null ? [["핑", `${picked.ping_ms < 10 ? picked.ping_ms.toFixed(2) : picked.ping_ms.toFixed(1)}ms`]] : []),
            ...(picked.notes ?? []).map((n) => {
              const at = n.indexOf(": ");
              return [n.slice(0, at), n.slice(at + 2)];
            }),
          ].map(([k, v]) => (
            <div key={k} className="flex gap-1.5">
              <dt className="shrink-0 text-dim">{k}</dt>
              <dd className="font-mono">{v}</dd>
            </div>
          ))}
        </dl>
      ) : (
        <p className="mt-2 text-xs text-dim">
          장비를 누르면 이름, MAC, 제조사, 열린 포트가 나옵니다. 종류는 열린 포트와 MAC으로 짐작한 것이라 틀릴 수 있습니다.
          {links.length === 0 && " 스위치·AP의 SNMP 커뮤니티 문자열을 설정에 넣으면 어느 포트에 물렸는지도 그립니다."}
        </p>
      )}
    </div>
  );
}
