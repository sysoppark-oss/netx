//! 기본 진단: WSL이 없는 PC에서 리눅스 도구 대신 쓰는 NetX 내장 기능.
//!
//! - ping, mtr(경로 추적), fping(대역 핑): ICMP (`icmp`)
//! - dig: DNS 직접 질의 (`dns`)
//! - nc: TCP 포트 확인 (`tcp`)
//! - curl: HTTP·HTTPS 요청과 인증서 만료일 (`http`)
//!
//! 출력은 리눅스 도구와 같은 모양으로 내서 결과 해설(`interpret`)과 그래프를 그대로 쓴다.
//! 리눅스 도구의 옵션을 다 흉내 내지 않는다. 지원하지 않는 옵션은 실행 전에 "전체 진단 필요"로 막는다(`supports`).
//! 외부 프로그램을 띄우지 않는다(리눅스 개발 환경의 ICMP만 예외, `icmp` 참고).

mod args;
mod dns;
mod http;
pub mod icmp;
mod ping;
mod sweep;
mod tcp;
mod trace;

use crate::model::{Engine, Issue, Severity, ValidatedPlan, ValidatedStep};

/// 출력 한 줄을 내보내는 곳
pub type Emit<'a> = &'a (dyn Fn(String) + Send + Sync);

/// 기본 진단으로 돌릴 수 있는 도구
pub const TOOLS: [&str; 6] = ["ping", "mtr", "fping", "dig", "nc", "curl"];

/// 이 인자로 기본 진단을 돌릴 수 있는지. 못 하면 까닭을 돌려준다.
pub fn supports(tool: &str, argv: &[String]) -> Result<(), String> {
    match tool {
        "ping" => ping::Plan::parse(argv).map(|_| ()),
        "mtr" => trace::Plan::parse(argv).map(|_| ()),
        "fping" => sweep::Plan::parse(argv).map(|_| ()),
        "dig" => dns::Plan::parse(argv).map(|_| ()),
        "nc" => tcp::Plan::parse(argv).map(|_| ()),
        "curl" => http::Plan::parse(argv).map(|_| ()),
        other => Err(format!("기본 진단에 없는 도구입니다: {other}")),
    }
}

/// 기본 진단을 실행한다. 돌려주는 값은 리눅스 도구의 종료 코드와 같은 뜻이다.
pub async fn run(tool: &str, argv: &[String], emit: Emit<'_>) -> Result<i32, String> {
    match tool {
        "ping" => ping::run(ping::Plan::parse(argv)?, icmp::system(), emit).await,
        "mtr" => trace::run(trace::Plan::parse(argv)?, icmp::system(), emit).await,
        "fping" => sweep::run(sweep::Plan::parse(argv)?, icmp::system(), emit).await,
        "dig" => dns::run(dns::Plan::parse(argv)?, emit).await,
        "nc" => tcp::run(tcp::Plan::parse(argv)?, emit).await,
        "curl" => http::run(http::Plan::parse(argv)?, emit).await,
        other => Err(format!("기본 진단에 없는 도구입니다: {other}")),
    }
}

/// 이 단계를 어디에서 실행할지. `full`은 WSL을 쓸 수 있는지.
/// WSL 도구인데 WSL이 없으면 기본 진단으로 돌린다(스키마가 `builtin`을 함께 적은 도구만).
pub fn engine_for(step: &ValidatedStep, full: bool) -> Option<Engine> {
    match step.engines.first()? {
        Engine::Wsl if !full => (step.engines.contains(&Engine::Builtin) && TOOLS.contains(&step.tool.as_str())).then_some(Engine::Builtin),
        first => Some(*first),
    }
}

/// WSL이 없을 때, 실행할 수 없는 단계를 막아 둔다(까닭을 함께 적는다). 나머지 단계는 그대로 실행된다.
pub fn fit_step(step: &mut ValidatedStep, full: bool) {
    if full || step.blocked || step.engines.first() != Some(&Engine::Wsl) {
        return;
    }
    let problem = match engine_for(step, false) {
        None => Some("이 도구는 전체 진단(WSL2)이 있어야 쓸 수 있습니다. 머리줄의 [기본 진단]을 눌러 준비하세요.".to_string()),
        Some(_) => supports(&step.tool, &step.argv).err(),
    };
    if let Some(message) = problem {
        step.blocked = true;
        step.confirm = crate::model::Confirm::None;
        step.issues.push(Issue { severity: Severity::Block, message });
    } else {
        step.needs_root = false;
    }
}

