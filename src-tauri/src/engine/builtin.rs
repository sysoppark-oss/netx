//! NetX에 내장된 도구. 외부 프로그램을 띄우지 않고 직접 수행한다.
//!
//! portscan: TCP 연결만 시도한다(연결되면 바로 끊는다). 데이터는 보내지 않는다.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpStream, UdpSocket};
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

// ---- 장비 이름 조회 (NetBIOS 이름, 역방향 DNS). 외부 프로그램 없이 UDP로 직접 묻는다. ----

const NAME_TIMEOUT: Duration = Duration::from_secs(1);

/// 이름은 출력 줄에 섞여 들어가므로 영문·숫자·`.`·`-`·`_`만 남기고 63자로 자른다. 남는 것이 없으면 None.
pub fn clean_name(raw: &str) -> Option<String> {
    let name: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).take(63).collect();
    let name = name.trim_matches('.').to_string();
    (!name.is_empty()).then_some(name)
}

/// NetBIOS node status 요청(50바이트 고정). 이름 칸은 "*"를 반 바이트씩 풀어 쓴 32바이트다.
pub fn nbstat_query(id: u16) -> [u8; 50] {
    let mut p = [0u8; 50];
    p[..2].copy_from_slice(&id.to_be_bytes());
    p[5] = 1; // 질문 1개
    p[12] = 32;
    p[13] = b'C';
    p[14] = b'K';
    p[15..45].fill(b'A');
    p[47] = 0x21; // NBSTAT
    p[49] = 0x01; // IN
    p
}

/// DNS 형식의 이름 하나를 건너뛴 다음 위치.
fn skip_name(p: &[u8], mut at: usize) -> Option<usize> {
    loop {
        let len = *p.get(at)?;
        if len & 0xc0 == 0xc0 {
            p.get(at + 1)?;
            return Some(at + 2);
        }
        if len == 0 {
            return Some(at + 1);
        }
        at += 1 + usize::from(len);
    }
}

/// node status 응답에서 장비 이름을 읽는다. 컴퓨터 이름(종류 0x00, 그룹 아님)을 먼저 찾고 없으면 첫 이름.
pub fn nbstat_name(p: &[u8], id: u16) -> Option<String> {
    if p.len() < 12 || p[..2] != id.to_be_bytes() || p[2] & 0x80 == 0 || u16::from_be_bytes([p[6], p[7]]) == 0 {
        return None;
    }
    let at = skip_name(p, 12)?;
    if p.get(at..at + 2)? != [0x00, 0x21] {
        return None;
    }
    let at = at + 10; // 종류 2 + 분류 2 + 수명 4 + 길이 2
    let count = usize::from(*p.get(at)?);
    let entries: Vec<&[u8]> = (0..count).map_while(|i| p.get(at + 1 + i * 18..at + 1 + (i + 1) * 18)).collect();
    let text = |e: &[u8]| clean_name(String::from_utf8_lossy(&e[..15]).trim_end_matches([' ', '\0']));
    entries.iter().filter(|e| e[15] == 0x00 && e[16] & 0x80 == 0).find_map(|e| text(e)).or_else(|| entries.first().and_then(|e| text(e)))
}

/// 역방향 DNS(PTR) 질의: `d.c.b.a.in-addr.arpa`
pub fn ptr_query(id: u16, ip: Ipv4Addr) -> Vec<u8> {
    let mut p = Vec::with_capacity(48);
    p.extend_from_slice(&id.to_be_bytes());
    p.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // 재귀 요청, 질문 1개
    let o = ip.octets();
    for label in [o[3].to_string(), o[2].to_string(), o[1].to_string(), o[0].to_string(), "in-addr".into(), "arpa".into()] {
        p.push(label.len() as u8);
        p.extend_from_slice(label.as_bytes());
    }
    p.extend_from_slice(&[0, 0, 12, 0, 1]); // 이름 끝, PTR, IN
    p
}

