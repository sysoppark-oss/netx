// 넷엑스 터미널 창의 백엔드 호출. 암호는 어디에도 넘기지 않는다(접속 뒤 터미널 안에서 직접 친다).
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type TermSpec =
  | { kind: "ssh"; host: string; port: number; user: string }
  | { kind: "telnet"; host: string; port: number }
  | { kind: "serial"; port: string; baud: number; data_bits: number; parity: string; stop_bits: number; flow: string }
  | { kind: "ftp"; host: string; port: number; user: string | null }
  | { kind: "tftp"; host: string; port: number };

export type TermKind = TermSpec["kind"];

/** 본창(스캔 결과)에서 넘어온 접속 정보 */
export interface TermPreset {
  kind: "ssh" | "telnet" | "ftp" | "tftp";
  host: string;
  port: number;
  user: string | null;
}

/** 터미널 창을 연다(본창에서 부른다). preset을 주면 그 접속을 바로 시작한다. */
export async function openTerminal(preset: TermPreset | null = null): Promise<void> {
  if (!isTauri()) throw new Error("터미널은 NetX 앱에서만 동작합니다.");
  return invoke<void>("open_terminal", { preset });
}

export const takePreset = () => invoke<TermPreset | null>("term_take_preset");
export const termPorts = () => invoke<string[]>("term_ports");
export const termOpen = (spec: TermSpec, cols: number, rows: number) => invoke<number>("term_open", { spec, cols, rows });
export const termWrite = (id: number, data: string) => invoke<boolean>("term_write", { id, data });
export const termResize = (id: number, cols: number, rows: number) => invoke<boolean>("term_resize", { id, cols, rows });
export const termClose = (id: number) => invoke<boolean>("term_close", { id });
export const termFolder = () => invoke<string>("term_folder");

export function onPreset(handler: () => void): Promise<() => void> {
  return listen("term:preset", handler);
}

interface Handlers {
  data: (bytes: Uint8Array) => void;
  end: (message: string) => void;
}

// 접속 번호를 받기 전에 먼저 도착한 출력은 모아 두었다가 넘긴다.
const handlers = new Map<number, Handlers>();
const early = new Map<number, { chunks: Uint8Array[]; end: string | null }>();
let listening: Promise<unknown> | null = null;

function decode(base64: string): Uint8Array {
  const raw = atob(base64);
  const bytes = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

function hold(id: number) {
  let slot = early.get(id);
  if (!slot) {
    slot = { chunks: [], end: null };
    early.set(id, slot);
  }
  return slot;
}

/** 출력 이벤트를 받기 시작한다. 접속을 열기 전에 끝나 있어야 한다. */
export function startListening(): Promise<unknown> {
  listening ??= Promise.all([
    listen<{ id: number; data: string }>("term:data", (e) => {
      const bytes = decode(e.payload.data);
      const h = handlers.get(e.payload.id);
      if (h) h.data(bytes);
      else hold(e.payload.id).chunks.push(bytes);
    }),
    listen<{ id: number; message: string }>("term:end", (e) => {
      const h = handlers.get(e.payload.id);
      if (h) {
        handlers.delete(e.payload.id);
        h.end(e.payload.message);
      } else hold(e.payload.id).end = e.payload.message;
    }),
  ]);
  return listening;
}

/** 한 접속의 출력을 받을 곳을 정한다. */
export function attach(id: number, h: Handlers) {
  const slot = early.get(id);
  early.delete(id);
  slot?.chunks.forEach(h.data);
  if (slot?.end != null) h.end(slot.end);
  else handlers.set(id, h);
}

export function detach(id: number) {
  handlers.delete(id);
  early.delete(id);
}
