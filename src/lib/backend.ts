// 백엔드 호출 모음. Tauri 창 밖(브라우저 미리보기)에서도 화면이 깨지지 않게 대체값을 준다.
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppInfo,
  KillResult,
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
  UpdateStatus,
  WordView,
  UserLevel,
  ValidatedPlan,
} from "./types";

// 브라우저 미리보기에서는 저장소의 JSON을 직접 읽어 목록만 보여 준다. 검증은 앱에서만 동작한다.
const previewTools = () =>
  Object.values(import.meta.glob("../../tools/*.json", { eager: true, import: "default" })) as ToolSchema[];
const previewRecipes = () =>
  Object.values(import.meta.glob("../../recipes/*.json", { eager: true, import: "default" })) as Recipe[];

export async function getAppInfo(): Promise<AppInfo> {
  if (!isTauri()) return { name: "NetX", version: "0.1.0", os: "browser", arch: "-", commit: "", channel: "" };
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
  if (!isTauri()) return { provider: "none", base_url: "", model: "", redact: false, manual_update: false, voice_input: false, has_key: false, has_snmp: false };
  return invoke<SettingsView>("get_settings");
}

/** apiKey와 snmpCommunity는 null이면 저장된 값을 그대로 두고, 빈 문자열이면 지운다. */
export async function saveSettings(settings: Settings, apiKey: string | null, snmpCommunity: string | null = null): Promise<SettingsView> {
  return invoke<SettingsView>("save_settings", { settings, apiKey, snmpCommunity });
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

/** Windows 프로세스를 종료한다. Windows Hello 확인을 거친다. typedName은 Hello를 쓸 수 없을 때 입력한 프로세스 이름. */
export async function killProcess(pid: number, typedName: string | null, level: UserLevel): Promise<KillResult> {
  return invoke<KillResult>("kill_process", { pid, typedName, level });
}

/** 구성도 그림(SVG)을 작업 폴더에 저장하고 저장한 파일의 전체 경로를 돌려준다. */
export async function saveTopology(svg: string): Promise<string> {
  if (!isTauri()) throw new Error("그림 저장은 NetX 앱에서만 동작합니다.");
  return invoke<string>("save_topology", { svg });
}

/** 자동 업데이트 상태 알림을 받는다. */
export function onUpdateStatus(handler: (s: UpdateStatus) => void): () => void {
  if (!isTauri()) return () => {};
  const sub = listen<UpdateStatus>("update:status", (e) => handler(e.payload));
  return () => void sub.then((off) => off());
}

/** 새 버전을 지금 내려받아 설치한다. 설치가 시작되면 NetX가 닫혔다가 다시 열린다. */
export async function applyUpdate(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("apply_update");
}

/** 말씀 자료를 받는다. 날짜는 이 PC의 현지 날짜이고, 구절을 고르는 데만 쓴다. 네트워크를 쓰지 않는다. */
export async function getWord(date: Date): Promise<WordView | null> {
  if (!isTauri()) return null;
  return invoke<WordView>("get_word", { year: date.getFullYear(), month: date.getMonth() + 1, day: date.getDate() });
}

/** 교회 안내에 적힌 전자우편 주소로 메일 프로그램을 연다(주소는 백엔드가 문안에서 읽는다). */
export async function openChurchMail(): Promise<void> {
  if (!isTauri()) return;
  return invoke<void>("open_church_mail");
}

/** Windows 음성 입력(Win+H)을 연다. 설정에서 켠 경우에만 된다. NetX는 소리를 받지 않는다. */
export async function voiceTyping(): Promise<void> {
  if (!isTauri()) throw new Error("음성 입력은 앱에서만 됩니다.");
  return invoke<void>("voice_typing");
}

/** 다른 프로그램(스토리방화벽 등)이 넘긴 진단 대상. 입력칸에 채울 뿐이고 실행은 사람이 누른다. */
export interface Handoff {
  target: string;
  from: string;
  note: string;
  /** 의뢰 번호. 이 대상으로 한 진단의 결과를 보낸 쪽에 돌려줄 때 쓴다. */
  request_id: string;
  /** 그 주소에 붙은 이름. 보이기만 한다. 입력칸이나 명령에 넣지 않는다. */
  name: string | null;
}

/** 결과를 돌려준 내역 */
export interface ResultSent {
  to: string;
  target: string;
  bytes: number;
  /** 기다리던 접속으로 바로 보냈다. 거짓이면 보관해 두었다가 물으면 준다. */
  live: boolean;
}

/** 끝난 진단의 기록을 그 의뢰를 보낸 프로그램에 결과로 돌려준다. 모르는 의뢰면 null. */
export async function bridgeReport(requestId: string, record: unknown): Promise<ResultSent | null> {
  if (!isTauri()) return null;
  return invoke<ResultSent | null>("bridge_report", { requestId, record });
}

/** 넘겨받은 대상이 있으면 알려 준다. 이미 와 있던 것도 한 번 가져온다. */
export async function onHandoff(handler: (h: Handoff) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  const take = () =>
    invoke<Handoff | null>("bridge_take_handoff")
      .then((h) => h && handler(h))
      .catch(() => {});
  const stop = await listen("bridge:handoff", take);
  void take();
  return stop;
}

export interface BridgeStatus {
  state: "open" | "unchecked" | "closed" | "none";
  members: number;
}

/** 다른 프로그램과의 연동 입구 상태 ("정보" 화면에 보인다) */
export async function bridgeStatus(): Promise<BridgeStatus> {
  if (!isTauri()) return { state: "none", members: 0 };
  return invoke<BridgeStatus>("bridge_status");
}
