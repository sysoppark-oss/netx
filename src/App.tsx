import { CaptureView } from "./components/CaptureView";
import { OPEN_CAPTURE_EVENT } from "./lib/capture";
import { useEffect, useRef, useState } from "react";
import { AddStep } from "./components/AddStep";
import { BriefPanel } from "./components/BriefPanel";
import { NetBar } from "./components/NetBar";
import { UpdateBar } from "./components/UpdateBar";
import { AboutDialog, WordCard } from "./components/Word";
import { openTerminal } from "./lib/term";
import { SetupWizard, markSetupDone, saveLevel, savedLevel, setupDone } from "./components/SetupWizard";
import { PlanCards, RiskBadge } from "./components/PlanCards";
import { SettingsDialog } from "./components/SettingsDialog";
import {
  briefRun,
  cancelRun,
  getAppInfo,
  getEnvironmentStatus,
  getRecipes,
  getSettings,
  getToolStatus,
  getTools,
  installTools,
  listInterfaces,
  planMixed,
  runMixed,
  suggest,
} from "./lib/backend";
import { useRun } from "./lib/runs";
import { USER_LEVELS, levelRank } from "./lib/types";
import type {
  AppInfo,
  Base,
  Briefing,
  EnvironmentStatus,
  Recipe,
  SettingsView,
  ToolSchema,
  UiMode,
  UserLevel,
  ValidatedPlan,
} from "./lib/types";

// 화면 배치는 docs/DEV_PLAN_v0.1.md 3장을 따른다.
// 핵심 루프: 상황 입력(또는 레시피) → 검증된 제안 카드 → 실행 → 실시간 출력 → AI 브리핑·판정

interface AiPlan {
  planId: number;
  plan: ValidatedPlan;
}

/** 한 계획에 직접 추가할 수 있는 명령 수 (백엔드 MAX_EXTRAS와 같다) */
const MAX_EXTRAS = 10;

interface BriefState {
  runId: number;
  loading: boolean;
  data?: Briefing;
  error?: string;
}

