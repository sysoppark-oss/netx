import { useEffect, useRef, useState } from "react";
import type { Risk, RunState, StepStatus, UserLevel, ValidatedPlan, ValidatedStep } from "../lib/types";

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
}

/** 검증을 통과한 계획을 제안 카드로 보여 주고 실행한다. */
export function PlanCards({ plan, level, run, disabledReason, onRun, onCancel }: Props) {
  // 확인을 기다리는 실행 대상. undefined는 없음, null은 전체, 숫자는 그 단계만.
  const [pending, setPending] = useState<number | null | undefined>(undefined);
  const runnable = plan.steps.filter((s) => !s.blocked);
  const busy = run?.active ?? false;
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
}: {
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
        <span className="flex-1 font-semibold">{step.title || step.tool}</span>
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

      {state && (
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
