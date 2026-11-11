// 캡처 분석 화면의 백엔드 호출. 작업 폴더의 캡처 파일만 이름으로 연다(읽기 전용).
import { invoke, isTauri } from "@tauri-apps/api/core";

export interface CaptureFile {
  name: string;
  size: number;
  modified: number;
}

export interface Finding {
  light: "red" | "yellow";
  title: string;
  text: string;
  count: number;
  mark: string;
}

export interface Analysis {
  name: string;
  size: number;
  started: number;
  seconds: number;
  packets: number;
  bytes: number;
  link: string;
  protocols: { label: string; packets: number; bytes: number }[];
  talks: { a: string; b: string; packets: number; bytes: number }[];
  findings: Finding[];
  notes: string[];
}

export interface PacketRow {
  no: number;
  time: number;
  src: string;
  dst: string;
  proto: string;
  len: number;
  info: string;
  mark: string;
}

export interface PacketPage {
  total: number;
  rows: PacketRow[];
}

export interface PacketDetail {
  no: number;
  time: number;
  len: number;
  captured: number;
  layers: { name: string; fields: { name: string; value: string }[] }[];
  hex: string[];
  cut: boolean;
}

export const OPEN_CAPTURE_EVENT = "netx:open-capture";

/** 다른 화면에서 캡처 분석을 연다. 이름을 주면 그 파일을 바로 연다. */
export function openCapture(name?: string) {
  window.dispatchEvent(new CustomEvent(OPEN_CAPTURE_EVENT, { detail: name ?? "" }));
}

/** tcpdump 인자에서 저장 파일 이름(-w 값)을 찾는다. */
export function captureNameOf(argv: string[]): string | null {
  const i = argv.indexOf("-w");
  const name = i >= 0 ? argv[i + 1] : undefined;
  return name && /^[A-Za-z0-9_-][A-Za-z0-9._-]*\.pcap$/.test(name) ? name : null;
}

export async function captureList(): Promise<CaptureFile[]> {
  if (!isTauri()) return [];
  return invoke<CaptureFile[]>("capture_list");
}

export async function captureOpen(name: string): Promise<Analysis> {
  return invoke<Analysis>("capture_open", { name });
}

export async function captureRows(name: string, text: string, mark: string, offset: number, limit: number): Promise<PacketPage> {
  return invoke<PacketPage>("capture_rows", { name, text, mark, offset, limit });
}

export async function capturePacket(name: string, no: number): Promise<PacketDetail> {
  return invoke<PacketDetail>("capture_packet", { name, no });
}

export async function captureFolder(): Promise<string> {
  if (!isTauri()) return "";
  return invoke<string>("capture_folder");
}
