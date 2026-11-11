//! 대역 핑 (기본 진단의 fping). 대역 안의 주소에 한꺼번에 핑을 보내 답한 장비를 적는다.

use std::net::Ipv4Addr;
use std::sync::Arc;

use tokio::sync::Semaphore;

use super::args::Args;
use super::icmp::{Echo, Prober, probe};
use super::trace::join_all;
use super::{Emit, unsupported};

/// 한 번에 보낼 수 있는 주소 수
const MAX_TARGETS: usize = 4096;
const PARALLEL: usize = 64;

#[derive(Debug)]
pub struct Plan {
    targets: Vec<Ipv4Addr>,
    retries: u32,
    wait_ms: u32,
    show_time: bool,
    alive_only: bool,
}

impl Plan {
    pub fn parse(argv: &[String]) -> Result<Plan, String> {
        let a = Args::parse(argv, &["-r", "-t"], &["-a", "-e", "-g", "-q", "-4", "-n"])?;
        let targets: Vec<Ipv4Addr> = if a.has("-g") {
            crate::engine::builtin::cidr_hosts(a.one("대역")?).ok_or("대역은 192.168.0.0/24처럼 적습니다")?
        } else {
            if a.rest.is_empty() {
                return Err("대상이 없습니다".into());
            }
            a.rest.iter().map(|t| t.parse().map_err(|_| unsupported("IP 주소가 아닌 대상(이름)"))).collect::<Result<_, _>>()?
        };
        if targets.len() > MAX_TARGETS {
            return Err(format!("한 번에 {MAX_TARGETS}개 주소까지 보낼 수 있습니다 (/20 이하)"));
        }
        Ok(Plan { targets, retries: a.number("-r", 3.0, 0.0, 5.0)? as u32, wait_ms: a.number("-t", 500.0, 50.0, 5000.0)? as u32, show_time: a.has("-e"), alive_only: a.has("-a") })
    }
}

pub async fn run(plan: Plan, prober: Prober, emit: Emit<'_>) -> Result<i32, String> {
    let gate = Arc::new(Semaphore::new(PARALLEL));
    let (retries, wait_ms) = (plan.retries, plan.wait_ms);
    let probes = plan.targets.iter().map(|&ip| {
        let (prober, gate) = (prober.clone(), gate.clone());
        async move {
            let _permit = gate.acquire().await;
            for _ in 0..=retries {
                match probe(&prober, ip, 64, wait_ms, 56).await {
                    Echo::Reply { ms, .. } => return (ip, Some(ms)),
                    Echo::Unreachable(_) | Echo::Failed(_) => break,
                    _ => {}
                }
            }
            (ip, None)
        }
    });
    let results = join_all(probes).await;
    let mut alive = 0;
    for (ip, ms) in &results {
        match ms {
            Some(ms) => {
                alive += 1;
                emit(if plan.show_time { format!("{ip} ({ms:.2} ms)") } else { ip.to_string() });
            }
            None if !plan.alive_only => emit(format!("{ip} is unreachable")),
            None => {}
        }
    }
    // fping과 같다: 모두 답하면 0, 답하지 않은 주소가 있으면 1
    Ok(if alive == results.len() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::super::ping::tests::{collect, s};
    use super::*;

    #[test]
    fn options() {
        let p = Plan::parse(&s(&["-a", "-e", "-r", "1", "-g", "192.168.0.0/24"])).unwrap();
        assert_eq!((p.targets.len(), p.retries, p.show_time, p.alive_only), (254, 1, true, true));
        assert_eq!(Plan::parse(&s(&["10.0.0.1", "10.0.0.2"])).unwrap().targets.len(), 2);
        for bad in [vec!["-g", "10.0.0.0/8"], vec!["-g", "nope"], vec!["-c", "3", "10.0.0.1"], vec!["example.com"], vec![]] {
            assert!(Plan::parse(&s(&bad)).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn lists_hosts_that_answer() {
        // 끝자리가 1과 7인 주소만 답한다. 7은 두 번째 시도에 답한다.
        let tries = Arc::new(std::sync::Mutex::new(0));
        let prober: Prober = Arc::new(move |ip, _, _, _| match ip.octets()[3] {
            1 => Echo::Reply { from: ip, ms: 0.52, ttl: 64 },
            7 => {
                let mut t = tries.lock().unwrap();
                *t += 1;
                if *t == 1 { Echo::Timeout } else { Echo::Reply { from: ip, ms: 3.1, ttl: 64 } }
            }
            _ => Echo::Timeout,
        });
        let (lines, emit) = collect();
        let code = run(Plan::parse(&s(&["-a", "-e", "-r", "1", "-g", "192.168.0.0/28"])).unwrap(), prober, &emit).await.unwrap();
        let lines = lines.lock().unwrap().clone();
        assert_eq!((code, lines.as_slice()), (1, &["192.168.0.1 (0.52 ms)".to_string(), "192.168.0.7 (3.10 ms)".to_string()][..]));
        let r = crate::interpret::interpret("fping", &lines, crate::engine::StepStatus::Done, Some(code));
        assert!(r.text.contains("2대"), "{}", r.text);
    }
}
