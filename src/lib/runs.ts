// 실행 상태 보관소. 백엔드 이벤트는 실행 번호를 받기 전에 먼저 도착할 수 있으므로,
// 화면 컴포넌트와 따로 모든 실행의 이벤트를 모아 두고 화면은 번호로 찾아 읽는다.
import { useEffect, useState } from "react";
import { onRunEvents } from "./backend";
import type { RunState } from "./types";

const MAX_LINES = 6000;
// 메뉴를 옮겨도 앞서 본 결과를 다시 보여 주려고 넉넉히 둔다.
const KEEP_RUNS = 24;

const runs = new Map<number, RunState>();
const listeners = new Set<() => void>();
let scheduled = false;

function notify() {
  if (scheduled) return;
  scheduled = true;
  requestAnimationFrame(() => {
    scheduled = false;
    listeners.forEach((l) => l());
  });
}

function get(id: number): RunState {
  let run = runs.get(id);
  if (!run) {
    run = { run_id: id, title: "", active: true, steps: {}, results: {}, lines: [] };
    runs.set(id, run);
    for (const old of [...runs.keys()].slice(0, Math.max(0, runs.size - KEEP_RUNS))) runs.delete(old);
  }
  return run;
}

onRunEvents({
  output: (ev) => {
    const run = get(ev.run_id);
    if (run.lines.length < MAX_LINES) run.lines.push({ order: ev.order, stream: ev.stream, line: ev.line });
    notify();
  },
  step: (ev) => {
    const run = get(ev.run_id);
    const started_at = run.steps[ev.order]?.started_at ?? Date.now();
    run.steps[ev.order] = { status: ev.status, exit_code: ev.exit_code, elapsed_ms: ev.elapsed_ms, started_at };
    if (ev.status === "running") delete run.results[ev.order];
    notify();
  },
  result: (ev) => {
    get(ev.run_id).results[ev.order] = ev.result;
    notify();
  },
  done: (id) => {
    get(id).active = false;
    notify();
  },
});

/** 실행 번호에 해당하는 상태를 읽는다. 이벤트가 올 때마다 다시 그린다. */
export function useRun(id: number | null): RunState | null {
  const [, force] = useState(0);
  useEffect(() => {
    const l = () => force((v) => v + 1);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return id == null ? null : get(id);
}

/** 그 번호의 실행이 아직 보관되어 있는지 */
export function hasRun(id: number): boolean {
  return runs.has(id);
}

/** 보관된 실행을 읽는다(없으면 만들지 않는다). */
export function peekRun(id: number): RunState | null {
  return runs.get(id) ?? null;
}

let restored = 0;

/** 기록에서 읽은 실행을 보관소에 넣고 그 번호를 돌려준다. 실제 실행 번호와 겹치지 않게 음수를 쓴다. */
export function restoreRun(saved: RunState): number {
  restored -= 1;
  const id = restored;
  runs.set(id, { ...saved, run_id: id, active: false });
  for (const old of [...runs.keys()].slice(0, Math.max(0, runs.size - KEEP_RUNS))) runs.delete(old);
  notify();
  return id;
}
