//! 화면에서 호출하는 Tauri 명령.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::env_check::{self, EnvironmentStatus};
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
