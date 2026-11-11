//! nc (기본 진단). 포트가 열려 있는지만 확인한다(`-z`). 글자를 주고받는 기능은 없다.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::net::TcpStream;

use super::args::Args;
use super::{Emit, unsupported};

const MAX_PORTS: usize = 1024;

#[derive(Debug)]
pub struct Plan {
    host: String,
    ports: Vec<u16>,
    wait: f64,
    verbose: bool,
}

impl Plan {
    pub fn parse(argv: &[String]) -> Result<Plan, String> {
        let a = Args::parse(argv, &["-w"], &["-v", "-z", "-n", "-4"])?;
        if !a.has("-z") {
            return Err(unsupported("포트 확인(-z)이 아닌 nc 사용"));
        }
        let [host, ports] = &a.rest[..] else { return Err("대상과 포트를 적습니다".into()) };
        let (lo, hi) = ports.split_once('-').unwrap_or((ports, ports));
        let port = |v: &str| v.parse::<u16>().ok().filter(|p| *p > 0).ok_or("포트 번호가 올바르지 않습니다");
        let (lo, hi) = (port(lo)?, port(hi)?);
        if lo > hi || usize::from(hi - lo) >= MAX_PORTS {
            return Err(format!("포트 범위가 올바르지 않습니다 (한 번에 {MAX_PORTS}개까지)"));
        }
        Ok(Plan { host: host.clone(), ports: (lo..=hi).collect(), wait: a.number("-w", 3.0, 1.0, 60.0)?, verbose: a.has("-v") })
    }
}

pub async fn run(plan: Plan, emit: Emit<'_>) -> Result<i32, String> {
    let literal = plan.host.parse::<std::net::IpAddr>().ok();
    let ip = match literal {
        Some(ip) => ip,
        None => match tokio::net::lookup_host((plan.host.as_str(), 0)).await.ok().and_then(|found| {
            let all: Vec<SocketAddr> = found.collect();
            all.iter().find(|a| a.is_ipv4()).or(all.first()).map(|a| a.ip())
        }) {
            Some(ip) => ip,
            None => {
                emit(format!("nc: getaddrinfo for host \"{}\" port {}: Name or service not known", plan.host, plan.ports[0]));
                return Ok(1);
            }
        },
    };
    // 이름으로 적었으면 리눅스 nc처럼 주소를 함께 보인다.
    let shown = if literal.is_some() { plan.host.clone() } else { format!("{} ({ip})", plan.host) };
    let mut open = 0;
    for &port in &plan.ports {
        let line = match tokio::time::timeout(Duration::from_secs_f64(plan.wait), TcpStream::connect(SocketAddr::new(ip, port))).await {
            Ok(Ok(_)) => {
                open += 1;
                let service = crate::engine::builtin::service_name(port).to_lowercase();
                format!("Connection to {shown} {port} port [tcp/{}] succeeded!", if service.is_empty() { "*" } else { &service })
            }
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => format!("nc: connect to {shown} port {port} (tcp) failed: Connection refused"),
            Ok(Err(e)) => format!("nc: connect to {shown} port {port} (tcp) failed: {e}"),
            Err(_) => format!("nc: connect to {shown} port {port} (tcp) timed out: Operation now in progress"),
        };
        if plan.verbose {
            emit(line);
        }
    }
    Ok(if open > 0 { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::super::ping::tests::{collect, s};
    use super::*;
    use crate::interpret::{Light, interpret};

    #[test]
    fn options() {
        let p = Plan::parse(&s(&["-vz", "-w", "3", "example.com", "443"])).unwrap();
        assert_eq!((p.host.as_str(), p.ports.as_slice(), p.wait, p.verbose), ("example.com", &[443][..], 3.0, true));
        assert_eq!(Plan::parse(&s(&["-z", "10.0.0.1", "20-25"])).unwrap().ports.len(), 6);
        for bad in [vec!["-v", "example.com", "443"], vec!["-vz", "-u", "h", "53"], vec!["-z", "h"], vec!["-z", "h", "0"], vec!["-z", "h", "25-20"], vec!["-z", "h", "1-65535"], vec!["-l", "-p", "80"]] {
            assert!(Plan::parse(&s(&bad)).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn open_and_closed_ports() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open = listener.local_addr().unwrap().port();
        // 방금 닫은 포트는 거부된다.
        let closed = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let (lines, emit) = collect();
        let code = run(Plan::parse(&s(&["-vz", "127.0.0.1", &open.to_string()])).unwrap(), &emit).await.unwrap();
        let out = lines.lock().unwrap().clone();
        assert_eq!(code, 0);
        assert!(out[0].starts_with(&format!("Connection to 127.0.0.1 {open} port [tcp/")) && out[0].ends_with("succeeded!"), "{out:?}");
        assert_eq!(interpret("nc", &out, crate::engine::StepStatus::Done, Some(0)).light, Light::Green);

        let (lines, emit) = collect();
        let code = run(Plan::parse(&s(&["-vz", "127.0.0.1", &closed.to_string()])).unwrap(), &emit).await.unwrap();
        let out = lines.lock().unwrap().clone();
        assert_eq!((code, out[0].as_str()), (1, format!("nc: connect to 127.0.0.1 port {closed} (tcp) failed: Connection refused").as_str()));
        assert!(interpret("nc", &out, crate::engine::StepStatus::Done, Some(1)).text.contains("거부"));
    }
}
