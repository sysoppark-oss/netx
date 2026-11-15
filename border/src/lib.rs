//! 국경경비대 출국 심사.
//! 비자(사람이 정함) · 신고(에이전트가 적음) · 실제 변경(git) 셋을 규칙으로 맞춰 본다.
//! 아무것도 고치거나 지우지 않는다. AI를 쓰지 않는다.

pub mod check;
pub mod deps;
pub mod git;
pub mod glob;
pub mod report;
pub mod visa;
