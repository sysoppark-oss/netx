//! NetX에 내장된 도구. 외부 프로그램을 띄우지 않고 직접 수행한다.
//!
//! portscan: TCP 연결만 시도한다(연결되면 바로 끊는다). 데이터는 보내지 않는다.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::{TcpStream, UdpSocket};

use super::probe;
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
    } else if let Ok(fixed) = std::env::var("NETX_TEST_NEIGHBORS") {
        // 개발용(리눅스에서 화면을 확인할 때): "ip mac" 줄을 이웃 목록 대신 쓴다. Windows 빌드에는 이 길이 없다.
        parse_neighbors_windows(&fixed)
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
/// 정밀 조사(`--deep`)에서 두드리는 포트: 기본 13종에 관리·프린터·NAS·카메라·DB·원격 포트를 더했다.
const DEEP_PORTS: &[u16] = &[
    21, 22, 23, 25, 53, 80, 81, 88, 110, 111, 135, 139, 143, 179, 389, 443, 445, 515, 548, 554, 631, 873, 993, 1433, 1521, 1883, 2049, 3000, 3306, 3389, 5000, 5001, 5432, 5900, 5985, 6379, 7547, 8000, 8008, 8080, 8081, 8443,
    8888, 9000, 9100, 9200, 62078,
];

/// `netmap`의 옵션
#[derive(Debug, PartialEq)]
pub struct MapOptions {
    pub target: String,
    pub ports: Vec<u16>,
    pub ping: bool,
    pub banner: bool,
    pub mdns: bool,
    pub snmp: bool,
    pub timeout: Duration,
}

impl MapOptions {
    pub fn parse(argv: &[String]) -> Result<MapOptions, String> {
        let mut o = MapOptions { target: String::new(), ports: MAP_PORTS.to_vec(), ping: false, banner: false, mdns: false, snmp: true, timeout: CONNECT_TIMEOUT };
        let (mut deep, mut custom) = (false, None);
        let mut it = argv.iter();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--" => {}
                "--deep" => deep = true,
                "--ping" => o.ping = true,
                "--banner" => o.banner = true,
                "--mdns" => o.mdns = true,
                "--snmp" => o.snmp = true,
                "--no-snmp" => o.snmp = false,
                "--ports" => custom = Some(parse_ports(it.next().ok_or("--ports 뒤에 포트 목록을 적습니다")?)?),
                "--timeout" => {
                    let ms = it.next().and_then(|v| v.parse::<u64>().ok()).filter(|ms| (200..=5000).contains(ms)).ok_or("--timeout은 200~5000(밀리초)입니다")?;
                    o.timeout = Duration::from_millis(ms);
                }
                a if a.starts_with('-') => return Err(format!("모르는 옵션입니다: {a}")),
                a if o.target.is_empty() => o.target = a.to_string(),
                _ => return Err("인자가 너무 많습니다".into()),
            }
        }
        if o.target.is_empty() {
            return Err("대역이 없습니다".into());
        }
        // 포트를 직접 적었으면 그것을, 아니면 정밀 조사일 때 넓은 목록을 쓴다.
        o.ports = custom.unwrap_or_else(|| if deep { DEEP_PORTS.to_vec() } else { MAP_PORTS.to_vec() });
        Ok(o)
    }
}

/// 대역 전체에 핑을 보내 답한 주소와 응답 시간을 모은다(한 번 더 시도).
async fn ping_sweep(hosts: Vec<Ipv4Addr>, timeout: Duration) -> Vec<(Ipv4Addr, f64)> {
    use crate::basic::icmp::{Echo, probe as echo, system};
    let prober = system();
    let gate = Arc::new(Semaphore::new(64));
    let wait_ms = timeout.as_millis().min(5000) as u32;
    let mut set: JoinSet<Option<(Ipv4Addr, f64)>> = JoinSet::new();
    for ip in hosts {
        let (prober, gate) = (prober.clone(), gate.clone());
        set.spawn(async move {
            let _permit = gate.acquire().await;
            for _ in 0..2 {
                if let Echo::Reply { ms, .. } = echo(&prober, ip, 64, wait_ms, 32).await {
                    return Some((ip, ms));
                }
            }
            None
        });
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        out.extend(r.ok().flatten());
    }
    out.sort_by_key(|(ip, _)| u32::from(*ip));
    out
}

