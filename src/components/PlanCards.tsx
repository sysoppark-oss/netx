import { WordCard } from "./Word";
import { useEffect, useRef, useState } from "react";
import { killProcess, openTarget } from "../lib/backend";
import { openTerminal } from "../lib/term";
import type { Light, Proc, Risk, RunState, StepResult, StepStatus, Target, UserLevel, ValidatedPlan, ValidatedStep } from "../lib/types";
import { ChartView } from "./Charts";
import { TopologyView } from "./Topology";

const RISK_LABEL: Record<Risk, string> = { safe: "안전", caution: "주의", danger: "위험", forbidden: "금지" };
const RISK_STYLE: Record<Risk, string> = {
  safe: "border-safe/40 text-safe",
  caution: "border-caution/40 text-caution",
  danger: "border-danger/40 text-danger",
  forbidden: "border-danger/40 text-danger",
};

export function RiskBadge({ risk }: { risk: Risk }) {
  return (
    <span className={`shrink-0 rounded-full border px-2 py-0.5 text-xs ${RISK_STYLE[risk]}`}>{RISK_LABEL[risk]}</span>
  );
}

const STATUS_LABEL: Record<StepStatus, string> = {
  running: "실행 중",
  done: "완료",
  timeout: "시간 초과",
  cancelled: "중지됨",
  capped: "출력 과다로 중단",
  error: "실행 실패",
};

interface Props {
  plan: ValidatedPlan;
  level: UserLevel;
  run: RunState | null;
  /** 실행할 수 없는 이유. 실행할 수 있으면 null */
  disabledReason: string | null;
  /** onlyStep이 null이면 전체 실행 */
  onRun: (onlyStep: number | null, confirmed: boolean) => void;
  onCancel: () => void;
  /** 직접 추가한 단계를 뺀다 */
  onRemove?: (order: number) => void;
}

/** 검증을 통과한 계획을 제안 카드로 보여 주고 실행한다. */
export function PlanCards({ plan, level, run, disabledReason, onRun, onCancel, onRemove }: Props) {
  // 확인을 기다리는 실행 대상. undefined는 없음, null은 전체, 숫자는 그 단계만.
  const [pending, setPending] = useState<number | null | undefined>(undefined);
  const runnable = plan.steps.filter((s) => !s.blocked);
  const busy = run?.active ?? false;

  // 실행 중에는 경과 시간과 진행 표시줄을 갱신한다.
  const [, tick] = useState(0);
  useEffect(() => {
    if (!busy) return;
    const timer = setInterval(() => tick((v) => v + 1), 500);
    return () => clearInterval(timer);
  }, [busy]);
  const disabled = busy || disabledReason !== null;

  function request(only: number | null) {
    const targets = runnable.filter((s) => only === null || s.order === only);
    if (targets.some((s) => s.confirm !== "none")) setPending(only);
    else onRun(only, false);
  }

  const pendingSteps = pending === undefined ? [] : runnable.filter((s) => pending === null || s.order === pending);

  return (
    <section className="mt-6">
      <div className="mb-3 flex items-center justify-between gap-3">
        <p className="text-sm text-dim">
          {plan.steps.length}단계 · {plan.parallel ? "동시에 실행" : "차례로 실행"}
          {plan.blocked && <span className="ml-2 text-danger">차단된 단계가 있습니다</span>}
        </p>
        {busy ? (
          <button onClick={onCancel} className="rounded-md border border-danger/50 px-4 py-1.5 text-sm text-danger hover:bg-danger/10">
            ■ 중지
          </button>
        ) : (
          <button
            onClick={() => request(null)}
            disabled={disabled || runnable.length === 0}
            title={disabledReason ?? ""}
            className="rounded-md bg-accent px-4 py-1.5 text-sm font-semibold whitespace-nowrap text-void disabled:bg-raised disabled:font-normal disabled:text-dim"
          >
            ▶ 전체 실행 ({runnable.length})
          </button>
        )}
      </div>

      {disabledReason && <p className="mb-3 text-xs text-caution">{disabledReason}</p>}

      {pending !== undefined && (
        <div className="mb-3 rounded-md border border-caution/50 bg-caution/10 px-4 py-3 text-sm">
          <p className="font-semibold text-caution">실행 전에 확인하세요</p>
          <ul className="mt-1 space-y-0.5 text-xs">
            {pendingSteps.flatMap((s) =>
              s.issues.filter((i) => i.severity === "warn").map((i, n) => <li key={`${s.order}-${n}`}>· {i.message}</li>),
            )}
          </ul>
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => {
                onRun(pending, true);
                setPending(undefined);
              }}
              className="rounded-md bg-caution px-3 py-1 text-sm font-semibold text-void"
            >
              확인하고 실행
            </button>
            <button onClick={() => setPending(undefined)} className="rounded-md border border-line px-3 py-1 text-sm text-dim">
              취소
            </button>
          </div>
        </div>
      )}

      <ol className="space-y-3">
        {plan.steps.map((step) => (
          <StepCard
            key={step.order}
            step={step}
            level={level}
            run={run}
            disabled={disabled}
            disabledReason={disabledReason}
            onRun={() => request(step.order)}
            onRemove={onRemove && step.manual && !busy ? () => onRemove(step.order) : undefined}
          />
        ))}
      </ol>
    </section>
  );
}

