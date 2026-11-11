//! 감사 로그. 실행한 명령을 한 줄에 하나씩 JSON으로 남긴다(DEV_PLAN 4.6).
//! 기록에 실패해도 실행을 막지 않는다.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::model::{Level, Risk};

#[derive(Serialize)]
pub struct AuditEntry<'a> {
    /// 유닉스 시각(초)
    pub time: u64,
    pub run_id: u64,
    pub level: Level,
    pub source: &'a str,
    pub tool: &'a str,
    pub argv: &'a [String],
    pub risk: Risk,
    pub root: bool,
    pub confirmed: bool,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn log(dir: &Path, entry: &AuditEntry) {
    let Ok(line) = serde_json::to_string(entry) else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("audit.log")) {
        let _ = writeln!(f, "{line}");
    }
}

/// 터미널 접속 기록. 종류, 대상, 사용자 이름만 남긴다(암호, 친 글자, 화면 내용은 남기지 않는다).
#[derive(Serialize)]
struct TerminalEntry<'a> {
    time: u64,
    source: &'a str,
    kind: &'a str,
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<&'a str>,
}

pub fn log_terminal(dir: &Path, kind: &str, target: &str, user: Option<&str>) {
    let Ok(line) = serde_json::to_string(&TerminalEntry { time: now(), source: "terminal", kind, target, user }) else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("audit.log")) {
        let _ = writeln!(f, "{line}");
    }
}
