// 백엔드 호출 모음. Tauri 창 밖(브라우저 미리보기)에서도 화면이 깨지지 않게 대체값을 준다.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppInfo,
  Base,
  NetInfo,
  ValidatedStep,
  Briefing,
  EnvironmentStatus,
  OutputEvent,
  Recipe,
  ResultEvent,
  RunStarted,
  Settings,
  SettingsView,
  StepEvent,
  Suggestion,
  ToolSchema,
  ToolStatus,
  UserLevel,
  ValidatedPlan,
} from "./types";

// 브라우저 미리보기에서는 저장소의 JSON을 직접 읽어 목록만 보여 준다. 검증은 앱에서만 동작한다.
const previewTools = () =>
  Object.values(import.meta.glob("../../tools/*.json", { eager: true, import: "default" })) as ToolSchema[];
const previewRecipes = () =>
  Object.values(import.meta.glob("../../recipes/*.json", { eager: true, import: "default" })) as Recipe[];

export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) return { name: "NetX", version: "0.1.0", os: "browser", arch: "-" };
  return invoke<AppInfo>("app_info");
}

export async function getEnvironmentStatus(): Promise<EnvironmentStatus> {
  if (!isTauri()) {
    return {
      mode: "basic",
      wsl_installed: false,
      distros: [],
      message: "브라우저 미리보기입니다. 실제 환경 확인은 NetX 앱에서 동작합니다.",
    };
  }
  return invoke<EnvironmentStatus>("environment_status");
}

export async function getTools(): Promise<ToolSchema[]> {
  if (!isTauri()) return previewTools();
  return invoke<ToolSchema[]>("get_tools");
}

export async function getRecipes(): Promise<Recipe[]> {
  if (!isTauri()) return previewRecipes();
  return invoke<Recipe[]>("get_recipes");
}

export async function planFromRecipe(
  id: string,
  inputs: Record<string, string>,
  level: UserLevel,
): Promise<ValidatedPlan> {
  if (!isTauri()) throw "브라우저 미리보기에서는 계획을 만들 수 없습니다. NetX 앱에서 실행하세요.";
  return invoke<ValidatedPlan>("plan_from_recipe", { id, inputs, level });
}

export async function runRecipe(
  id: string,
  inputs: Record<string, string>,
  level: UserLevel,
  confirmed: boolean,
  onlyStep: number | null,
): Promise<RunStarted> {
  return invoke<RunStarted>("run_recipe", { id, inputs, level, confirmed, onlyStep });
}

export async function cancelRun(runId: number): Promise<boolean> {
  return invoke<boolean>("cancel_run", { runId });
}

export async function getToolStatus(): Promise<ToolStatus[]> {
  if (!isTauri()) return [];
  return invoke<ToolStatus[]>("tool_status");
}

export async function installTools(): Promise<RunStarted> {
  return invoke<RunStarted>("install_tools");
}

/** 실행 이벤트를 구독한다. 돌려주는 함수를 부르면 구독을 끝낸다. */
export function onRunEvents(handlers: {
  output: (ev: OutputEvent) => void;
  step: (ev: StepEvent) => void;
  result: (ev: ResultEvent) => void;
  done: (runId: number) => void;
}): () => void {
  if (!isTauri()) return () => {};
  const subs = [
    listen<OutputEvent>("run:output", (e) => handlers.output(e.payload)),
    listen<StepEvent>("run:step", (e) => handlers.step(e.payload)),
    listen<ResultEvent>("run:result", (e) => handlers.result(e.payload)),
    listen<number>("run:done", (e) => handlers.done(e.payload)),
  ];
  return () => subs.forEach((p) => p.then((off) => off()));
}

export async function suggest(text: string, level: UserLevel): Promise<Suggestion> {
  if (!isTauri()) return { kind: "message", text: "브라우저 미리보기에서는 제안을 받을 수 없습니다." };
  return invoke<Suggestion>("suggest", { text, level });
}

export async function runAiPlan(
  planId: number,
  level: UserLevel,
  confirmed: boolean,
  onlyStep: number | null,
): Promise<RunStarted> {
  return invoke<RunStarted>("run_ai_plan", { planId, level, confirmed, onlyStep });
}

export async function briefRun(runId: number, level: UserLevel): Promise<Briefing> {
  return invoke<Briefing>("brief_run", { runId, level });
}

export async function getSettings(): Promise<SettingsView> {
  if (!isTauri()) return { provider: "none", base_url: "", model: "", redact: false, has_key: false };
  return invoke<SettingsView>("get_settings");
}

/** apiKey가 null이면 저장된 키를 그대로 두고, 빈 문자열이면 지운다. */
export async function saveSettings(settings: Settings, apiKey: string | null): Promise<SettingsView> {
  return invoke<SettingsView>("save_settings", { settings, apiKey });
}

export async function testLlm(): Promise<string> {
  return invoke<string>("test_llm");
}

/** 캡처할 수 있는 인터페이스 이름. 맨 앞의 "any"는 전체. */
export async function listInterfaces(): Promise<string[]> {
  if (!isTauri()) return ["any"];
  return invoke<string[]>("list_interfaces");
}

/** 내 네트워크 정보. 앱 밖에서는 null. */
export async function networkInfo(): Promise<NetInfo | null> {
  if (!isTauri()) return null;
  return invoke<NetInfo>("network_info");
}

/** 직접 적은 명령 한 줄을 검증한다(실행하지 않음). */
export async function checkCommand(text: string, level: UserLevel): Promise<ValidatedStep> {
  if (!isTauri()) throw "브라우저 미리보기에서는 검증할 수 없습니다. NetX 앱에서 실행하세요.";
  return invoke<ValidatedStep>("check_command", { text, level });
}

/** 바탕 계획에 직접 추가한 명령을 붙여 검증한다. */
export async function planMixed(base: Base, extras: string[], level: UserLevel): Promise<ValidatedPlan> {
  if (!isTauri()) throw "브라우저 미리보기에서는 계획을 만들 수 없습니다. NetX 앱에서 실행하세요.";
  return invoke<ValidatedPlan>("plan_mixed", { base, extras, level });
}

export async function runMixed(
  base: Base,
  extras: string[],
  level: UserLevel,
  confirmed: boolean,
  onlyStep: number | null,
): Promise<RunStarted> {
  return invoke<RunStarted>("run_mixed", { base, extras, level, confirmed, onlyStep });
}

/** 스캔 결과의 장비에 접속해 본다. 웹은 브라우저, SSH·Telnet은 새 터미널 창. */
export async function openTarget(host: string, port: number, kind: "web" | "ssh" | "telnet", user: string | null): Promise<string> {
  return invoke<string>("open_target", { host, port, kind, user });
}
