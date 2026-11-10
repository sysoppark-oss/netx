import { useEffect, useState } from "react";
import { PlanCards, RiskBadge } from "./components/PlanCards";
import { getAppInfo, getEnvironmentStatus, getRecipes, getTools, planFromRecipe } from "./lib/backend";
import { USER_LEVELS, levelRank } from "./lib/types";
import type {
  AppInfo,
  EnvironmentStatus,
  Recipe,
  ToolSchema,
  UiMode,
  UserLevel,
  ValidatedPlan,
} from "./lib/types";

// 화면 배치는 docs/DEV_PLAN_v0.1.md 3장을 따른다.
// Phase 2: 레시피 → 검증된 제안 카드, 도구 옵션 목록 보기. 실행은 Phase 3, AI 제안은 Phase 5.

export default function App() {
  const [level, setLevel] = useState<UserLevel>("middle");
  const [mode, setMode] = useState<UiMode>("easy");
  const [situation, setSituation] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [env, setEnv] = useState<EnvironmentStatus | null>(null);
  const [recipes, setRecipes] = useState<Recipe[]>([]);
  const [tools, setTools] = useState<ToolSchema[]>([]);
  const [recipe, setRecipe] = useState<Recipe | null>(null);
  const [tool, setTool] = useState<ToolSchema | null>(null);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [plan, setPlan] = useState<ValidatedPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rawOpen, setRawOpen] = useState(false);

  useEffect(() => {
    getAppInfo().then(setInfo).catch(() => setInfo(null));
    getEnvironmentStatus().then(setEnv).catch(() => setEnv(null));
    getRecipes().then(setRecipes).catch(() => setRecipes([]));
    getTools().then(setTools).catch(() => setTools([]));
  }, []);

  // 등급이 바뀌면 허용 범위가 달라지므로 계획을 다시 만들게 한다.
  useEffect(() => {
    setPlan(null);
    setError(null);
    if (level === "general") setMode("easy");
  }, [level]);

  const levelInfo = USER_LEVELS.find((l) => l.id === level)!;
  const showCommands = level !== "general";
  const allowed = (min: UserLevel) => levelRank(level) >= levelRank(min);

  function pickRecipe(r: Recipe) {
    setRecipe(r);
    setInputs({});
    setPlan(null);
    setError(null);
  }

  async function makePlan() {
    if (!recipe) return;
    setError(null);
    try {
      setPlan(await planFromRecipe(recipe.id, inputs, level));
    } catch (e) {
      setPlan(null);
      setError(String(e));
    }
  }

  return (
    <div className="flex h-full flex-col">
      {/* 상단 헤더 */}
      <header className="flex items-center gap-4 border-b border-line bg-panel px-5 py-3">
        <div className="flex shrink-0 items-baseline gap-2">
          <span className="text-lg font-bold tracking-tight text-accent">NetX</span>
          <span className="text-xs text-dim">v{info?.version ?? "…"}</span>
        </div>

        <form className="flex min-w-0 flex-1" onSubmit={(e) => e.preventDefault()}>
          <input
            value={situation}
            onChange={(e) => setSituation(e.target.value)}
            placeholder="상황을 입력하세요. 예: 회사 홈페이지가 가끔 느려요"
            className="w-full min-w-0 rounded-l-md border border-line bg-raised px-4 py-2 text-sm outline-none placeholder:text-dim focus:border-accent"
          />
          <button
            type="submit"
            disabled
            title="AI 제안은 Phase 5에서 연결됩니다"
            className="shrink-0 whitespace-nowrap rounded-r-md border border-l-0 border-line bg-raised px-4 text-sm text-dim"
          >
            제안 받기
          </button>
        </form>

        <label className="flex shrink-0 items-center gap-2 whitespace-nowrap text-sm">
          <span className="text-dim">등급</span>
          <select
            value={level}
            onChange={(e) => setLevel(e.target.value as UserLevel)}
            className="rounded-md border border-line bg-raised px-2 py-1.5 text-sm outline-none focus:border-accent"
          >
            {USER_LEVELS.map((l) => (
              <option key={l.id} value={l.id}>
                {l.label}
              </option>
            ))}
          </select>
        </label>

        <div className="flex shrink-0 overflow-hidden rounded-md border border-line text-sm">
          {(["easy", "expert"] as UiMode[]).map((m) => (
            <button
              key={m}
              onClick={() => setMode(m)}
              disabled={m === "expert" && level === "general"}
              className={`px-3 py-1.5 ${mode === m ? "bg-accent font-semibold text-void" : "bg-raised text-dim"} disabled:opacity-40`}
            >
              {m === "easy" ? "Easy" : "Expert"}
            </button>
          ))}
        </div>

        <EnvBadge env={env} />
      </header>

      <div className="flex min-h-0 flex-1">
        {/* 좌측 패널 */}
        <aside className="w-60 shrink-0 overflow-y-auto border-r border-line bg-panel p-3">
          {mode === "easy" ? (
            <>
              <h2 className="mb-2 px-1 text-xs font-semibold text-dim">상황 레시피</h2>
              <ul className="space-y-1">
                {recipes.map((r) => {
                  const ok = allowed(r.min_level);
                  return (
                    <li key={r.id}>
                      <button
                        onClick={() => pickRecipe(r)}
                        disabled={!ok}
                        title={ok ? r.description : "현재 등급에서는 쓸 수 없습니다"}
                        className={`flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-raised disabled:opacity-40 disabled:hover:bg-transparent ${
                          recipe?.id === r.id ? "bg-raised" : ""
                        }`}
                      >
                        <span className="text-base">{r.icon}</span>
                        <span className="flex-1">{r.title}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            </>
          ) : (
            <>
              <h2 className="mb-2 px-1 text-xs font-semibold text-dim">도구</h2>
              <ul className="space-y-1">
                {tools.map((t) => (
                  <li key={t.tool}>
                    <button
                      onClick={() => setTool(t)}
                      disabled={!allowed(t.min_level)}
                      title={t.description}
                      className={`w-full rounded-md px-2 py-2 text-left text-sm hover:bg-raised disabled:opacity-40 ${
                        tool?.tool === t.tool ? "bg-raised" : ""
                      }`}
                    >
                      <span className="font-mono">{t.tool}</span>
                      <span className="ml-2 text-xs text-dim">{t.options.length}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
        </aside>

        {/* 중앙: 제안 카드 영역 */}
        <main className="min-w-0 flex-1 overflow-y-auto p-6">
          {mode === "easy" && recipe ? (
            <div className="mx-auto max-w-3xl">
              <h1 className="text-xl font-semibold">
                {recipe.icon} {recipe.title}
              </h1>
              <p className="mt-1 text-sm text-dim">{recipe.description}</p>

              <form
                className="mt-5 flex flex-wrap items-end gap-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  makePlan();
                }}
              >
                {recipe.inputs.map((input) => (
                  <label key={input.name} className="flex min-w-48 flex-1 flex-col gap-1 text-xs text-dim">
                    {input.label}
                    <input
                      value={inputs[input.name] ?? ""}
                      onChange={(e) => setInputs({ ...inputs, [input.name]: e.target.value })}
                      placeholder={input.example ?? ""}
                      spellCheck={false}
                      className="rounded-md border border-line bg-raised px-3 py-2 font-mono text-sm text-ink outline-none placeholder:text-dim/60 focus:border-accent"
                    />
                  </label>
                ))}
                <button
                  type="submit"
                  className="shrink-0 rounded-md bg-accent px-4 py-2 text-sm font-semibold whitespace-nowrap text-void"
                >
                  계획 보기
                </button>
              </form>

              {error && <p className="mt-4 rounded-md border border-danger/40 px-3 py-2 text-sm text-danger">{error}</p>}
              {plan && <PlanCards plan={plan} level={level} />}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center">
              <div className="max-w-lg text-center">
                <p className="text-2xl font-semibold">무엇이 문제인가요?</p>
                <p className="mt-3 text-sm leading-relaxed text-dim">
                  {mode === "easy"
                    ? "왼쪽에서 상황 레시피를 고르면, 실행할 도구와 옵션 조합이 이유·예상 결과와 함께 표시됩니다."
                    : "왼쪽에서 도구를 고르면 오른쪽에 쓸 수 있는 옵션이 표시됩니다."}
                </p>
                <p className="mt-6 text-xs text-dim">
                  현재 등급: <span className="text-ink">{levelInfo.label}</span> · {levelInfo.hint}
                </p>
              </div>
            </div>
          )}
        </main>

        {/* 우측: 옵션 패널 (Phase 2에서는 목록 보기만) */}
        {showCommands && mode === "expert" && (
          <aside className="w-96 shrink-0 overflow-y-auto border-l border-line bg-panel p-4">
            {tool ? <OptionList tool={tool} /> : <p className="text-sm text-dim">왼쪽에서 도구를 고르면 옵션이 표시됩니다.</p>}
          </aside>
        )}
      </div>

      {/* 하단: 명령 미리보기, 브리핑, 원본 출력 */}
      <footer className="border-t border-line bg-panel">
        {showCommands && (
          <div className="flex items-center gap-3 border-b border-line px-5 py-2 font-mono text-sm">
            <span className="text-accent">$</span>
            <span className="truncate text-dim">
              {plan && mode === "easy" ? plan.steps.map((s) => s.display).join("  ·  ") : "실행할 명령이 여기에 조립됩니다"}
            </span>
          </div>
        )}
        <div className="flex items-center justify-between px-5 py-2 text-sm">
          <span className="text-dim">AI 브리핑과 판정이 이곳에 표시됩니다</span>
          {showCommands && (
            <button onClick={() => setRawOpen((v) => !v)} className="text-xs text-dim hover:text-ink">
              원본 출력 {rawOpen ? "접기 ▾" : "펼치기 ▴"}
            </button>
          )}
        </div>
        {showCommands && rawOpen && (
          <pre className="h-48 overflow-auto border-t border-line bg-void px-5 py-3 font-mono text-xs text-dim">
            (아직 실행 기록이 없습니다)
          </pre>
        )}
      </footer>
    </div>
  );
}

function OptionList({ tool }: { tool: ToolSchema }) {
  const groups = new Map<string, ToolSchema["options"]>();
  for (const o of tool.options) {
    const key = o.risk === "forbidden" ? "금지 (실행 불가)" : (o.group ?? "기타");
    groups.set(key, [...(groups.get(key) ?? []), o]);
  }
  return (
    <>
      <h2 className="font-semibold">{tool.title}</h2>
      <p className="mt-1 text-xs leading-relaxed text-dim">{tool.description}</p>
      {[...groups.entries()].map(([group, options]) => (
        <section key={group} className="mt-4">
          <h3 className="mb-1 text-xs font-semibold text-dim">{group}</h3>
          <ul className="space-y-1">
            {options.map((o) => (
              <li key={o.flag} className="flex items-start gap-2 text-sm">
                <code className="w-28 shrink-0 font-mono text-xs leading-5 break-all text-accent">{o.flag}</code>
                <span className={`flex-1 text-xs leading-5 ${o.risk === "forbidden" ? "text-dim line-through" : ""}`}>
                  {o.label}
                </span>
                {o.risk !== "safe" && <RiskBadge risk={o.risk} />}
              </li>
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}

function EnvBadge({ env }: { env: EnvironmentStatus | null }) {
  if (!env) return <span className="shrink-0 text-xs text-dim">환경 확인 중…</span>;
  const full = env.mode === "full";
  return (
    <span
      title={env.message}
      className={`flex shrink-0 items-center gap-1.5 rounded-full border px-3 py-1 text-xs whitespace-nowrap ${
        full ? "border-safe/40 text-safe" : "border-caution/40 text-caution"
      }`}
    >
      <span className={`h-2 w-2 rounded-full ${full ? "bg-safe" : "bg-caution"}`} />
      {full ? "전체 진단" : "기본 진단"}
    </span>
  );
}
