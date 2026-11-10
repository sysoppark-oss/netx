//! 실행 전 검증. 기준: docs/DESIGN.md 11.2, docs/DEV_PLAN_v0.1.md 4장
//!
//! AI가 만든 계획, 레시피, 사용자가 직접 조립한 명령 모두 이 검증기를 통과해야 실행 후보가 된다.

pub mod validator;
pub mod values;