/// 압축된 DNS 이름을 풀어 읽는다. 되돌아가기는 16번까지만 따라간다.
fn read_name(p: &[u8], mut at: usize) -> Option<String> {
    let (mut out, mut jumps) = (String::new(), 0);
    loop {
        let len = usize::from(*p.get(at)?);
        if len & 0xc0 == 0xc0 {
            jumps += 1;
            if jumps > 16 {
                return None;
            }
            at = ((len & 0x3f) << 8) | usize::from(*p.get(at + 1)?);
        } else if len == 0 {
            return Some(out);
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(&String::from_utf8_lossy(p.get(at + 1..at + 1 + len)?));
            if out.len() > 255 {
                return None;
            }
            at += 1 + len;
        }
    }
}

/// PTR 응답에서 이름을 읽는다. 오류 응답이거나 PTR 답이 없으면 None.
pub fn ptr_name(p: &[u8], id: u16) -> Option<String> {
    if p.len() < 12 || p[..2] != id.to_be_bytes() || p[2] & 0x80 == 0 || p[3] & 0x0f != 0 {
        return None;
    }
    let (questions, answers) = (u16::from_be_bytes([p[4], p[5]]), u16::from_be_bytes([p[6], p[7]]));
    let mut at = 12;
    for _ in 0..questions {
        at = skip_name(p, at)? + 4;
    }
    for _ in 0..answers {
        at = skip_name(p, at)?;
        let head = p.get(at..at + 10)?;
        let len = usize::from(u16::from_be_bytes([head[8], head[9]]));
        if head[..2] == [0, 12] {
            p.get(at + 10..at + 10 + len)?;
            return clean_name(&read_name(p, at + 10)?);
        }
        at += 10 + len;
    }
    None
}

/// 한 곳에 UDP로 묻고 그곳에서 온 답 하나를 제한 시간 안에 받는다.
async fn ask_udp(to: SocketAddr, packet: &[u8]) -> Option<Vec<u8>> {
    let sock = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await.ok()?;
    sock.connect(to).await.ok()?;
    sock.send(packet).await.ok()?;
    let mut buf = [0u8; 1500];
    let n = tokio::time::timeout(NAME_TIMEOUT, sock.recv(&mut buf)).await.ok()?.ok()?;
    Some(buf[..n].to_vec())
}

fn query_id(ip: Ipv4Addr, salt: u32) -> u16 {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let v = nanos ^ u32::from(ip).rotate_left(7) ^ salt;
    (v ^ (v >> 16)) as u16
}

/// 이 PC의 이름. 네트워크로 묻지 않고 운영체제가 알려 주는 값을 읽는다(Windows는 COMPUTERNAME).
pub fn local_name() -> Option<String> {
    let raw = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).or_else(|_| std::fs::read_to_string("/etc/hostname")).ok()?;
    clean_name(raw.trim())
}

/// 장비들의 이름을 한꺼번에 묻는다. NetBIOS 이름이 있으면 그것을, 없으면 역방향 DNS 이름을 쓴다.
async fn lookup_names(ips: &[Ipv4Addr], dns: Option<Ipv4Addr>) -> Vec<(Ipv4Addr, String)> {
    let mut set: JoinSet<Option<(Ipv4Addr, String)>> = JoinSet::new();
    for ip in ips.iter().copied() {
        set.spawn(async move {
            let netbios = async {
                let id = query_id(ip, 0x4e42);
                nbstat_name(&ask_udp(SocketAddr::new(IpAddr::V4(ip), 137), &nbstat_query(id)).await?, id)
            };
            let reverse = async {
                let id = query_id(ip, 0x5054);
                ptr_name(&ask_udp(SocketAddr::new(IpAddr::V4(dns?), 53), &ptr_query(id, ip)).await?, id)
            };
            let (a, b) = tokio::join!(netbios, reverse);
            a.or(b).map(|name| (ip, name))
        });
    }
    let mut names = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(found)) = r {
            names.push(found);
        }
    }
    names
}

