// 관제 PC: 이 PC의 진단 결과를 다른 PC의 NetX로 보내거나, 다른 PC가 보낸 결과를 이 PC에 모은다.
// 근거: 통합프로젝트 「관제 PC 변경 제안 (2판)」 1단계. 보내기와 받기는 기본 꺼짐이고 이 PC에서 사람이 켠다.
import { invoke, isTauri } from "@tauri-apps/api/core";

export interface Collector {
  id: string;
  name: string;
  addr: string;
  port: number;
}

export interface CollectConfig {
  send: boolean;
  mask: boolean;
  receive: boolean;
  name: string;
  collector: Collector | null;
  keep_days: number;
  port: number;
}

export interface PairWindow {
  open: boolean;
  code: string;
  secs_left: number;
  wrong: number;
  pending: { name: string; addr: string; number: string } | null;
}

export type Join =
  | { state: "idle" }
  | { state: "waiting"; number: string; name: string; addr: string }
  | { state: "done"; name: string }
  | { state: "failed"; why: string };

export interface Overview {
  config: CollectConfig;
  id: string;
  status: { listening: string | null; listen_error: string | null; linked: boolean; send_error: string | null; last_sent: number };
  window: PairWindow;
  join: Join;
  waiting: number;
  tailnet: string | null;
  limits: { out_queue: number; out_keep_days: number; hour_items: number; pc_items: number; keep_days_max: number };
}

export type Verdict = "ok" | "caution" | "danger" | "unknown";

export interface Pc {
  id: string;
  name: string;
  addr: string;
  state: "active" | "stopped" | "removed";
  own: boolean;
  paired: number;
  last_seq: number;
  seen: number;
  quiet: boolean;
  items: number;
  bytes: number;
}

export interface Row {
  pc: string;
  pc_name: string;
  seq: number;
  at: number;
  got: number;
  kind: string;
  title: string;
  target: string;
  verdict: Verdict;
  skew: boolean;
  stale: boolean;
  masked: boolean;
}

export interface Board {
  pcs: Pc[];
  kinds: string[];
  gaps: { pc: string; first: number; last: number; got: number; dropped: boolean }[];
  alerts: { pc: string; at: number; kind: string; note: string }[];
  downtime: [number, number][];
  /** 짝이 아닌 곳에서 온 접속(최근 것부터) */
  strangers: Conn[];
}

/** 접속 한 번의 기록 */
export interface Conn {
  at: number;
  pc: string;
  addr: string;
  outcome: string;
  ms: number;
  stored: number;
  again: number;
  refused: number;
  bytes: number;
  first_seq: number;
  last_seq: number;
}

export interface Transfer {
  seq: number;
  at: number;
  got: number;
  kind: string;
  title: string;
  verdict: string;
  size: number;
  skew: number;
  stale: boolean;
}

export interface Hour {
  at: number;
  items: number;
  danger: number;
  conns: number;
  failed: number;
}

/** 거점 하나의 상세 */
export interface Site {
  pc: Pc;
  conns: Conn[];
  transfers: Transfer[];
  timeline: Hour[];
  gaps: Board["gaps"];
  alerts: Board["alerts"];
}

/** 접속이 끝난 까닭(백엔드가 정해진 낱말로 준다) */
export const CONN_LABEL: Record<string, string> = {
  ok: "정상으로 끝남",
  closed: "연결이 끊김",
  pair_ok: "짝 맺기 승인",
  pair_no: "짝 맺기 거절",
  unknown: "짝이 아님",
  stopped: "받기 멈춤이라 거절",
  refused: "결과를 거절하고 끊음",
  strange: "모르는 말을 해서 끊음",
  version: "판이 달라 끊음",
  tls: "인증 실패",
  busy: "접속이 너무 많아 받지 않음",
  outside: "테일스케일 밖에서 옴",
};

export function sizeText(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}

export interface Filter {
  pc: string;
  from: number;
  to: number;
  target: string;
  kind: string;
  verdict: string;
  offset: number;
}

export interface Step {
  order: number;
  title: string;
  tool: string;
  command: string;
  state: string;
  exit: number | null;
  secs: number | null;
  verdict: Verdict;
  notes: string[];
}

export interface Finished {
  kind: string;
  title: string;
  target: string;
  verdict: Verdict;
  summary: string[];
  started: number;
  ended: number;
  steps: Step[];
}

