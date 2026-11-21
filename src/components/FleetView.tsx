// 거점 관리 대시보드(관제 PC)와 예약 수집 동의 칸(거점 PC).
// 프로그램 이름, 만든 곳, 운영체제 이름은 거점이 보낸 자료다. 글자로만 그린다.
import { SiteDetail } from "./SiteDetail";
import { Board, type BoardItem } from "./Board";
import { MapTiles, MapZone, useSiteMap } from "./SiteMap";
import { Zone } from "./Zone";
import { BTN, FIELD, hidden } from "../lib/ui";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  EVERY_CHOICES,
  collectFleet,
  collectInventory,
  collectInventoryPreview,
  collectInventoryRun,
  collectInventorySet,
  collectPlan,
  collectPlanNow,
  collectPlanSet,
  gbText,
  timeText,
  type FleetSite,
  type InvItem,
  type InvPreview,
  type PlanRow,
} from "../lib/collect";

/** 디스크 부족: 1GB 넘는 볼륨에서 남은 비율이 10% 아래이고 남은 양이 20GB 아래일 때. 작은 복구·붙임 볼륨은 보지 않는다. */
const GB = 1024 * 1024 * 1024;
function diskLow(d: { total: number; free: number }): boolean {
  return d.total >= GB && d.free / d.total < 0.1 && d.free < 20 * GB;
}

function uptimeText(secs: number): string {
  if (secs < 3600) return `${Math.floor(secs / 60)}분`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}시간`;
  return `${Math.floor(secs / 86400)}일`;
}

/** 칸으로 나뉜 계기 막대(24칸). 쓴 만큼 불이 켜진다. */
const BAR_CELLS = 24;
function Bar({ used, tone }: { used: number; tone: string }) {
  const lit = Math.min(BAR_CELLS, Math.max(used > 0 ? 1 : 0, Math.round(used * BAR_CELLS)));
  return (
    <span className="flex h-2.5 w-full gap-px" aria-hidden>
      {Array.from({ length: BAR_CELLS }, (_, i) => (
        <span key={i} className={`h-full flex-1 rounded-[1px] ${i < lit ? `bg-current ${tone}` : "bg-raised"}`} />
      ))}
    </span>
  );
}

/** 최근 속도의 추이(왼쪽이 옛날). 내려받기 선 하나. */
function Trend({ values }: { values: number[] }) {
  if (values.length < 2) return null;
  const max = Math.max(...values, 1);
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * 100},${22 - (v / max) * 20}`).join(" ");
  return (
    <svg viewBox="0 0 100 24" preserveAspectRatio="none" className="h-8 w-32 shrink-0 text-accent" aria-hidden>
      <polygon points={`0,24 ${pts} 100,24`} fill="currentColor" opacity="0.14" />
      <polyline points={pts} fill="none" stroke="currentColor" strokeWidth="1.5" vectorEffect="non-scaling-stroke" />
    </svg>
  );
}

function median(values: number[]): number {
  const s = [...values].sort((a, b) => a - b);
  return s.length ? s[Math.floor(s.length / 2)] : 0;
}

/** 거점 하나에서 눈여겨볼 것 */
function signs(s: FleetSite) {
  const speed = s.speeds[0]?.[1];
  const downs = s.speeds.map((x) => x[1].down_mbps);
  const usual = median(downs.slice(1));
  // 지난 측정이 넷 넘게 쌓였을 때만 견준다: 이번 내려받기 속도가 그동안 중앙값의 절반 아래면 느려진 것으로 본다.
  const slow = !!speed && downs.length >= 5 && speed.down_mbps < usual / 2;
  const lowDisks = s.system?.disks.filter(diskLow) ?? [];
  const moved = s.added.length + s.removed.length + s.changed.length;
  return { speed, downs, usual, slow, lowDisks, low: lowDisks.length > 0, moved };
}

type Alert = { id: string; site: string; tone: string; tag: string; text: string; lines?: string[] };