pub async fn netmap(argv: &[String], emit: &(dyn Fn(String) + Send + Sync)) -> Result<i32, String> {
    let MapOptions { target, ports, ping, banner, mdns, snmp, timeout } = MapOptions::parse(argv)?;
    let target = &target;
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
    if hosts.len() * ports.len() > MAX_PROBES {
        return Err(format!("범위가 너무 넓습니다(장비 {}대 × 포트 {}개). 대역을 좁히거나 포트를 줄이세요.", hosts.len(), ports.len()));
    }
    let extras: Vec<&str> = [(ping, "핑"), (mdns, "mDNS·UPnP"), (banner, "포트 첫 응답")].iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
    emit(format!("{cidr} 대역의 주소 {}개를 조사합니다. (포트 {}종 연결 시도 + 이웃 목록{})", hosts.len(), ports.len(), extras.iter().map(|e| format!(" + {e}")).collect::<String>()));

    // 핑과 mDNS·UPnP는 연결 시도와 함께 돌린다.
    let ping_task = ping.then(|| tauri::async_runtime::spawn(ping_sweep(hosts.clone(), timeout)));
    let cast_task = mdns.then(|| tauri::async_runtime::spawn(async move { (probe::mdns(probe::MDNS, Duration::from_millis(2500)).await, probe::ssdp(probe::SSDP, Duration::from_millis(2500)).await) }));

    // 연결되거나(열림) 거절당하면(닫힘) 켜져 있는 장비다. 응답이 없으면 알 수 없다.
    let gate = Arc::new(Semaphore::new(CONCURRENCY));
    let mut set: JoinSet<(Ipv4Addr, Vec<u16>, bool)> = JoinSet::new();
    for host in hosts.iter().copied() {
        let gate = gate.clone();
        let ports = ports.clone();
        set.spawn(async move {
            let mut probes: JoinSet<(u16, u8)> = JoinSet::new();
            for port in ports.iter().copied() {
                let gate = gate.clone();
                probes.spawn(async move {
                    let Ok(_permit) = gate.acquire().await else { return (port, 0) };
                    match tokio::time::timeout(timeout, TcpStream::connect(SocketAddr::new(IpAddr::V4(host), port))).await {
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

    // 핑에 답한 장비, mDNS·UPnP에 답한 장비도 켜져 있는 장비다.
    let pings: Vec<(Ipv4Addr, f64)> = match ping_task {
        Some(task) => task.await.unwrap_or_default(),
        None => Vec::new(),
    };
    let (services, upnp) = match cast_task {
        Some(task) => task.await.unwrap_or_default(),
        None => (Vec::new(), Vec::new()),
    };
    for ip in pings.iter().map(|(ip, _)| *ip).chain(services.iter().map(|(ip, _)| *ip)).chain(upnp.iter().map(|(ip, _)| *ip)) {
        if hosts.contains(&ip) && !found.iter().any(|(h, _)| *h == ip) {
            found.push((ip, vec![]));
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

    // 내 PC의 MAC은 이웃 목록에 없으므로 어댑터에서 읽어 더한다(스위치의 MAC 표와 맞춰 보는 데 쓴다).
    let mut macs = macs;
    if let Some(ip) = me.filter(|ip| ips.contains(ip) && !macs.iter().any(|(m, _)| m == ip)) {
        macs.extend(crate::netinfo::my_mac(info.iface.as_deref()).map(|mac| (ip, mac)));
    }

    // 스위치 연결 관계: 커뮤니티 문자열이 설정돼 있을 때만 SNMP(읽기 전용)로 묻는다.
    let switches = match crate::settings::snmp_community().filter(|_| snmp) {
        Some(community) => {
            emit("SNMP로 스위치·AP에 연결 관계를 묻습니다. (읽기 전용, 답하는 장비만)".to_string());
            let targets: Vec<(Ipv4Addr, SocketAddr)> = ips.iter().filter(|ip| Some(**ip) != me).map(|ip| (*ip, SocketAddr::new(IpAddr::V4(*ip), crate::snmp::PORT))).collect();
            snmp_survey(&targets, &community).await
        }
        None => Vec::new(),
    };
    // 장비가 스스로 알려 준 이름(sysName)을 먼저 쓴다.
    for sw in &switches {
        if let Some(name) = &sw.name {
            names.retain(|(ip, _)| *ip != sw.ip);
            names.push((sw.ip, name.clone()));
        }
    }
    let links = derive_links(&switches, &macs, &names, gateway);

    let show = |ip: Option<Ipv4Addr>| ip.map(|i| i.to_string()).unwrap_or_else(|| "-".into());
    emit(format!("NET {cidr} GW {} ME {}", show(gateway), show(me)));
    for (ip, open) in &found {
        let mac = macs.iter().find(|(m, _)| m == ip).map(|(_, m)| m.as_str()).unwrap_or("-");
        let role = if Some(*ip) == me { "me" } else if Some(*ip) == gateway { "gateway" } else { "host" };
        let ports = if open.is_empty() { "-".to_string() } else { open.iter().map(u16::to_string).collect::<Vec<_>>().join(",") };
        let name = names.iter().find(|(n, _)| n == ip).map(|(_, n)| n.as_str()).unwrap_or("-");
        emit(format!("NODE {ip} {mac} {role} {ports} {name}"));
    }
    for sw in &switches {
        emit(format!("SNMP {} {} {} {}", sw.ip, sw.fdb.len(), sw.lldp.len(), sw.descr.as_deref().unwrap_or("-")));
    }
    // 덧붙임 조사 결과: "INFO <ip> <ping|mdns|upnp|banner> <글…>"
    for (ip, ms) in pings.iter().filter(|(ip, _)| ips.contains(ip)) {
        emit(format!("INFO {ip} ping {ms:.2}"));
    }
    for (ip, labels) in services.iter().filter(|(ip, l)| ips.contains(ip) && !l.is_empty()) {
        emit(format!("INFO {ip} mdns {}", labels.join(", ")));
    }
    for (ip, server) in upnp.iter().filter(|(ip, _)| ips.contains(ip)) {
        emit(format!("INFO {ip} upnp {server}"));
    }
    if banner {
        // 열린 포트 가운데 첫 응답을 읽을 줄 아는 것만, 장비마다 네 개까지.
        let mut tasks: JoinSet<(Ipv4Addr, u16, Option<String>)> = JoinSet::new();
        let gate = Arc::new(Semaphore::new(32));
        for (ip, open) in found.iter().filter(|(ip, _)| Some(*ip) != me) {
            for port in open.iter().copied().filter(|p| probe::TALK_FIRST.contains(p) || probe::WEB.contains(p)).take(4) {
                let (gate, ip) = (gate.clone(), *ip);
                tasks.spawn(async move {
                    let _permit = gate.acquire().await;
                    (ip, port, probe::banner(SocketAddr::new(IpAddr::V4(ip), port)).await)
                });
            }
        }
        let mut banners = Vec::new();
        while let Some(r) = tasks.join_next().await {
            if let Ok((ip, port, Some(text))) = r {
                banners.push((ip, port, text));
            }
        }
        banners.sort();
        for (ip, port, text) in banners {
            emit(format!("INFO {ip} banner {port} {text}"));
        }
    }
    for l in &links {
        emit(format!("LINK {} {} {} {} {}", l.switch, l.port, l.device, l.via, l.shared));
    }
    emit(format!("조사 끝: 장비 {}대를 찾았습니다.", found.len()));
    Ok(if found.is_empty() { 1 } else { 0 })
}

// ---- 스위치·AP 연결 관계 (SNMP). 패킷을 만들고 읽는 일은 `crate::snmp`가 하고, 여기서는 구성도에 쓸 모양으로 바꾼다. ----

const SYS_DESCR: &str = "1.3.6.1.2.1.1.1.0";
const SYS_NAME: &str = "1.3.6.1.2.1.1.5.0";
/// LLDP 이웃 표의 칸들. 색인은 (시각 표시, 내 포트 번호, 이웃 번호)다.
const LLDP_CHASSIS_KIND: &str = "1.0.8802.1.1.2.1.4.1.1.4";
const LLDP_CHASSIS_ID: &str = "1.0.8802.1.1.2.1.4.1.1.5";
const LLDP_SYS_NAME: &str = "1.0.8802.1.1.2.1.4.1.1.9";
/// 브리지 MAC 표: 색인이 MAC 6바이트, 값이 포트 번호
const FDB_PORT: &str = "1.3.6.1.2.1.17.4.3.1.2";
/// VLAN을 쓰는 스위치의 MAC 표: 색인이 (VLAN 묶음 번호, MAC 6바이트)
const QFDB_PORT: &str = "1.3.6.1.2.1.17.7.1.2.2.1.2";
/// 브리지 포트 번호 → 인터페이스 번호, 인터페이스 번호 → 이름
const BRIDGE_IFINDEX: &str = "1.3.6.1.2.1.17.1.4.1.2";
const IF_NAME: &str = "1.3.6.1.2.1.31.1.1.1.1";

const SNMP_TIMEOUT: Duration = Duration::from_secs(1);
/// 장비 한 대에서 표를 읽는 데 쓰는 최대 시간
const SNMP_DEVICE_LIMIT: Duration = Duration::from_secs(25);
const MAX_FDB_ROWS: usize = 2048;

/// LLDP로 알게 된 이웃 한 대
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbor {
    pub port: u32,
    pub mac: Option<String>,
    pub name: Option<String>,
}

/// SNMP에 답한 장비에서 읽은 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchInfo {
    pub ip: Ipv4Addr,
    pub name: Option<String>,
    pub descr: Option<String>,
    /// (MAC, 포트 번호)
    pub fdb: Vec<(String, u32)>,
    pub lldp: Vec<Neighbor>,
    /// (포트 번호, 포트 이름)
    pub port_names: Vec<(u32, String)>,
}

/// 구성도에 그을 선 하나: `device`가 `switch`의 `port`에 물려 있다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub switch: Ipv4Addr,
    pub port: String,
    pub device: Ipv4Addr,
    /// lldp(이웃 알림으로 확인) 또는 fdb(MAC 표로 짐작)
    pub via: &'static str,
    /// 그 포트에서 보이는 MAC 수. 1이면 직접 물린 것, 더 많으면 사이에 다른 스위치·AP가 있다.
    pub shared: usize,
}

/// 장비 설명은 출력 줄에 섞여 들어가므로 보이는 영문·숫자·기호와 빈칸만 남기고 80자로 자른다.
pub fn clean_descr(raw: &str) -> Option<String> {
    let kept: String = raw.chars().map(|c| if c.is_ascii_graphic() { c } else { ' ' }).collect();
    let text: String = kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(80).collect();
    (!text.is_empty()).then_some(text)
}

/// 포트 이름은 한 칸에 들어가야 하므로 영문·숫자와 `/ . : - _`만 남기고 24자로 자른다.
pub fn clean_port(raw: &str) -> Option<String> {
    let name: String = raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | ':' | '-' | '_')).take(24).collect();
    (!name.is_empty()).then_some(name)
}

/// OID 색인의 여섯 마디를 MAC 글자로
fn mac_from_arcs(arcs: &[u32]) -> Option<String> {
    (arcs.len() == 6 && arcs.iter().all(|a| *a <= 255)).then(|| arcs.iter().map(|a| format!("{a:02x}")).collect::<Vec<_>>().join(":"))
}

async fn snmp_rows(client: &mut crate::snmp::Client, root: &str, max: usize) -> Vec<(Vec<u32>, crate::snmp::Value)> {
    let Ok(root) = root.parse::<crate::snmp::Oid>() else { return Vec::new() };
    let rows = client.walk(&root, max).await.unwrap_or_default();
    rows.into_iter().filter_map(|r| Some((r.oid.suffix_of(&root)?.to_vec(), r.value))).collect()
}

/// 장비 한 대에 묻는다. SNMP에 답하지 않으면 None.
async fn snmp_probe(ip: Ipv4Addr, addr: SocketAddr, community: &str) -> Option<SwitchInfo> {
    use crate::snmp::{Client, Oid};
    // 먼저 한 번만 물어 SNMP에 답하는 장비인지 본다(답이 없는 장비가 대부분이다).
    let mut first = Client::connect(addr, community, SNMP_TIMEOUT, 0).await.ok()?;
    let name = first.get(&[SYS_NAME.parse::<Oid>().ok()?]).await.ok()?;
    let descr = first.get(&[SYS_DESCR.parse::<Oid>().ok()?]).await.unwrap_or_default();
    let text = |rows: &[crate::snmp::VarBind]| rows.first().and_then(|v| v.value.as_bytes()).map(|b| String::from_utf8_lossy(b).into_owned());
    let mut info = SwitchInfo { ip, name: text(&name).and_then(|t| clean_name(&t)), descr: text(&descr).and_then(|t| clean_descr(&t)), fdb: vec![], lldp: vec![], port_names: vec![] };

    let Ok(mut c) = Client::connect(addr, community, SNMP_TIMEOUT, 1).await else { return Some(info) };
    let tables = async {
        // LLDP 이웃: 색인의 둘째 마디가 내 포트 번호다.
        let kinds = snmp_rows(&mut c, LLDP_CHASSIS_KIND, 256).await;
        let names = snmp_rows(&mut c, LLDP_SYS_NAME, 256).await;
        let mut lldp = Vec::new();
        for (index, value) in snmp_rows(&mut c, LLDP_CHASSIS_ID, 256).await {
            let Some(port) = index.get(1).copied().filter(|_| index.len() == 3) else { continue };
            let kind = kinds.iter().find(|(i, _)| *i == index).and_then(|(_, v)| v.as_u64());
            // 종류 4가 MAC 주소다. 종류를 알려 주지 않는 장비는 길이가 6바이트면 MAC으로 본다.
            let mac = value.as_bytes().filter(|b| b.len() == 6 && kind.is_none_or(|k| k == 4)).and_then(|b| mac_from_arcs(&b.iter().map(|x| u32::from(*x)).collect::<Vec<_>>()));
            let name = names.iter().find(|(i, _)| *i == index).and_then(|(_, v)| v.as_bytes()).and_then(|b| clean_name(&String::from_utf8_lossy(b)));
            if mac.is_some() || name.is_some() {
                lldp.push(Neighbor { port, mac, name });
            }
        }
        // MAC 표: 기본 표가 비어 있으면 VLAN용 표를 읽는다(색인 맨 앞의 묶음 번호는 버린다).
        let port_of = |v: &crate::snmp::Value| v.as_u64().and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0);
        let mut fdb: Vec<(String, u32)> = snmp_rows(&mut c, FDB_PORT, MAX_FDB_ROWS).await.iter().filter_map(|(i, v)| Some((mac_from_arcs(i)?, port_of(v)?))).collect();
        if fdb.is_empty() {
            fdb = snmp_rows(&mut c, QFDB_PORT, MAX_FDB_ROWS).await.iter().filter_map(|(i, v)| Some((mac_from_arcs(i.get(1..)?)?, port_of(v)?))).collect();
        }
        fdb.sort();
        fdb.dedup_by(|a, b| a.0 == b.0);
        // 포트 이름: 번호보다 이름(ifName)이 장비 겉면의 표시와 맞는다.
        let mut port_names: Vec<(u32, String)> = Vec::new();
        if !fdb.is_empty() || !lldp.is_empty() {
            let if_names: Vec<(u32, String)> = snmp_rows(&mut c, IF_NAME, 1024).await.iter().filter_map(|(i, v)| Some((*i.first()?, clean_port(&String::from_utf8_lossy(v.as_bytes()?))?))).collect();
            let name_of = |ifindex: u32| if_names.iter().find(|(i, _)| *i == ifindex).map(|(_, n)| n.clone());
            for (i, v) in snmp_rows(&mut c, BRIDGE_IFINDEX, 512).await {
                if let (Some(port), Some(name)) = (i.first().copied(), port_of(&v).and_then(name_of)) {
                    port_names.push((port, name));
                }
            }
            // LLDP의 포트 번호는 보통 인터페이스 번호와 같다. 브리지 표에 없는 번호만 그렇게 채운다.
            for n in &lldp {
                if !port_names.iter().any(|(p, _)| *p == n.port) {
                    port_names.extend(name_of(n.port).map(|name| (n.port, name)));
                }
            }
        }
        (fdb, lldp, port_names)
    };
    if let Ok((fdb, lldp, port_names)) = tokio::time::timeout(SNMP_DEVICE_LIMIT, tables).await {
        info.fdb = fdb;
        info.lldp = lldp;
        info.port_names = port_names;
    }
    Some(info)
}

/// 여러 장비에 한꺼번에 묻고, 답한 장비만 주소 순으로 돌려준다.
pub async fn snmp_survey(targets: &[(Ipv4Addr, SocketAddr)], community: &str) -> Vec<SwitchInfo> {
    let mut set: JoinSet<Option<SwitchInfo>> = JoinSet::new();
    for (ip, addr) in targets.iter().copied() {
        let community = community.to_string();
        set.spawn(async move { snmp_probe(ip, addr, &community).await });
    }
    let mut found = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(info)) = r {
            found.push(info);
        }
    }
    found.sort_by_key(|s| u32::from(s.ip));
    found
}

/// 스위치들이 알려 준 표에서 "어느 장비가 어느 스위치의 어느 포트에 물렸는지"를 정한다.
///
/// - LLDP 이웃(장비가 스스로 알린 것)이 MAC 표(짐작)보다 먼저다.
/// - 스위치마다 공유기의 MAC이 보이는 포트는 위로 올라가는 포트다. 그 포트에 함께 보이는 장비는 이 스위치에 물린 것이
///   아니므로 선을 긋지 않는다(공유기 자신만 긋는다).
/// - 한 장비가 여러 스위치에서 보이면 그 포트의 MAC 수가 가장 적은 스위치에 붙인다(가장 가까운 스위치).
pub fn derive_links(switches: &[SwitchInfo], macs: &[(Ipv4Addr, String)], names: &[(Ipv4Addr, String)], gateway: Option<Ipv4Addr>) -> Vec<Link> {
    let label = |sw: &SwitchInfo, port: u32| sw.port_names.iter().find(|(p, _)| *p == port).map(|(_, n)| n.clone()).unwrap_or_else(|| port.to_string());
    let gateway_mac = gateway.and_then(|g| macs.iter().find(|(ip, _)| *ip == g)).map(|(_, m)| m.as_str());
    // (LLDP가 아니면 1, 그 포트의 MAC 수, 스위치 주소)가 작은 것이 이긴다.
    let mut best: Vec<((u8, usize, u32), Link)> = Vec::new();
    let mut offer = |rank: (u8, usize, u32), link: Link| match best.iter_mut().find(|(_, l)| l.device == link.device) {
        Some(slot) if rank < slot.0 => *slot = (rank, link),
        Some(_) => {}
        None => best.push((rank, link)),
    };
    for sw in switches {
        let on_port = |port: u32| sw.fdb.iter().filter(|(_, p)| *p == port).count();
        for n in &sw.lldp {
            let by_mac = n.mac.as_deref().and_then(|m| macs.iter().find(|(_, mac)| mac == m)).map(|(ip, _)| *ip);
            let by_name = || n.name.as_deref().and_then(|x| names.iter().find(|(_, name)| name.eq_ignore_ascii_case(x))).map(|(ip, _)| *ip);
            if let Some(device) = by_mac.or_else(by_name).filter(|d| *d != sw.ip) {
                offer((0, 1, u32::from(sw.ip)), Link { switch: sw.ip, port: label(sw, n.port), device, via: "lldp", shared: on_port(n.port).max(1) });
            }
        }
        let uplink = gateway_mac.and_then(|g| sw.fdb.iter().find(|(m, _)| m == g)).map(|(_, p)| *p);
        for (device, mac) in macs.iter().filter(|(ip, _)| *ip != sw.ip) {
            let Some(port) = sw.fdb.iter().find(|(m, _)| m == mac).map(|(_, p)| *p) else { continue };
            if Some(port) == uplink && Some(*device) != gateway {
                continue;
            }
            let shared = on_port(port);
            offer((1, shared, u32::from(sw.ip)), Link { switch: sw.ip, port: label(sw, port), device: *device, via: "fdb", shared });
        }
    }
    let mut links: Vec<Link> = best.into_iter().map(|(_, l)| l).collect();
    links.sort_by_key(|l| (u32::from(l.switch), u32::from(l.device)));
    links
}

#[cfg(test)]
mod switch_tests {
    use super::*;

    fn ip(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(192, 168, 0, last)
    }

    fn mac(last: u8) -> String {
        format!("02:00:00:00:00:{last:02x}")
    }

    fn switch(last: u8, fdb: &[(u8, u32)]) -> SwitchInfo {
        SwitchInfo { ip: ip(last), name: None, descr: None, fdb: fdb.iter().map(|(m, p)| (mac(*m), *p)).collect(), lldp: vec![], port_names: vec![] }
    }

    #[test]
    fn text_from_devices_is_cleaned() {
        assert_eq!(clean_descr("  Managed\tSwitch\r\n 8-port <b>v1.2</b> \u{d55c}").as_deref(), Some("Managed Switch 8-port <b>v1.2</b>"));
        assert_eq!(clean_descr(&"x".repeat(200)).map(|d| d.len()), Some(80));
        assert_eq!(clean_descr(" \n "), None);
        assert_eq!(clean_port("GigabitEthernet1/0/3").as_deref(), Some("GigabitEthernet1/0/3"));
        assert_eq!(clean_port("Port 3 <x>").as_deref(), Some("Port3x"));
        assert_eq!(clean_port("  "), None);
        assert_eq!(mac_from_arcs(&[0, 17, 34, 51, 68, 255]).as_deref(), Some("00:11:22:33:44:ff"));
        assert_eq!(mac_from_arcs(&[0, 17, 34, 51, 68]), None);
        assert_eq!(mac_from_arcs(&[0, 17, 34, 51, 68, 256]), None);
    }

    #[test]
    fn devices_attach_to_the_nearest_switch_port() {
        // 공유기(.1) ─ 스위치 A(.21) ─ 스위치 B(.22). PC .30은 A의 3번, PC .40과 .41은 B의 2번·5번.
        // 휴대폰 .50은 공유기의 무선에 붙어 있어 두 스위치 모두 위로 올라가는 포트에서 보인다.
        let macs: Vec<(Ipv4Addr, String)> = [1u8, 21, 22, 30, 40, 41, 50].iter().map(|n| (ip(*n), mac(*n))).collect();
        let a = switch(21, &[(1, 8), (50, 8), (30, 3), (22, 7), (40, 7), (41, 7)]);
        let b = switch(22, &[(1, 1), (50, 1), (21, 1), (30, 1), (40, 2), (41, 5)]);
        let links = derive_links(&[a.clone(), b.clone()], &macs, &[], Some(ip(1)));
        let show: Vec<String> = links.iter().map(|l| format!("{}:{}>{} {} {}", l.switch.octets()[3], l.port, l.device.octets()[3], l.via, l.shared)).collect();
        assert_eq!(show, ["21:8>1 fdb 2", "21:7>22 fdb 3", "21:3>30 fdb 1", "22:2>40 fdb 1", "22:5>41 fdb 1"]);
        // 공유기를 모르면 위로 올라가는 포트를 가릴 수 없다. 그래도 가장 한가한 포트로 붙인다.
        let blind = derive_links(&[a, b], &macs, &[], None);
        assert!(blind.iter().any(|l| l.device == ip(40) && l.switch == ip(22) && l.shared == 1));
        assert!(derive_links(&[], &macs, &[], Some(ip(1))).is_empty());
    }

    #[test]
    fn lldp_wins_over_the_mac_table_and_port_names_are_used() {
        let macs = vec![(ip(1), mac(1)), (ip(21), mac(21)), (ip(60), mac(60))];
        let names = vec![(ip(70), "ap-lobby".to_string())];
        let mut sw = switch(21, &[(1, 8), (60, 4), (99, 4)]);
        sw.lldp = vec![Neighbor { port: 4, mac: Some(mac(60)), name: None }, Neighbor { port: 6, mac: None, name: Some("AP-Lobby".into()) }, Neighbor { port: 9, mac: Some(mac(21)), name: None }];
        sw.port_names = vec![(4, "ge-0/0/4".into())];
        let links = derive_links(&[sw], &macs, &names, Some(ip(1)));
        let find = |d: u8| links.iter().find(|l| l.device == ip(d)).cloned();
        assert_eq!(find(60).map(|l| (l.port, l.via, l.shared)), Some(("ge-0/0/4".to_string(), "lldp", 2)));
        assert_eq!(find(70).map(|l| (l.port, l.via)), Some(("6".to_string(), "lldp")), "이름으로 맞춘 이웃");
        assert!(links.iter().all(|l| l.device != l.switch), "자기 자신에게는 선을 긋지 않는다");
    }

    /// 가짜 스위치: 받은 OID 표로 GET·GETNEXT에 답한다.
    async fn fake_switch(rows: &[(&str, u8, Vec<u8>)]) -> SocketAddr {
        use crate::snmp::{ber, Oid, Request};
        let table: std::collections::BTreeMap<Oid, (u8, Vec<u8>)> = rows.iter().map(|(o, t, b)| (o.parse().unwrap(), (*t, b.clone()))).collect();
        let sock = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = sock.local_addr().unwrap();
        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                let Ok((n, from)) = sock.recv_from(&mut buf).await else { return };
                let Some((kind, community, id, asked)) = ber::tests::read_request(&buf[..n]) else { continue };
                if community != b"ro-test" {
                    continue;
                }
                let hit = match kind {
                    Request::Get => table.get_key_value(&asked),
                    Request::GetNext => table.range((std::ops::Bound::Excluded(asked.clone()), std::ops::Bound::Unbounded)).next(),
                };
                let reply = match hit {
                    Some((o, (tag, body))) => ber::tests::response(id, 0, &[(o, *tag, body.clone())]),
                    None => ber::tests::response(id, 0, &[(&asked, 0x82, vec![])]),
                };
                let _ = sock.send_to(&reply, from).await;
            }
        });
        addr
    }

    #[tokio::test]
    async fn reads_a_switch_over_snmp() {
        let addr = fake_switch(&[
            ("1.3.6.1.2.1.1.1.0", 0x04, b"Managed Switch\r\n8-port".to_vec()),
            ("1.3.6.1.2.1.1.5.0", 0x04, b"sw office".to_vec()),
            // LLDP: 5번 포트에 MAC으로 알린 이웃, 6번 포트에 이름만 알린 이웃
            ("1.0.8802.1.1.2.1.4.1.1.4.0.5.1", 0x02, vec![4]),
            ("1.0.8802.1.1.2.1.4.1.1.4.0.6.1", 0x02, vec![7]),
            ("1.0.8802.1.1.2.1.4.1.1.5.0.5.1", 0x04, vec![2, 0, 0, 0, 0, 0x28]),
            ("1.0.8802.1.1.2.1.4.1.1.5.0.6.1", 0x04, b"ap0001".to_vec()),
            ("1.0.8802.1.1.2.1.4.1.1.9.0.6.1", 0x04, b"ap-lobby".to_vec()),
            // MAC 표
            ("1.3.6.1.2.1.17.4.3.1.2.2.0.0.0.0.1", 0x02, vec![8]),
            ("1.3.6.1.2.1.17.4.3.1.2.2.0.0.0.0.30", 0x02, vec![3]),
            ("1.3.6.1.2.1.17.4.3.1.2.2.0.0.0.0.40", 0x02, vec![0]),
            // 포트 이름: 브리지 포트 3 → 인터페이스 103 → "ge3"
            ("1.3.6.1.2.1.17.1.4.1.2.3", 0x02, vec![103]),
            ("1.3.6.1.2.1.31.1.1.1.1.5", 0x04, b"ge5".to_vec()),
            ("1.3.6.1.2.1.31.1.1.1.1.103", 0x04, b"ge3".to_vec()),
        ])
        .await;
        let found = snmp_survey(&[(ip(21), addr)], "ro-test").await;
        assert_eq!(found.len(), 1);
        let sw = &found[0];
        assert_eq!((sw.name.as_deref(), sw.descr.as_deref()), (Some("swoffice"), Some("Managed Switch 8-port")));
        assert_eq!(sw.fdb, [(mac(1), 8), (mac(30), 3)], "포트 0은 버린다");
        assert_eq!(sw.lldp, [Neighbor { port: 5, mac: Some(mac(0x28)), name: None }, Neighbor { port: 6, mac: None, name: Some("ap-lobby".into()) }]);
        assert_eq!(sw.port_names, [(3, "ge3".to_string()), (5, "ge5".to_string())]);
        // 커뮤니티 문자열이 틀리면 답이 없어 목록에 들지 않는다.
        assert!(snmp_survey(&[(ip(21), addr)], "wrong").await.is_empty());
    }
}