/// 내부 네트워크 조사. 출력: `NET <대역> GW <게이트웨이|-> ME <내 주소|->` 한 줄과 장비마다 `NODE <ip> <mac|-> <역할> <열린 포트|-> <이름|->`.
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

    // 장비 이름(NetBIOS, 역방향 DNS)을 한꺼번에 묻는다. 장비마다 1초까지만 기다린다.
    let dns: Option<Ipv4Addr> = info.dns.first().and_then(|s| s.parse().ok());
    let ips: Vec<Ipv4Addr> = found.iter().map(|(h, _)| *h).collect();
    let mut names = lookup_names(&ips, dns).await;
    // 내 PC는 스스로에게 답하지 않는 경우가 많으므로, 이름을 못 얻었으면 운영체제가 아는 이름을 쓴다.
    if let Some(ip) = me.filter(|ip| ips.contains(ip) && !names.iter().any(|(n, _)| n == ip)) {
        names.extend(local_name().map(|name| (ip, name)));
    }

    let show = |ip: Option<Ipv4Addr>| ip.map(|i| i.to_string()).unwrap_or_else(|| "-".into());
    emit(format!("NET {cidr} GW {} ME {}", show(gateway), show(me)));
    for (ip, open) in &found {
        let mac = macs.iter().find(|(m, _)| m == ip).map(|(_, m)| m.as_str()).unwrap_or("-");
        let role = if Some(*ip) == me { "me" } else if Some(*ip) == gateway { "gateway" } else { "host" };
        let ports = if open.is_empty() { "-".to_string() } else { open.iter().map(u16::to_string).collect::<Vec<_>>().join(",") };
        let name = names.iter().find(|(n, _)| n == ip).map(|(_, n)| n.as_str()).unwrap_or("-");
        emit(format!("NODE {ip} {mac} {role} {ports} {name}"));
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

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("DESKTOP-ABC  ").as_deref(), Some("DESKTOP-ABC"));
        assert_eq!(clean_name("pc 1;rm -rf\n<b>").as_deref(), Some("pc1rm-rfb"));
        assert_eq!(clean_name("printer.lan.").as_deref(), Some("printer.lan"));
        assert_eq!(clean_name(&"a".repeat(100)).map(|n| n.len()), Some(63));
        assert_eq!(clean_name(" \u{d55c}\u{ae00} "), None);
    }

    /// 이름 표 항목: 이름 15바이트(공백 채움) + 종류 + 깃발 2바이트
    fn nb_entry(name: &str, suffix: u8, group: bool) -> Vec<u8> {
        let mut e = format!("{name:<15}").into_bytes();
        e.extend_from_slice(&[suffix, if group { 0x84 } else { 0x04 }, 0x00]);
        e
    }

    fn nb_reply(id: u16, entries: &[Vec<u8>]) -> Vec<u8> {
        let mut p = id.to_be_bytes().to_vec();
        p.extend_from_slice(&[0x84, 0x00, 0, 0, 0, 1, 0, 0, 0, 0]);
        p.extend_from_slice(&nbstat_query(0)[12..46]); // 이름 칸
        p.extend_from_slice(&[0x00, 0x21, 0x00, 0x01, 0, 0, 0, 0, 0, 0]);
        p.push(entries.len() as u8);
        entries.iter().for_each(|e| p.extend_from_slice(e));
        p.extend_from_slice(&[0u8; 6]); // 장비 MAC
        p
    }

    #[test]
    fn netbios_query_and_reply() {
        let q = nbstat_query(0x1234);
        assert_eq!(q.len(), 50);
        assert_eq!(&q[..12], &[0x12, 0x34, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[12..46], b"\x20CKAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\x00");
        assert_eq!(&q[46..], &[0x00, 0x21, 0x00, 0x01]);

        // 그룹 이름이 먼저 와도 컴퓨터 이름을 고른다.
        let reply = nb_reply(0x1234, &[nb_entry("WORKGROUP", 0x00, true), nb_entry("DESKTOP-ABC", 0x00, false), nb_entry("DESKTOP-ABC", 0x20, false)]);
        assert_eq!(nbstat_name(&reply, 0x1234).as_deref(), Some("DESKTOP-ABC"));
        // 컴퓨터 이름이 없으면 첫 이름
        assert_eq!(nbstat_name(&nb_reply(7, &[nb_entry("BRW1234", 0x20, false)]), 7).as_deref(), Some("BRW1234"));
        // 번호가 다르거나, 요청 그대로이거나, 잘린 답은 버린다.
        assert_eq!(nbstat_name(&reply, 0x9999), None);
        assert_eq!(nbstat_name(&q, 0x1234), None);
        assert_eq!(nbstat_name(&reply[..60], 0x1234), None);
        assert_eq!(nbstat_name(&[], 0), None);
        // 이름 개수를 부풀린 답: 실제로 들어 있는 것까지만 읽는다.
        let mut lying = nb_reply(1, &[nb_entry("NAS", 0x00, false)]);
        lying[56] = 200;
        assert_eq!(nbstat_name(&lying, 1).as_deref(), Some("NAS"));
    }

    #[test]
    fn reverse_dns_query_and_reply() {
        let q = ptr_query(0xbeef, Ipv4Addr::new(192, 168, 123, 21));
        assert_eq!(&q[..12], &[0xbe, 0xef, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&q[12..], b"\x0221\x03123\x03168\x03192\x07in-addr\x04arpa\x00\x00\x0c\x00\x01");

        // 답: 질문 그대로 + (질문 이름을 가리키는 포인터, PTR, IN, 수명, "printer" + "lan")
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 1;
        r.extend_from_slice(&[0xc0, 0x0c, 0, 12, 0, 1, 0, 0, 0, 60, 0, 13]);
        r.extend_from_slice(b"\x07printer\x03lan\x00");
        assert_eq!(ptr_name(&r, 0xbeef).as_deref(), Some("printer.lan"));
        assert_eq!(ptr_name(&r, 1), None);
        assert_eq!(ptr_name(&q, 0xbeef), None, "요청은 답이 아니다");
        assert_eq!(ptr_name(&r[..r.len() - 3], 0xbeef), None, "잘린 답");

        // 이름 없음(NXDOMAIN)
        let mut nx = q.clone();
        nx[2] = 0x81;
        nx[3] = 0x83;
        assert_eq!(ptr_name(&nx, 0xbeef), None);

        // 자기 자신을 가리키는 포인터(끝없는 되돌아가기)는 멈춘다.
        let mut looped = q.clone();
        looped[2] = 0x81;
        looped[7] = 1;
        let at = looped.len() + 12;
        looped.extend_from_slice(&[0xc0, 0x0c, 0, 12, 0, 1, 0, 0, 0, 60, 0, 2, 0xc0 | (at >> 8) as u8, at as u8]);
        assert_eq!(ptr_name(&looped, 0xbeef), None);
    }

    #[tokio::test]
    async fn name_lookup_talks_to_a_dns_server() {
        // 가짜 DNS 서버: 받은 질의에 "nas.home"으로 답한다.
        let server = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = server.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 512];
            let (n, from) = server.recv_from(&mut buf).await.unwrap();
            let mut r = buf[..n].to_vec();
            r[2] = 0x81;
            r[3] = 0x80;
            r[7] = 1;
            r.extend_from_slice(&[0xc0, 0x0c, 0, 12, 0, 1, 0, 0, 0, 60, 0, 10]);
            r.extend_from_slice(b"\x03nas\x04home\x00");
            server.send_to(&r, from).await.unwrap();
        });
        let ip = Ipv4Addr::new(192, 0, 2, 7);
        let id = 0x0101;
        let reply = ask_udp(addr, &ptr_query(id, ip)).await.expect("답이 와야 한다");
        assert_eq!(ptr_name(&reply, id).as_deref(), Some("nas.home"));
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
        let node = out.lines().find(|l| l.starts_with("NODE ")).unwrap();
        assert_eq!(node.split_whitespace().count(), 6, "{node}");
        assert!(netmap(&["10.0.0.0/16".into()], &|_| {}).await.is_err());
    }
}
