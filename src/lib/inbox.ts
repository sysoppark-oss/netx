import type { Handoff } from "./backend";

/** 받은 조회 의뢰 한 건. 자료로만 다룬다(보이기와 입력칸 채우기). */
export interface Received extends Handoff {
  /** 받은 시각(밀리초) */
  at: number;
  read: boolean;
}

const KEY = "netx.handoffs";
export const KEEP_RECEIVED = 20;

const TARGET = /^[0-9A-Za-z.:[\]-]{1,270}$/;
const FROM = /^[a-z0-9_-]{1,32}$/;
const NAME = /^[a-z0-9.-]{1,253}$/;

/** 저장해 둔 목록에서 모양이 맞는 것만 되살린다. */
export function parseReceived(text: string | null): Received[] {
  let raw: unknown;
  try {
    raw = JSON.parse(text ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(raw)) return [];
  const out: Received[] = [];
  for (const item of raw.slice(0, KEEP_RECEIVED)) {
    if (typeof item !== "object" || item === null) continue;
    const { at, target, from, note, name, read } = item as Record<
      string,
      unknown
    >;
    if (typeof at !== "number" || !Number.isFinite(at)) continue;
    if (typeof target !== "string" || !TARGET.test(target)) continue;
    if (typeof from !== "string" || !FROM.test(from)) continue;
    out.push({
      at,
      target,
      from,
      note: typeof note === "string" ? note.slice(0, 200) : "",
      name: typeof name === "string" && NAME.test(name) ? name : null,
      read: read === true,
    });
  }
  return out;
}

/** 새로 받은 것을 맨 위에 넣고 정해진 건수만 남긴다. */
export function addReceived(
  list: Received[],
  h: Handoff,
  at: number,
): Received[] {
  return [{ ...h, at, read: false }, ...list].slice(0, KEEP_RECEIVED);
}

export function loadReceived(): Received[] {
  try {
    return parseReceived(localStorage.getItem(KEY));
  } catch {
    return [];
  }
}

export function saveReceived(list: Received[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    // 저장하지 못해도 화면의 목록은 그대로 쓴다.
  }
}

/** "11월 12일 12:05" 모양의 받은 시각 */
export function receivedAt(at: number): string {
  const d = new Date(at);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${d.getMonth() + 1}월 ${d.getDate()}일 ${two(d.getHours())}:${two(d.getMinutes())}`;
}
