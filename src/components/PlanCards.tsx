import type { Risk, UserLevel, ValidatedPlan, ValidatedStep } from "../lib/types";

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

/** 검증을 통과한 계획을 제안 카드로 보여 준다. 실행은 Phase 3에서 연결한다. */
export function PlanCards({ plan, level }: { plan: ValidatedPlan; level: UserLevel }) {
  const runnable = plan.steps.filter((s) => !s.blocked).length;
  return (
    <section className="mt-6">
      <div className="mb-3 flex items-center justify-between">
        <p className="text-sm text-dim">
          {plan.steps.length}단계 · {plan.parallel ? "동시에 실행" : "차례로 실행"}
          {plan.blocked && <span className="ml-2 text-danger">차단된 단계가 있습니다</span>}
        </p>
        <button
          disabled
          title="실행 엔진은 Phase 3에서 연결됩니다"
          className="rounded-md border border-line bg-raised px-4 py-1.5 text-sm text-dim"
        >
          ▶ 전체 실행 ({runnable})
        </button>
      </div>
      <ol className="space-y-3">
        {plan.steps.map((step) => (
          <StepCard key={step.order} step={step} level={level} />
        ))}
      </ol>
    </section>
  );
}

function StepCard({ step, level }: { step: ValidatedStep; level: UserLevel }) {
  const showCommand = level !== "general";
  return (
    <li className={`rounded-lg border bg-panel p-4 ${step.blocked ? "border-danger/50" : "border-line"}`}>
      <div className="flex items-center gap-3">
        <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-raised text-xs text-dim">
          {step.order}
        </span>
        <span className="flex-1 font-semibold">{step.title || step.tool}</span>
        {step.needs_root && showCommand && (
          <span title="WSL2 root 권한이 필요합니다" className="shrink-0 rounded-full border border-line px-2 py-0.5 text-xs text-dim">
            root
          </span>
        )}
        {step.blocked ? (
          <span className="shrink-0 rounded-full border border-danger/40 px-2 py-0.5 text-xs text-danger">차단</span>
        ) : (
          <RiskBadge risk={step.risk} />
        )}
      </div>

      {showCommand && (
        <pre className="mt-3 overflow-x-auto rounded-md bg-void px-3 py-2 font-mono text-sm whitespace-pre-wrap break-all">
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
    </li>
  );
}