function StepCard({
  step,
  level,
  run,
  disabled,
  disabledReason,
  onRun,
  onRemove,
}: {
  onRemove?: () => void;
  step: ValidatedStep;
  level: UserLevel;
  run: RunState | null;
  disabled: boolean;
  disabledReason: string | null;
  onRun: () => void;
}) {
  const showCommand = level !== "general";
  const state = run?.steps[step.order];
  const lines = run ? run.lines.filter((l) => l.order === step.order) : [];
  const result = run?.results[step.order];
  const outRef = useRef<HTMLPreElement>(null);

  // 새 출력이 오면 맨 아래로 내린다.
  useEffect(() => {
    if (outRef.current) outRef.current.scrollTop = outRef.current.scrollHeight;
  }, [lines.length]);

  return (
    <li className={`rounded-lg border bg-panel p-4 ${step.blocked ? "border-danger/50" : "border-line"}`}>
      <div className="flex items-center gap-3">
        <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-raised text-xs text-dim">
          {step.order}
        </span>
        <span className="flex-1 font-semibold">
          {step.title || step.tool}
          {step.manual && <span className="ml-2 rounded-full border border-accent/40 px-2 py-0.5 text-xs font-normal text-accent">직접 추가</span>}
        </span>
        {onRemove && (
          <button onClick={onRemove} title="이 단계를 계획에서 뺍니다" className="shrink-0 text-xs text-dim hover:text-danger">
            ✕ 빼기
          </button>
        )}
        {state && <StatusChip status={state.status} exitCode={state.exit_code} elapsedMs={state.elapsed_ms} />}
        {step.needs_root && showCommand && (
          <span title="WSL2 root 권한으로 실행합니다" className="shrink-0 rounded-full border border-line px-2 py-0.5 text-xs text-dim">
            root
          </span>
        )}
        {step.blocked ? (
          <span className="shrink-0 rounded-full border border-danger/40 px-2 py-0.5 text-xs text-danger">차단</span>
        ) : (
          <>
            <RiskBadge risk={step.risk} />
            <button
              onClick={onRun}
              disabled={disabled}
              title={disabledReason ?? "이 단계만 실행"}
              className="shrink-0 rounded-md border border-line px-3 py-1 text-xs hover:border-accent hover:text-accent disabled:text-dim disabled:hover:border-line"
            >
              ▶ 실행
            </button>
          </>
        )}
      </div>

      {showCommand && (
        <pre className="mt-3 overflow-x-auto rounded-md bg-void px-3 py-2 font-mono text-sm break-all whitespace-pre-wrap">
          <span className="text-accent">$ </span>
          {step.display}
        </pre>
      )}

      {showCommand && step.explain.length > 0 && (level === "junior" || step.manual) && (
        <dl className="mt-2 space-y-0.5 text-xs">
          {step.explain.map((e) => (
            <div key={e.token} className="flex gap-2">
              <dt className="w-32 shrink-0 font-mono text-accent">{e.token}</dt>
              <dd className="text-dim">{e.label}</dd>
            </div>
          ))}
        </dl>
      )}

      {step.reason && (
        <p className="mt-3 text-sm">
          <span className="text-dim">이유 </span>
          {step.reason}
        </p>
      )}
      {step.expect && (
        <p className="mt-1 text-sm">
          <span className="text-dim">예상 </span>
          {step.expect}
        </p>
      )}

      {step.issues.length > 0 && (
        <ul className="mt-3 space-y-1 text-xs">
          {step.issues.map((issue, i) => (
            <li key={i} className={issue.severity === "block" ? "text-danger" : "text-caution"}>
              {issue.severity === "block" ? "⛔ " : "⚠ "}
              {issue.message}
            </li>
          ))}
        </ul>
      )}

      {state?.status === "running" && (
        <>
          <Progress startedAt={state.started_at} estimateSecs={step.estimate_secs} />
          {/* 기다리는 동안에만 보인다. 결과와 원본 출력에는 섞지 않는다. */}
          <WordCard className="mt-3 border-l-2 border-line pl-3" />
        </>
      )}

      {result && state?.status !== "running" && <ResultView result={result} showFacts={showCommand} level={level} />}

      {state && showCommand && (
        <pre ref={outRef} className="mt-3 max-h-64 overflow-auto rounded-md border border-line bg-void px-3 py-2 font-mono text-xs leading-5 whitespace-pre-wrap">
          {lines.length === 0 ? (
            <span className="text-dim">{state.status === "running" ? "출력을 기다리는 중…" : "(출력 없음)"}</span>
          ) : (
            lines.slice(-300).map((l, i) => (
              <div key={i} className={l.stream === "stderr" ? "text-caution" : ""}>
                {l.line || " "}
              </div>
            ))
          )}
        </pre>
      )}
    </li>
  );
}

