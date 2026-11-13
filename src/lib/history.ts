// 최근 진단 기록: 끝난 진단의 계획과 결과를 이 PC에만 남겨 다시 볼 수 있게 한다.
import { invoke, isTauri } from "@tauri-apps/api/core";
import type { Light, RunState, ValidatedPlan } from "./types";

export interface HistoryMeta {
  title: string;
  recipe: string;
  target: string;
  light: Light;
  steps: number;
  /** 다른 PC에서 내보낸 것을 가져온 기록 */
  imported?: boolean;
}

export interface HistoryItem {
  id: string;
  time: number;
  size: number;
  meta: HistoryMeta;
}

export interface HistoryEntry {
  meta: HistoryMeta;
  recipe_id: string;
  inputs: Record<string, string>;
  extras: string[];
  plan: ValidatedPlan;
  run: RunState;
  /** 단계 번호별 구성도 그림(SVG). 내보낸 보고서에 넣는 데 쓴다. */
  topo_svg?: Record<string, string>;
}

/** 원본 출력은 건당 이만큼(글자 수)까지만 남긴다. */
const MAX_RAW_CHARS = 200_000;
/** 백엔드가 받는 한 건의 상한보다 조금 작게 */
const MAX_ENTRY_CHARS = 1_200_000;

const RANK: Record<string, number> = { red: 3, yellow: 2, green: 1, info: 0 };

/** 단계 판정 가운데 가장 나쁜 것 */
export function worstLight(run: RunState): Light {
  let worst: Light = "info";
  for (const r of Object.values(run.results)) if ((RANK[r.light] ?? 0) > (RANK[worst] ?? 0)) worst = r.light;
  return worst;
}

function trimmed(run: RunState): RunState {
  let used = 0;
  const lines = [];
  for (const l of run.lines) {
    used += l.line.length + 1;
    if (used > MAX_RAW_CHARS) break;
    lines.push(l);
  }
  return { ...run, active: false, lines };
}

/** 다른 프로그램에 결과로 돌려줄 기록: 원본 출력을 저장할 때와 같은 만큼만 싣는다. */
export function reportEntry(entry: HistoryEntry): unknown {
  return { meta: entry.meta, recipe_id: entry.recipe_id, inputs: entry.inputs, plan: entry.plan, run: trimmed(entry.run) };
}

/** 저장할 글을 만든다. 너무 크면 원본 출력을 빼고, 그래도 크면 null(저장하지 않는다). */
export function entryJson(entry: HistoryEntry): string | null {
  let json = JSON.stringify({ ...entry, run: trimmed(entry.run) });
  if (json.length > MAX_ENTRY_CHARS) json = JSON.stringify({ ...entry, run: { ...entry.run, active: false, lines: [] } });
  return json.length > MAX_ENTRY_CHARS ? null : json;
}

/** 읽어 온 글이 기록 모양인지 확인한다. */
export function parseEntry(json: string): HistoryEntry | null {
  try {
    const e = JSON.parse(json) as HistoryEntry;
    const ok =
      typeof e.recipe_id === "string" &&
      e.inputs !== null &&
      typeof e.inputs === "object" &&
      Array.isArray(e.plan?.steps) &&
      e.run !== null &&
      typeof e.run === "object" &&
      typeof e.run.steps === "object" &&
      typeof e.run.results === "object" &&
      Array.isArray(e.run.lines);
    return ok ? { ...e, extras: Array.isArray(e.extras) ? e.extras : [] } : null;
  } catch {
    return null;
  }
}

export async function historySave(entry: HistoryEntry): Promise<void> {
  const json = entryJson(entry);
  if (!isTauri() || json === null) return;
  await invoke<string>("history_save", { entry: json });
}

export async function historyList(): Promise<HistoryItem[]> {
  if (!isTauri()) return [];
  return invoke<HistoryItem[]>("history_list");
}

export async function historyLoad(id: string): Promise<HistoryEntry | null> {
  return parseEntry(await invoke<string>("history_load", { id }));
}

/** 번호를 주면 한 건, 주지 않으면 모두 지운다. */
export async function historyDelete(id?: string): Promise<void> {
  await invoke<number>("history_delete", { id: id ?? null });
}

/** 기록을 보고서 파일(HTML)로 작업 폴더에 내보내고 파일 경로를 돌려준다. 번호를 주지 않으면 전부. */
export async function historyExport(id: string | undefined, mask: boolean): Promise<string> {
  return invoke<string>("history_export", { id: id ?? null, offsetMinutes: -new Date().getTimezoneOffset(), mask });
}

/** 다른 PC의 NetX가 내보낸 보고서 파일의 내용을 기록에 더하고, 더한 건수를 돌려준다. */
export async function historyImport(html: string): Promise<number> {
  return invoke<number>("history_import", { html });
}
