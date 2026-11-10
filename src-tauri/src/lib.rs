//! NetX 백엔드 진입점.
//!
//! Phase 1에서는 앱 정보와 실행 환경 확인 명령만 둔다.
//! 이후 단계의 모듈(registry, security, engine, planner, audit, setup, auth)은
//! docs/DEV_PLAN_v0.1.md 5장의 인터페이스를 따른다.

mod commands;
mod env_check;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::environment_status
        ])
        .run(tauri::generate_context!())
        .expect("NetX 실행 중 오류가 발생했습니다");
}
