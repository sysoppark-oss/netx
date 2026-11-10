//! 화면에서 호출하는 Tauri 명령.

use std::collections::BTreeMap;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audit::{self, AuditEntry};
use crate::engine::{self, Job, JobEngine, OutputEvent, Sink, StepEvent};
use crate::env_check::{self, EnvironmentStatus, Mode};
use crate::interpret;
use crate::planner::{self, Briefing, Suggestion};
use crate::settings::{self, Provider, Settings};
use crate::setup::{self, ToolStatus};
use crate::model::{ExecutionPlan, Level, Recipe, Source, ToolSchema, ValidatedPlan};
use crate::recipe;
use crate::registry::Registry;
use crate::security::validator;

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

/// 앱 이름, 버전, 실행 OS를 돌려준다.
#[tauri::command]
pub fn app_info() -> AppInfo {
    AppInfo {
        name: "NetX",
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// 실행 환경(WSL2 설치 여부, 기본 배포판)을 확인한다.
/// 외부 프로세스를 호출하므로 비동기로 실행한다.
#[tauri::command]
pub async fn environment_status() -> EnvironmentStatus {
    env_check::check().await
}

/// 등록된 도구와 옵션 스키마. 옵션 패널과 자동완성이 쓴다.
#[tauri::command]
pub fn get_tools() -> Vec<ToolSchema> {
    Registry::global().tools().to_vec()
}

/// 상황 레시피 목록.
#[tauri::command]
pub fn get_recipes() -> Vec<Recipe> {
    Registry::global().recipes().to_vec()
}

/// 레시피에 입력값을 넣어 계획을 만들고 검증한다. 실행하지는 않는다.
#[tauri::command]
pub fn plan_from_recipe(id: String, inputs: BTreeMap<String, String>, level: Level) -> Result<ValidatedPlan, String> {
    let reg = Registry::global();
    let recipe = reg.recipe(&id).ok_or("등록되지 않은 레시피입니다")?;
    if level < recipe.min_level {
        return Err("현재 등급에서는 쓸 수 없는 레시피입니다".into());
    }
    let plan = recipe::instantiate(recipe, &inputs)?;
    Ok(validator::validate_plan(reg, &plan, level, Source::Recipe, recipe.parallel))
}

/// 화면에서 조립한 계획을 검증한다. 화면이 보낸 계획은 항상 직접 입력으로 취급한다
/// (레시피 권한은 `plan_from_recipe`를 거칠 때만 주어진다).
#[tauri::command]
pub fn validate_plan(plan: ExecutionPlan, level: Level) -> ValidatedPlan {
    validator::validate_plan(Registry::global(), &plan, level, Source::Manual, false)
}

// ---- 실행 ----

/// 도구 한 단계의 기본 제한 시간(초). 넘으면 리눅스 쪽 `timeout`이 중지시킨다.
const STEP_TIMEOUT_SECS: u32 = 120;

struct TauriSink(AppHandle);

#[derive(Clone, Serialize)]
struct ResultEvent {
    run_id: u64,
    order: u32,
    result: interpret::StepResult,
}

/// 실행 중인 단계의 출력(해설용). 단계가 끝나면 비운다.
static OUTPUTS: std::sync::Mutex<Option<std::collections::HashMap<(u64, u32), Vec<String>>>> = std::sync::Mutex::new(None);

fn with_outputs<R>(f: impl FnOnce(&mut std::collections::HashMap<(u64, u32), Vec<String>>) -> R) -> R {
    let mut guard = OUTPUTS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(std::collections::HashMap::new))
}

impl Sink for TauriSink {
    fn output(&self, ev: OutputEvent) {
        planner::record_output(ev.run_id, ev.order, &ev.line);
        with_outputs(|m| {
            let lines = m.entry((ev.run_id, ev.order)).or_default();
            if lines.len() < 6000 {
                lines.push(ev.line.clone());
            }
        });
        let _ = self.0.emit("run:output", ev);
    }
    fn step(&self, ev: StepEvent) {
        planner::record_step(ev.run_id, ev.order, ev.status, ev.exit_code);
        // 단계가 끝나면 출력을 해석해 결과 해설을 보낸다.
        if ev.status != engine::StepStatus::Running {
            let lines = with_outputs(|m| m.remove(&(ev.run_id, ev.order))).unwrap_or_default();
            let result = interpret::interpret(&ev.program, &lines, ev.status, ev.exit_code);
            let _ = self.0.emit("run:result", ResultEvent { run_id: ev.run_id, order: ev.order, result });
        }
        let _ = self.0.emit("run:step", ev);
    }
    fn done(&self, run_id: u64) {
        let _ = self.0.emit("run:done", run_id);
    }
}

#[derive(Serialize)]
pub struct RunStarted {
    pub run_id: u64,
    pub orders: Vec<u32>,
}

/// 앱 데이터 폴더와 그 안의 작업 폴더(캡처 파일 등이 저장되는 곳).
fn data_dirs(app: &AppHandle) -> Result<(PathBuf, PathBuf), String> {
    let data = app.path().app_local_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했습니다: {e}"))?;
    let work = data.join("work");
    std::fs::create_dir_all(&work).map_err(|e| format!("작업 폴더를 만들지 못했습니다: {e}"))?;
    Ok((data, work))
}

async fn require_wsl() -> Result<(), String> {
    if cfg!(target_os = "windows") && !matches!(env_check::check().await.mode, Mode::Full) {
        return Err("WSL2가 필요합니다. 관리자 PowerShell에서 `wsl --install` 후 재부팅하세요.".into());
    }
    Ok(())
}

/// 레시피를 실행한다. 화면이 보낸 명령을 그대로 믿지 않고, 입력값으로 계획을 다시 만들고 다시 검증한다.
/// `only_step`을 주면 그 단계만 실행한다. 주의·위험 단계가 있으면 `confirmed`가 참이어야 한다.
#[tauri::command]
pub async fn run_recipe(
    app: AppHandle,
    id: String,
    inputs: BTreeMap<String, String>,
    level: Level,
    confirmed: bool,
    only_step: Option<u32>,
) -> Result<RunStarted, String> {
    let plan = plan_from_recipe(id.clone(), inputs, level)?;
    let title = Registry::global().recipe(&id).map(|r| r.title.clone()).unwrap_or_default();
    start_plan(app, plan, &title, &format!("recipe:{id}"), level, confirmed, only_step).await
}

/// AI가 제안한 계획을 실행한다. 화면은 계획 번호만 보내고, 백엔드가 보관해 둔 원본을 다시 검증한다.
#[tauri::command]
pub async fn run_ai_plan(app: AppHandle, plan_id: u64, level: Level, confirmed: bool, only_step: Option<u32>) -> Result<RunStarted, String> {
    let original = planner::ai_plan(plan_id).ok_or("제안이 만료되었습니다. 다시 제안을 받아 주세요.")?;
    let plan = validator::validate_plan(Registry::global(), &original, level, Source::Ai, false);
    let title = if plan.summary.is_empty() { "AI 제안".to_string() } else { plan.summary.clone() };
    start_plan(app, plan, &title, &format!("ai:{plan_id}"), level, confirmed, only_step).await
}

async fn start_plan(
    app: AppHandle,
    plan: ValidatedPlan,
    title: &str,
    source: &str,
    level: Level,
    confirmed: bool,
    only_step: Option<u32>,
) -> Result<RunStarted, String> {
    let steps: Vec<_> = plan.steps.iter().filter(|s| only_step.is_none_or(|o| o == s.order)).collect();
    if steps.is_empty() {
        return Err("실행할 단계가 없습니다".into());
    }
    // 전체 실행에서는 차단된 단계를 건너뛰고, 한 단계만 고른 경우에는 거부한다.
    let steps: Vec<_> = if only_step.is_some() { steps } else { steps.into_iter().filter(|s| !s.blocked).collect() };
    if steps.is_empty() || steps.iter().any(|s| s.blocked) {
        return Err("차단된 단계는 실행할 수 없습니다".into());
    }
    if !confirmed && steps.iter().any(|s| s.confirm != crate::model::Confirm::None) {
        return Err("주의 또는 위험 단계가 있습니다. 확인 후 실행하세요.".into());
    }
    if steps.iter().any(|s| s.engines.first() != Some(&crate::model::Engine::Windows)) {
        require_wsl().await?;
    }

    let (data_dir, work_dir) = data_dirs(&app)?;
    let jobs: Vec<Job> = steps
        .iter()
        .map(|s| Job {
            order: s.order,
            program: s.tool.clone(),
            argv: s.argv.clone(),
            root: s.needs_root,
            timeout_secs: STEP_TIMEOUT_SECS,
            engine: if s.engines.first() == Some(&crate::model::Engine::Windows) { JobEngine::Windows } else { JobEngine::Wsl },
        })
        .collect();
    let orders = jobs.iter().map(|j| j.order).collect();
    let run_id = engine::start(jobs, plan.parallel, work_dir, Arc::new(TauriSink(app)));
    planner::record_start(run_id, title, &steps);

    for s in &steps {
        audit::log(
            &data_dir,
            &AuditEntry { time: audit::now(), run_id, level, source, tool: &s.tool, argv: &s.argv, risk: s.risk, root: s.needs_root, confirmed },
        );
    }
    Ok(RunStarted { run_id, orders })
}

/// 실행을 중지한다.
#[tauri::command]
pub fn cancel_run(run_id: u64) -> bool {
    engine::cancel(run_id)
}

// ---- 초기 설정 ----

/// WSL 안에 도구가 설치되어 있는지 확인한다.
#[tauri::command]
pub async fn tool_status() -> Result<Vec<ToolStatus>, String> {
    require_wsl().await?;
    setup::tool_status().await
}

/// WSL 안에 도구를 설치한다(apt). 진행 상황은 실행 이벤트로 흘러간다.
#[tauri::command]
pub async fn install_tools(app: AppHandle) -> Result<RunStarted, String> {
    if !cfg!(target_os = "windows") {
        return Err("도구 설치는 Windows의 WSL2에서만 지원합니다".into());
    }
    require_wsl().await?;
    let (_, work_dir) = data_dirs(&app)?;
    let jobs = setup::install_jobs();
    let orders = jobs.iter().map(|j| j.order).collect();
    let run_id = engine::start(jobs, false, work_dir, Arc::new(TauriSink(app)));
    Ok(RunStarted { run_id, orders })
}

// ---- AI 제안과 브리핑 ----

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_local_data_dir().map_err(|e| format!("앱 데이터 폴더를 찾지 못했습니다: {e}"))
}