/** 알림 구역: 받기 멈춤, 소식 없음, 디스크 부족, 속도 느려짐, 프로그램 바뀜. 색과 함께 글자로 적는다. */
function Alerts({ sites, onPick }: { sites: FleetSite[]; onPick: (id: string) => void }) {
  const rows: Alert[] = [];
  for (const s of sites) {
    const id = s.pc.id;
    const site = s.pc.name || id.slice(0, 8);
    const g = signs(s);
    if (s.pc.state === "stopped") rows.push({ id, site, tone: "text-danger", tag: "받기 멈춤", text: "이 거점에서 받기를 멈춰 두었습니다." });
    else if (s.pc.quiet && !s.pc.own) rows.push({ id, site, tone: "text-caution", tag: "소식 없음", text: "한동안 접속이 없습니다." });
    if (g.low) rows.push({ id, site, tone: "text-danger", tag: "디스크 부족", text: g.lowDisks.map((d) => `${d.name} 남음 ${gbText(d.free)}`).join(", ") });
    if (g.slow && g.speed) rows.push({ id, site, tone: "text-caution", tag: "속도 느려짐", text: `내려받기 ${g.speed.down_mbps} Mbps (평소 ${g.usual})` });
    if (g.moved > 0) {
      const lines = [...s.added.map((p) => `+ 새로 깔림: ${p.name} ${p.version}`), ...s.removed.map((p) => `− 없어짐: ${p.name} ${p.version}`), ...s.changed.map(([name, from, to]) => `↻ 판 바뀜: ${name} ${from} → ${to}`)];
      rows.push({ id, site, tone: "text-accent", tag: "프로그램 바뀜", text: `${timeText(s.prev_at)} → ${timeText(s.software_at)} · +${s.added.length} −${s.removed.length} ↻${s.changed.length}`, lines });
    }
  }
  const order = ["text-danger", "text-caution", "text-accent"];
  rows.sort((a, b) => order.indexOf(a.tone) - order.indexOf(b.tone));
  return (
    <Zone className="min-h-0 flex-1" code="02" title="알림" right={<span className={`font-mono text-xs font-semibold ${rows.length ? "text-caution" : "text-safe"}`}>{rows.length}건</span>} flush>
      {rows.length === 0 ? (
        <div className="p-3 text-xs">
          <p className="font-semibold text-safe">✓ 지금 알릴 것이 없습니다.</p>
          <p className="mt-1 text-dim">받기 멈춤, 소식 없음, 디스크 부족, 속도 느려짐, 프로그램 바뀜이 여기에 뜹니다.</p>
        </div>
      ) : (
        <ul className="max-h-[26rem] divide-y divide-accent/15 overflow-auto text-xs">
          {rows.map((r, i) => (
            <li key={i}>
              <button onClick={() => onPick(r.id)} className="block w-full px-3 py-2 text-left hover:bg-raised" title="누르면 거점 상세가 열립니다">
                <span className="flex items-center gap-2">
                  <span className={`shrink-0 rounded-sm border border-current px-1.5 py-px text-[11px] font-semibold ${r.tone}`}>{r.tag}</span>
                  <span className="truncate font-mono font-semibold">{r.site}</span>
                </span>
                <span className="mt-1 block text-dim">{r.text}</span>
                {r.lines && (
                  <span className="mt-1 block space-y-0.5 text-[11px]">
                    {r.lines.slice(0, 5).map((l, k) => (
                      <span key={k} className="block truncate">
                        {l}
                      </span>
                    ))}
                    {r.lines.length > 5 && <span className="block text-dim">외 {r.lines.length - 5}건</span>}
                  </span>
                )}
              </button>
            </li>
          ))}
        </ul>
      )}
    </Zone>
  );
}

