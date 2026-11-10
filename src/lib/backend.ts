// 백엔드 호출 모음. Tauri 창 밖(브라우저 미리보기)에서도 화면이 깨지지 않게 대체값을 준다.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppInfo,
  EnvironmentStatus,
  OutputEvent,
  Recipe,
  RunStarted,
  StepEvent,
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
  done: (runId: number) => void;
}): () => void {
  if (!isTauri()) return () => {};
  const subs = [
    listen<OutputEvent>("run:output", (e) => handlers.output(e.payload)),
    listen<StepEvent>("run:step", (e) => handlers.step(e.payload)),
    listen<number>("run:done", (e) => handlers.done(e.payload)),
  ];
  return () => subs.forEach((p) => p.then((off) => off()));
}
