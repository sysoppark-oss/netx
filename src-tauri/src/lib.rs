//! NetX 백엔드 진입점.
//!
//! Phase 2까지: 환경 확인, 도구·레시피 레지스트리, 보안 검증기.
//! 이후 단계의 모듈(engine, planner, audit, setup, auth)은
//! docs/DEV_PLAN_v0.1.md 5장의 인터페이스를 따른다.

mod commands;
mod env_check;
mod model;
mod recipe;
mod registry;
mod security;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::environment_status,
            commands::get_tools,
            commands::get_recipes,
            commands::plan_from_recipe,
            commands::validate_plan
        ])
        .run(tauri::generate_context!())
        .expect("NetX 실행 중 오류가 발생했습니다");
}
