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
use crate::manual;
use crate::netinfo::{self, NetInfo};
use crate::planner::{self, Briefing, Suggestion};
use crate::settings::{self, Provider, Settings};
use crate::setup::{self, ToolStatus};
use crate::model::{ExecutionPlan, Level, Recipe, Risk, Source, ToolSchema, ValidatedPlan, ValidatedStep};
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

// ---- 자동 업데이트 ----

fn update_status(app: &AppHandle, state: &'static str, commit: &str, message: impl Into<String>) {
    let _ = app.emit("update:status", crate::updater::Status { state, commit: commit.chars().take(7).collect(), message: message.into() });
}

/// 새 버전을 내려받아 확인하고 설치 프로그램을 실행한 뒤 NetX를 끝낸다. 새 버전이 없으면 아무것도 하지 않는다.
async fn install_update(app: &AppHandle) -> Result<(), String> {
    let Some(m) = crate::updater::check().await? else { return Ok(()) };
    update_status(app, "downloading", &m.commit, "새 버전을 내려받고 있습니다.");
    let path = crate::updater::download(&m, &data_dir(app)?.join("update")).await?;
    update_status(app, "installing", &m.commit, "새 버전을 설치합니다. NetX가 잠시 닫혔다가 다시 열립니다.");
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    crate::updater::launch(&path)?;
    let (data_dir, _) = data_dirs(app)?;
    audit::log(&data_dir, &AuditEntry { time: audit::now(), run_id: 0, level: Level::General, source: "update", tool: "netx-setup", argv: &[m.commit.clone(), m.sha256.clone()], risk: Risk::Caution, root: false, confirmed: true });
    app.exit(0);
    Ok(())
}

/// 시작한 뒤 한 번, 그 뒤로 3시간마다 새 버전을 확인한다.
/// 시작 직후에는 (설정에서 끄지 않았다면) 바로 설치하고, 쓰는 도중에는 알리기만 한다.
pub fn watch_updates(app: AppHandle) {
    if !crate::updater::enabled() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        let mut first = true;
        loop {
            let manual = data_dir(&app).map(|d| settings::load(&d).manual_update).unwrap_or(true);
            let result = if first && !manual {
                install_update(&app).await
            } else {
                crate::updater::check().await.map(|m| {
                    if let Some(m) = m {
                        update_status(&app, "available", &m.commit, "새 버전이 있습니다.");
                    }
                })
            };
            if let Err(e) = result {
                // 인터넷이 없을 때마다 알리지는 않는다. 설치를 시도하다 실패한 경우만 알린다.
                if first && !manual && !e.contains("연결하지 못했습니다") {
                    update_status(&app, "error", "", e);
                }
            }
            first = false;
            tokio::time::sleep(std::time::Duration::from_secs(3 * 3600)).await;
        }
    });
}

/// 화면의 [지금 업데이트] 버튼
#[tauri::command]
pub async fn apply_update(app: AppHandle) -> Result<(), String> {
    install_update(&app).await
}

/// 구성도 그림으로 받을 수 있는 가장 큰 크기
const MAX_SVG_BYTES: usize = 1024 * 1024;

/// 화면이 보낸 구성도 그림(SVG)을 저장해도 되는지 본다.
/// 그림 파일은 나중에 브라우저로 열릴 수 있으므로 스크립트, 이벤트 속성, 링크, 외부 내용 삽입을 막는다.
pub fn check_topology_svg(svg: &str) -> Result<(), String> {
    if svg.len() > MAX_SVG_BYTES {
        return Err("그림이 너무 큽니다 (1MB까지).".into());
    }
    if !svg.starts_with("<svg") {
        return Err("SVG 그림이 아닙니다.".into());
    }
    let low = svg.to_ascii_lowercase();
    if low.contains("<script") || low.contains("<foreignobject") || low.contains("href") {
        return Err("그림에 허용하지 않는 내용(스크립트·링크)이 있습니다.".into());
    }
    // on...= 꼴의 이벤트 속성(onload= 등): 글자가 아닌 것 뒤의 "on" + 영문자들 + (공백) + "="
    let b = low.as_bytes();
    for i in 0..b.len().saturating_sub(2) {
        if &b[i..i + 2] != b"on" || (i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'-')) {
            continue;
        }
        let mut j = i + 2;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }
        let word_end = j;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if word_end > i + 2 && b.get(j) == Some(&b'=') {
            return Err("그림에 허용하지 않는 내용(이벤트 속성)이 있습니다.".into());
        }
    }
    Ok(())
}

