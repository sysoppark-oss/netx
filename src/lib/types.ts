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
