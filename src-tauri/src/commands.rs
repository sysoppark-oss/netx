//! 화면에서 호출하는 Tauri 명령.

use std::collections::BTreeMap;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audit::{self, AuditEntry};
use crate::engine::{self, Job, JobEngine, OutputEvent, Sink, StepEvent};
use crate::env_check::{self, EnvironmentStatus, Mode};
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

impl Sink for TauriSink {
    fn output(&self, ev: OutputEvent) {
        let _ = self.0.emit("run:output", ev);
    }
    fn step(&self, ev: StepEvent) {
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
    let steps: Vec<_> = plan.steps.iter().filter(|s| only_step.is_none_or(|o| o == s.order)).collect();
    if steps.is_empty() {
        return Err("실행할 단계가 없습니다".into());
    }
    if steps.iter().any(|s| s.blocked) {
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

    let source = format!("recipe:{id}");
    for s in &steps {
        audit::log(
            &data_dir,
            &AuditEntry { time: audit::now(), run_id, level, source: &source, tool: &s.tool, argv: &s.argv, risk: s.risk, root: s.needs_root, confirmed },
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
