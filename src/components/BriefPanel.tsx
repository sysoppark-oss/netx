import type { Briefing } from "../lib/types";

const VERDICT = {
  normal: { label: "정상", style: "border-safe/50 bg-safe/10", text: "text-safe" },
  problem: { label: "문제 발견", style: "border-danger/50 bg-danger/10", text: "text-danger" },
  unclear: { label: "판단 보류", style: "border-caution/50 bg-caution/10", text: "text-caution" },
} as const;

const LIGHT = { green: "🟢", yellow: "🟡", red: "🔴" } as const;

/** 실행 결과에 대한 AI 요약과 판정. */
export function BriefPanel({ brief }: { brief: { loading: boolean; data?: Briefing; error?: string } }) {
  if (brief.loading) {
    return (
      <section className="mt-6 rounded-lg border border-line bg-panel px-4 py-3 text-sm text-dim">
        <span className="mr-2 inline-block animate-pulse text-accent">●</span>AI가 결과를 읽고 있습니다…
      </section>
    );
  }
  if (brief.error || !brief.data) {
    return (
      <section className="mt-6 rounded-lg border border-line bg-panel px-4 py-3 text-sm text-dim">
        AI 브리핑을 받지 못했습니다. {brief.error} 아래 단계별 출력을 직접 확인하세요.
      </section>
    );
  }

  const b = brief.data;
  const v = VERDICT[b.verdict];
  return (
    <section className={`mt-6 rounded-lg border px-4 py-4 ${v.style}`}>
      <p className={`text-xs font-semibold ${v.text}`}>AI 브리핑 · {v.label}</p>
      <p className="mt-1 text-base leading-relaxed font-semibold">{b.headline}</p>
      {b.findings.length > 0 && (
        <ul className="mt-3 space-y-1 text-sm">
          {b.findings.map((f, i) => (
            <li key={i} className="flex gap-2">
              <span className="shrink-0">{LIGHT[f.light]}</span>
              <span>
                <span className="text-dim">{f.order}단계 </span>
                {f.text}
              </span>
            </li>
          ))}
        </ul>
      )}
      {b.next && (
        <p className="mt-3 text-sm">
          <span className="text-dim">다음 </span>
          {b.next}
        </p>
      )}
      <p className="mt-3 text-xs text-dim">AI가 출력을 읽고 쓴 요약입니다. 틀릴 수 있으니 아래 원본 출력과 함께 보세요.</p>
    </section>
  );
}