/// 상황 설명을 받아 제안을 만든다. AI가 설정되어 있으면 AI 계획을, 아니면 키워드로 찾은 레시피를 돌려준다.
#[tauri::command]
pub async fn suggest(app: AppHandle, text: String, level: Level) -> Result<Suggestion, String> {
    let settings = settings::load(&data_dir(&app)?);
    Ok(planner::suggest(&text, level, &settings, settings::api_key().as_deref()).await)
}

/// 끝난 실행의 결과를 AI가 요약하고 판정한다.
#[tauri::command]
pub async fn brief_run(app: AppHandle, run_id: u64, level: Level) -> Result<Briefing, String> {
    let settings = settings::load(&data_dir(&app)?);
    planner::brief(run_id, level, &settings, settings::api_key().as_deref()).await
}

#[derive(Serialize)]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    /// API 키가 저장되어 있는지. 키 자체는 화면으로 보내지 않는다.
    pub has_key: bool,
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<SettingsView, String> {
    Ok(SettingsView { settings: settings::load(&data_dir(&app)?), has_key: settings::api_key().is_some() })
}

/// 설정을 저장한다. `api_key`가 없으면 키는 그대로 두고, 빈 문자열이면 지운다.
#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings, api_key: Option<String>) -> Result<SettingsView, String> {
    let dir = data_dir(&app)?;
    settings::save(&dir, &settings)?;
    if let Some(key) = api_key {
        let key = key.trim();
        if key.len() > 500 || key.chars().any(|c| c.is_control()) {
            return Err("API 키 형식이 올바르지 않습니다".into());
        }
        settings::set_api_key(key)?;
    }
    get_settings(app)
}

/// 저장된 설정으로 AI에 짧은 요청을 보내 연결을 확인한다.
#[tauri::command]
pub async fn test_llm(app: AppHandle) -> Result<String, String> {
    let settings = settings::load(&data_dir(&app)?);
    if settings.provider == Provider::None {
        return Err("AI 사용 안 함으로 설정되어 있습니다".into());
    }
    planner::test_connection(&settings, settings::api_key().as_deref()).await
}

/// 캡처할 수 있는 네트워크 인터페이스 이름 목록(WSL 기준). 맨 앞의 "any"는 전체를 뜻한다.
#[tauri::command]
pub async fn list_interfaces() -> Vec<String> {
    let mut names = vec!["any".to_string()];
    names.extend(setup::list_interfaces().await);
    names
}