/// 구성도 그림을 작업 폴더에 `topology-<유닉스 시각>.svg`로 저장하고 전체 경로를 돌려준다.
#[tauri::command]
pub fn save_topology(app: AppHandle, svg: String) -> Result<String, String> {
    check_topology_svg(&svg)?;
    let (_, work) = data_dirs(&app)?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = work.join(format!("topology-{now}.svg"));
    std::fs::write(&path, svg.as_bytes()).map_err(|e| format!("그림을 저장하지 못했습니다: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
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
    if steps.iter().any(|s| s.engines.first() == Some(&crate::model::Engine::Wsl)) {
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
            engine: match s.engines.first() {
                Some(crate::model::Engine::Windows) => JobEngine::Windows,
                Some(crate::model::Engine::Builtin) => JobEngine::Builtin,
                _ => JobEngine::Wsl,
            },
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

// ---- 스캔 결과에서 바로 접속 ----

/// 스캔 결과의 장비에 접속해 본다. 웹은 기본 브라우저로, SSH·Telnet은 새 터미널 창으로 연다.
/// 주소는 IP 형식, 포트는 숫자, 사용자 이름은 정해진 글자만 받는다. 암호는 받지 않는다(터미널 창에서 직접 입력).
#[tauri::command]
pub fn open_target(host: String, port: u16, kind: String, user: Option<String>) -> Result<String, String> {
    let (program, args) = connect_command(&host, port, &kind, user.as_deref())?;
    let mut cmd = std::process::Command::new(program);
    cmd.args(&args).stdin(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    if kind != "web" {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    cmd.spawn().map_err(|e| format!("열지 못했습니다: {e}"))?;
    Ok(match kind.as_str() {
        "web" => "브라우저에서 열었습니다".into(),
        _ => "새 터미널 창을 열었습니다. 암호는 그 창에서 입력하세요.".into(),
    })
}

/// 접속에 쓸 프로그램과 인자. 사용자 입력은 모두 검사한 뒤에만 인자가 된다.
fn connect_command(host: &str, port: u16, kind: &str, user: Option<&str>) -> Result<(&'static str, Vec<String>), String> {
    let ip: std::net::IpAddr = host.parse().map_err(|_| "주소가 IP 형식이 아닙니다".to_string())?;
    if port == 0 {
        return Err("포트 번호가 올바르지 않습니다".into());
    }
    let windows = cfg!(target_os = "windows");
    match kind {
        "web" => {
            let scheme = if matches!(port, 443 | 8443 | 9443) { "https" } else { "http" };
            let at = match ip {
                std::net::IpAddr::V6(v6) => format!("[{v6}]"),
                std::net::IpAddr::V4(v4) => v4.to_string(),
            };
            Ok((if windows { "explorer.exe" } else { "xdg-open" }, vec![format!("{scheme}://{at}:{port}/")]))
        }
        "ssh" => {
            let user = user.map(str::trim).filter(|u| !u.is_empty()).ok_or("SSH 사용자 이름을 입력하세요")?;
            if user.len() > 32 || user.starts_with('-') || !user.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) {
                return Err("사용자 이름에는 영문, 숫자, '.', '_', '-'만 쓸 수 있습니다".into());
            }
            let ssh = vec!["ssh".to_string(), "-p".into(), port.to_string(), "-l".into(), user.to_string(), ip.to_string()];
            Ok(if windows { ("cmd.exe", [vec!["/k".to_string()], ssh].concat()) } else { ("x-terminal-emulator", [vec!["-e".to_string()], ssh].concat()) })
        }
        "telnet" => {
            let telnet = vec!["telnet".to_string(), ip.to_string(), port.to_string()];
            Ok(if windows { ("cmd.exe", [vec!["/k".to_string()], telnet].concat()) } else { ("x-terminal-emulator", [vec!["-e".to_string()], telnet].concat()) })
        }
        _ => Err("지원하지 않는 접속 방식입니다".into()),
    }
}

// ---- 프로세스 종료 ----

/// Windows 프로세스를 종료한다. Windows Hello로 본인 확인을 받고, 쓸 수 없는 PC에서는 프로세스 이름 입력을 요구한다.
/// 중급 이상 등급만 쓸 수 있고, 결과는 감사 로그에 남긴다.
#[tauri::command]
pub async fn kill_process(app: AppHandle, pid: u32, typed_name: Option<String>, level: Level) -> Result<crate::prockill::KillResult, String> {
    if level < Level::Middle {
        return Err("프로세스 종료는 중급 엔지니어 이상 등급에서만 쓸 수 있습니다".into());
    }
    #[cfg(target_os = "windows")]
    let hwnd = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0);
    #[cfg(not(target_os = "windows"))]
    let hwnd = 0isize;

    let (result, killed) = tauri::async_runtime::spawn_blocking(move || crate::prockill::kill_process(hwnd, pid, typed_name.as_deref()))
        .await
        .map_err(|e| format!("종료 작업이 중단되었습니다: {e}"))??;
    if let Some(name) = killed {
        let (data_dir, _) = data_dirs(&app)?;
        audit::log(
            &data_dir,
            &AuditEntry { time: audit::now(), run_id: 0, level, source: "kill", tool: "taskkill", argv: &[pid.to_string(), name], risk: Risk::Danger, root: false, confirmed: true },
        );
    }
    Ok(result)
}

// ---- 내 네트워크 ----

/// 쓰고 있는 어댑터의 주소, 게이트웨이, DNS, 공인 IP, 누적 송수신량.
#[tauri::command]
pub async fn network_info() -> NetInfo {
    netinfo::current().await
}

// ---- 직접 추가한 명령 ----

/// 한 계획에 직접 추가할 수 있는 명령 수
const MAX_EXTRAS: usize = 10;

/// 직접 추가한 명령이 붙는 바탕 계획.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Base {
    Recipe { id: String, inputs: BTreeMap<String, String> },
    Ai { plan_id: u64 },
    None,
}

fn manual_step(text: &str, order: u32, level: Level) -> Result<ValidatedStep, String> {
    if level == Level::General {
        return Err("일반사용자 등급에서는 명령을 직접 입력할 수 없습니다".into());
    }
    let step = manual::to_step(text)?;
    let mut v = validator::validate_step(Registry::global(), &step, order, level, Source::Manual);
    v.manual = true;
    Ok(v)
}

/// 직접 적은 명령 한 줄을 검증한다. 실행하지는 않는다. 위험도, 옵션 풀이, 예상 시간과 결과를 돌려준다.
#[tauri::command]
pub fn check_command(text: String, level: Level) -> Result<ValidatedStep, String> {
    manual_step(&text, 1, level)
}

/// 바탕 계획(레시피 또는 AI 제안) 뒤에 직접 추가한 명령을 붙인 계획. (계획, 제목, 감사 로그용 출처)
fn build_mixed(base: Base, extras: &[String], level: Level) -> Result<(ValidatedPlan, String, String), String> {
    if extras.len() > MAX_EXTRAS {
        return Err(format!("직접 추가할 수 있는 명령은 {MAX_EXTRAS}개까지입니다"));
    }
    let reg = Registry::global();
    let (mut plan, title, source) = match base {
        Base::Recipe { id, inputs } => {
            let plan = plan_from_recipe(id.clone(), inputs, level)?;
            let title = reg.recipe(&id).map(|r| r.title.clone()).unwrap_or_default();
            (plan, title, format!("recipe:{id}"))
        }
        Base::Ai { plan_id } => {
            let original = planner::ai_plan(plan_id).ok_or("제안이 만료되었습니다. 다시 제안을 받아 주세요.")?;
            let plan = validator::validate_plan(reg, &original, level, Source::Ai, false);
            let title = if plan.summary.is_empty() { "AI 제안".to_string() } else { plan.summary.clone() };
            (plan, title, format!("ai:{plan_id}"))
        }
        Base::None => {
            let empty = ExecutionPlan { intent: "manual".into(), summary: "직접 명령".into(), target: None, steps: vec![] };
            (validator::validate_plan(reg, &empty, level, Source::Manual, false), "직접 명령".to_string(), "manual".to_string())
        }
    };
    for text in extras {
        let order = plan.steps.len() as u32 + 1;
        plan.steps.push(manual_step(text, order, level)?);
    }
    plan.risk = plan.steps.iter().map(|s| s.risk).max().unwrap_or(Risk::Safe);
    plan.blocked = plan.steps.is_empty() || plan.steps.iter().any(|s| s.blocked);
    let source = if extras.is_empty() || source == "manual" { source } else { format!("{source}+manual") };
    Ok((plan, title, source))
}

/// 바탕 계획에 직접 추가한 명령을 붙여 검증한다. 실행하지는 않는다.
#[tauri::command]
pub fn plan_mixed(base: Base, extras: Vec<String>, level: Level) -> Result<ValidatedPlan, String> {
    build_mixed(base, &extras, level).map(|(plan, _, _)| plan)
}

/// 바탕 계획과 직접 추가한 명령을 함께 실행한다. 화면이 보낸 글에서 계획을 다시 만들고 다시 검증한다.
#[tauri::command]
pub async fn run_mixed(app: AppHandle, base: Base, extras: Vec<String>, level: Level, confirmed: bool, only_step: Option<u32>) -> Result<RunStarted, String> {
    let (plan, title, source) = build_mixed(base, &extras, level)?;
    start_plan(app, plan, &title, &source, level, confirmed, only_step).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn topology_svg_is_checked_before_saving() {
        let good = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 760 364"><rect x="0" y="0" width="760" height="364" fill="#0b1020"/><text x="8" y="20" font-family="Consolas,monospace" font-size="11" text-anchor="middle">192.168.0.30 DESKTOP-ABC 공유기 online</text></svg>"##;
        assert!(check_topology_svg(good).is_ok());
        for bad in [
            "<html><svg></svg></html>",
            " <svg></svg>",
            "<svg><script>alert(1)</script></svg>",
            "<svg><SCRIPT src='x'/></svg>",
            "<svg onload=\"x()\"></svg>",
            "<svg/onload=x()>",
            "<svg><rect OnClick = 'x'/></svg>",
            "<svg><a href='https://example.com'>x</a></svg>",
            "<svg><image xlink:HREF='x'/></svg>",
            "<svg><foreignObject><b>x</b></foreignObject></svg>",
        ] {
            assert!(check_topology_svg(bad).is_err(), "{bad}");
        }
        let big = format!("<svg>{}</svg>", "a".repeat(MAX_SVG_BYTES));
        assert!(check_topology_svg(&big).is_err());
    }

    #[test]
    fn extras_are_appended_after_the_recipe_and_marked_manual() {
        let base = Base::Recipe { id: "route_latency".into(), inputs: [("target".to_string(), "8.8.8.8".to_string())].into() };
        let (plan, title, source) = build_mixed(base, &s(&["ping -c 4 8.8.8.8", "dig example.com"]), Level::Middle).unwrap();
        let n = plan.steps.len();
        assert!(n >= 3);
        assert!(!plan.blocked);
        assert!(plan.steps[..n - 2].iter().all(|x| !x.manual));
        assert!(plan.steps[n - 2..].iter().all(|x| x.manual));
        assert_eq!(plan.steps.iter().map(|x| x.order).collect::<Vec<_>>(), (1..=n as u32).collect::<Vec<_>>());
        assert_eq!(source, "recipe:route_latency+manual");
        assert!(!title.is_empty());
    }

    #[test]
    fn connect_command_only_accepts_checked_values() {
        let (_, a) = connect_command("192.168.0.1", 443, "web", None).unwrap();
        assert_eq!(a, ["https://192.168.0.1:443/"]);
        assert_eq!(connect_command("192.168.0.1", 8080, "web", None).unwrap().1, ["http://192.168.0.1:8080/"]);
        let (_, a) = connect_command("10.0.0.2", 22, "ssh", Some("admin")).unwrap();
        assert_eq!(a[a.len() - 6..], ["ssh", "-p", "22", "-l", "admin", "10.0.0.2"]);
        for (host, kind, user) in [
            ("example.com", "web", None),
            ("10.0.0.1 & calc", "web", None),
            ("10.0.0.1", "ssh", None),
            ("10.0.0.1", "ssh", Some("a b")),
            ("10.0.0.1", "ssh", Some("-oProxyCommand=calc")),
            ("10.0.0.1", "ssh", Some("a&calc")),
            ("10.0.0.1", "rdp", None),
        ] {
            assert!(connect_command(host, 22, kind, user).is_err(), "{host} {kind} {user:?}");
        }
        assert!(connect_command("10.0.0.1", 0, "web", None).is_err());
    }

    #[test]
    fn manual_only_plan_and_gates() {
        let (plan, _, source) = build_mixed(Base::None, &s(&["ping -c 2 1.1.1.1"]), Level::Junior).unwrap();
        assert_eq!((plan.steps.len(), plan.blocked, source.as_str()), (1, false, "manual"));
        assert!(build_mixed(Base::None, &[], Level::Middle).unwrap().0.blocked, "빈 계획은 실행할 수 없다");
        assert!(build_mixed(Base::None, &s(&["ping 1.1.1.1"]), Level::General).is_err());
        assert!(build_mixed(Base::None, &s(&["ping 1.1.1.1 | sh"]), Level::Op).is_err());
        assert!(build_mixed(Base::None, &s(&["rm -rf /"]), Level::Op).unwrap().0.blocked);
        assert!(build_mixed(Base::None, &vec!["ping 1.1.1.1".to_string(); 11], Level::Op).is_err());
        // 레시피 권한은 직접 추가한 단계로 번지지 않는다: 초급의 주의 옵션은 차단된다.
        let v = check_command("tcpdump -i eth0 -w x.pcap".into(), Level::Junior).unwrap();
        assert!(v.blocked);
    }
}
