//! 경로 추적 (기본 진단의 mtr). TTL을 1부터 늘려 가며 핑을 보내 가는 길의 라우터를 찾고,
//! 여러 번 되풀이해 홉마다 손실과 지연을 낸다. 출력은 `mtr -r`과 같은 모양이다.

use std::net::Ipv4Addr;

use super::args::Args;
use super::icmp::{Echo, Prober, probe};
use super::{Emit, resolve_v4};

const MAX_HOPS: u8 = 30;
const WAIT_MS: u32 = 1000;

#[derive(Debug)]
pub struct Plan {
    host: String,
    cycles: u32,
    max_hops: u8,
}

impl Plan {
    pub fn parse(argv: &[String]) -> Result<Plan, String> {
        // -r(보고서), -w(넓게), -b(이름과 주소), -n(주소만)은 받아 두기만 한다. 기본 진단은 항상 주소만 보고서로 낸다.
        let a = Args::parse(argv, &["-c", "-m"], &["-r", "-w", "-b", "-n", "-4"])?;
        Ok(Plan { host: a.one("대상")?.to_string(), cycles: a.number("-c", 10.0, 1.0, 100.0)? as u32, max_hops: a.number("-m", f64::from(MAX_HOPS), 1.0, f64::from(MAX_HOPS))? as u8 })
    }
}

#[derive(Default, Clone)]
struct Hop {
    host: Option<Ipv4Addr>,
    sent: u32,
    times: Vec<f64>,
}

impl Hop {
    fn line(&self, n: usize, width: usize) -> String {
        let host = self.host.map(|h| h.to_string()).unwrap_or_else(|| "???".into());
        let lost = self.sent - self.times.len() as u32;
        let loss = if self.sent == 0 { 0.0 } else { f64::from(lost) * 100.0 / f64::from(self.sent) };
        let (last, avg, best, worst, stdev) = if self.times.is_empty() {
            (0.0, 0.0, 0.0, 0.0, 0.0)
        } else {
            let n = self.times.len() as f64;
            let avg = self.times.iter().sum::<f64>() / n;
            let var = self.times.iter().map(|t| (t - avg).powi(2)).sum::<f64>() / n;
            (*self.times.last().unwrap_or(&0.0), avg, self.times.iter().copied().fold(f64::INFINITY, f64::min), self.times.iter().copied().fold(0.0, f64::max), var.sqrt())
        };
        format!("{n:>3}.|-- {host:<width$} {loss:>5.1}% {:>5} {last:>6.1} {avg:>5.1} {best:>5.1} {worst:>5.1} {stdev:>5.1}", self.sent)
    }
}

pub async fn run(plan: Plan, prober: Prober, emit: Emit<'_>) -> Result<i32, String> {
    let ip = resolve_v4(&plan.host).await?;
    let mut hops: Vec<Hop> = vec![Hop::default(); usize::from(plan.max_hops)];
    // 목적지가 답한 가장 가까운 홉. 알게 되면 그 뒤로는 거기까지만 보낸다.
    let mut reach: Option<usize> = None;
    for cycle in 0..plan.cycles {
        let upto = reach.map(|r| r + 1).unwrap_or(hops.len());
        let probes = (0..upto).map(|i| {
            let prober = prober.clone();
            async move { probe(&prober, ip, i as u8 + 1, WAIT_MS, 32).await }
        });
        let started = tokio::time::Instant::now();
        let results = join_all(probes).await;
        for (i, result) in results.into_iter().enumerate() {
            if reach.is_some_and(|r| i > r) {
                break;
            }
            hops[i].sent += 1;
            match result {
                Echo::Reply { from, ms, .. } => {
                    hops[i].host = Some(from);
                    hops[i].times.push(ms);
                    if reach.is_none_or(|r| i < r) {
                        reach = Some(i);
                    }
                }
                Echo::Expired { from, ms } => {
                    hops[i].host = Some(from);
                    hops[i].times.push(ms);
                }
                Echo::Unreachable(from) => hops[i].host = hops[i].host.or(from),
                Echo::Timeout => {}
                Echo::Failed(e) => return Err(format!("mtr: {e}")),
            }
        }
        if let Some(r) = reach {
            // 목적지 너머로 보낸 것은 버린다.
            for hop in hops.iter_mut().skip(r + 1) {
                *hop = Hop::default();
            }
        }
        if cycle + 1 < plan.cycles {
            tokio::time::sleep_until(started + std::time::Duration::from_secs(1)).await;
        }
    }

    // 목적지에 닿지 못했으면, 마지막으로 답한 홉 다음 한 줄(???)까지만 보인다.
    let shown = match reach {
        Some(r) => r + 1,
        None => (hops.iter().rposition(|h| h.host.is_some()).map(|i| i + 2).unwrap_or(1)).min(hops.len()),
    };
    let width = hops[..shown].iter().map(|h| h.host.map(|a| a.to_string().len()).unwrap_or(3)).max().unwrap_or(3).max(plan.host.len().min(40));
    emit(format!("HOST: {:<w$} Loss%   Snt   Last   Avg  Best  Wrst StDev", plan.host, w = width + 2));
    for (i, hop) in hops[..shown].iter().enumerate() {
        emit(hop.line(i + 1, width));
    }
    Ok(0)
}

