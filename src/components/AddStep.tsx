import { useState } from "react";
import { checkCommand } from "../lib/backend";
import { levelRank } from "../lib/types";
import type { ToolSchema, UserLevel, ValidatedStep } from "../lib/types";
import { RiskBadge } from "./PlanCards";

const short = (label: string) => label.replace(/\s*\(.*\)\s*$/, "");

/** 도구의 사용 형식. 예: ping [옵션] <대상 주소> */
export function usage(tool: ToolSchema): string {
  const parts = tool.positional.map((p) => {
    const name = short(p.label) + (p.variadic ? "…" : "");
    return p.required ? `<${name}>` : `[${name}]`;
  });
  return [tool.tool, tool.options.length > 0 ? "[옵션]" : "", ...parts].filter(Boolean).join(" ");
}

interface Props {
  tools: ToolSchema[];
  level: UserLevel;
  /** 지금까지 추가한 명령 수와 한도 */
  count: number;
  max: number;
  onAdd: (text: string) => void;
}

/** 계획에 명령을 직접 추가한다. 도구의 사용 형식과 옵션을 보여 주고, [검증]으로 위험도와 예상을 미리 본다. */
export function AddStep({ tools, level, count, max, onAdd }: Props) {
  const usable = tools.filter((t) => levelRank(level) >= levelRank(t.min_level));
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [text, setText] = useState("");
  const [checked, setChecked] = useState<{ text: string; step: ValidatedStep } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const tool = usable.find((t) => t.tool === name) ?? null;

  function pick(next: string) {
    setName(next);
    setText(next ? `${next} ` : "");
    setChecked(null);
    setError(null);
  }

  function edit(next: string) {
    setText(next);
    setChecked(null);
    setError(null);
    // 직접 적은 첫 낱말이 도구 이름이면 그 도구의 형식을 보여 준다.
    const first = next.trim().split(/\s+/)[0] ?? "";
    if (usable.some((t) => t.tool === first)) setName(first);
  }

  async function check(): Promise<ValidatedStep | null> {
    setError(null);
    try {
      const step = await checkCommand(text, level);
      setChecked({ text, step });
      return step;
    } catch (e) {
      setChecked(null);
      setError(String(e));
      return null;
    }
  }

  async function add() {
    const step = checked?.text === text ? checked.step : await check();
    if (!step || step.blocked) return;
    onAdd(text.trim());
    setText(name ? `${name} ` : "");
    setChecked(null);
  }

  if (level === "general") return null;

  if (!open) {
    return (
      <button
        onClick={() => setOpen(true)}
        className="mt-4 w-full rounded-lg border border-dashed border-line px-4 py-3 text-sm text-dim hover:border-accent hover:text-accent"
      >
        ＋ 계획에 직접 추가 (도구와 옵션을 내가 정해서 넣기)
      </button>
    );
  }

  const step = checked?.text === text ? checked.step : null;
  const full = count >= max;

  return (
    <section className="mt-4 rounded-lg border border-line bg-panel p-4">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold">＋ 계획에 직접 추가</h2>
        <button onClick={() => setOpen(false)} className="text-xs text-dim hover:text-ink">
          접기
        </button>
      </div>
      <p className="mt-1 text-xs leading-relaxed text-dim">
        도구를 고르면 사용 형식과 옵션이 나옵니다. 옵션을 누르면 명령에 붙습니다. [검증]을 누르면 실행하지 않고 위험도, 옵션 풀이, 예상을 보여 줍니다.
        파이프(|)나 리다이렉션(&gt;)은 쓸 수 없습니다.
      </p>

      <div className="mt-3 flex gap-2">
        <select
          value={name}
          onChange={(e) => pick(e.target.value)}
          className="w-44 shrink-0 rounded-md border border-line bg-raised px-2 py-2 font-mono text-sm outline-none focus:border-accent"
        >
          <option value="">도구 선택</option>
          {usable.map((t) => (
            <option key={t.tool} value={t.tool}>
              {t.tool} · {t.title}
            </option>
          ))}
        </select>
        <input
          value={text}
          onChange={(e) => edit(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && check()}
          placeholder={tool ? usage(tool) : "예: ping -c 4 8.8.8.8"}
          spellCheck={false}
          className="min-w-0 flex-1 rounded-md border border-line bg-raised px-3 py-2 font-mono text-sm outline-none placeholder:text-dim/60 focus:border-accent"
        />
        <button
          onClick={check}
          disabled={text.trim() === ""}
          className="shrink-0 rounded-md border border-accent px-3 py-2 text-sm whitespace-nowrap text-accent disabled:border-line disabled:text-dim"
        >
          검증
        </button>
        <button
          onClick={add}
          disabled={text.trim() === "" || full || step?.blocked === true}
          title={full ? `직접 추가는 ${max}개까지입니다` : "검증을 통과하면 계획 맨 뒤에 붙습니다"}
          className="shrink-0 rounded-md bg-accent px-3 py-2 text-sm font-semibold whitespace-nowrap text-void disabled:bg-raised disabled:font-normal disabled:text-dim"
        >
          계획에 추가
        </button>
      </div>

      {tool && (
        <div className="mt-3 rounded-md bg-void px-3 py-2">
          <p className="font-mono text-sm">
            <span className="text-dim">형식 </span>
            {usage(tool)}
          </p>
          {tool.positional.length > 0 && (
            <ul className="mt-1 space-y-0.5 text-xs text-dim">
              {tool.positional.map((p) => (
                <li key={p.name}>
                  · {p.label}
                  {p.required ? " (필수)" : " (생략 가능)"}
                </li>
              ))}
            </ul>
          )}
          {tool.options.length > 0 && (
            <>
              <p className="mt-2 text-xs text-dim">옵션 (누르면 명령 끝에 붙습니다. 값이 필요한 옵션은 뒤에 값을 적으세요)</p>
              <div className="mt-1 flex max-h-36 flex-wrap gap-1.5 overflow-y-auto">
                {tool.options
                  .filter((o) => o.risk !== "forbidden")
                  .map((o) => (
                    <button
                      key={o.flag}
                      onClick={() => edit(`${text.trimEnd()} ${o.flag}${o.type === "boolean" ? "" : " "}`)}
                      title={`${o.label}${o.values.length > 0 ? ` · 값: ${o.values.join(", ")}` : ""}${o.risk !== "safe" ? " · 주의가 필요한 옵션" : ""}`}
                      className={`rounded border px-1.5 py-0.5 text-left text-xs hover:border-accent ${
                        o.risk === "safe" ? "border-line" : "border-caution/50"
                      }`}
                    >
                      <span className="font-mono text-accent">
                        {o.flag}
                        {o.type !== "boolean" && <span className="text-dim"> &lt;값&gt;</span>}
                      </span>
                      <span className="ml-1.5 text-dim">{o.label}</span>
                    </button>
                  ))}
              </div>
            </>
          )}
        </div>
      )}

      {error && <p className="mt-3 text-sm text-danger">⛔ {error}</p>}

      {step && (
        <div className={`mt-3 rounded-md border px-3 py-3 ${step.blocked ? "border-danger/50" : "border-safe/40"}`}>
          <div className="flex items-center gap-2 text-sm">
            <span className="font-semibold">{step.blocked ? "실행할 수 없는 명령입니다" : "검증 통과"}</span>
            {!step.blocked && <RiskBadge risk={step.risk} />}
            {step.needs_root && <span className="rounded-full border border-line px-2 py-0.5 text-xs text-dim">root로 실행</span>}
            {step.estimate_secs !== null && <span className="text-xs text-dim">예상 약 {step.estimate_secs}초</span>}
          </div>
          <pre className="mt-2 rounded bg-void px-3 py-2 font-mono text-sm break-all whitespace-pre-wrap">
            <span className="text-accent">$ </span>
            {step.display}
          </pre>
          {step.issues.length > 0 && (
            <ul className="mt-2 space-y-1 text-xs">
              {step.issues.map((issue, i) => (
                <li key={i} className={issue.severity === "block" ? "text-danger" : "text-caution"}>
                  {issue.severity === "block" ? "⛔ " : "⚠ "}
                  {issue.message}
                </li>
              ))}
            </ul>
          )}
          {step.explain.length > 0 && (
            <dl className="mt-2 space-y-0.5 text-xs">
              {step.explain.map((e) => (
                <div key={e.token} className="flex gap-2">
                  <dt className="w-32 shrink-0 font-mono text-accent">{e.token}</dt>
                  <dd className="text-dim">{e.label}</dd>
                </div>
              ))}
            </dl>
          )}
          {step.expect && !step.blocked && (
            <p className="mt-2 text-sm">
              <span className="text-dim">예상 </span>
              {step.expect}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
