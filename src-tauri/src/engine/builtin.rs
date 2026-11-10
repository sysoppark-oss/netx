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
        // 1번 포트는 쓰는 프로그램이 없다.
        assert_eq!(portscan(&["127.0.0.1".into(), "1".into()], &|_| {}).await.unwrap(), 1);
        assert!(portscan(&["10.0.0.0/20".into(), "1-50".into()], &|_| {}).await.is_err(), "너무 넓은 범위");
        assert!(portscan(&["127.0.0.1".into()], &|_| {}).await.is_err());
    }
}

// ------------------------------------------------------------------ netmap

/// 장비가 켜져 있는지와 종류를 짐작하는 데 쓰는 포트
const MAP_PORTS: &[u16] = &[22, 23, 53, 80, 139, 443, 445, 554, 3389, 5000, 8080, 9100, 62078];

/// 이웃(ARP) 목록에서 읽은 (IP, MAC)
pub fn parse_neighbors_windows(stdout: &str) -> Vec<(Ipv4Addr, String)> {
    // "192.168.0.1 AA-BB-CC-DD-EE-FF" 형태의 줄
    stdout.lines().filter_map(|l| {
        let mut c = l.split_whitespace();
        let ip: Ipv4Addr = c.next()?.parse().ok()?;
        Some((ip, normalize_mac(c.next()?)?))
    }).collect()
}

pub fn parse_neighbors_linux(stdout: &str) -> Vec<(Ipv4Addr, String)> {
    let v: serde_json::Value = serde_json::from_str(stdout).unwrap_or(serde_json::Value::Null);
    v.as_array().map(|a| a.iter().filter_map(|n| {
        let ip: Ipv4Addr = n.get("dst")?.as_str()?.parse().ok()?;
        Some((ip, normalize_mac(n.get("lladdr")?.as_str()?)?))
    }).collect()).unwrap_or_default()
}

/// MAC을 "aa:bb:cc:dd:ee:ff"로 맞춘다. 빈 주소, 브로드캐스트, 멀티캐스트는 버린다.
pub fn normalize_mac(text: &str) -> Option<String> {
    let hex: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect::<String>().to_lowercase();
    if hex.len() != 12 || text.chars().any(|c| !(c.is_ascii_hexdigit() || c == '-' || c == ':')) {
        return None;
    }
    let first = u8::from_str_radix(&hex[..2], 16).ok()?;
    if hex == "000000000000" || hex == "ffffffffffff" || first & 1 == 1 {
        return None;
    }
    Some(hex.as_bytes().chunks(2).map(|p| std::str::from_utf8(p).unwrap_or("")).collect::<Vec<_>>().join(":"))
}