function SiteCard({ s, onPick, picked }: { s: FleetSite; onPick: () => void; picked: boolean }) {
  const pc = s.pc;
  const state = pc.state === "stopped" ? "받기 멈춤" : pc.own ? "이 PC" : pc.quiet ? "미확인" : "연결됨";
  const tone = pc.state !== "active" ? "text-danger" : pc.quiet ? "text-caution" : "text-safe";
  const sys = s.system;
  const { moved, low, speed, downs, slow } = signs(s);
  return (
    <button onClick={onPick} className={`hud p-3 text-left text-xs hover:brightness-125 ${picked ? "ring-1 ring-accent" : ""}`}>
      <div className="mb-2 flex items-center gap-2 border-b border-accent/20 pb-1.5">
        <span className={`h-2 w-2 shrink-0 rounded-full bg-current ${tone} ${pc.state === "active" && !pc.quiet ? "animate-pulse" : ""}`} aria-hidden />
        <span className="font-mono text-sm font-semibold tracking-wide">{pc.name || "이름 없음"}</span>
        <span className={`rounded-sm border border-current px-1.5 py-px text-[11px] font-semibold ${tone}`}>{state}</span>
        <span className="flex-1" />
        {slow && <span className="rounded-sm border border-caution px-1.5 py-px text-[11px] text-caution">속도 느려짐</span>}
        {low && <span className="rounded-sm border border-danger px-1.5 py-px text-[11px] text-danger">디스크 부족</span>}
      </div>
      {speed && (
        <div className="mb-2 flex items-end gap-4" title={`${speed.server}와 잰 인터넷 속도 · ${timeText(s.speeds[0][0])} · 최근 ${s.speeds.length}번`}>
          <div>
            <div className="text-[10px] tracking-[0.18em] text-dim">내려받기</div>
            <div className={`font-mono text-lg leading-tight font-semibold ${slow ? "text-caution" : "text-accent"}`}>
              ↓ {speed.down_mbps}
              <span className="ml-1 text-[10px] font-normal text-dim">Mbps</span>
            </div>
          </div>
          <div>
            <div className="text-[10px] tracking-[0.18em] text-dim">올리기</div>
            <div className="font-mono text-lg leading-tight font-semibold">
              ↑ {speed.up_mbps}
              <span className="ml-1 text-[10px] font-normal text-dim">Mbps</span>
            </div>
          </div>
          <div>
            <div className="text-[10px] tracking-[0.18em] text-dim">지연</div>
            <div className="font-mono text-lg leading-tight font-semibold">
              {speed.latency_ms}
              <span className="ml-1 text-[10px] font-normal text-dim">ms</span>
            </div>
          </div>
          <span className="flex-1" />
          <Trend values={[...downs].reverse()} />
        </div>
      )}
      {!sys && !speed && s.software_at === 0 && <p className="text-dim">아직 수집된 것이 없습니다. {pc.own ? '이 PC의 정보도 보려면 "보내기" 탭의 예약 수집을 켜세요.' : '이 거점의 NetX에서 "보내기" 탭의 예약 수집을 켜야 합니다.'}</p>}
      {sys && (
        <div className="space-y-1">
          <p>
            {sys.os || "운영체제 모름"} <span className="text-dim">· NetX {sys.netx}</span>
          </p>
          <p className="text-dim">
            {sys.cpu ? `${sys.cpu} · ` : ""}
            {sys.cores}코어 · 켜진 지 {uptimeText(sys.uptime_secs)}
          </p>
          <div className="flex items-center gap-2">
            <span className="w-14 shrink-0 text-dim">메모리</span>
            <Bar used={sys.mem_total ? sys.mem_used / sys.mem_total : 0} tone="text-accent" />
            <span className="w-28 shrink-0 text-right font-mono">
              {gbText(sys.mem_used)} / {gbText(sys.mem_total)}
            </span>
          </div>
          {sys.disks.map((d) => {
            const used = d.total ? 1 - d.free / d.total : 0;
            return (
              <div key={d.name} className="flex items-center gap-2">
                <span className="w-14 shrink-0 truncate text-dim">{d.name}</span>
                <Bar used={used} tone={diskLow(d) ? "text-danger" : "text-safe"} />
                <span className="w-28 shrink-0 text-right font-mono">남음 {gbText(d.free)}</span>
              </div>
            );
          })}
        </div>
      )}
      {s.software_at > 0 && (
        <p className="mt-1.5">
          프로그램 {s.programs.length}개{s.cut && " (목록이 길어 뒤가 잘림)"}
          {s.prev_at > 0 && (moved > 0 ? <span className="text-caution"> · 바뀜 +{s.added.length} −{s.removed.length} ↻{s.changed.length}</span> : <span className="text-dim"> · 바뀐 것 없음</span>)}
        </p>
      )}
      <p className="mt-1.5 text-[11px] text-dim">
        {speed && `속도 ${timeText(s.speeds[0][0])}${s.system_at > 0 || s.software_at > 0 ? " · " : ""}`}
        {s.system_at > 0 && `시스템 ${timeText(s.system_at)}`}
        {s.system_at > 0 && s.software_at > 0 && " · "}
        {s.software_at > 0 && `프로그램 ${timeText(s.software_at)}`}
      </p>
    </button>
  );
}

