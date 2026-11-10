//! NetX에 내장된 도구. 외부 프로그램을 띄우지 않고 직접 수행한다.
//!
//! portscan: TCP 연결만 시도한다(연결되면 바로 끊는다). 데이터는 보내지 않는다.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

/// 한 번에 시도할 수 있는 (장비 × 포트) 수
pub const MAX_PROBES: usize = 40_000;
pub const MAX_PORTS: usize = 50;
const CONCURRENCY: usize = 256;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);

/// "22,80,443,8000-8010" → 포트 목록(중복 제거, 오름차순)
pub fn parse_ports(text: &str) -> Result<Vec<u16>, String> {
    let mut ports: Vec<u16> = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        let (lo, hi) = part.split_once('-').unwrap_or((part, part));
        let parse = |v: &str| v.trim().parse::<u16>().ok().filter(|p| *p >= 1).ok_or_else(|| format!("포트 번호가 올바르지 않습니다: {part}"));
        let (lo, hi) = (parse(lo)?, parse(hi)?);
        if lo > hi {
            return Err(format!("포트 범위의 시작이 끝보다 큽니다: {part}"));
        }
        if usize::from(hi - lo) >= MAX_PORTS {
            return Err(format!("포트는 한 번에 {MAX_PORTS}개까지 고를 수 있습니다"));
        }
        ports.extend(lo..=hi);
    }
    ports.sort_unstable();
    ports.dedup();
    if ports.is_empty() || ports.len() > MAX_PORTS {
        return Err(format!("포트는 1개 이상 {MAX_PORTS}개 이하로 고르세요"));
    }
    Ok(ports)
}

/// "192.168.0.0/24" → 대역 안의 장비 주소(네트워크·브로드캐스트 주소 제외). 대역이 아니면 None.
pub fn cidr_hosts(text: &str) -> Option<Vec<Ipv4Addr>> {
    let (addr, prefix) = text.split_once('/')?;
    let addr: Ipv4Addr = addr.parse().ok()?;
    let prefix: u32 = prefix.parse().ok().filter(|p| *p <= 32)?;
    let mask = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
    let net = u32::from(addr) & mask;
    let last = net | !mask;
    let range = if prefix >= 31 { net..=last } else { net + 1..=last - 1 };
    Some(range.map(Ipv4Addr::from).collect())
}

pub fn service_name(port: u16) -> &'static str {
    match port {
        20 | 21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 | 587 => "SMTP",
        53 => "DNS",
        80 | 8000 | 8080 | 8888 => "HTTP",
        110 => "POP3",
        135 => "RPC",
        139 | 445 => "SMB",
        143 => "IMAP",
        161 => "SNMP",
        179 => "BGP",
        389 => "LDAP",
        443 | 8443 => "HTTPS",
        515 | 631 | 9100 => "프린터",
        554 => "RTSP",
        1433 => "MSSQL",
        1521 => "Oracle",
        3306 => "MySQL",
        3389 => "RDP",
        5432 => "PostgreSQL",
        5900 => "VNC",
        6379 => "Redis",
        _ => "",
    }
}

