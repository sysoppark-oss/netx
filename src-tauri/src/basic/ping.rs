//! ping (기본 진단). 출력은 리눅스 ping과 같은 모양이다.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use super::args::Args;
use super::icmp::{Echo, Prober, probe};
use super::{Emit, resolve_v4, unsupported};

#[derive(Debug)]
pub struct Plan {
    host: String,
    count: u32,
    interval: f64,
    wait_ms: u32,
    deadline: Option<f64>,
    size: u16,
    ttl: u8,
    quiet: bool,
    show_missing: bool,
}

impl Plan {
    pub fn parse(argv: &[String]) -> Result<Plan, String> {
        let a = Args::parse(argv, &["-c", "-i", "-W", "-w", "-s", "-t"], &["-4", "-n", "-q", "-O"])?;
        Ok(Plan {
            host: a.one("대상")?.to_string(),
            // 리눅스 ping은 횟수를 안 적으면 계속 보낸다. 기본 진단은 5번으로 끝낸다.
            count: a.number("-c", 5.0, 1.0, 1000.0)? as u32,
            interval: a.number("-i", 1.0, 0.2, 60.0)?,
            wait_ms: (a.number("-W", 1.0, 0.1, 60.0)? * 1000.0) as u32,
            deadline: a.value("-w").map(|_| a.number("-w", 0.0, 1.0, 3600.0)).transpose()?,
            size: a.number("-s", 56.0, 0.0, 1400.0).map_err(|_| unsupported("1400바이트를 넘는 패킷 크기"))? as u16,
            ttl: a.number("-t", 64.0, 1.0, 255.0)? as u8,
            quiet: a.has("-q"),
            show_missing: a.has("-O"),
        })
    }
}

/// 응답 시간 표기: 리눅스 ping처럼 작을수록 자릿수를 늘린다.
pub fn time_text(ms: f64) -> String {
    if ms < 1.0 {
        format!("{ms:.3}")
    } else if ms < 10.0 {
        format!("{ms:.2}")
    } else if ms < 100.0 {
        format!("{ms:.1}")
    } else {
        format!("{ms:.0}")
    }
}

/// 통계 두 줄
pub fn summary(host: &str, sent: u32, times: &[f64], elapsed_ms: u128) -> Vec<String> {
    let received = times.len() as u32;
    let loss = if sent == 0 { 0.0 } else { f64::from(sent - received) * 100.0 / f64::from(sent) };
    let mut lines = vec![String::new(), format!("--- {host} ping statistics ---"), format!("{sent} packets transmitted, {received} received, {}% packet loss, time {elapsed_ms}ms", time_loss(loss))];
    if !times.is_empty() {
        let n = times.len() as f64;
        let avg = times.iter().sum::<f64>() / n;
        let min = times.iter().copied().fold(f64::INFINITY, f64::min);
        let max = times.iter().copied().fold(0.0, f64::max);
        let mdev = (times.iter().map(|t| (t - avg).powi(2)).sum::<f64>() / n).sqrt();
        lines.push(format!("rtt min/avg/max/mdev = {min:.3}/{avg:.3}/{max:.3}/{mdev:.3} ms"));
    }
    lines
}

fn time_loss(loss: f64) -> String {
    if loss.fract() == 0.0 { format!("{loss:.0}") } else { format!("{loss:.4}").trim_end_matches('0').to_string() }
}