export default function App() {
  const [level, setLevelState] = useState<UserLevel>(() => savedLevel() ?? "middle");
  const setLevel = (l: UserLevel) => {
    setLevelState(l);
    saveLevel(l);
  };
  // 초기 설정 마법사: 첫 실행 때 한 번, 그 뒤로는 환경 배지를 눌렀을 때
  const [wizardOpen, setWizardOpen] = useState(() => !setupDone());
  const [mode, setMode] = useState<UiMode>("easy");
  const [situation, setSituation] = useState("");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [env, setEnv] = useState<EnvironmentStatus | null>(null);
  const [recipes, setRecipes] = useState<Recipe[]>([]);
  const [tools, setTools] = useState<ToolSchema[]>([]);
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);

  // 가운데 화면에 띄운 것: AI 제안 또는 레시피
  const [ai, setAi] = useState<AiPlan | null>(null);
  const [recipe, setRecipe] = useState<Recipe | null>(null);
  const [inputs, setInputs] = useState<Record<string, string>>({});
  const [plan, setPlan] = useState<ValidatedPlan | null>(null);
  const [tool, setTool] = useState<ToolSchema | null>(null);
  // 직접 추가한 명령과 "직접 명령" 화면
  const [extras, setExtras] = useState<string[]>([]);
  const [manualMode, setManualMode] = useState(false);
  // 캡처 분석 화면: null이면 닫힘, 글자는 먼저 열 파일 이름(빈 글자면 가장 최근 것)
  const [capture, setCapture] = useState<string | null>(null);
  useEffect(() => {
    const open = (e: Event) => setCapture(String((e as CustomEvent).detail ?? ""));
    window.addEventListener(OPEN_CAPTURE_EVENT, open);
    return () => window.removeEventListener(OPEN_CAPTURE_EVENT, open);
  }, []);
  // 추천 목록 대신 직접 입력을 고른 입력칸
  const [custom, setCustom] = useState<Record<string, boolean>>({});
  // Expert 옵션 패널에서 누른 옵션을 명령 입력칸에 붙인다.
  const [appendReq, setAppendReq] = useState({ text: "", n: 0 });
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [suggesting, setSuggesting] = useState(false);

  // 실행
  const [rawOpen, setRawOpen] = useState(false);
  const [runId, setRunId] = useState<number | null>(null);
  const [runTitle, setRunTitle] = useState("");
  const [planRunId, setPlanRunId] = useState<number | null>(null);
  const [brief, setBrief] = useState<BriefState | null>(null);
  const [missing, setMissing] = useState<string[]>([]);
  const [interfaces, setInterfaces] = useState<string[]>([]);
  const [installRunId, setInstallRunId] = useState<number | null>(null);
  const run = useRun(runId);
  const planRun = useRun(planRunId);
  const installing = useRun(installRunId)?.active ?? false;
  const rawRef = useRef<HTMLPreElement>(null);
  const rawCount = run?.lines.length ?? 0;

  const aiOn = settings !== null && settings.provider !== "none";

  const refreshTools = () =>
    getToolStatus()
      .then((list) => setMissing(list.filter((t) => !t.installed).map((t) => t.tool)))
      .catch(() => setMissing([]));

  useEffect(() => {
    getAppInfo().then(setInfo).catch(() => setInfo(null));
    getEnvironmentStatus().then(setEnv).catch(() => setEnv(null));
    getRecipes().then(setRecipes).catch(() => setRecipes([]));
    getTools().then(setTools).catch(() => setTools([]));
    getSettings().then(setSettings).catch(() => setSettings(null));
    listInterfaces().then(setInterfaces).catch(() => setInterfaces([]));
    refreshTools();
  }, []);

  // 도구 설치가 끝나면 설치 상태를 다시 확인한다.
  useEffect(() => {
    if (installRunId !== null && !installing) refreshTools();
  }, [installRunId, installing]);

  // 새 출력이 오면 원본 출력 창을 맨 아래로 내린다.
  useEffect(() => {
    if (rawRef.current) rawRef.current.scrollTop = rawRef.current.scrollHeight;
  }, [rawCount, rawOpen]);

  // 등급이 바뀌면 허용 범위가 달라지므로 제안과 계획을 다시 받게 한다.
  useEffect(() => {
    setPlan(null);
    setAi(null);
    setExtras([]);
    setPlanRunId(null);
    setError(null);
    if (level === "general") {
      setMode("easy");
      setManualMode(false);
    }
  }, [level]);

  // Easy와 Expert는 계획을 따로 쓴다. 모드를 바꾸면 직접 추가한 명령과 계획을 비운다.
  useEffect(() => {
    setPlan(null);
    setExtras([]);
    setPlanRunId(null);
    setError(null);
  }, [mode]);

  // 실행이 끝나면 AI에게 요약과 판정을 받는다.
  const planRunDone = planRun !== null && !planRun.active && Object.keys(planRun.steps).length > 0;
  useEffect(() => {
    if (!planRunDone || planRunId === null || !aiOn || brief?.runId === planRunId) return;
    const id = planRunId;
    setBrief({ runId: id, loading: true });
    briefRun(id, level)
      .then((data) => setBrief({ runId: id, loading: false, data }))
      .catch((e) => setBrief({ runId: id, loading: false, error: String(e) }));
  }, [planRunDone, planRunId, aiOn, brief?.runId, level]);

  const levelInfo = USER_LEVELS.find((l) => l.id === level)!;
  const showCommands = level !== "general";
  const allowed = (min: UserLevel) => levelRank(level) >= levelRank(min);
  const expert = mode === "expert";
  const shownPlan = expert ? plan : (plan ?? ai?.plan ?? null);
  const base: Base | null = expert
    ? { kind: "none" }
    : ai
    ? { kind: "ai", plan_id: ai.planId }
    : recipe
      ? { kind: "recipe", id: recipe.id, inputs }
      : manualMode
        ? { kind: "none" }
        : null;
  const shownBrief = brief && brief.runId === planRunId ? brief : null;

  function pickRecipe(r: Recipe, prefilled: Record<string, string> = {}) {
    setAi(null);
    setManualMode(false);
    setExtras([]);
    setCustom({});
    setRecipe(r);
    // 기본값이 있는 입력은 미리 채워 둔다.
    const defaults = Object.fromEntries(r.inputs.filter((i) => i.default).map((i) => [i.name, i.default as string]));
    setInputs({ ...defaults, ...prefilled });
    setPlan(null);
    setPlanRunId(null);
    setError(null);
    setNotice(null);
  }

  async function makePlan(r: Recipe | null = recipe, values: Record<string, string> = inputs, added: string[] = extras) {
    if (!r) return;
    setError(null);
    setPlanRunId(null);
    try {
      setPlan(await planMixed({ kind: "recipe", id: r.id, inputs: values }, added, level));
    } catch (e) {
      setPlan(null);
      setError(String(e));
    }
  }

  // 직접 추가한 명령이 바뀌면 계획을 다시 검증해 보여 준다.
  async function changeExtras(next: string[]) {
    if (!base) return;
    setError(null);
    setPlanRunId(null);
    try {
      if (base.kind === "none" && next.length === 0) setPlan(null);
      else setPlan(await planMixed(base, next, level));
      setExtras(next);
    } catch (e) {
      setError(String(e));
    }
  }

  function openManual() {
    setAi(null);
    setRecipe(null);
    setPlan(null);
    setExtras([]);
    setPlanRunId(null);
    setError(null);
    setNotice(null);
    setManualMode(true);
  }

  // 직접 추가한 단계를 뺀다. 직접 추가한 단계는 계획 맨 뒤에 차례로 붙어 있다.
  function removeStep(order: number) {
    if (!shownPlan) return;
    const firstManual = shownPlan.steps.length - extras.length + 1;
    changeExtras(extras.filter((_, i) => i !== order - firstManual));
  }

  // 옴니박스: 상황을 보내 제안을 받는다.
  async function ask() {
    if (suggesting || situation.trim() === "") return;
    setSuggesting(true);
    setError(null);
    setNotice(null);
    try {
      const res = await suggest(situation, level);
      setMode("easy");
      if (res.kind === "plan") {
        setRecipe(null);
        setManualMode(false);
        setExtras([]);
        setPlan(null);
        setPlanRunId(null);
        setAi({ planId: res.plan_id, plan: res.plan });
        setNotice(res.note);
      } else if (res.kind === "recipe") {
        const r = recipes.find((x) => x.id === res.recipe_id);
        if (r && allowed(r.min_level)) {
          pickRecipe(r, res.inputs);
          setNotice(res.note);
          if (r.inputs.every((i) => res.inputs[i.name] || i.optional || i.default)) await makePlan(r, res.inputs);
        } else {
          setNotice("맞는 레시피가 있지만 현재 등급에서는 쓸 수 없습니다.");
        }
      } else {
        setNotice(res.text);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSuggesting(false);
    }
  }

  async function execute(onlyStep: number | null, confirmed: boolean) {
    setError(null);
    try {
      if (!base) return;
      const started = await runMixed(base, extras, level, confirmed, onlyStep);
      const title = ai ? ai.plan.summary || "AI 제안" : recipe ? recipe.title : "직접 명령";
      setRunId(started.run_id);
      setPlanRunId(started.run_id);
      setRunTitle(title);
    } catch (e) {
      setError(String(e));
    }
  }

  async function install() {
    setError(null);
    try {
      const started = await installTools();
      setRunId(started.run_id);
      setInstallRunId(started.run_id);
      setRunTitle("도구 설치");
      setRawOpen(true);
    } catch (e) {
      setError(String(e));
    }
  }

  // WSL이 없어도 기본 진단으로 실행한다. 기본 진단으로 못 하는 단계는 계획에서 따로 막혀 나온다.
  const disabledReason = env === null ? "환경을 확인하는 중입니다." : installing ? "도구를 설치하는 중입니다." : null;

  const cards = shownPlan && (
    <>
      {shownBrief && <BriefPanel brief={shownBrief} />}
      <PlanCards
        plan={shownPlan}
        level={level}
        run={planRun}
        disabledReason={disabledReason}
        onRun={execute}
        onCancel={() => planRunId !== null && cancelRun(planRunId)}
        onRemove={removeStep}
      />
    </>
  );
  const addStep = base && (shownPlan || manualMode) && (
    <AddStep tools={tools} level={level} count={extras.length} max={MAX_EXTRAS} onAdd={(text) => changeExtras([...extras, text])} />
  );

  return (
    <div className="flex h-full flex-col">
      {/* 상단 헤더 */}
      <header className="flex items-center gap-4 border-b border-line bg-panel px-5 py-3">
        <div className="flex shrink-0 items-baseline gap-2">
          <span className="text-lg font-bold tracking-tight text-accent">NetX</span>
          <span className="text-xs text-dim" title={info?.channel ? `${info.channel} 채널 · 새 버전이 올라오면 서명을 확인한 뒤 자동으로 업데이트됩니다` : undefined}>
            v{info?.version ?? "…"}
            {info?.commit ? ` · ${info.commit}` : ""}
          </span>
          <button type="button" onClick={() => setAboutOpen(true)} className="text-xs text-dim underline decoration-dotted hover:text-ink">
            정보
          </button>
          <button
            type="button"
            onClick={() => openTerminal().catch((e) => setError(String(e)))}
            title="넷엑스 터미널: 시리얼, SSH, Telnet, FTP, TFTP 접속 (별도 창)"
            className="ml-1 rounded border border-line px-2 py-0.5 text-xs text-dim hover:border-accent hover:text-accent"
          >
            터미널
          </button>
        </div>

        <form
          className="flex min-w-0 flex-1"
          onSubmit={(e) => {
            e.preventDefault();
            ask();
          }}
        >
          <input
            value={situation}
            onChange={(e) => setSituation(e.target.value)}
            placeholder="상황을 입력하세요. 예: 회사 홈페이지가 가끔 느려요"
            className="w-full min-w-0 rounded-l-md border border-line bg-raised px-4 py-2 text-sm outline-none placeholder:text-dim focus:border-accent"
          />
          <button
            type="submit"
            disabled={suggesting || situation.trim() === ""}
            title={aiOn ? "AI가 도구와 옵션을 제안합니다" : "AI가 연결되지 않아 키워드로 레시피를 찾습니다"}
            className="shrink-0 rounded-r-md bg-accent px-4 text-sm font-semibold whitespace-nowrap text-void disabled:bg-raised disabled:font-normal disabled:text-dim"
          >
            {suggesting ? "찾는 중…" : "제안 받기"}
          </button>
        </form>

        <label className="flex shrink-0 items-center gap-2 text-sm whitespace-nowrap">
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

        <button
          onClick={() => setSettingsOpen(true)}
          title="설정 (AI 연결, 구성도용 SNMP, 자동 업데이트)"
          className={`flex shrink-0 items-center gap-1.5 rounded-full border px-3 py-1 text-xs whitespace-nowrap ${
            aiOn ? "border-safe/40 text-safe" : "border-line text-dim"
          }`}
        >
          <span className={`h-2 w-2 rounded-full ${aiOn ? "bg-safe" : "bg-dim"}`} />
          {aiOn ? (settings?.provider === "ollama" ? "AI 로컬" : "AI 온라인") : "AI 꺼짐"}
        </button>

        <EnvBadge env={env} onClick={() => setWizardOpen(true)} />
      </header>

      <UpdateBar />
      <NetBar />

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
                          !ai && recipe?.id === r.id ? "bg-raised" : ""
                        }`}
                      >
                        <span className="text-base">{r.icon}</span>
                        <span className="flex-1">{r.title}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
              <h2 className="mt-4 mb-2 px-1 text-xs font-semibold text-dim">직접 하기</h2>
              <button
                onClick={() => setCapture("")}
                title="저장해 둔 패킷 캡처를 열어 요약, 문제 찾기, 패킷 목록을 봅니다."
                className="flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-raised"
              >
                <span className="text-base">📂</span>
                <span className="flex-1">캡처 분석</span>
              </button>
              {level !== "general" && (
                <>
                  <button
                    onClick={openManual}
                    title="도구와 옵션을 직접 정해서 실행합니다. 실행 전에 검증으로 위험도와 예상을 볼 수 있습니다."
                    className={`flex w-full items-center gap-2 rounded-md px-2 py-2 text-left text-sm hover:bg-raised ${
                      manualMode && !ai && !recipe ? "bg-raised" : ""
                    }`}
                  >
                    <span className="text-base">⌨️</span>
                    <span className="flex-1">직접 명령</span>
                  </button>
                </>
              )}
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
          {error && (
            <p className="mx-auto mb-5 max-w-3xl rounded-md border border-danger/40 px-3 py-2 text-sm text-danger">{error}</p>
          )}
          {notice && (
            <p className="mx-auto mb-5 max-w-3xl rounded-md border border-accent/40 bg-accent/10 px-3 py-2 text-sm">{notice}</p>
          )}
          {missing.length > 0 && (
            <div className="mx-auto mb-5 flex max-w-3xl items-center gap-4 rounded-md border border-caution/50 bg-caution/10 px-4 py-3 text-sm">
              <p className="flex-1">
                <span className="font-semibold text-caution">WSL에 설치되지 않은 도구가 있습니다.</span>
                <span className="ml-2 font-mono text-xs text-dim">{missing.join(", ")}</span>
              </p>
              <button
                onClick={install}
                disabled={installing}
                title="WSL(Ubuntu)에서 apt로 진단 도구를 설치합니다. root 권한으로 실행됩니다."
                className="shrink-0 rounded-md bg-caution px-3 py-1.5 text-sm font-semibold whitespace-nowrap text-void disabled:opacity-50"
              >
                {installing ? "설치 중…" : "도구 설치"}
              </button>
            </div>
          )}

          {expert ? (
            <div className="mx-auto max-w-3xl">
              <h1 className="text-xl font-semibold">{tool ? tool.title : "명령 조립"}</h1>
              <p className="mt-1 text-sm text-dim">
                {tool
                  ? `${tool.description} 오른쪽 옵션을 누르면 명령에 붙습니다. [검증]으로 확인한 뒤 [계획에 추가]하고 실행하세요.`
                  : "왼쪽에서 도구를 고르면 사용 형식이 나오고, 오른쪽 옵션을 눌러 명령을 조립할 수 있습니다."}
              </p>
              {cards}
              <AddStep
                key={tool?.tool ?? "none"}
                tools={tools}
                level={level}
                count={extras.length}
                max={MAX_EXTRAS}
                onAdd={(text) => changeExtras([...extras, text])}
                initialTool={tool?.tool}
                defaultOpen
                appendReq={appendReq}
                hideOptions
              />
            </div>
          ) : mode === "easy" && ai ? (
            <div className="mx-auto max-w-3xl">
              <p className="text-xs font-semibold text-accent">AI 제안</p>
              <h1 className="mt-1 text-xl font-semibold">{ai.plan.summary || "진단 계획"}</h1>
              {ai.plan.target && (
                <p className="mt-1 text-sm text-dim">
                  대상 <span className="font-mono text-ink">{ai.plan.target}</span>
                </p>
              )}
              <p className="mt-2 text-xs text-dim">AI가 만든 계획입니다. 명령과 이유를 확인한 뒤 실행하세요.</p>
              {cards}
              {addStep}
            </div>
          ) : mode === "easy" && manualMode ? (
            <div className="mx-auto max-w-3xl">
              <h1 className="text-xl font-semibold">⌨️ 직접 명령</h1>
              <p className="mt-1 text-sm text-dim">
                도구와 옵션을 직접 정해서 실행합니다. 등록된 도구와 허용된 옵션만 쓸 수 있고, 모든 명령은 실행 전에 검증을 거칩니다.
              </p>
              {cards}
              {addStep}
            </div>
          ) : mode === "easy" && recipe ? (
            <div className="mx-auto max-w-3xl">
              <h1 className="text-xl font-semibold">
                {recipe.icon} {recipe.title}
              </h1>
              <p className="mt-1 text-sm text-dim">{recipe.description}</p>

              <form
                className="mt-5 flex flex-wrap items-start gap-3"
                onSubmit={(e) => {
                  e.preventDefault();
                  makePlan();
                }}
              >
                {recipe.inputs.map((input) => {
                  const change = (value: string) => {
                    setInputs({ ...inputs, [input.name]: value });
                    setPlan(null);
                    setPlanRunId(null);
                  };
                  const field =
                    "rounded-md border border-line bg-raised px-3 py-2 font-mono text-sm text-ink outline-none placeholder:text-dim/60 focus:border-accent";
                  // 고를 수 있는 값이 정해진 입력은 목록으로 보여 준다.
                  const choices =
                    input.values.length > 0
                      ? input.values.map((v, i) => ({ value: v, label: input.value_labels[i] ?? v }))
                      : input.source === "interfaces" && interfaces.length > 0
                        ? interfaces.map((v) => ({ value: v, label: v === "any" ? "any (전체)" : v }))
                        : null;
                  const suggested = input.suggestions.length > 0;
                  const value = inputs[input.name] ?? input.default ?? "";
                  const typing = suggested && (custom[input.name] || (value !== "" && !input.suggestions.includes(value)));
                  return (
                    <label
                      key={input.name}
                      className={`flex flex-col gap-1 text-xs text-dim ${suggested ? "min-w-full" : "min-w-36 flex-1"}`}
                    >
                      <span>
                        {input.label}
                        {input.optional && <span className="ml-1 opacity-60">(선택)</span>}
                      </span>
                      {choices ? (
                        <select value={value} onChange={(e) => change(e.target.value)} className={field}>
                          {choices.map((c) => (
                            <option key={c.value} value={c.value}>
                              {c.label}
                            </option>
                          ))}
                        </select>
                      ) : suggested ? (
                        <div className="flex gap-2">
                          <select
                            value={typing ? "" : value}
                            onChange={(e) => {
                              setCustom({ ...custom, [input.name]: e.target.value === "" });
                              change(e.target.value);
                            }}
                            className={`${field} min-w-0 flex-1`}
                          >
                            {input.suggestions.map((v, i) => (
                              <option key={v} value={v}>
                                {input.suggestion_labels[i] ? `${input.suggestion_labels[i]} · ${v}` : v}
                              </option>
                            ))}
                            <option value="">직접 입력…</option>
                          </select>
                          {typing && (
                            <input
                              value={value}
                              onChange={(e) => change(e.target.value)}
                              placeholder={input.example ?? ""}
                              spellCheck={false}
                              autoFocus
                              className={`${field} min-w-0 flex-1`}
                            />
                          )}
                        </div>
                      ) : (
                        <input
                          value={inputs[input.name] ?? ""}
                          onChange={(e) => change(e.target.value)}
                          placeholder={input.example ? `예: ${input.example}` : ""}
                          spellCheck={false}
                          className={field}
                        />
                      )}
                      {input.hint && <span className="leading-relaxed opacity-80">{input.hint}</span>}
                    </label>
                  );
                })}
                <button
                  type="submit"
                  className="mt-5 shrink-0 rounded-md bg-accent px-4 py-2 text-sm font-semibold whitespace-nowrap text-void"
                >
                  계획 보기
                </button>
              </form>
              {cards}
              {addStep}
            </div>
          ) : (
            <div className="flex h-full items-center justify-center">
              <div className="max-w-lg text-center">
                <p className="text-2xl font-semibold">무엇이 문제인가요?</p>
                <p className="mt-3 text-sm leading-relaxed text-dim">
                  {mode === "easy"
                    ? "위 입력창에 상황을 적고 [제안 받기]를 누르거나, 왼쪽에서 상황 레시피를 고르세요. 실행할 도구와 옵션 조합이 이유·예상 결과와 함께 표시됩니다."
                    : "왼쪽에서 도구를 고르면 오른쪽에 쓸 수 있는 옵션이 표시됩니다."}
                </p>
                <p className="mt-6 text-xs text-dim">
                  현재 등급: <span className="text-ink">{levelInfo.label}</span> · {levelInfo.hint}
                </p>
                <WordCard className="mt-10 border-t border-line pt-6" />
              </div>
            </div>
          )}
        </main>

        {/* 우측: 옵션 패널 (목록 보기) */}
        {showCommands && mode === "expert" && (
          <aside className="w-96 shrink-0 overflow-y-auto border-l border-line bg-panel p-4">
            {tool ? (
              <OptionList
                tool={tool}
                onPick={(o) => setAppendReq((r) => ({ text: `${o.flag}${o.type === "boolean" ? "" : " "}`, n: r.n + 1 }))}
              />
            ) : (
              <>
                <p className="text-sm text-dim">왼쪽에서 도구를 고르면 옵션이 표시됩니다.</p>
                <WordCard className="mt-8 border-t border-line pt-5" />
              </>
            )}
          </aside>
        )}
      </div>

      {/* 하단: 명령 미리보기, 브리핑, 원본 출력 */}
      <footer className="border-t border-line bg-panel">
        {showCommands && (
          <div className="flex items-center gap-3 border-b border-line px-5 py-2 font-mono text-sm">
            <span className="text-accent">$</span>
            <span className="truncate text-dim">
              {shownPlan
                ? shownPlan.steps.map((s) => s.display).join("  ·  ")
                : "실행할 명령이 여기에 조립됩니다"}
            </span>
          </div>
        )}
        <div className="flex items-center justify-between gap-4 px-5 py-2 text-sm">
          <span className="flex min-w-0 items-center gap-3 text-dim">
            {run ? (
              <>
                <span className={`shrink-0 ${run.active ? "text-accent" : ""}`}>
                  {runTitle} · {run.active ? "실행 중…" : "실행 끝"} · 출력 {run.lines.length}줄
                </span>
                {run.active && (
                  <button onClick={() => cancelRun(run.run_id)} className="shrink-0 rounded border border-danger/50 px-2 py-0.5 text-xs text-danger">
                    ■ 중지
                  </button>
                )}
                {shownBrief?.data && <span className="truncate text-ink">{shownBrief.data.headline}</span>}
              </>
            ) : aiOn ? (
              "실행이 끝나면 AI 브리핑과 판정이 표시됩니다"
            ) : (
              "AI를 연결하면 실행 결과의 요약과 판정을 볼 수 있습니다 (우측 위 AI 꺼짐)"
            )}
          </span>
          {showCommands && (
            <button onClick={() => setRawOpen((v) => !v)} className="shrink-0 text-xs text-dim hover:text-ink">
              원본 출력 {rawOpen ? "접기 ▾" : "펼치기 ▴"}
            </button>
          )}
        </div>
        {showCommands && rawOpen && (
          <pre ref={rawRef} className="h-48 overflow-auto border-t border-line bg-void px-5 py-3 font-mono text-xs text-dim">
            {run && run.lines.length > 0
              ? run.lines.slice(-500).map((l, i) => (
                  <div key={i} className={l.stream === "stderr" ? "text-caution" : "text-ink"}>
                    <span className="text-dim">[{l.order}] </span>
                    {l.line}
                  </div>
                ))
              : "(아직 실행 기록이 없습니다)"}
          </pre>
        )}
      </footer>

      {aboutOpen && <AboutDialog info={info} onClose={() => setAboutOpen(false)} />}
      {wizardOpen && (
        <SetupWizard
          env={env}
          level={level}
          missing={missing}
          installing={installing}
          aiOn={aiOn}
          onLevel={setLevel}
          onRecheck={async () => {
            setEnv(await getEnvironmentStatus().catch(() => null));
            await refreshTools();
          }}
          onInstall={install}
          onOpenSettings={() => setSettingsOpen(true)}
          onClose={() => {
            markSetupDone();
            setWizardOpen(false);
          }}
        />
      )}
      {settingsOpen && settings && (
        <SettingsDialog initial={settings} onClose={() => setSettingsOpen(false)} onSaved={setSettings} />
      )}
      {capture !== null && <CaptureView key={capture} initial={capture} onClose={() => setCapture(null)} />}
    </div>
  );
}

function OptionList({ tool, onPick }: { tool: ToolSchema; onPick: (o: ToolSchema["options"][number]) => void }) {
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
              <li key={o.flag}>
                <button
                  onClick={() => onPick(o)}
                  disabled={o.risk === "forbidden"}
                  title={o.risk === "forbidden" ? "실행할 수 없는 옵션입니다" : "누르면 명령 끝에 붙습니다"}
                  className="flex w-full items-start gap-2 rounded px-1 py-0.5 text-left text-sm hover:bg-raised disabled:hover:bg-transparent"
                >
                  <code className="w-28 shrink-0 font-mono text-xs leading-5 break-all text-accent">
                    {o.flag}
                    {o.type !== "boolean" && o.risk !== "forbidden" && <span className="text-dim"> &lt;값&gt;</span>}
                  </code>
                  <span className={`flex-1 text-xs leading-5 ${o.risk === "forbidden" ? "text-dim line-through" : ""}`}>{o.label}</span>
                  {o.risk !== "safe" && <RiskBadge risk={o.risk} />}
                </button>
              </li>
            ))}
          </ul>
        </section>
      ))}
    </>
  );
}

function EnvBadge({ env, onClick }: { env: EnvironmentStatus | null; onClick: () => void }) {
  if (!env) return <span className="shrink-0 text-xs text-dim">환경 확인 중…</span>;
  const full = env.mode === "full";
  return (
    <button
      type="button"
      onClick={onClick}
      title={`${env.message}\n누르면 초기 설정을 다시 엽니다.`}
      className={`flex shrink-0 items-center gap-1.5 rounded-full border px-3 py-1 text-xs whitespace-nowrap ${
        full ? "border-safe/40 text-safe" : "border-caution/40 text-caution"
      }`}
    >
      <span className={`h-2 w-2 rounded-full ${full ? "bg-safe" : "bg-caution"}`} />
      {full ? "전체 진단" : "기본 진단"}
    </button>
  );
}
