// 백엔드 호출 모음. Tauri 창 밖(브라우저 미리보기)에서도 화면이 깨지지 않게 대체값을 준다.
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppInfo, EnvironmentStatus, Recipe, ToolSchema, UserLevel, ValidatedPlan } from "./types";

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