async fn neighbors() -> Vec<(Ipv4Addr, String)> {
    if cfg!(target_os = "windows") {
        const SCRIPT: &str = "$ErrorActionPreference='SilentlyContinue'; Get-NetNeighbor -AddressFamily IPv4 | Where-Object { $_.State -ne 'Unreachable' -and $_.State -ne 'Incomplete' } | ForEach-Object { \"$($_.IPAddress) $($_.LinkLayerAddress)\" }";
        crate::netinfo::run("powershell.exe", &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", SCRIPT]).await.map(|o| parse_neighbors_windows(&o)).unwrap_or_default()
    } else {
        crate::netinfo::run("ip", &["-j", "-4", "neigh"]).await.map(|o| parse_neighbors_linux(&o)).unwrap_or_default()
    }
}

/// 내 주소와 길이에서 조사할 대역을 정한다. 너무 넓으면 내 주소가 든 /24로 줄인다.
pub fn auto_cidr(ip: Ipv4Addr, prefix: u8) -> String {
    let prefix = if (22..=30).contains(&prefix) { prefix } else { 24 };
    let mask = u32::MAX << (32 - u32::from(prefix));
    format!("{}/{prefix}", Ipv4Addr::from(u32::from(ip) & mask))
}

/// 내부 네트워크 조사. 출력: `NET <대역> GW <게이트웨이|-> ME <내 주소|->` 한 줄과 장비마다 `NODE <ip> <mac|-> <역할> <열린 포트|->`.
pub async fn netmap(argv: &[String], emit: &(dyn Fn(String) + Send + Sync)) -> Result<i32, String> {
    let mut values = argv.iter().filter(|a| *a != "--");
    let target = values.next().ok_or("대역이 없습니다")?;
    if values.next().is_some() {
        return Err("인자가 너무 많습니다".into());
    }
    let info = crate::netinfo::current().await;
    let me: Option<Ipv4Addr> = info.ip.as_deref().and_then(|s| s.parse().ok());
    let gateway: Option<Ipv4Addr> = info.gateway.as_deref().and_then(|s| s.parse().ok());
    let cidr = if target == "auto" {
        let ip = me.ok_or("내 네트워크 주소를 알 수 없습니다. 대역을 직접 적어 주세요.")?;
        auto_cidr(ip, info.prefix.unwrap_or(24))
    } else {
        target.clone()
    };
    let hosts = cidr_hosts(&cidr).ok_or("대역 형식이 아닙니다 (예: 192.168.0.0/24)")?;
    if hosts.is_empty() || hosts.len() > 1022 {
        return Err("대역이 너무 넓습니다. /22 이상으로 좁혀 주세요.".into());
    }
    if !hosts.iter().all(|h| h.is_private() || h.is_loopback() || h.is_link_local()) && target == "auto" {
        return Err("내 네트워크가 사설 대역이 아닙니다. 조사할 대역을 직접 적어 주세요.".into());
    }
    emit(format!("{cidr} 대역의 주소 {}개를 조사합니다. (포트 {}종 연결 시도 + 이웃 목록)", hosts.len(), MAP_PORTS.len()));

    // 연결되거나(열림) 거절당하면(닫힘) 켜져 있는 장비다. 응답이 없으면 알 수 없다.
    let gate = Arc::new(Semaphore::new(CONCURRENCY));
    let mut set: JoinSet<(Ipv4Addr, Vec<u16>, bool)> = JoinSet::new();
    for host in hosts.iter().copied() {
        let gate = gate.clone();
        set.spawn(async move {
            let mut probes: JoinSet<(u16, u8)> = JoinSet::new();
            for port in MAP_PORTS.iter().copied() {
                let gate = gate.clone();
                probes.spawn(async move {
                    let Ok(_permit) = gate.acquire().await else { return (port, 0) };
                    match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(SocketAddr::new(IpAddr::V4(host), port))).await {
                        Ok(Ok(_)) => (port, 2),
                        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => (port, 1),
                        _ => (port, 0),
                    }
                });
            }
            let (mut open, mut alive) = (Vec::new(), false);
            while let Some(r) = probes.join_next().await {
                match r {
                    Ok((p, 2)) => {
                        open.push(p);
                        alive = true;
                    }
                    Ok((_, 1)) => alive = true,
                    _ => {}
                }
            }
            open.sort_unstable();
            (host, open, alive)
        });
    }
    let mut found: Vec<(Ipv4Addr, Vec<u16>)> = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok((host, open, true)) = r {
            found.push((host, open));
        }
    }

    // 연결 시도 뒤에는 같은 망의 장비가 이웃 목록에 올라온다(포트를 다 막은 장비도 포함).
    let macs = neighbors().await;
    for (ip, _) in &macs {
        if hosts.contains(ip) && !found.iter().any(|(h, _)| h == ip) {
            found.push((*ip, vec![]));
        }
    }
    if let Some(ip) = me.filter(|ip| hosts.contains(ip) && !found.iter().any(|(h, _)| h == ip)) {
        found.push((ip, vec![]));
    }
    found.sort_by_key(|(h, _)| u32::from(*h));

    let show = |ip: Option<Ipv4Addr>| ip.map(|i| i.to_string()).unwrap_or_else(|| "-".into());
    emit(format!("NET {cidr} GW {} ME {}", show(gateway), show(me)));
    for (ip, open) in &found {
        let mac = macs.iter().find(|(m, _)| m == ip).map(|(_, m)| m.as_str()).unwrap_or("-");
        let role = if Some(*ip) == me { "me" } else if Some(*ip) == gateway { "gateway" } else { "host" };
        let ports = if open.is_empty() { "-".to_string() } else { open.iter().map(u16::to_string).collect::<Vec<_>>().join(",") };
        emit(format!("NODE {ip} {mac} {role} {ports}"));
    }
    emit(format!("조사 끝: 장비 {}대를 찾았습니다.", found.len()));
    Ok(if found.is_empty() { 1 } else { 0 })
}

#[cfg(test)]
mod netmap_tests {
    use super::*;

    #[test]
    fn neighbor_lists_and_mac_rules() {
        let w = parse_neighbors_windows("192.168.0.1 AA-BB-CC-DD-EE-FF\r\n192.168.0.255 FF-FF-FF-FF-FF-FF\r\n224.0.0.22 01-00-5E-00-00-16\r\n192.168.0.9 00-00-00-00-00-00\r\nbad line\r\n");
        assert_eq!(w, [(Ipv4Addr::new(192, 168, 0, 1), "aa:bb:cc:dd:ee:ff".to_string())]);
        let l = parse_neighbors_linux(r#"[{"dst":"192.0.2.1","dev":"eth0","lladdr":"02:fc:00:00:00:05","state":["REACHABLE"]},{"dst":"192.0.2.9","state":["FAILED"]}]"#);
        assert_eq!(l, [(Ipv4Addr::new(192, 0, 2, 1), "02:fc:00:00:00:05".to_string())]);
        assert_eq!(normalize_mac("zz-bb-cc-dd-ee-ff; rm"), None);
    }

    #[test]
    fn auto_range_stays_small() {
        assert_eq!(auto_cidr(Ipv4Addr::new(192, 168, 123, 103), 24), "192.168.123.0/24");
        assert_eq!(auto_cidr(Ipv4Addr::new(10, 1, 5, 9), 8), "10.1.5.0/24");
        assert_eq!(auto_cidr(Ipv4Addr::new(172, 20, 5, 9), 22), "172.20.4.0/22");
    }

    #[tokio::test]
    async fn maps_a_single_local_address() {
        let lines = std::sync::Mutex::new(Vec::<String>::new());
        let emit = |l: String| lines.lock().unwrap().push(l);
        // 127.0.0.1은 닫힌 포트에 거절로 답하므로 켜져 있는 장비로 잡힌다.
        assert_eq!(netmap(&["127.0.0.1/32".into()], &emit).await.unwrap(), 0);
        let out = lines.lock().unwrap().join("\n");
        assert!(out.contains("NET 127.0.0.1/32 GW "), "{out}");
        assert!(out.contains("NODE 127.0.0.1 "), "{out}");
        assert!(netmap(&["10.0.0.0/16".into()], &|_| {}).await.is_err());
    }
}
