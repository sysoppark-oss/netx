// 보고서 만들기: 고른 구역을 보고서(HTML) 한 파일로 만들어 사용자가 고른 자리에 저장한다.
// 근거: docs/REPORT_FEATURE_REQUEST_v1.md. 저장할 자리는 백엔드가 띄우는 저장 창에서 고른다(화면은 경로를 정하지 않는다).
import { invoke } from "@tauri-apps/api/core";

export interface ReportZone {
  id: string;
  title: string;
  hint: string;
  /** 아직 만들지 않은 구역 */
  later?: boolean;
  /** 기본으로 체크하지 않는 구역 */
  off?: boolean;
}

/** 메뉴와 같은 순서. `later`는 다음 단계에서 들어온다. */
export const REPORT_ZONES: ReportZone[] = [
  { id: "mynet", title: "내 네트워크", hint: "어댑터, 내부·공인 주소, 게이트웨이, DNS (만든 시각 기준)" },
  { id: "history", title: "최근 진단", hint: "기간 안의 진단 기록: 입력, 단계별 결과와 판정, 그림" },
  { id: "topology", title: "구성도", hint: "기간 안에서 가장 최근에 조사한 구성도의 그림, 장비 목록, 포트 연결 관계" },
  { id: "capture", title: "캡처 분석", hint: "기간 안의 캡처 파일(최근 10개): 요약, 찾은 문제, 상위 대화 (패킷 내용은 싣지 않음)" },
  { id: "manual", title: "직접 명령", hint: "직접 명령으로 실행한 것: 명령, 종료 상태, 출력 앞 40줄" },
  { id: "terminal", title: "터미널", hint: "접속 목록 (장비 설정과 암호가 섞일 수 있어 기본 해제)", later: true, off: true },
  { id: "audit", title: "감사 로그", hint: "실행 확인, 다른 프로그램에서 받은 의뢰" },
];

export type PeriodKind = "today" | "7d" | "30d" | "custom";

const DAY = 86_400_000;

/** 기간을 밀리초로. `custom`은 날짜(YYYY-MM-DD) 두 개가 맞아야 한다. 틀리면 null. */
export function periodOf(kind: PeriodKind, from: string, to: string, now = Date.now()): { from_ms: number; to_ms: number } | null {
  if (kind === "today") {
    const start = new Date(now);
    start.setHours(0, 0, 0, 0);
    return { from_ms: start.getTime(), to_ms: now };
  }
  if (kind === "7d") return { from_ms: now - 7 * DAY, to_ms: now };
  if (kind === "30d") return { from_ms: now - 30 * DAY, to_ms: now };
  const day = /^\d{4}-\d{2}-\d{2}$/;
  if (!day.test(from) || !day.test(to)) return null;
  const a = new Date(`${from}T00:00:00`).getTime();
  const b = new Date(`${to}T23:59:59.999`).getTime();
  return Number.isFinite(a) && Number.isFinite(b) && a <= b ? { from_ms: a, to_ms: Math.min(b, Math.max(now, a)) } : null;
}

export interface ReportSaved {
  path: string;
  /** [구역 id, 기간 안 전체 건수, 실은 건수] */
  counts: [string, number, number][];
}

export interface ReportRequest {
  sections: string[];
  from_ms: number;
  to_ms: number;
  mask: boolean;
  history_id?: string;
}

/** 보고서를 만들고 저장한다. 저장 창을 닫았거나 취소했으면 null. */
export async function reportMake(request: ReportRequest): Promise<ReportSaved | null> {
  return invoke<ReportSaved | null>("report_make", { request: { ...request, history_id: request.history_id ?? null, offset_minutes: -new Date().getTimezoneOffset() } });
}

export async function reportCancel(): Promise<void> {
  await invoke("report_cancel");
}

/** 방금 저장한 보고서를 브라우저로 열거나, 그 폴더를 연다. */
export async function reportOpen(folder: boolean): Promise<void> {
  await invoke("report_open", { folder });
}

/** 직접 명령으로 실행한 것이 끝나면 명령·종료 상태·출력 앞부분을 남긴다(보고서의 "직접 명령" 구역이 읽는다). */
export async function manualSave(record: unknown): Promise<number> {
  return invoke<number>("manual_save", { record });
}