/** 관제 PC: 거점마다 무엇을 얼마나 자주 수집할지 정한다. 거점이 다음에 접속할 때 받아 가고, 거점에서 승인한 항목만 돈다. */
function PlanTable({ onError }: { onError: (m: string) => void }) {
  const [rows, setRows] = useState<PlanRow[]>([]);
  const load = useCallback(() => {
    if (hidden()) return;
    collectPlan()
      .then(setRows)
      .catch((e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const t = window.setInterval(load, 15_000);
    return () => window.clearInterval(t);
  }, [load]);
  if (rows.length === 0)
    return (
      <Zone className="min-h-0 flex-1" code="04" title="수집 예약">
        <p className="text-xs text-dim">짝 맺은 거점이 생기면 여기서 거점마다 수집할 항목과 주기를 고릅니다.</p>
      </Zone>
    );
  const set = (id: string, what: string, value: string) =>
    collectPlanSet(id, what, value === "" ? null : Number(value))
      .then(load)
      .catch((e) => onError(String(e)));
  const runNow = (id: string, what: string) =>
    collectPlanNow(id, what)
      .then(load)
      .catch((e) => onError(String(e)));
  return (
    <Zone className="min-h-0 flex-1" code="04" title="수집 예약" note="거점마다 항목과 주기">
      <p className="mb-2 text-xs text-dim">거점마다 수집할 항목과 주기를 고릅니다. [바로 수집]은 주기를 기다리지 않고 한 번 수집합니다. 거점이 다음에 접속할 때 받아 가며, 그 거점 PC에서 사람이 승인한 항목만 수집됩니다. 여기서 거점에 명령을 보내지는 않습니다.</p>
      <div className="overflow-x-auto rounded border border-line">
      <table className="w-full min-w-[44rem] border-collapse text-xs">
        <thead className="text-left text-dim">
          <tr>
            <th scope="col" className="px-2 py-1 font-normal">거점</th>
            {rows[0].cells.map((c) => (
              <th key={c.what} className="px-2 py-1 font-normal">
                {c.title}
              </th>
            ))}
            <th scope="col" className="px-2 py-1 font-normal">전달</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.pc.id} className="border-t border-line">
              <td className="px-2 py-1 font-semibold">{r.pc.name || r.pc.id.slice(0, 8)}</td>
              {r.cells.map((c) => (
                <td key={c.what} className="px-2 py-1">
                  <select aria-label={`${r.pc.name || "거점"} — ${c.title} 주기`} value={c.every ?? ""} onChange={(e) => set(r.pc.id, c.what, e.target.value)} className={FIELD}>
                    <option value="">요청 안 함</option>
                    {EVERY_CHOICES.filter((o) => o.minutes >= c.floor).map((o) => (
                      <option key={o.minutes} value={o.minutes}>
                        {o.label}
                      </option>
                    ))}
                  </select>{" "}
                  {c.approved ? <span className="text-safe">거점 승인됨</span> : c.every ? <span className="text-caution">거점 승인 기다림</span> : null}
                  <div className="mt-1 flex flex-wrap items-center gap-1.5">
                    <button
                      onClick={() => runNow(r.pc.id, c.what)}
                      disabled={!c.approved}
                      className={BTN}
                      title={c.approved ? "거점이 다음에 접속할 때(보통 20초 안) 한 번 바로 수집합니다. 거점의 NetX가 켜져 있어야 합니다." : "거점 PC에서 이 항목을 켜야 수집할 수 있습니다."}
                    >
                      바로 수집
                    </button>
                    <span className="text-[11px] text-dim">
                      {c.now > c.last ? `요청함 ${timeText(c.now)} — 거점을 기다리는 중` : c.last ? `마지막 수집 ${timeText(c.last)}` : "아직 받은 것 없음"}
                    </span>
                  </div>
                </td>
              ))}
              <td className="px-2 py-1 text-dim">{r.asked === 0 ? "거점이 아직 물어 오지 않음" : r.delivered ? `받아 감 (${timeText(r.asked)})` : "다음 접속 때 받아 감"}</td>
            </tr>
          ))}
        </tbody>
      </table>
      </div>
    </Zone>
  );
}