pub async fn run(plan: Plan, prober: Prober, emit: Emit<'_>) -> Result<i32, String> {
    let ip: Ipv4Addr = resolve_v4(&plan.host).await?;
    emit(format!("PING {} ({ip}) {}({}) bytes of data.", plan.host, plan.size, u32::from(plan.size) + 28));
    let started = Instant::now();
    let mut times = Vec::new();
    let mut sent = 0;
    for seq in 1..=plan.count {
        if plan.deadline.is_some_and(|d| started.elapsed().as_secs_f64() >= d) {
            break;
        }
        let at = Instant::now();
        sent += 1;
        match probe(&prober, ip, plan.ttl, plan.wait_ms, plan.size).await {
            Echo::Reply { from, ms, ttl } => {
                times.push(ms);
                if !plan.quiet {
                    emit(format!("{} bytes from {from}: icmp_seq={seq} ttl={ttl} time={} ms", u32::from(plan.size) + 8, time_text(ms)));
                }
            }
            Echo::Expired { from, .. } if !plan.quiet => emit(format!("From {from} icmp_seq={seq} Time to live exceeded")),
            Echo::Unreachable(from) if !plan.quiet => emit(format!("From {} icmp_seq={seq} Destination Host Unreachable", from.map(|f| f.to_string()).unwrap_or_else(|| ip.to_string()))),
            Echo::Failed(e) => return Err(format!("ping: {e}")),
            Echo::Timeout if plan.show_missing && !plan.quiet => emit(format!("no answer yet for icmp_seq={seq}")),
            _ => {}
        }
        if seq < plan.count {
            let left = Duration::from_secs_f64(plan.interval).saturating_sub(at.elapsed());
            tokio::time::sleep(left).await;
        }
    }
    for line in summary(&plan.host, sent, &times, started.elapsed().as_millis()) {
        emit(line);
    }
    Ok(if times.is_empty() { 1 } else { 0 })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    /// 출력 줄을 모으는 곳
    pub fn collect() -> (Arc<Mutex<Vec<String>>>, impl Fn(String) + Send + Sync) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        (lines, move |l| sink.lock().unwrap().push(l))
    }

    #[test]
    fn options() {
        let p = Plan::parse(&s(&["-c", "3", "-i", "0.5", "-W", "2", "-s", "100", "-t", "5", "-n", "-q", "--", "8.8.8.8"])).unwrap();
        assert_eq!((p.count, p.interval, p.wait_ms, p.size, p.ttl, p.quiet), (3, 0.5, 2000, 100, 5, true));
        assert_eq!(Plan::parse(&s(&["8.8.8.8"])).unwrap().count, 5);
        for bad in [vec!["-f", "8.8.8.8"], vec!["-I", "eth0", "8.8.8.8"], vec!["-6", "::1"], vec!["-c", "0", "8.8.8.8"], vec!["-s", "9000", "8.8.8.8"], vec![], vec!["a", "b"]] {
            assert!(Plan::parse(&s(&bad)).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn output_is_read_by_the_interpreter() {
        // 두 번째 요청만 답이 없다.
        let n = Arc::new(Mutex::new(0));
        let prober: Prober = Arc::new(move |ip, _, _, _| {
            let mut n = n.lock().unwrap();
            *n += 1;
            if *n == 2 { Echo::Timeout } else { Echo::Reply { from: ip, ms: 10.0 * f64::from(*n), ttl: 57 } }
        });
        let (lines, emit) = collect();
        let code = run(Plan::parse(&s(&["-c", "4", "-i", "0.2", "192.0.2.7"])).unwrap(), prober, &emit).await.unwrap();
        let lines = lines.lock().unwrap().clone();
        assert_eq!(code, 0);
        assert_eq!(lines[0], "PING 192.0.2.7 (192.0.2.7) 56(84) bytes of data.");
        assert_eq!(lines[1], "64 bytes from 192.0.2.7: icmp_seq=1 ttl=57 time=10.0 ms");
        assert!(lines.iter().any(|l| l.starts_with("4 packets transmitted, 3 received, 25% packet loss")), "{lines:?}");
        assert!(lines.last().unwrap().starts_with("rtt min/avg/max/mdev = 10.000/26.667/40.000/"));
        let r = crate::interpret::interpret("ping", &lines, crate::engine::StepStatus::Done, Some(code));
        assert!(r.text.contains("25"), "{}", r.text);

        // 모두 답이 없으면 종료 코드 1
        let (lines, emit) = collect();
        let code = run(Plan::parse(&s(&["-c", "2", "-i", "0.2", "192.0.2.7"])).unwrap(), Arc::new(|_, _, _, _| Echo::Timeout), &emit).await.unwrap();
        assert_eq!(code, 1);
        assert!(lines.lock().unwrap().iter().any(|l| l.contains("100% packet loss")));
    }

    #[test]
    fn time_format() {
        assert_eq!((time_text(0.5123), time_text(3.456), time_text(31.24), time_text(250.4)), ("0.512".into(), "3.46".into(), "31.2".into(), "250".into()));
    }
}
