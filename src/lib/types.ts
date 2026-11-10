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
}

export interface ToolSchema {
  tool: string;
  title: string;
  description: string;
  needs_root: boolean;
  min_level: UserLevel;
  options: OptionSpec[];
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
}

export interface SettingsView extends Settings {
  has_key: boolean;
}
