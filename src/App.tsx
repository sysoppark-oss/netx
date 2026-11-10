import { useEffect, useState } from "react";
import { getAppInfo, getEnvironmentStatus } from "./lib/backend";
import { RECIPES, TOOLS, USER_LEVELS } from "./lib/types";
import type { AppInfo, EnvironmentStatus, UiMode, UserLevel } from "./lib/types";

// Phase 1 화면 뼈대. 배치는 docs/DEV_PLAN_v0.1.md 3장을 따른다.
// 제안 카드, 옵션 패널, 실행은 Phase 4~5에서 채운다.

export default function App() {
  const [level, setLevel] = useState<UserLevel>("middle");
  const [mode, setMode] = useState<UiMode>("easy");
  const [situation, setSituation] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [env, setEnv] = useState<EnvironmentStatus | null>(null);
  const [rawOpen, setRawOpen] = useState(false);

  useEffect(() => {
    getAppInfo().then(setInfo).catch(() => setInfo(null));
    getEnvironmentStatus().then(setEnv).catch(() => setEnv(null));
  }, []);

  const levelInfo = USER_LEVELS.find((l) => l.id === level)!;
  const showOptions = level !== "general";

  return (
    <div className="flex h-full flex-col">
      {/* 상단 헤더 */}
      <header className="flex items-center gap-4 border-b border-line bg-panel px-5 py-3">
        <div className="flex items-baseline gap-2">
          <span className="text-lg font-bold tracking-tight text-accent">NetX</span>
          <span className="text-xs text-dim">v{info?.version ?? "…"}</span>
        </div>

        <form
          className="flex flex-1"
          onSubmit={(e) => {
            e.preventDefault();
          }}
        >
          <input
            value={situation}
            onChange={(e) => setSituation(e.target.value)}
            placeholder="상황을 입력하세요. 예: 회사 홈페이지가 가끔 느려요"
            className="w-full rounded-l-md border border-line bg-raised px-4 py-2 text-sm outline-none placeholder:text-dim focus:border-accent"
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

        <div className="flex overflow-hidden rounded-md border border-line text-sm">
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
                {RECIPES.map((r) => (
                  <li key={r.id}>
                    <button className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-raised">
                      <span className="text-base">{r.icon}</span>
                      <span className="flex-1">{r.title}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <>
              <h2 className="mb-2 px-1 text-xs font-semibold text-dim">도구</h2>
              <ul className="space-y-1">
                {TOOLS.map((t) => (
                  <li key={t.id}>
                    <button className="w-full rounded-md px-2 py-2 text-left font-mono text-sm hover:bg-raised">
                      {t.title}
                    </button>
                  </li>
                ))}
              </ul>
            </>
          )}
        </aside>

        {/* 중앙: 제안 카드 영역 */}
        <main className="flex min-w-0 flex-1 flex-col overflow-y-auto p-6">
          <div className="m-auto max-w-lg text-center">
            <p className="text-2xl font-semibold">무엇이 문제인가요?</p>
            <p className="mt-3 text-sm leading-relaxed text-dim">
              위 입력창에 상황을 적거나 왼쪽 레시피를 고르면, 해결에 쓸 도구의 우선순위와 옵션 조합이
              이유·예상 결과와 함께 여기에 표시됩니다.
            </p>
            <p className="mt-6 text-xs text-dim">
              현재 등급: <span className="text-ink">{levelInfo.label}</span> · {levelInfo.hint}
            </p>
          </div>
        </main>

        {/* 우측: 동적 옵션 패널 */}
        {showOptions && mode === "expert" && (
          <aside className="w-80 shrink-0 overflow-y-auto border-l border-line bg-panel p-4">
            <h2 className="text-xs font-semibold text-dim">옵션 패널</h2>
            <p className="mt-3 text-sm text-dim">왼쪽에서 도구를 고르면 옵션이 표시됩니다.</p>
          </aside>
        )}
      </div>

      {/* 하단: 명령 미리보기, 브리핑, 원본 출력 */}
      <footer className="border-t border-line bg-panel">
        {showOptions && (
          <div className="flex items-center gap-3 border-b border-line px-5 py-2 font-mono text-sm">
            <span className="text-accent">$</span>
            <span className="text-dim">실행할 명령이 여기에 조립됩니다</span>
          </div>
        )}
        <div className="flex items-center justify-between px-5 py-2 text-sm">
          <span className="text-dim">AI 브리핑과 판정이 이곳에 표시됩니다</span>
          {showOptions && (
            <button onClick={() => setRawOpen((v) => !v)} className="text-xs text-dim hover:text-ink">
              원본 출력 {rawOpen ? "접기 ▾" : "펼치기 ▴"}
            </button>
          )}
        </div>
        {showOptions && rawOpen && (
          <pre className="h-48 overflow-auto border-t border-line bg-void px-5 py-3 font-mono text-xs text-dim">
            (아직 실행 기록이 없습니다)
          </pre>
        )}
      </footer>
    </div>
  );
}

function EnvBadge({ env }: { env: EnvironmentStatus | null }) {
  if (!env) {
    return <span className="text-xs text-dim">환경 확인 중…</span>;
  }
  const full = env.mode === "full";
  return (
    <span
      title={env.message}
      className={`flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-full border px-3 py-1 text-xs ${
        full ? "border-safe/40 text-safe" : "border-caution/40 text-caution"
      }`}
    >
      <span className={`h-2 w-2 rounded-full ${full ? "bg-safe" : "bg-caution"}`} />
      {full ? "전체 진단" : "기본 진단"}
    </span>
  );
}