function StatusChip({ status, exitCode, elapsedMs }: { status: StepStatus; exitCode: number | null; elapsedMs: number }) {
  const failed = status === "error" || status === "timeout" || status === "capped";
  const nonzero = status === "done" && exitCode !== 0;
  const color =
    status === "running" ? "text-accent" : failed ? "text-danger" : nonzero || status === "cancelled" ? "text-caution" : "text-safe";
  return (
    <span className={`shrink-0 text-xs ${color}`}>
      {status === "running" && <span className="mr-1 inline-block animate-pulse">●</span>}
      {STATUS_LABEL[status]}
      {nonzero && ` (종료 코드 ${exitCode})`}
      {status !== "running" && ` · ${(elapsedMs / 1000).toFixed(1)}초`}
    </span>
  );
}

const LIMIT_SECS = 120;

/** 실행 중 진행 표시. 예상 시간을 알면 그에 맞춰 차오르고, 모르면 경과 시간과 제한 시간을 보여 준다. */
function Progress({ startedAt, estimateSecs }: { startedAt: number; estimateSecs: number | null }) {
  const elapsed = Math.max(0, (Date.now() - startedAt) / 1000);
  const known = estimateSecs !== null;
  const ratio = known ? Math.min(0.97, elapsed / estimateSecs) : Math.min(1, elapsed / LIMIT_SECS);
  const over = known && elapsed > estimateSecs + 3;
  return (
    <div className="mt-3">
      <div className="h-1.5 overflow-hidden rounded-full bg-raised">
        <div
          className={`h-full rounded-full transition-[width] duration-500 ${known ? "bg-accent" : "animate-pulse bg-accent/60"}`}
          style={{ width: `${Math.max(2, ratio * 100)}%` }}
        />
      </div>
      <p className="mt-1 flex justify-between text-xs text-dim">
        <span>
          {elapsed.toFixed(0)}초 경과
          {known && !over && ` · 약 ${Math.max(0, Math.ceil(estimateSecs - elapsed))}초 남음`}
          {over && " · 예상보다 오래 걸리고 있습니다(대상이 응답하지 않을 수 있습니다)"}
        </span>
        <span>
          {known ? `예상 ${estimateSecs}초 · ` : "끝나는 시간을 알 수 없는 작업 · "}최대 {LIMIT_SECS}초에 자동 중단
        </span>
      </p>
    </div>
  );
}

const LIGHT_MARK: Record<Light, string> = { green: "🟢", yellow: "🟡", red: "🔴", info: "🔵" };
const LIGHT_BOX: Record<Light, string> = {
  green: "border-safe/40 bg-safe/10",
  yellow: "border-caution/40 bg-caution/10",
  red: "border-danger/40 bg-danger/10",
  info: "border-line bg-raised/50",
};

