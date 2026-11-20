//! NetX 백엔드 진입점.
//!
//! Phase 3까지: 환경 확인, 레지스트리, 보안 검증기, 실행 엔진(WSL), 감사 로그, 도구 설치.
//! 이후 단계의 모듈(planner, auth)은
//! docs/DEV_PLAN_v0.1.md 5장의 인터페이스를 따른다.

mod audit;
mod basic;
mod bridge;
mod capview;
mod collect;
mod commands;
mod engine;
mod env_check;
mod history;
mod manuallog;
mod report;
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
mod wslsetup;

/// 창이 화면보다 크면 줄일 크기(가로, 세로). 화면에 들어가면 없음. 단위는 모두 실제 화소.
/// 작업 표시줄과 창 테두리 몫으로 화면의 92%까지만 쓴다.
fn fitted_size(window: (u32, u32), screen: (u32, u32)) -> Option<(u32, u32)> {
    let room = |v: u32| (u64::from(v) * 92 / 100) as u32;
    let (w, h) = (window.0.min(room(screen.0)), window.1.min(room(screen.1)));
    (screen.0 > 0 && screen.1 > 0 && (w, h) != window).then_some((w, h))
}

/// 켤 때 한 번: 기본 크기(1440×900)가 화면보다 크면 화면 안으로 줄이고 가운데에 둔다.
/// 화면보다 큰 창은 가장자리가 화면 밖에 있어 끌어서 크기를 바꾸기 어렵다.
fn fit_to_screen(app: &tauri::AppHandle) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("main") else { return };
    let (Ok(size), Ok(Some(monitor))) = (window.outer_size(), window.current_monitor()) else { return };
    if let Some((w, h)) = fitted_size((size.width, size.height), (monitor.size().width, monitor.size().height)) {
        let _ = window.set_size(tauri::PhysicalSize::new(w, h));
        let _ = window.center();
    }
}

#[cfg(test)]
mod window_tests {
    use super::fitted_size;

    #[test]
    fn window_larger_than_the_screen_is_shrunk_to_fit() {
        // 1366×768 노트북: 기본 창이 화면보다 크다
        assert_eq!(fitted_size((1440, 900), (1366, 768)), Some((1256, 706)));
        // 세로만 넘칠 때는 세로만
        assert_eq!(fitted_size((1440, 900), (1920, 900)), Some((1440, 828)));
        // 넉넉한 화면에서는 건드리지 않는다
        assert_eq!(fitted_size((1440, 900), (2560, 1440)), None);
        assert_eq!(fitted_size((1440, 900), (0, 0)), None);
    }
}

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
        // 보고서를 저장할 자리를 사용자가 고르는 저장 창. 백엔드에서만 띄운다(화면에는 이 플러그인의 권한을 주지 않는다).
        .plugin(tauri_plugin_dialog::init())
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
            // 관제 PC: 보내기도 받기도 기본은 꺼져 있다. 켜기 전에는 듣지도 보내지도 않는다.
            collect::app::start(app.handle());
            fit_to_screen(app.handle());
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
            commands::set_word_lang,
            commands::open_church_mail,
            commands::history_save,
            commands::history_list,
            commands::history_load,
            commands::history_delete,
            commands::manual_save,
            commands::report_make,
            commands::report_cancel,
            commands::report_open,
            commands::history_import,
            commands::capture_list,
            commands::capture_open,
            commands::capture_rows,
            commands::capture_packet,
            commands::capture_folder,
            voice::voice_typing,
            bridge::bridge_take_handoff,
            bridge::bridge_report,
            bridge::bridge_status,
            collect::app::collect_overview,
            collect::app::collect_set,
            collect::app::collect_submit,
            collect::app::collect_join,
            collect::app::collect_join_cancel,
            collect::app::collect_forget,
            collect::app::collect_sent,
            collect::app::collect_pair_open,
            collect::app::collect_pair_close,
            collect::app::collect_pair_decide,
            collect::app::collect_pc_set,
            commands::wsl_setup_status,
            commands::wsl_setup_start,
            commands::wsl_setup_restart,
            commands::wsl_setup_clear,
            collect::app::collect_site,
            collect::app::collect_inventory,
            collect::app::collect_inventory_set,
            collect::app::collect_inventory_preview,
            collect::app::collect_fleet,
            collect::app::collect_plan,
            collect::app::collect_plan_set,
            collect::app::collect_plan_now,
            collect::app::collect_inventory_run,
            collect::app::collect_board,
            collect::app::collect_list,
            collect::app::collect_item,
            collect::app::collect_analysis,
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