export interface SentRow {
  seq: number;
  at: number;
  kind: string;
  title: string;
  target: string;
  verdict: Verdict;
  state: "queued" | "sent" | "dropped" | "local";
  sent: number;
}

export interface Seen {
  pc: string;
  pc_name: string;
  n: number;
  ok: number;
  caution: number;
  danger: number;
  first_at: number;
  last_at: number;
  last_verdict: Verdict;
}

export interface TargetView {
  target: string;
  total: number;
  seen: Seen[];
  single: boolean;
  differs: boolean;
  alias: boolean;
}

export interface Analysis {
  made: number;
  targets: TargetView[];
  /** [1970년부터의 시간 번호, 정상, 주의, 위험, 판정 없음] */
  hours: [number, number, number, number, number][];
}

export const collectAnalysis = () => invoke<Analysis>("collect_analysis");

export interface DayCount {
  day: string;
  ok: number;
  caution: number;
  danger: number;
  unknown: number;
}

/** 시간별 건수를 이 PC의 시각으로 날짜별, 시각(0~23시)별로 묶는다. */
export function trend(hours: Analysis["hours"]): { days: DayCount[]; byHour: number[] } {
  const days = new Map<string, DayCount>();
  const byHour = new Array<number>(24).fill(0);
  const two = (v: number) => String(v).padStart(2, "0");
  for (const [hour, ok, caution, danger, unknown] of hours) {
    const d = new Date(hour * 3_600_000);
    const day = `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
    const sum = days.get(day) ?? { day, ok: 0, caution: 0, danger: 0, unknown: 0 };
    sum.ok += ok;
    sum.caution += caution;
    sum.danger += danger;
    sum.unknown += unknown;
    days.set(day, sum);
    byHour[d.getHours()] += caution + danger;
  }
  return { days: [...days.values()].sort((a, b) => a.day.localeCompare(b.day)), byHour };
}

export const collectOverview = () => invoke<Overview>("collect_overview");
export const collectSet = (c: Pick<CollectConfig, "send" | "mask" | "receive" | "name" | "keep_days">) => invoke<CollectConfig>("collect_set", { send: c.send, mask: c.mask, receive: c.receive, name: c.name, keepDays: c.keep_days });
export const collectJoin = (addr: string, port: number | null, code: string) => invoke<Join>("collect_join", { addr, port, code });
export const collectJoinCancel = () => invoke<void>("collect_join_cancel");
export const collectForget = () => invoke<void>("collect_forget");
export const collectSent = () => invoke<SentRow[]>("collect_sent");
export const collectPairOpen = () => invoke<PairWindow>("collect_pair_open");
export const collectPairClose = () => invoke<void>("collect_pair_close");
export const collectPairDecide = (approve: boolean) => invoke<void>("collect_pair_decide", { approve });
export const collectPcSet = (id: string, state: Pc["state"]) => invoke<void>("collect_pc_set", { id, state });
export const collectSite = (id: string) => invoke<Site>("collect_site", { id });
export const collectBoard = () => invoke<Board>("collect_board");
export const collectList = (filter: Filter) => invoke<{ rows: Row[]; total: number; page: number }>("collect_list", { filter });
export const collectItem = (pc: string, seq: number) => invoke<Finished | null>("collect_item", { pc, seq });

/** 끝난 진단의 기록을 넘긴다. 보내기도 받기도 꺼져 있으면 백엔드가 아무것도 하지 않는다. */
export async function collectSubmit(record: unknown): Promise<number | null> {
  if (!isTauri()) return null;
  return invoke<number | null>("collect_submit", { record });
}

export const VERDICT_LABEL: Record<Verdict, string> = { ok: "정상", caution: "주의", danger: "위험", unknown: "판정 없음" };

export const STEP_STATE_LABEL: Record<string, string> = { ok: "끝남", timeout: "시간 초과", cancelled: "취소", capped: "출력 상한", error: "오류", skipped: "실행 안 함" };

/** 나가는 항목(켤 때 보여 주는 목록) */
export const SENT_FIELDS = [
  "진단 종류와 제목, 대상(주소·이름), 종합 판정과 요약",
  "단계마다: 제목, 도구, 실행한 명령줄, 끝난 상태와 종료 코드, 걸린 시간, 판정, 해석 문장",
  "진단을 시작하고 끝낸 시각, 이 PC의 id(공개 키 지문)와 순번, NetX 판 번호",
];

/** 나가지 않는 것 */
export const NOT_SENT = "명령의 원본 출력, 캡처 파일, 구성도 그림, 터미널 내용, 스토리방화벽의 사건 기록은 보내지 않습니다.";

export function timeText(ms: number): string {
  if (!ms) return "-";
  const d = new Date(ms);
  const two = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** 날짜 칸(YYYY-MM-DD)을 그날의 처음 또는 끝 시각으로. 비었거나 틀리면 0(제한 없음). */
export function dayBound(day: string, end: boolean): number {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return 0;
  const t = new Date(`${day}T${end ? "23:59:59.999" : "00:00:00"}`).getTime();
  return Number.isFinite(t) ? t : 0;
}

// ---- 예약 수집과 거점 관리 대시보드 ----

export interface InvItem {
  what: string;
  title: string;
  /** 가장 잦은 주기(분) */
  floor: number;
  on: boolean;
  every: number;
  last: number;
  /** 관제 PC가 예약표에서 정한 주기(분). 없으면 요청하지 않은 것 */
  asked: number | null;
}

export interface PlanCell {
  what: string;
  title: string;
  floor: number;
  every: number | null;
  approved: boolean;
  /** 이 항목을 마지막으로 받은 때(없으면 0) */
  last: number;
  /** [바로 수집]을 마지막으로 누른 때(없으면 0) */
  now: number;
}

export interface PlanRow {
  pc: Pc;
  asked: number;
  delivered: boolean;
  cells: PlanCell[];
}

export const collectPlan = () => invoke<PlanRow[]>("collect_plan");
export const collectPlanNow = (id: string, what: string) => invoke<void>("collect_plan_now", { id, what });
export const collectInventoryRun = (what: string) => invoke<InvItem[]>("collect_inventory_run", { what });
export const collectPlanSet = (id: string, what: string, every: number | null) => invoke<void>("collect_plan_set", { id, what, every });

export interface Disk {
  name: string;
  total: number;
  free: number;
}

export interface SystemInfo {
  os: string;
  build: string;
  host: string;
  cpu: string;
  cores: number;
  mem_total: number;
  mem_used: number;
  disks: Disk[];
  uptime_secs: number;
  netx: string;
  tailnet: string;
}

export interface Program {
  name: string;
  version: string;
  publisher: string;
  installed: string;
}

export interface SpeedInfo {
  down_mbps: number;
  up_mbps: number;
  latency_ms: number;
  server: string;
  bytes: number;
}

export interface FleetSite {
  pc: Pc;
  /** 최근 속도(가장 최근 것이 맨 앞): [잰 때, 속도] */
  speeds: [number, SpeedInfo][];
  system_at: number;
  system: SystemInfo | null;
  software_at: number;
  programs: Program[];
  cut: boolean;
  prev_at: number;
  added: Program[];
  removed: Program[];
  changed: [string, string, string][];
}

/** 미리 보기: 지금 나갈 모양 그대로 */
export interface InvPreview {
  title: string;
  summary: string[];
  data?: { system?: SystemInfo; software?: { programs: Program[]; cut: boolean } };
}

export const collectInventory = () => invoke<InvItem[]>("collect_inventory");
export const collectInventorySet = (what: string, on: boolean, every: number) => invoke<InvItem[]>("collect_inventory_set", { what, on, every });
export const collectInventoryPreview = (what: string) => invoke<InvPreview>("collect_inventory_preview", { what });
export const collectFleet = () => invoke<FleetSite[]>("collect_fleet");

export const EVERY_CHOICES: { minutes: number; label: string }[] = [
  { minutes: 15, label: "15분마다" },
  { minutes: 60, label: "1시간마다" },
  { minutes: 360, label: "6시간마다" },
  { minutes: 720, label: "12시간마다" },
  { minutes: 1440, label: "하루 한 번" },
  { minutes: 10080, label: "일주일에 한 번" },
];

export function gbText(bytes: number): string {
  const gb = bytes / 1024 / 1024 / 1024;
  return gb >= 100 ? `${Math.round(gb)}GB` : `${gb.toFixed(1)}GB`;
}