/** 관제 PC: 거점 카드, 바뀐 것, 프로그램 찾기 */
export function FleetDashboard({ onError }: { onError: (m: string) => void }) {
  const [detail, setDetail] = useState<string | null>(null);
  const [sites, setSites] = useState<FleetSite[] | null>(null);
  const [pick, setPick] = useState<string | null>(null);
  const [find, setFind] = useState("");
  const map = useSiteMap(onError);

  const load = useCallback(() => {
    if (hidden()) return;
    collectFleet()
      .then(setSites)
      .catch((e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const t = window.setInterval(load, 30_000);
    return () => window.clearInterval(t);
  }, [load]);

  const word = find.trim().toLowerCase();
  const hits = useMemo(() => {
    if (!sites) return [];
    const from = pick ? sites.filter((s) => s.pc.id === pick) : sites;
    const rows = from.flatMap((s) => s.programs.filter((p) => !word || p.name.toLowerCase().includes(word) || p.publisher.toLowerCase().includes(word)).map((p) => ({ site: s.pc.name || s.pc.id.slice(0, 8), p })));
    return rows.sort((a, b) => a.p.name.localeCompare(b.p.name) || a.site.localeCompare(b.site));
  }, [sites, pick, word]);
  // 같은 프로그램이 거점마다 판이 다르면 표시한다.
  const mixed = useMemo(() => {
    const seen = new Map<string, Set<string>>();
    for (const s of sites ?? []) for (const p of s.programs) seen.set(p.name, (seen.get(p.name) ?? new Set()).add(p.version));
    return new Set([...seen].filter(([, v]) => v.size > 1).map(([k]) => k));
  }, [sites]);

  if (!sites) return <p className="text-sm text-dim">읽는 중…</p>;
  if (sites.length === 0) return <p className="rounded border border-line px-3 py-8 text-center text-sm text-dim">아직 짝 맺은 거점이 없습니다. "이 PC를 관제 PC로" 탭에서 켜고 [새 PC 짝 맺기]를 누르세요.</p>;
  const shown = hits.slice(0, 300);

  const items: BoardItem[] = [
    // 지도 자료가 아직 없어도 자리는 늘 둔다(구역 목록이 바뀌면 저장된 배치와 어긋난다).
    { id: "stats", title: "숫자판", span: 12, min: 6, node: map ? <MapTiles map={map} /> : <p className="text-xs text-dim">읽는 중…</p> },
    {
      id: "map",
      title: "연결 지도",
      span: 8,
      min: 4,
      node: map ? (
        <MapZone className="min-h-0 flex-1" map={map} onPick={setDetail} />
      ) : (
        <Zone className="min-h-0 flex-1" code="01" title="연결 지도">
          <p className="text-xs text-dim">읽는 중…</p>
        </Zone>
      ),
    },
    { id: "alerts", title: "알림", span: 4, min: 3, node: <Alerts sites={sites} onPick={setDetail} /> },
    {
      id: "sites",
      title: "거점 상태",
      span: 12,
      min: 4,
      node: (
        <Zone className="min-h-0 flex-1" code="03" title="거점 상태" note="시스템 · 속도 · 카드를 누르면 프로그램 현황을 그 거점만으로 좁힙니다" bare>
          <div className="grid gap-3 p-px [grid-template-columns:repeat(auto-fill,minmax(22rem,1fr))]">
            {sites.map((s) => (
              <SiteCard key={s.pc.id} s={s} picked={pick === s.pc.id} onPick={() => setPick(pick === s.pc.id ? null : s.pc.id)} />
            ))}
          </div>
        </Zone>
      ),
    },
    { id: "plan", title: "수집 예약", span: 12, min: 6, node: <PlanTable onError={onError} /> },
    {
      id: "programs",
      title: "프로그램 현황",
      span: 12,
      min: 4,
      node: (
      <Zone
        className="min-h-0 flex-1"
        code="05"
        title="프로그램 현황"
        note={`${pick ? `${sites.find((s) => s.pc.id === pick)?.pc.name ?? ""}만` : "모든 거점"} · ${hits.length}개`}
        right={
          <span className="flex flex-wrap items-center gap-2 text-xs">
            {pick && (
              <button onClick={() => setPick(null)} className={BTN}>
                모든 거점 보기
              </button>
            )}
            <input aria-label="프로그램 이름이나 만든 곳으로 찾기" value={find} onChange={(e) => setFind(e.target.value)} placeholder="프로그램 이름이나 만든 곳" className={`${FIELD} w-56`} />
          </span>
        }
      >
        {hits.length === 0 ? (
          <p className="text-xs text-dim">{sites.some((s) => s.software_at > 0) ? "맞는 프로그램이 없습니다." : "아직 프로그램 목록을 보낸 거점이 없습니다."}</p>
        ) : (
          <div className="max-h-80 overflow-auto rounded border border-line">
            <table className="w-full border-collapse text-xs">
              <thead className="sticky top-0 bg-panel text-left text-dim">
                <tr>
                  <th scope="col" className="px-2 py-1 font-normal">프로그램</th>
                  <th scope="col" className="px-2 py-1 font-normal">판</th>
                  <th scope="col" className="px-2 py-1 font-normal">만든 곳</th>
                  <th scope="col" className="px-2 py-1 font-normal">설치한 날</th>
                  <th scope="col" className="px-2 py-1 font-normal">거점</th>
                </tr>
              </thead>
              <tbody>
                {shown.map(({ site, p }, i) => (
                  <tr key={i} className="border-t border-line">
                    <td className="px-2 py-1">{p.name}</td>
                    <td className="px-2 py-1 whitespace-nowrap">
                      {p.version}
                      {mixed.has(p.name) && (
                        <span className="text-caution" title="거점마다 판이 다릅니다">
                          {" "}
                          ≠
                        </span>
                      )}
                    </td>
                    <td className="px-2 py-1 text-dim">{p.publisher}</td>
                    <td className="px-2 py-1 whitespace-nowrap text-dim">{p.installed.length === 8 ? `${p.installed.slice(0, 4)}-${p.installed.slice(4, 6)}-${p.installed.slice(6)}` : ""}</td>
                    <td className="px-2 py-1 whitespace-nowrap">{site}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            {hits.length > shown.length && <p className="px-2 py-1 text-[11px] text-dim">처음 300개만 보입니다. 이름을 적어 좁혀 보세요.</p>}
          </div>
        )}
      </Zone>
      ),
    },
  ];

  return (
    <div>
      {detail && <SiteDetail id={detail} onClose={() => setDetail(null)} />}
      <Board storageKey="netx.dash.layout" items={items} />
    </div>
  );
}

/** 거점 PC: 예약 수집을 항목마다 켜고 끈다. 켜기 전에 지금 나갈 내용을 보여 준다. */
export function InventoryConsent({ ready, onError, onRan }: { ready: boolean; onError: (m: string) => void; onRan?: () => void }) {
  const [items, setItems] = useState<InvItem[]>([]);
  const [preview, setPreview] = useState<{ what: string; data: InvPreview } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    collectInventory()
      .then(setItems)
      .catch((e) => onError(String(e)));
  }, [onError]);

  const set = (it: InvItem, on: boolean, every: number) => {
    setBusy(true);
    collectInventorySet(it.what, on, every)
      .then(setItems)
      .catch((e) => onError(String(e)))
      .finally(() => setBusy(false));
  };
  const [running, setRunning] = useState("");
  const runNow = (it: InvItem) => {
    setBusy(true);
    setRunning(it.what);
    collectInventoryRun(it.what)
      .then((now) => {
        setItems(now);
        onRan?.();
      })
      .catch((e) => onError(String(e)))
      .finally(() => {
        setBusy(false);
        setRunning("");
      });
  };
  const show = (what: string) => {
    if (preview?.what === what) return setPreview(null);
    collectInventoryPreview(what)
      .then((data) => setPreview({ what, data }))
      .catch((e) => onError(String(e)));
  };

  return (
    <section>
      <h2 className="mb-1 text-xs font-semibold text-dim">예약 수집 (기본 꺼짐)</h2>
      <p className="mb-1.5 text-xs text-dim">
        켠 항목만, 정한 주기로 이 PC의 정보를 읽어 관제 PC로 보냅니다. NetX가 켜져 있을 때만 돌고 명령은 실행하지 않습니다. 네트워크 속도는 잴 때마다 측정 서버와 15~50MB를 주고받습니다. 제품 키, 계정, 실행 중인 프로그램, 파일 목록은 읽지 않습니다. 프로그램 이름은 가리지 않고 나갑니다.
        {!ready && " 지금은 보내기가 꺼져 있어 켜 두어도 수집하지 않습니다."}
      </p>
      <ul className="space-y-1">
        {items.map((it) => (
          <li key={it.what} className="rounded border border-line px-2 py-1.5 text-xs">
            <div className="flex flex-wrap items-center gap-2">
              <label className="flex items-center gap-1.5">
                <input type="checkbox" checked={it.on} disabled={busy} onChange={(e) => set(it, e.target.checked, it.every)} />
                <span className="font-semibold">{it.title}</span>
              </label>
              <select aria-label={`${it.title} 주기`} value={it.every} disabled={busy} onChange={(e) => set(it, it.on, Number(e.target.value))} className={FIELD}>
                {EVERY_CHOICES.filter((c) => c.minutes >= it.floor).map((c) => (
                  <option key={c.minutes} value={c.minutes}>
                    {c.label}
                  </option>
                ))}
              </select>
              {it.asked != null && (
                <span className={it.on ? "text-dim" : "text-caution"}>
                  관제 PC가 정한 주기: {EVERY_CHOICES.find((c) => c.minutes === it.asked)?.label ?? `${it.asked}분마다`}
                  {it.on ? " (이 주기로 돕니다)" : " — 켜야 수집됩니다"}
                </span>
              )}
              <span className="text-dim">{it.last ? `마지막 수집 ${timeText(it.last)}` : "아직 수집한 적 없음"}</span>
              <span className="flex-1" />
              <button onClick={() => runNow(it)} disabled={busy || !it.on || !ready} className={BTN} title={!it.on ? "먼저 이 항목을 켜야 합니다" : !ready ? "보내기가 꺼져 있습니다" : "주기를 기다리지 않고 지금 한 번 수집해 보냅니다"}>
                {running === it.what ? "수집하는 중…" : "지금 수집"}
              </button>
              <button onClick={() => show(it.what)} className={BTN}>
                {preview?.what === it.what ? "미리 보기 닫기" : "무엇이 나가는지 보기"}
              </button>
            </div>
            {preview?.what === it.what && <Preview data={preview.data} />}
          </li>
        ))}
      </ul>
    </section>
  );
}

function Preview({ data }: { data: InvPreview }) {
  const sys = data.data?.system;
  const sw = data.data?.software;
  return (
    <div className="mt-1.5 rounded bg-raised p-2">
      {!sys && !sw ? (
        <ul className="space-y-0.5">
          {data.summary.map((line, i) => (
            <li key={i}>{line}</li>
          ))}
        </ul>
      ) : (
        <p className="mb-1 text-dim">지금 켜면 아래 내용이 그대로 나갑니다.</p>
      )}
      {sys && (
        <ul className="space-y-0.5">
          <li>운영체제: {sys.os} ({sys.build})</li>
          <li>PC 이름: {sys.host || "(가려서 나가지 않음)"}</li>
          <li>
            CPU: {sys.cpu} · {sys.cores}코어
          </li>
          <li>
            메모리: {gbText(sys.mem_used)} / {gbText(sys.mem_total)}
          </li>
          {sys.disks.map((d) => (
            <li key={d.name}>
              디스크 {d.name}: 남음 {gbText(d.free)} / {gbText(d.total)}
            </li>
          ))}
          <li>
            켜진 지: {uptimeText(sys.uptime_secs)} · NetX {sys.netx} · 테일스케일 {sys.tailnet || "없음"}
          </li>
        </ul>
      )}
      {sw && (
        <>
          <p>
            프로그램 {sw.programs.length}개{sw.cut && " (목록이 길어 뒤가 잘림)"} — 이름, 판, 만든 곳, 설치한 날
          </p>
          <ul className="mt-1 max-h-40 overflow-y-auto font-mono text-[11px]">
            {sw.programs.map((p, i) => (
              <li key={i}>
                {p.name} {p.version} {p.publisher && `· ${p.publisher}`}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
