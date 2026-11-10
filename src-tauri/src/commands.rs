//! 화면에서 호출하는 Tauri 명령.

use serde::Serialize;

use crate::env_check::{self, EnvironmentStatus};

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
