//! NetX 백엔드 진입점.
//!
//! Phase 3까지: 환경 확인, 레지스트리, 보안 검증기, 실행 엔진(WSL), 감사 로그, 도구 설치.
//! 이후 단계의 모듈(planner, auth)은
//! docs/DEV_PLAN_v0.1.md 5장의 인터페이스를 따른다.

mod audit;
mod commands;
mod engine;
mod env_check;
mod estimate;
mod interpret;
mod manual;
mod model;
mod netinfo;
mod planner;
mod prockill;
mod recipe;
mod registry;
mod security;
mod settings;
mod setup;
mod updater;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 창을 하나만 띄운다. 다시 실행하면 이미 떠 있는 창을 앞으로 가져온다.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .setup(|app| {
            commands::watch_updates(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::environment_status,
            commands::get_tools,
            commands::get_recipes,
            commands::plan_from_recipe,
            commands::validate_plan,
            commands::run_recipe,
            commands::cancel_run,
            commands::tool_status,
            commands::install_tools,
            commands::run_ai_plan,
            commands::suggest,
            commands::brief_run,
            commands::get_settings,
            commands::save_settings,
            commands::test_llm,
            commands::list_interfaces,
            commands::network_info,
            commands::check_command,
            commands::plan_mixed,
            commands::run_mixed,
            commands::open_target,
            commands::kill_process,
            commands::save_topology,
            commands::apply_update
        ])
        .run(tauri::generate_context!())
        .expect("NetX 실행 중 오류가 발생했습니다");
}