#[cfg(test)]
mod map_option_tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn options() {
        let o = MapOptions::parse(&s(&["192.168.0.0/24"])).unwrap();
        assert_eq!((o.ports.len(), o.ping, o.banner, o.mdns, o.snmp, o.timeout), (13, false, false, false, true, CONNECT_TIMEOUT));
        let o = MapOptions::parse(&s(&["--deep", "--ping", "--mdns", "--banner", "--no-snmp", "--timeout", "1500", "--", "auto"])).unwrap();
        assert_eq!((o.target.as_str(), o.ports.len(), o.ping, o.banner, o.mdns, o.snmp, o.timeout), ("auto", DEEP_PORTS.len(), true, true, true, false, Duration::from_millis(1500)));
        // 정밀 조사 목록은 기본 목록을 모두 품고, 중복이 없고, 50개를 넘지 않는다.
        assert!(MAP_PORTS.iter().all(|p| DEEP_PORTS.contains(p)) && DEEP_PORTS.len() <= MAX_PORTS);
        assert!(DEEP_PORTS.windows(2).all(|w| w[0] < w[1]));
        // 포트를 직접 적으면 --deep보다 앞선다.
        assert_eq!(MapOptions::parse(&s(&["--deep", "--ports", "22,80", "10.0.0.0/24"])).unwrap().ports, [22, 80]);
        for bad in [vec![], vec!["--ping"], vec!["--timeout", "50", "a"], vec!["--timeout", "a"], vec!["--ports", "0", "a"], vec!["--ports"], vec!["--fast", "a"], vec!["a", "b"]] {
            assert!(MapOptions::parse(&s(&bad)).is_err(), "{bad:?}");
        }
    }
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