/** 결과 해설: 한 줄 결론, 핵심 수치, 그래프. */
function ResultView({ result, showFacts, level }: { result: StepResult; showFacts: boolean; level: UserLevel }) {
  return (
    <div className={`mt-3 rounded-md border px-3 py-3 ${LIGHT_BOX[result.light]}`}>
      <p className="flex gap-2 text-sm leading-relaxed">
        <span className="shrink-0">{LIGHT_MARK[result.light]}</span>
        <span>
          <span className="text-dim">결과 </span>
          {result.text}
        </span>
      </p>
      {showFacts && result.facts.length > 0 && (
        <dl className="mt-2 flex flex-wrap gap-x-5 gap-y-1 pl-7 text-xs">
          {result.facts.map((f, i) => (
            <div key={i} className="flex gap-1.5">
              <dt className="text-dim">{f.label}</dt>
              <dd className="font-mono">{f.value}</dd>
            </div>
          ))}
        </dl>
      )}
      {result.charts.length > 0 && (
        <div className="mt-3 space-y-2">
          {result.charts.map((c, i) => (
            <ChartView key={i} chart={c} />
          ))}
        </div>
      )}
      {result.topology && <TopologyView topology={result.topology} />}
      {result.targets.length > 0 && <TargetList targets={result.targets} allowTerminal={showFacts} />}
      {result.processes.length > 0 && showFacts && <ProcessList processes={result.processes} level={level} />}
    </div>
  );
}