pub fn fit_plan(plan: &mut ValidatedPlan, full: bool) {
    for step in &mut plan.steps {
        fit_step(step, full);
    }
    plan.blocked = plan.steps.is_empty() || plan.steps.iter().any(|s| s.blocked);
}

/// 지원하지 않는 옵션을 만났을 때의 글
fn unsupported(what: &str) -> String {
    format!("기본 진단(WSL 없음)에서는 {what}을(를) 쓸 수 없습니다. 전체 진단(WSL2)이 필요합니다.")
}

/// 상대가 보낸 글에서 제어 문자를 지운다(화면에는 글자로만 보여야 한다).
fn clean(text: &str) -> String {
    text.chars().map(|c| if c == '\t' || !c.is_control() { c } else { '?' }).collect()
}

/// 이름을 IPv4 주소로 바꾼다. 내장 ICMP는 IPv4만 다룬다.
async fn resolve_v4(host: &str) -> Result<std::net::Ipv4Addr, String> {
    if let Ok(ip) = host.parse() {
        return Ok(ip);
    }
    let found = tokio::net::lookup_host((host, 0)).await.map_err(|_| format!("ping: {host}: Name or service not known"))?;
    found.filter_map(|a| if let std::net::IpAddr::V4(v4) = a.ip() { Some(v4) } else { None }).next().ok_or_else(|| format!("ping: {host}: IPv4 주소가 없습니다"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Level;
    use crate::registry::Registry;

    fn step(tool: &str, args: &[&str]) -> ValidatedStep {
        let plan = crate::model::ExecutionPlan {
            intent: String::new(),
            summary: String::new(),
            target: None,
            steps: vec![crate::model::PlanStep { order: None, tool: tool.into(), args: args.iter().map(|s| s.to_string()).collect(), reason: None, expect: None }],
        };
        crate::security::validator::validate_plan(Registry::global(), &plan, Level::Op, crate::model::Source::Manual, false).steps.remove(0)
    }

    #[test]
    fn recipes_that_work_without_wsl() {
        // 레시피가 쓰는 인자 그대로 기본 진단이 받는다.
        for (tool, args) in [
            ("dig", vec!["+short", "example.com"]),
            ("nc", vec!["-vz", "-w", "3", "example.com", "443"]),
            ("curl", vec!["-I", "-v", "-k", "-s", "-m", "10", "-w", "\\nnetx_times dns=%{time_namelookup}\\n", "https://example.com"]),
            ("mtr", vec!["-r", "-w", "-b", "-c", "10", "8.8.8.8"]),
            ("ping", vec!["-c", "5", "8.8.8.8"]),
            ("fping", vec!["-a", "-e", "-r", "1", "-g", "192.168.0.0/24"]),
        ] {
            let mut s = step(tool, &args);
            assert!(!s.blocked, "{tool}: {:?}", s.issues);
            fit_step(&mut s, false);
            assert!(!s.blocked, "{tool}: {:?}", s.issues);
            assert_eq!(engine_for(&s, false), Some(Engine::Builtin), "{tool}");
            assert_eq!(engine_for(&s, true), Some(Engine::Wsl), "{tool}");
        }
    }

    #[test]
    fn blocks_what_basic_mode_cannot_do() {
        // WSL에만 있는 도구
        let mut s = step("tcpdump", &["-c", "10", "-i", "eth0"]);
        fit_step(&mut s, false);
        assert!(s.blocked && s.issues.iter().any(|i| i.message.contains("전체 진단")), "{:?}", s.issues);
        // 내장이 지원하지 않는 옵션
        let mut s = step("ping", &["-I", "eth0", "8.8.8.8"]);
        fit_step(&mut s, false);
        assert!(s.blocked && s.issues.iter().any(|i| i.message.contains("-I")), "{:?}", s.issues);
        // WSL이 있으면 그대로 둔다. Windows 조회와 내장 도구는 어느 쪽이든 그대로다.
        let mut s = step("tcpdump", &["-c", "10", "-i", "eth0"]);
        fit_step(&mut s, true);
        assert!(!s.blocked);
        let mut s = step("portscan", &["192.168.0.0/30", "80"]);
        fit_step(&mut s, false);
        assert!(!s.blocked && engine_for(&s, false) == Some(Engine::Builtin));
        let mut s = step("winports", &["80"]);
        fit_step(&mut s, false);
        assert!(!s.blocked && engine_for(&s, false) == Some(Engine::Windows));
    }
}
