// 백엔드 호출 모음. Tauri 창 밖(브라우저 미리보기)에서도 화면이 깨지지 않게 대체값을 준다.
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { AppInfo, EnvironmentStatus } from "./types";

export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) {
    return { name: "NetX", version: "0.1.0", os: "browser", arch: "-" };
  }
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