/** 포트를 쓰고 있는 Windows 프로세스 목록과 종료 버튼. 종료는 Windows Hello 확인을 거친다. */
function ProcessList({ processes, level }: { processes: Proc[]; level: UserLevel }) {
  const canKill = level === "middle" || level === "senior" || level === "op";
  // 종료를 물어보는 중인 프로세스
  const [asking, setAsking] = useState<Proc | null>(null);
  const [needName, setNeedName] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [gone, setGone] = useState<number[]>([]);

  function ask(p: Proc) {
    setAsking(p);
    setNeedName(null);
    setTyped("");
    setMessage(null);
  }

  async function kill() {
    if (!asking) return;
    setBusy(true);
    try {
      const r = await killProcess(asking.pid, needName !== null ? typed : null, level);
      if (r.status === "killed") {
        setGone((g) => [...g, asking.pid]);
        setAsking(null);
        setMessage({ ok: true, text: r.message });
      } else if (r.status === "need_name") {
        setNeedName(r.name);
        setMessage({ ok: false, text: r.message });
      } else {
        setAsking(null);
        setMessage({ ok: false, text: r.message });
      }
    } catch (e) {
      setAsking(null);
      setMessage({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-3 rounded-md border border-line bg-void px-3 py-2">
      <p className="text-xs font-semibold">
        포트를 쓰고 있는 프로그램
        {!canKill && <span className="ml-2 font-normal text-dim">종료는 중급 엔지니어 이상 등급에서 할 수 있습니다.</span>}
      </p>
      {asking && (
        <div className="mt-2 rounded-md border border-danger/50 bg-danger/10 px-3 py-2 text-xs">
          <p className="font-semibold text-danger">
            {asking.name} (PID {asking.pid})을(를) 강제로 종료할까요?
          </p>
          <p className="mt-1 text-dim">
            저장하지 않은 작업은 사라지고 되돌릴 수 없습니다. [종료]를 누르면 Windows Hello(지문, 얼굴, PIN) 확인 창이 뜹니다.
          </p>
          {needName !== null && (
            <label className="mt-2 flex items-center gap-2">
              프로세스 이름 입력
              <input
                value={typed}
                onChange={(e) => setTyped(e.target.value)}
                placeholder={needName}
                spellCheck={false}
                autoFocus
                className="w-48 rounded border border-line bg-raised px-2 py-0.5 font-mono text-ink outline-none focus:border-danger"
              />
            </label>
          )}
          <div className="mt-2 flex gap-2">
            <button
              onClick={kill}
              disabled={busy || (needName !== null && typed.trim() === "")}
              className="rounded bg-danger px-3 py-1 font-semibold text-void disabled:opacity-50"
            >
              {busy ? "확인을 기다리는 중…" : "종료"}
            </button>
            <button onClick={() => setAsking(null)} disabled={busy} className="rounded border border-line px-3 py-1 text-dim">
              취소
            </button>
          </div>
        </div>
      )}
      {message && <p className={`mt-1 text-xs ${message.ok ? "text-safe" : "text-caution"}`}>{message.text}</p>}
      <ul className="mt-2 max-h-56 space-y-1 overflow-y-auto">
        {processes.map((p) => {
          const ended = gone.includes(p.pid);
          return (
            <li key={p.pid} className={`flex items-center gap-3 text-xs ${ended ? "opacity-40" : ""}`}>
              <span className="w-40 shrink-0 truncate font-mono">{p.name}</span>
              <span className="w-20 shrink-0 font-mono text-dim">PID {p.pid}</span>
              <span className="min-w-0 flex-1 truncate font-mono text-accent">{p.ports}</span>
              {ended ? (
                <span className="shrink-0 text-dim">종료됨</span>
              ) : (
                canKill && (
                  <button
                    onClick={() => ask(p)}
                    disabled={busy}
                    className="shrink-0 rounded border border-danger/50 px-2 py-0.5 text-danger hover:bg-danger/10 disabled:opacity-50"
                  >
                    종료
                  </button>
                )
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

const WEB_PORTS = new Set([80, 443, 8000, 8080, 8081, 8443, 8888, 9443]);

/** 열린 포트 목록. 웹은 브라우저로, SSH·Telnet은 NetX 터미널 창으로 바로 접속해 본다. */
function TargetList({ targets, allowTerminal }: { targets: Target[]; allowTerminal: boolean }) {
  const [user, setUser] = useState("");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const hasSsh = allowTerminal && targets.some((t) => t.port === 22 || t.service === "SSH");

  async function open(t: Target, kind: "web" | "ssh" | "telnet") {
    try {
      if (kind === "web") setMessage({ ok: true, text: `${t.host}:${t.port} · ${await openTarget(t.host, t.port, kind, null)}` });
      else {
        await openTerminal({ kind, host: t.host, port: t.port, user: kind === "ssh" ? user.trim() : null });
        setMessage({ ok: true, text: `${t.host}:${t.port} · NetX 터미널 창에서 엽니다. 암호는 그 창에서 입력하세요.` });
      }
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    }
  }

  const button = "rounded border border-line px-2 py-0.5 text-xs hover:border-accent hover:text-accent disabled:text-dim disabled:hover:border-line";
  return (
    <div className="mt-3 rounded-md border border-line bg-void px-3 py-2">
      <div className="flex flex-wrap items-center gap-3">
        <p className="text-xs font-semibold">바로 접속해 보기</p>
        {hasSsh && (
          <label className="flex items-center gap-1.5 text-xs text-dim">
            SSH 사용자
            <input
              value={user}
              onChange={(e) => setUser(e.target.value)}
              placeholder="예: admin"
              spellCheck={false}
              className="w-28 rounded border border-line bg-raised px-2 py-0.5 font-mono text-xs text-ink outline-none focus:border-accent"
            />
          </label>
        )}
        {hasSsh && <span className="text-xs text-dim">암호는 열리는 NetX 터미널 창에서 직접 입력합니다.</span>}
      </div>
      {message && <p className={`mt-1 text-xs ${message.ok ? "text-safe" : "text-danger"}`}>{message.text}</p>}
      <ul className="mt-2 max-h-56 space-y-1 overflow-y-auto">
        {targets.map((t) => {
          const ssh = t.port === 22 || t.service === "SSH";
          const telnet = t.port === 23 || t.service === "Telnet";
          const web = WEB_PORTS.has(t.port) || t.service.startsWith("HTTP");
          return (
            <li key={`${t.host}:${t.port}`} className="flex items-center gap-3 text-xs">
              <span className="w-32 shrink-0 font-mono">{t.host}</span>
              <span className="w-14 shrink-0 font-mono text-accent">{t.port}</span>
              <span className="w-20 shrink-0 text-dim">{t.service || "-"}</span>
              <span className="flex gap-1.5">
                {(web || (!ssh && !telnet)) && (
                  <button onClick={() => open(t, "web")} className={button} title="기본 브라우저에서 엽니다">
                    웹 열기
                  </button>
                )}
                {allowTerminal && ssh && (
                  <button
                    onClick={() => open(t, "ssh")}
                    disabled={user.trim() === ""}
                    className={button}
                    title={user.trim() === "" ? "위에 SSH 사용자 이름을 먼저 적으세요" : "NetX 터미널 창에서 SSH로 접속합니다"}
                  >
                    터미널 접속 (SSH)
                  </button>
                )}
                {allowTerminal && telnet && (
                  <button onClick={() => open(t, "telnet")} className={button} title="NetX 터미널 창에서 Telnet으로 접속합니다 (내장 Telnet, 암호화되지 않음)">
                    터미널 접속 (Telnet)
                  </button>
                )}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