/// 포트 스캔을 수행한다. `emit`으로 한 줄씩 내보낸다. 성공하면 종료 코드(열린 포트가 있으면 0, 없으면 1).
pub async fn portscan(argv: &[String], emit: &(dyn Fn(String) + Send + Sync)) -> Result<i32, String> {
    let mut values = argv.iter().filter(|a| *a != "--");
    let target = values.next().ok_or("대상이 없습니다")?;
    let ports = parse_ports(values.next().ok_or("포트 목록이 없습니다")?)?;
    if values.next().is_some() {
        return Err("인자가 너무 많습니다".into());
    }

    let hosts: Vec<IpAddr> = match cidr_hosts(target) {
        Some(list) => list.into_iter().map(IpAddr::V4).collect(),
        None if target.contains('/') => return Err("대역 형식이 아닙니다 (예: 192.168.0.0/24)".into()),
        None => match target.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => {
                let found = tokio::net::lookup_host((target.as_str(), 0)).await.map_err(|e| format!("이름을 주소로 바꾸지 못했습니다: {e}"))?;
                found.map(|a| a.ip()).take(1).collect()
            }
        },
    };
    if hosts.is_empty() {
        return Err("스캔할 장비가 없습니다".into());
    }
    if hosts.len() * ports.len() > MAX_PROBES {
        return Err(format!("범위가 너무 넓습니다(장비 {}대 × 포트 {}개). 대역이나 포트를 줄이세요.", hosts.len(), ports.len()));
    }

    emit(format!("장비 {}대 × 포트 {}개에 TCP 연결을 시도합니다.", hosts.len(), ports.len()));
    let gate = Arc::new(Semaphore::new(CONCURRENCY));
    let mut set: JoinSet<(IpAddr, Vec<u16>)> = JoinSet::new();
    for host in hosts.iter().copied() {
        let ports = ports.clone();
        let gate = gate.clone();
        set.spawn(async move {
            let mut probes: JoinSet<Option<u16>> = JoinSet::new();
            for port in ports {
                let gate = gate.clone();
                probes.spawn(async move {
                    let _permit = gate.acquire().await.ok()?;
                    let done = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(SocketAddr::new(host, port))).await;
                    matches!(done, Ok(Ok(_))).then_some(port)
                });
            }
            let mut open = Vec::new();
            while let Some(r) = probes.join_next().await {
                if let Ok(Some(p)) = r {
                    open.push(p);
                }
            }
            open.sort_unstable();
            (host, open)
        });
    }

    let mut found = 0usize;
    while let Some(r) = set.join_next().await {
        let Ok((host, open)) = r else { continue };
        if open.is_empty() {
            continue;
        }
        found += 1;
        let list = open.iter().map(|p| match service_name(*p) { "" => p.to_string(), name => format!("{p}({name})") }).collect::<Vec<_>>().join(" ");
        emit(format!("{host}  open  {list}"));
    }
    emit(format!("스캔 끝: 열린 포트가 있는 장비 {found}대 / 전체 {}대", hosts.len()));
    Ok(if found > 0 { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_port_lists() {
        assert_eq!(parse_ports("22,80,443").unwrap(), [22, 80, 443]);
        assert_eq!(parse_ports("443, 20-22,22").unwrap(), [20, 21, 22, 443]);
        for bad in ["", "0", "70000", "22,,80", "30-20", "1-100", "22;id", "a", "1-51"] {
            assert!(parse_ports(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_ports("1-50").unwrap().len(), 50);
    }

    #[test]
    fn expands_cidr() {
        let h = cidr_hosts("192.168.0.0/24").unwrap();
        assert_eq!((h.len(), h[0].to_string(), h[253].to_string()), (254, "192.168.0.1".into(), "192.168.0.254".into()));
        assert_eq!(cidr_hosts("10.0.0.5/32").unwrap(), [Ipv4Addr::new(10, 0, 0, 5)]);
        assert_eq!(cidr_hosts("10.0.0.4/30").unwrap().len(), 2);
        assert_eq!(cidr_hosts("10.0.0.77/24").unwrap()[0].to_string(), "10.0.0.1");
        assert!(cidr_hosts("10.0.0.1").is_none());
        assert!(cidr_hosts("10.0.0.0/40").is_none());
    }

    #[tokio::test]
    async fn finds_an_open_local_port_and_reports_closed() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let emit = |l: String| lines.lock().unwrap().push(l);
        let code = portscan(&["--".into(), "127.0.0.1".into(), port.to_string()], &emit).await.unwrap();
        assert_eq!(code, 0);
        assert!(lines.lock().unwrap().iter().any(|l| l.starts_with("127.0.0.1  open  ") && l.contains(&port.to_string())));
        drop(listener);
        assert_eq!(portscan(&["127.0.0.1".into(), port.to_string()], &|_| {}).await.unwrap(), 1);
        assert!(portscan(&["10.0.0.0/20".into(), "1-50".into()], &|_| {}).await.is_err(), "너무 넓은 범위");
        assert!(portscan(&["127.0.0.1".into()], &|_| {}).await.is_err());
    }
}
