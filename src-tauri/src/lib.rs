//! NetX 백엔드 진입점.
//!
//! Phase 3까지: 환경 확인, 레지스트리, 보안 검증기, 실행 엔진(WSL), 감사 로그, 도구 설치.
//! 이후 단계의 모듈(planner, auth)은
//! docs/DEV_PLAN_v0.1.md 5장의 인터페이스를 따른다.

mod audit;
mod basic;
mod bridge;
mod capview;
mod commands;
mod engine;
mod env_check;
mod history;
mod report_fig;
mod estimate;
mod interpret;
mod manual;
mod model;
mod netinfo;
mod pcap;
mod planner;
mod prockill;
mod recipe;
mod registry;
mod security;
mod settings;
mod setup;
mod snmp;
mod term;
mod updater;
mod voice;
mod word;

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
        // 다른 프로그램이 진단 대상을 넘겼을 때 Windows 알림 창을 띄우는 데 쓴다.
        .plugin(tauri_plugin_notification::init())
        // 본창을 닫으면 터미널 창도 닫는다(터미널 창만 남으면 본창을 다시 열 길이 없다).
        .on_window_event(|window, event| {
            use tauri::Manager;
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                term::close_all();
                #[cfg(target_os = "windows")]
                bridge::pipe::unpublish();
                if let Some(terminal) = window.app_handle().get_webview_window("terminal") {
                    let _ = terminal.close();
                }
            }
        })
        .setup(|app| {
            commands::watch_updates(app.handle().clone());
            // 다른 프로그램(스토리방화벽 등)이 진단 대상을 넘길 수 있는 입구를 연다.
            #[cfg(target_os = "windows")]
            bridge::pipe::start(app.handle().clone());
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
            commands::apply_update,
            commands::get_word,
            commands::open_church_mail,
            commands::history_save,
            commands::history_list,
            commands::history_load,
            commands::history_delete,
            commands::history_export,
            commands::history_import,
            commands::capture_list,
            commands::capture_open,
            commands::capture_rows,
            commands::capture_packet,
            commands::capture_folder,
            voice::voice_typing,
            bridge::bridge_take_handoff,
            bridge::bridge_status,
            term::cmd::open_terminal,
            term::cmd::term_take_preset,
            term::cmd::term_ports,
            term::cmd::term_open,
            term::cmd::term_write,
            term::cmd::term_resize,
            term::cmd::term_close,
            term::cmd::term_folder
        ])
        .run(tauri::generate_context!())
        .expect("NetX 실행 중 오류가 발생했습니다");
}
