//! 초기 설정: WSL 안에 진단 도구가 있는지 확인하고 설치한다.
//! 명령과 패키지 목록은 모두 코드에 고정되어 있고, 사용자 입력이 들어가지 않는다.

use std::process::Stdio;
use std::time::Duration;

use serde::Serialize;
use tokio::process::Command;

use crate::engine::{Job, JobEngine};
use crate::model::Engine;
use crate::env_check::hide_console_window;
use crate::registry::Registry;

/// 실행 엔진이 쓰는 보조 프로그램
const HELPERS: &[&str] = &["timeout", "stdbuf", "pkill"];

/// 도구 9종과 보조 프로그램이 들어 있는 Ubuntu 패키지
const PACKAGES: &[&str] = &[
    "iputils-ping",
    "fping",
    "tcpdump",
    "mtr-tiny",
    "iproute2",
    "dnsutils",
    "netcat-openbsd",
    "curl",
    "lsof",
    "iperf3",
    "procps",
    "coreutils",
];

#[derive(Serialize)]
pub struct ToolStatus {
    pub tool: String,
    pub installed: bool,
}

/// `which`로 도구가 설치되어 있는지 확인한다.
pub async fn tool_status() -> Result<Vec<ToolStatus>, String> {
    let names: Vec<&str> = Registry::global()
        .tools()
        .iter()
        .filter(|t| t.engines.contains(&Engine::Wsl))
        .map(|t| t.tool.as_str()).chain(HELPERS.iter().copied()).collect();

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("wsl.exe");
        c.args(["--exec", "which"]).args(&names);
        c
    } else {
        let mut c = Command::new("which");
        c.args(&names);
        c
    };
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    hide_console_window(&mut cmd);

    let output = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .map_err(|_| "도구 확인이 시간 안에 끝나지 않았습니다".to_string())?
        .map_err(|e| format!("도구 확인을 실행하지 못했습니다: {e}"))?;
    let found = parse_which(&String::from_utf8_lossy(&output.stdout));

    Ok(names.iter().map(|n| ToolStatus { tool: n.to_string(), installed: found.iter().any(|f| f == n) }).collect())
}

/// `/sys/class/net`의 인터페이스 이름을 읽는다. 실패하면 빈 목록.
pub async fn list_interfaces() -> Vec<String> {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("wsl.exe");
        c.args(["--exec", "ls", "/sys/class/net"]);
        c
    } else {
        let mut c = Command::new("ls");
        c.arg("/sys/class/net");
        c
    };
    cmd.stdin(Stdio::null()).kill_on_drop(true);
    hide_console_window(&mut cmd);
    match tokio::time::timeout(Duration::from_secs(10), cmd.output()).await {
        Ok(Ok(out)) if out.status.success() => parse_interfaces(&String::from_utf8_lossy(&out.stdout)),
        _ => vec![],
    }
}

fn parse_interfaces(stdout: &str) -> Vec<String> {
    stdout
        .split_whitespace()
        .filter(|n| n.len() <= 32 && n.chars().all(|c| c.is_ascii_alphanumeric() || "._-@:".contains(c)))
        .map(str::to_string)
        .take(64)
        .collect()
}

fn parse_which(stdout: &str) -> Vec<String> {
    stdout.lines().filter_map(|l| l.trim().rsplit('/').next()).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// 도구 설치 작업(패키지 목록 갱신 → 설치). root로 실행한다.
pub fn install_jobs() -> Vec<Job> {
    let apt = |order: u32, args: &[&str]| Job {
        order,
        program: "env".into(),
        argv: ["DEBIAN_FRONTEND=noninteractive", "apt-get"].iter().chain(args).map(|s| s.to_string()).collect(),
        root: true,
        timeout_secs: 900,
        engine: JobEngine::Wsl,
    };
    let mut install = vec!["install", "-y", "-q"];
    install.extend(PACKAGES);
    vec![apt(1, &["update", "-q"]), apt(2, &install)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_which_output() {
        assert_eq!(parse_which("/usr/bin/ping\n/usr/sbin/tcpdump\n\n"), ["ping", "tcpdump"]);
        assert!(parse_which("").is_empty());
    }

    #[test]
    fn parses_interface_names() {
        assert_eq!(parse_interfaces("eth0\nlo\nwlan0 docker0\n"), ["eth0", "lo", "wlan0", "docker0"]);
        assert!(parse_interfaces("ls: cannot access '/sys/class/net'; rm -rf").iter().all(|n| !n.contains(';')));
    }

    #[test]
    fn install_jobs_are_fixed_commands() {
        let jobs = install_jobs();
        assert_eq!(jobs.len(), 2);
        assert!(jobs.iter().all(|j| j.root && j.program == "env"));
        assert_eq!(jobs[0].argv, ["DEBIAN_FRONTEND=noninteractive", "apt-get", "update", "-q"]);
        assert!(jobs[1].argv.contains(&"fping".to_string()));
    }
}
