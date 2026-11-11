// 화면과 백엔드가 주고받는 데이터 형식.
// 기준: docs/DEV_PLAN_v0.1.md 4장, src-tauri/src/model.rs

export type UserLevel = "general" | "junior" | "middle" | "senior" | "op";

export const USER_LEVELS: { id: UserLevel; label: string; hint: string }[] = [
  { id: "general", label: "일반사용자", hint: "쉬운 말 결과와 해결 방법만" },
  { id: "junior", label: "초급 엔지니어", hint: "옵션별 학습용 해설, 안전 옵션만" },
  { id: "middle", label: "중급 엔지니어", hint: "전체 옵션, 위험 옵션은 경고" },
  { id: "senior", label: "고급 엔지니어", hint: "전체 옵션과 직접 입력, 원본 출력 펼침" },
  { id: "op", label: "전산관리자(OP)", hint: "고급 + 감사 로그, 운영 기능" },
];

export function levelRank(level: UserLevel): number {
  return USER_LEVELS.findIndex((l) => l.id === level);
}

export type UiMode = "easy" | "expert";
export type EnvMode = "full" | "basic";
export type Risk = "safe" | "caution" | "danger" | "forbidden";

export interface AppInfo {
  name: string;
  version: string;
  os: string;
  arch: string;
}

export interface EnvironmentStatus {
  mode: EnvMode;
  wsl_installed: boolean;
  distros: string[];
  message: string;
}

export interface OptionSpec {
  flag: string;
  aliases: string[];
  type: string;
  label: string;
  risk: Risk;
  group: string | null;
  values: string[];
  min_level: UserLevel | null;
}

export interface PositionalSpec {
  name: string;
  type: string;
  label: string;
  required: boolean;
  variadic: boolean;
}

export interface ToolSchema {
  tool: string;
  title: string;
  description: string;
  needs_root: boolean;
  min_level: UserLevel;
  options: OptionSpec[];
  positional: PositionalSpec[];
}

export interface RecipeInput {
  name: string;
  type: string;
  label: string;
  example: string | null;
  values: string[];
  value_labels: string[];
  default: string | null;
  optional: boolean;
  source: string | null;
  hint: string | null;
  suggestions: string[];
  suggestion_labels: string[];
}

export interface Recipe {
  id: string;
  icon: string;
  title: string;
  description: string;
  min_level: UserLevel;
  inputs: RecipeInput[];
}

export interface Issue {
  severity: "block" | "warn";
  message: string;
}

export interface ValidatedStep {
  order: number;
  tool: string;
  title: string;
  argv: string[];
  display: string;
  risk: Risk;
  needs_root: boolean;
  streaming: boolean;
  estimate_secs: number | null;
  confirm: "none" | "click" | "hello";
  blocked: boolean;
  issues: Issue[];
  reason: string | null;
  expect: string | null;
  manual: boolean;
  explain: { token: string; label: string }[];
}

/** 직접 추가한 명령이 붙는 바탕 계획 */
export type Base =
  | { kind: "recipe"; id: string; inputs: Record<string, string> }
  | { kind: "ai"; plan_id: number }
  | { kind: "none" };

// ---- 내 네트워크 (src-tauri/src/netinfo.rs) ----

export interface NetInfo {
  iface: string | null;
  ip: string | null;
  prefix: number | null;
  gateway: string | null;
  dns: string[];
  link: string | null;
  public_ip: string | null;
  rx_bytes: number | null;
  tx_bytes: number | null;
  at_ms: number;
}

export interface ValidatedPlan {
  intent: string;
  summary: string;
  target: string | null;
  parallel: boolean;
  risk: Risk;
  blocked: boolean;
  steps: ValidatedStep[];
}

// ---- 실행 (src-tauri/src/engine) ----

export type StepStatus = "running" | "done" | "timeout" | "cancelled" | "capped" | "error";

export interface OutputEvent {
  run_id: number;
  order: number;
  stream: "stdout" | "stderr";
  line: string;
}

export interface StepEvent {
  run_id: number;
  order: number;
  status: StepStatus;
  exit_code: number | null;
  elapsed_ms: number;
}

export interface RunStarted {
  run_id: number;
  orders: number[];
}

export interface ToolStatus {
  tool: string;
  installed: boolean;
}

export interface OutputLine {
  order: number;
  stream: "stdout" | "stderr";
  line: string;
}

/// 화면이 들고 있는 실행 상태
export interface RunState {
  run_id: number;
  title: string;
  active: boolean;
  steps: Record<number, { status: StepStatus; exit_code: number | null; elapsed_ms: number; started_at: number }>;
  results: Record<number, StepResult>;
  lines: OutputLine[];
}

// ---- 결과 해설 (src-tauri/src/interpret.rs) ----

export type Light = "green" | "yellow" | "red" | "info";

export interface Bar {
  label: string;
  value: number;
  light: Light;
  note: string;
}

export type Chart =
  | { kind: "bars"; title: string; unit: string; bars: Bar[] }
  | { kind: "line"; title: string; unit: string; points: number[] }
  | { kind: "gauge"; title: string; unit: string; value: number; max: number };

export interface StepResult {
  light: Light;
  text: string;
  facts: { label: string; value: string }[];
  charts: Chart[];
  targets: Target[];
  processes: Proc[];
  topology: Topology | null;
}

/** 네트워크 구성도 (src-tauri/src/interpret.rs) */
export interface TopoNode {
  ip: string;
  mac: string | null;
  role: "me" | "gateway" | "host";
  kind: string;
  label: string;
  vendor: string;
  ports: number[];
  /** 장비 이름(NetBIOS 이름 또는 역방향 DNS). 알아내지 못하면 null */
  name: string | null;
}

export interface Topology {
  cidr: string;
  gateway: string | null;
  me: string | null;
  nodes: TopoNode[];
}

/** 포트를 쓰고 있는 Windows 프로세스 */
export interface Proc {
  pid: number;
  name: string;
  ports: string;
}

export type KillResult =
  | { status: "killed"; message: string }
  | { status: "need_name"; name: string; message: string }
  | { status: "denied"; message: string };

/** 결과에서 바로 접속해 볼 수 있는 대상(열린 포트) */
export interface Target {
  host: string;
  port: number;
  service: string;
}

export interface ResultEvent {
  run_id: number;
  order: number;
  result: StepResult;
}

// ---- AI 제안과 브리핑 (src-tauri/src/planner) ----

export type Suggestion =
  | { kind: "plan"; plan_id: number; plan: ValidatedPlan; note: string | null }
  | { kind: "recipe"; recipe_id: string; inputs: Record<string, string>; note: string }
  | { kind: "message"; text: string };

export interface Briefing {
  headline: string;
  verdict: "normal" | "problem" | "unclear";
  findings: { order: number; light: "green" | "yellow" | "red"; text: string }[];
  next: string;
}

export type Provider = "none" | "ollama" | "openai";

export interface Settings {
  provider: Provider;
  base_url: string;
  model: string;
  redact: boolean;
  /** 새 버전을 자동으로 설치하지 않는다(알림만) */
  manual_update: boolean;
}

/** 자동 업데이트 상태 (src-tauri/src/updater.rs) */
export interface UpdateStatus {
  state: "available" | "downloading" | "installing" | "error";
  commit: string;
  message: string;
}

export interface SettingsView extends Settings {
  has_key: boolean;
}