/// 여러 일을 함께 돌리고 순서대로 결과를 모은다.
pub async fn join_all<F: std::future::Future>(futures: impl IntoIterator<Item = F>) -> Vec<F::Output>
where
    F: Send + 'static,
    F::Output: Send + 'static,
{
    let handles: Vec<_> = futures.into_iter().map(tauri::async_runtime::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        if let Ok(v) = h.await {
            out.push(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::ping::tests::{collect, s};
    use super::*;
    use std::sync::Arc;

    /// 세 홉짜리 길: 1번과 3번(목적지)은 답하고 2번 라우터는 답하지 않는다.
    fn path(dest_alive: bool) -> Prober {
        Arc::new(move |ip, ttl, _, _| match ttl {
            1 => Echo::Expired { from: Ipv4Addr::new(192, 168, 0, 1), ms: 1.5 },
            2 => Echo::Timeout,
            _ if dest_alive => Echo::Reply { from: ip, ms: 30.0 + f64::from(ttl), ttl: 55 },
            _ => Echo::Timeout,
        })
    }

    #[test]
    fn options() {
        let p = Plan::parse(&s(&["-r", "-w", "-b", "-c", "10", "8.8.8.8"])).unwrap();
        assert_eq!((p.cycles, p.max_hops), (10, 30));
        assert!(Plan::parse(&s(&["-u", "8.8.8.8"])).is_err());
        assert!(Plan::parse(&s(&["-T", "-P", "443", "8.8.8.8"])).is_err());
    }

    #[tokio::test]
    async fn report_is_read_by_the_interpreter() {
        let (lines, emit) = collect();
        run(Plan { host: "192.0.2.9".into(), cycles: 2, max_hops: 30 }, path(true), &emit).await.unwrap();
        let lines = lines.lock().unwrap().clone();
        assert_eq!(lines.len(), 4, "{lines:?}");
        assert!(lines[0].starts_with("HOST: 192.0.2.9"));
        assert_eq!(lines[1].split_whitespace().collect::<Vec<_>>(), ["1.|--", "192.168.0.1", "0.0%", "2", "1.5", "1.5", "1.5", "1.5", "0.0"]);
        assert_eq!(lines[2].split_whitespace().collect::<Vec<_>>()[..4], ["2.|--", "???", "100.0%", "2"]);
        assert_eq!(lines[3].split_whitespace().collect::<Vec<_>>()[..4], ["3.|--", "192.0.2.9", "0.0%", "2"]);
        let r = crate::interpret::interpret("mtr", &lines, crate::engine::StepStatus::Done, Some(0));
        assert_eq!(r.light, crate::interpret::Light::Green, "{}", r.text);
        assert!(r.text.contains("2번 홉"), "{}", r.text);
    }

    #[tokio::test]
    async fn unreachable_destination() {
        let (lines, emit) = collect();
        run(Plan { host: "192.0.2.9".into(), cycles: 1, max_hops: 30 }, path(false), &emit).await.unwrap();
        let lines = lines.lock().unwrap().clone();
        // 1번 홉과 그다음 한 줄(???)만
        assert_eq!(lines.len(), 3, "{lines:?}");
        let r = crate::interpret::interpret("mtr", &lines, crate::engine::StepStatus::Done, Some(0));
        assert_eq!(r.light, crate::interpret::Light::Red, "{}", r.text);
    }
}
