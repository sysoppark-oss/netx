// 여러 화면이 함께 쓰는 단추·입력칸 모양과 판정 색. 파일마다 다시 적지 않는다.
import type { Verdict } from "./collect";

export const BTN = "rounded border border-line px-2.5 py-1 text-xs hover:bg-raised disabled:opacity-50";
export const BTN_MAIN = "rounded border border-accent/60 px-2.5 py-1 text-xs text-accent hover:bg-raised disabled:opacity-50";
export const FIELD = "rounded border border-line bg-void px-2 py-1 text-xs";
export const TONE: Record<Verdict, string> = { ok: "text-safe", caution: "text-caution", danger: "text-danger", unknown: "text-dim" };

/** 창이 가려져 있거나 최소화돼 있으면 참. 주기로 묻는 일은 이때 건너뛴다. */
export function hidden(): boolean {
  return typeof document !== "undefined" && document.visibilityState !== "visible";
}
